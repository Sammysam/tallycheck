use super::*;
use crate::password::Verified;

async fn fresh() -> AppResult<(tempfile::TempDir, Db)> {
    let dir = tempfile::tempdir().map_err(AppError::internal)?;
    let db = Db::open(&dir.path().join("t.db")).await?;
    Ok((dir, db))
}

async fn role_id(db: &Db, name: &str) -> AppResult<String> {
    let mut c = db.read().acquire().await?;
    Ok(roles::find_by_name(&mut c, name).await?.map(|r| r.id).unwrap_or_default())
}

fn create(username: &str, role: Option<String>) -> CreateUser {
    CreateUser { username: username.into(), password: "password123".into(), role_id: role }
}

#[tokio::test]
async fn create_rejects_duplicates_and_unknown_roles() -> AppResult<()> {
    let (_d, db) = fresh().await?;
    let admin = role_id(&db, "Admin").await?;
    let u = create_user(&db, create("alice", Some(admin.clone()))).await?;
    assert!(u.is_admin());
    assert!(u.password_hash.starts_with("$argon2id$"));

    let dup = create_user(&db, create("alice", None)).await;
    assert!(matches!(dup, Err(AppError::Conflict(m)) if m == "User alice already exists"));

    let missing = "00000000-0000-4000-8000-000000000000".to_string();
    let bad = create_user(&db, create("bob", Some(missing.clone()))).await;
    assert!(matches!(bad, Err(AppError::NotFound(m)) if m == format!("Role {missing} not found")));
    Ok(())
}

#[tokio::test]
async fn last_active_admin_cannot_be_demoted_or_deactivated() -> AppResult<()> {
    let (_d, db) = fresh().await?;
    let admin = role_id(&db, "Admin").await?;
    let viewer = role_id(&db, "Viewer").await?;
    let a = create_user(&db, create("a", Some(admin.clone()))).await?;

    let r = update_user(&db, &a.id, UpdateUser { is_active: Some(false), ..Default::default() }).await;
    assert!(matches!(r, Err(AppError::BadRequest(_))));
    let r = update_user(&db, &a.id, UpdateUser { role_id: Some(viewer.clone()), ..Default::default() }).await;
    assert!(matches!(r, Err(AppError::BadRequest(_))));

    create_user(&db, create("b", Some(admin))).await?;
    let demoted = update_user(&db, &a.id, UpdateUser { role_id: Some(viewer), ..Default::default() }).await?;
    assert_eq!(demoted.role.map(|r| r.name).as_deref(), Some("Viewer"));
    Ok(())
}

#[tokio::test]
async fn rename_conflicts_and_unchanged_update_keeps_timestamp() -> AppResult<()> {
    let (_d, db) = fresh().await?;
    let a = create_user(&db, create("a", None)).await?;
    create_user(&db, create("b", None)).await?;
    let r = update_user(&db, &a.id, UpdateUser { username: Some("b".into()), ..Default::default() }).await;
    assert!(matches!(r, Err(AppError::Conflict(m)) if m == "User b already exists"));
    let same = update_user(&db, &a.id, UpdateUser { username: Some("a".into()), ..Default::default() }).await?;
    assert_eq!(same.updated_at, a.updated_at);
    let missing = update_user(&db, "nope", UpdateUser::default()).await;
    assert!(matches!(missing, Err(AppError::NotFound(m)) if m == "User nope not found"));
    Ok(())
}

#[tokio::test]
async fn reset_password_and_two_factor() -> AppResult<()> {
    let (_d, db) = fresh().await?;
    let a = create_user(&db, create("a", None)).await?;
    set_two_factor_secret(&db, &a.id, "JBSWY3DPEHPK3PXP").await?;
    enable_two_factor(&db, &a.id).await?;
    let u = reset_two_factor(&db, &a.id).await?;
    assert!(!u.is_two_factor_enabled);
    assert!(u.two_factor_secret.is_none());

    let u = reset_password(&db, &a.id, "brand-new-pass").await?;
    assert_eq!(password::verify("brand-new-pass", &u.password_hash), Verified::Match { needs_rehash: false });
    Ok(())
}

#[tokio::test]
async fn legacy_hash_is_upgraded() -> AppResult<()> {
    let (_d, db) = fresh().await?;
    let a = create_user(&db, create("a", None)).await?;
    let legacy = bcrypt::hash("password123", 4).map_err(AppError::internal)?;
    sqlx::query(r#"UPDATE "users" SET "passwordHash" = ?1 WHERE "id" = ?2"#)
        .bind(&legacy)
        .bind(&a.id)
        .execute(db.write())
        .await?;
    upgrade_password_hash(&db, &a.id, "password123").await?;
    let u = get_by_id(&db, &a.id).await?.ok_or_else(|| AppError::internal("missing"))?;
    assert!(u.password_hash.starts_with("$argon2id$"));
    Ok(())
}

#[tokio::test]
async fn bootstrap_admin_is_idempotent() -> AppResult<()> {
    let (_d, db) = fresh().await?;
    assert!(bootstrap_admin(&db, "admin", "admin12345").await?);
    assert!(!bootstrap_admin(&db, "admin", "other-password").await?);
    let u = get_by_username(&db, "admin").await?.ok_or_else(|| AppError::internal("missing"))?;
    assert!(u.is_admin());
    assert_eq!(password::verify("admin12345", &u.password_hash), Verified::Match { needs_rehash: false });
    Ok(())
}

#[tokio::test]
async fn permissions_follow_role_and_admin_bypasses() -> AppResult<()> {
    let (_d, db) = fresh().await?;
    let viewer = create_user(&db, create("v", Some(role_id(&db, "Viewer").await?))).await?;
    assert!(viewer.has_permission("reports"));
    assert!(!viewer.has_permission("users"));
    let none = create_user(&db, create("n", None)).await?;
    assert!(!none.has_permission("dashboard"));
    let admin = create_user(&db, create("x", Some(role_id(&db, "Admin").await?))).await?;
    assert!(admin.has_permission("anything-at-all"));
    Ok(())
}

#[tokio::test]
async fn role_rules_match_reference() -> AppResult<()> {
    let (_d, db) = fresh().await?;
    let admin = role_id(&db, "Admin").await?;
    let r = roles::update(&db, &admin, Some("Boss"), None).await;
    assert!(matches!(r, Err(AppError::Forbidden(m)) if m == "The Admin role cannot be modified"));
    let r = roles::remove(&db, &admin).await;
    assert!(matches!(r, Err(AppError::Forbidden(m)) if m == "The Admin role cannot be deleted"));

    let clerk = roles::create(&db, "Clerk", &["invoices".to_string()]).await?;
    assert!(!clerk.is_system);
    let dup = roles::create(&db, "Clerk", &[]).await;
    assert!(matches!(dup, Err(AppError::Conflict(m)) if m == "Role \"Clerk\" already exists"));
    let clash = roles::update(&db, &clerk.id, Some("Viewer"), None).await;
    assert!(matches!(clash, Err(AppError::Conflict(_))));

    create_user(&db, create("c", Some(clerk.id.clone()))).await?;
    let in_use = roles::remove(&db, &clerk.id).await;
    assert!(matches!(in_use, Err(AppError::Conflict(m)) if m == "Cannot delete a role that is assigned to users"));

    let spare = roles::create(&db, "Spare", &[]).await?;
    roles::remove(&db, &spare.id).await?;
    assert_eq!(roles::find_all(&db).await?.len(), 4);
    Ok(())
}
