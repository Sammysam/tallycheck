//! Users (reference `users.service.ts` and the user-facing parts of `auth.service.ts`).

use crate::password;
use crate::roles::{self, Role};
use app_core::ids::new_id;
use app_core::time::now_iso;
use app_core::{AppError, AppResult};
use database::Db;
use serde::Serialize;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

#[derive(Debug, Clone)]
pub struct User {
    pub id: String,
    pub username: String,
    pub password_hash: String,
    pub two_factor_secret: Option<String>,
    pub is_two_factor_enabled: bool,
    pub role_id: Option<String>,
    pub role: Option<Role>,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// A user as the API returns it: the reference's `sanitize()` drops `passwordHash` and
/// `twoFactorSecret` and keeps the eager-loaded `role`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserView {
    pub id: String,
    pub username: String,
    pub is_two_factor_enabled: bool,
    pub role_id: Option<String>,
    pub role: Option<Role>,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl From<User> for UserView {
    fn from(u: User) -> Self {
        UserView {
            id: u.id,
            username: u.username,
            is_two_factor_enabled: u.is_two_factor_enabled,
            role_id: u.role_id,
            role: u.role,
            is_active: u.is_active,
            created_at: u.created_at,
            updated_at: u.updated_at,
        }
    }
}

impl User {
    pub fn is_admin(&self) -> bool {
        self.role.as_ref().is_some_and(Role::is_admin)
    }

    /// Admin bypasses every check, as in the reference `PermissionsGuard`.
    pub fn has_permission(&self, permission: &str) -> bool {
        self.is_admin() || self.role.as_ref().is_some_and(|r| r.grants(permission))
    }
}

const COLUMNS: &str = r#""id","username","passwordHash","twoFactorSecret","isTwoFactorEnabled","roleId","isActive","createdAt","updatedAt""#;

fn from_row(row: &SqliteRow) -> AppResult<User> {
    Ok(User {
        id: row.try_get("id")?,
        username: row.try_get("username")?,
        password_hash: row.try_get("passwordHash")?,
        two_factor_secret: row.try_get("twoFactorSecret")?,
        is_two_factor_enabled: row.try_get::<i64, _>("isTwoFactorEnabled")? != 0,
        role_id: row.try_get("roleId")?,
        role: None,
        is_active: row.try_get::<i64, _>("isActive")? != 0,
        created_at: row.try_get("createdAt")?,
        updated_at: row.try_get("updatedAt")?,
    })
}

async fn with_role(conn: &mut SqliteConnection, mut user: User) -> AppResult<User> {
    if let Some(role_id) = user.role_id.clone() {
        user.role = roles::get(conn, &role_id).await?;
    }
    Ok(user)
}

async fn fetch_one_by(conn: &mut SqliteConnection, column: &str, value: &str) -> AppResult<Option<User>> {
    let row = sqlx::query(&format!(r#"SELECT {COLUMNS} FROM "users" WHERE "{column}" = ?1"#))
        .bind(value)
        .fetch_optional(&mut *conn)
        .await?;
    match row {
        Some(r) => Ok(Some(with_role(conn, from_row(&r)?).await?)),
        None => Ok(None),
    }
}

pub async fn find_by_id(conn: &mut SqliteConnection, id: &str) -> AppResult<Option<User>> {
    fetch_one_by(conn, "id", id).await
}

pub async fn find_by_username(conn: &mut SqliteConnection, username: &str) -> AppResult<Option<User>> {
    fetch_one_by(conn, "username", username).await
}

pub async fn get_by_id(db: &Db, id: &str) -> AppResult<Option<User>> {
    let mut conn = db.read().acquire().await?;
    find_by_id(&mut conn, id).await
}

pub async fn get_by_username(db: &Db, username: &str) -> AppResult<Option<User>> {
    let mut conn = db.read().acquire().await?;
    find_by_username(&mut conn, username).await
}

async fn require(conn: &mut SqliteConnection, id: &str) -> AppResult<User> {
    find_by_id(conn, id).await?.ok_or_else(|| AppError::NotFound(format!("User {id} not found")))
}

pub async fn find_all(db: &Db) -> AppResult<Vec<User>> {
    let mut conn = db.read().acquire().await?;
    let rows = sqlx::query(&format!(r#"SELECT {COLUMNS} FROM "users" ORDER BY "createdAt" ASC, rowid ASC"#))
        .fetch_all(&mut *conn)
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        out.push(with_role(&mut conn, from_row(r)?).await?);
    }
    Ok(out)
}

/// Argon2 is deliberately slow; keep it off the async executor threads.
pub async fn hash_password(password: &str) -> AppResult<String> {
    let p = password.to_string();
    tokio::task::spawn_blocking(move || password::hash(&p)).await.map_err(AppError::internal)?
}

pub async fn verify_password(password: &str, stored: &str) -> AppResult<password::Verified> {
    let (p, h) = (password.to_string(), stored.to_string());
    tokio::task::spawn_blocking(move || password::verify(&p, &h)).await.map_err(AppError::internal)
}

pub async fn burn_equivalent_time(password: &str) -> AppResult<()> {
    let p = password.to_string();
    tokio::task::spawn_blocking(move || password::burn_equivalent_time(&p)).await.map_err(AppError::internal)
}

async fn insert(
    conn: &mut SqliteConnection,
    username: &str,
    password_hash: &str,
    role_id: Option<&str>,
) -> AppResult<String> {
    let id = new_id();
    let now = now_iso();
    sqlx::query(
        r#"INSERT INTO "users" ("id","username","passwordHash","isTwoFactorEnabled","roleId","isActive","createdAt","updatedAt")
           VALUES (?1,?2,?3,0,?4,1,?5,?5)"#,
    )
    .bind(&id)
    .bind(username)
    .bind(password_hash)
    .bind(role_id)
    .bind(&now)
    .execute(conn)
    .await?;
    Ok(id)
}

pub struct CreateUser {
    pub username: String,
    pub password: String,
    pub role_id: Option<String>,
}

pub async fn create_user(db: &Db, dto: CreateUser) -> AppResult<User> {
    let hash = hash_password(&dto.password).await?;
    let mut tx = db.begin().await?;
    if find_by_username(&mut tx, &dto.username).await?.is_some() {
        return Err(AppError::Conflict(format!("User {} already exists", dto.username)));
    }
    if let Some(role_id) = &dto.role_id {
        roles::find_by_id(&mut tx, role_id).await?;
    }
    let id = insert(&mut tx, &dto.username, &hash, dto.role_id.as_deref()).await?;
    let user = require(&mut tx, &id).await?;
    tx.commit().await?;
    Ok(user)
}

#[derive(Default)]
pub struct UpdateUser {
    pub username: Option<String>,
    pub role_id: Option<String>,
    pub is_active: Option<bool>,
}

pub async fn update_user(db: &Db, id: &str, dto: UpdateUser) -> AppResult<User> {
    let mut tx = db.begin().await?;
    let user = require(&mut tx, id).await?;

    let currently_active_admin = user.is_admin() && user.is_active;
    let will_be_deactivated = dto.is_active == Some(false);
    let will_lose_admin = dto.role_id.as_ref().is_some_and(|r| Some(r) != user.role_id.as_ref());
    if currently_active_admin && (will_be_deactivated || will_lose_admin) {
        assert_another_active_admin_exists(&mut tx, id).await?;
    }

    let mut username = user.username.clone();
    if let Some(new_name) = dto.username.filter(|n| *n != user.username) {
        if find_by_username(&mut tx, &new_name).await?.is_some() {
            return Err(AppError::Conflict(format!("User {new_name} already exists")));
        }
        username = new_name;
    }
    let mut role_id = user.role_id.clone();
    if let Some(r) = dto.role_id {
        roles::find_by_id(&mut tx, &r).await?;
        role_id = Some(r);
    }
    let is_active = dto.is_active.unwrap_or(user.is_active);

    if username != user.username || role_id != user.role_id || is_active != user.is_active {
        sqlx::query(
            r#"UPDATE "users" SET "username" = ?1, "roleId" = ?2, "isActive" = ?3, "updatedAt" = ?4 WHERE "id" = ?5"#,
        )
        .bind(&username)
        .bind(&role_id)
        .bind(i64::from(is_active))
        .bind(now_iso())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }
    let updated = require(&mut tx, id).await?;
    tx.commit().await?;
    Ok(updated)
}

async fn assert_another_active_admin_exists(conn: &mut SqliteConnection, exclude_user_id: &str) -> AppResult<()> {
    let count: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM "users" u JOIN "roles" r ON u."roleId" = r."id"
           WHERE r."name" = 'Admin' AND u."isActive" = 1 AND u."id" <> ?1"#,
    )
    .bind(exclude_user_id)
    .fetch_one(conn)
    .await?;
    if count == 0 {
        return Err(AppError::bad_request("Cannot remove or deactivate the last active Admin"));
    }
    Ok(())
}

pub async fn reset_password(db: &Db, id: &str, new_password: &str) -> AppResult<User> {
    // Reference order: the user must exist before the password is hashed.
    {
        let mut conn = db.read().acquire().await?;
        require(&mut conn, id).await?;
    }
    let hash = hash_password(new_password).await?;
    let mut tx = db.begin().await?;
    require(&mut tx, id).await?;
    set_password_hash_in(&mut tx, id, &hash).await?;
    let user = require(&mut tx, id).await?;
    tx.commit().await?;
    Ok(user)
}

async fn set_password_hash_in(conn: &mut SqliteConnection, id: &str, hash: &str) -> AppResult<()> {
    sqlx::query(r#"UPDATE "users" SET "passwordHash" = ?1, "updatedAt" = ?2 WHERE "id" = ?3"#)
        .bind(hash)
        .bind(now_iso())
        .bind(id)
        .execute(conn)
        .await?;
    Ok(())
}

/// Replaces a legacy bcrypt hash with Argon2id after a successful login.
pub async fn upgrade_password_hash(db: &Db, id: &str, password: &str) -> AppResult<()> {
    let hash = hash_password(password).await?;
    let mut tx = db.begin().await?;
    set_password_hash_in(&mut tx, id, &hash).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn reset_two_factor(db: &Db, id: &str) -> AppResult<User> {
    let mut tx = db.begin().await?;
    require(&mut tx, id).await?;
    sqlx::query(
        r#"UPDATE "users" SET "isTwoFactorEnabled" = 0, "twoFactorSecret" = NULL, "updatedAt" = ?1 WHERE "id" = ?2"#,
    )
    .bind(now_iso())
    .bind(id)
    .execute(&mut *tx)
    .await?;
    let user = require(&mut tx, id).await?;
    tx.commit().await?;
    Ok(user)
}

pub async fn set_two_factor_secret(db: &Db, id: &str, secret: &str) -> AppResult<()> {
    let mut tx = db.begin().await?;
    sqlx::query(r#"UPDATE "users" SET "twoFactorSecret" = ?1, "updatedAt" = ?2 WHERE "id" = ?3"#)
        .bind(secret)
        .bind(now_iso())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn enable_two_factor(db: &Db, id: &str) -> AppResult<()> {
    let mut tx = db.begin().await?;
    sqlx::query(r#"UPDATE "users" SET "isTwoFactorEnabled" = 1, "updatedAt" = ?1 WHERE "id" = ?2"#)
        .bind(now_iso())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// First-run admin (reference `AuthService.onModuleInit`): creates `username` with the Admin
/// role unless that username already exists. Returns whether a user was created.
pub async fn bootstrap_admin(db: &Db, username: &str, password: &str) -> AppResult<bool> {
    if get_by_username(db, username).await?.is_some() {
        return Ok(false);
    }
    let hash = hash_password(password).await?;
    let mut tx = db.begin().await?;
    if find_by_username(&mut tx, username).await?.is_some() {
        return Ok(false);
    }
    let admin = roles::find_by_name(&mut tx, app_core::permissions::ADMIN_ROLE).await?;
    insert(&mut tx, username, &hash, admin.as_ref().map(|r| r.id.as_str())).await?;
    tx.commit().await?;
    Ok(true)
}

pub async fn any_users(db: &Db) -> AppResult<bool> {
    let n: i64 = sqlx::query_scalar(r#"SELECT COUNT(*) FROM "users""#).fetch_one(db.read()).await?;
    Ok(n > 0)
}

/// First-run setup (D17): creates the first user with the Admin role, but only while the
/// database has no users at all. The check and the insert share the writer transaction, so two
/// simultaneous requests cannot both succeed. Returns the new user, or `None` if users exist.
pub async fn create_first_admin(db: &Db, username: &str, password: &str) -> AppResult<Option<User>> {
    if any_users(db).await? {
        return Ok(None);
    }
    let hash = hash_password(password).await?;
    let mut tx = db.begin().await?;
    let n: i64 = sqlx::query_scalar(r#"SELECT COUNT(*) FROM "users""#).fetch_one(&mut *tx).await?;
    if n > 0 {
        return Ok(None);
    }
    let admin = roles::find_by_name(&mut tx, app_core::permissions::ADMIN_ROLE)
        .await?
        .ok_or_else(|| AppError::internal("Admin role missing"))?;
    let id = insert(&mut tx, username, &hash, Some(admin.id.as_str())).await?;
    let user = require(&mut tx, &id).await?;
    tx.commit().await?;
    Ok(Some(user))
}

#[cfg(test)]
mod tests;
