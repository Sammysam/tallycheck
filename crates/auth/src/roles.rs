//! Roles (reference `roles.service.ts`).

use app_core::ids::new_id;
use app_core::time::now_iso;
use app_core::{AppError, AppResult};
use database::Db;
use serde::Serialize;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Role {
    pub id: String,
    pub name: String,
    pub permissions: Vec<String>,
    pub is_system: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl Role {
    pub fn is_admin(&self) -> bool {
        self.name == app_core::permissions::ADMIN_ROLE
    }

    pub fn grants(&self, permission: &str) -> bool {
        self.permissions.iter().any(|p| p == permission)
    }
}

const COLUMNS: &str = r#""id","name","permissions","isSystem","createdAt","updatedAt""#;

fn from_row(row: &SqliteRow) -> AppResult<Role> {
    let permissions: String = row.try_get("permissions")?;
    Ok(Role {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        permissions: serde_json::from_str(&permissions)
            .map_err(|e| AppError::internal(format!("roles.permissions is not a JSON string array: {e}")))?,
        is_system: row.try_get::<i64, _>("isSystem")? != 0,
        created_at: row.try_get("createdAt")?,
        updated_at: row.try_get("updatedAt")?,
    })
}

pub async fn find_all(db: &Db) -> AppResult<Vec<Role>> {
    let rows = sqlx::query(&format!(r#"SELECT {COLUMNS} FROM "roles" ORDER BY "createdAt" ASC, rowid ASC"#))
        .fetch_all(db.read())
        .await?;
    rows.iter().map(from_row).collect()
}

pub async fn get(conn: &mut SqliteConnection, id: &str) -> AppResult<Option<Role>> {
    let row =
        sqlx::query(&format!(r#"SELECT {COLUMNS} FROM "roles" WHERE "id" = ?1"#)).bind(id).fetch_optional(conn).await?;
    row.as_ref().map(from_row).transpose()
}

/// Reference `findById`: 404 `Role <id> not found`.
pub async fn find_by_id(conn: &mut SqliteConnection, id: &str) -> AppResult<Role> {
    get(conn, id).await?.ok_or_else(|| AppError::NotFound(format!("Role {id} not found")))
}

pub async fn find_by_name(conn: &mut SqliteConnection, name: &str) -> AppResult<Option<Role>> {
    let row = sqlx::query(&format!(r#"SELECT {COLUMNS} FROM "roles" WHERE "name" = ?1"#))
        .bind(name)
        .fetch_optional(conn)
        .await?;
    row.as_ref().map(from_row).transpose()
}

fn permissions_json(p: &[String]) -> AppResult<String> {
    serde_json::to_string(p).map_err(AppError::internal)
}

pub async fn create(db: &Db, name: &str, permissions: &[String]) -> AppResult<Role> {
    let mut tx = db.begin().await?;
    if find_by_name(&mut tx, name).await?.is_some() {
        return Err(AppError::Conflict(format!("Role \"{name}\" already exists")));
    }
    let id = new_id();
    let now = now_iso();
    sqlx::query(
        r#"INSERT INTO "roles" ("id","name","permissions","isSystem","createdAt","updatedAt") VALUES (?1,?2,?3,0,?4,?4)"#,
    )
    .bind(&id)
    .bind(name)
    .bind(permissions_json(permissions)?)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let role = find_by_id(&mut tx, &id).await?;
    tx.commit().await?;
    Ok(role)
}

pub async fn update(db: &Db, id: &str, name: Option<&str>, permissions: Option<&[String]>) -> AppResult<Role> {
    let mut tx = db.begin().await?;
    let mut role = find_by_id(&mut tx, id).await?;
    if role.is_system {
        return Err(AppError::Forbidden("The Admin role cannot be modified".into()));
    }
    if let Some(n) = name {
        if n != role.name && find_by_name(&mut tx, n).await?.is_some() {
            return Err(AppError::Conflict(format!("Role \"{n}\" already exists")));
        }
    }
    let new_name = name.map(str::to_string).unwrap_or_else(|| role.name.clone());
    let new_permissions = permissions.map(<[String]>::to_vec).unwrap_or_else(|| role.permissions.clone());
    // TypeORM only issues an UPDATE (and bumps updatedAt) when something changed.
    if new_name != role.name || new_permissions != role.permissions {
        let now = now_iso();
        sqlx::query(r#"UPDATE "roles" SET "name" = ?1, "permissions" = ?2, "updatedAt" = ?3 WHERE "id" = ?4"#)
            .bind(&new_name)
            .bind(permissions_json(&new_permissions)?)
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        role = find_by_id(&mut tx, id).await?;
    }
    tx.commit().await?;
    Ok(role)
}

pub async fn remove(db: &Db, id: &str) -> AppResult<()> {
    let mut tx = db.begin().await?;
    let role = find_by_id(&mut tx, id).await?;
    if role.is_system {
        return Err(AppError::Forbidden("The Admin role cannot be deleted".into()));
    }
    let assigned: i64 =
        sqlx::query_scalar(r#"SELECT COUNT(*) FROM "users" WHERE "roleId" = ?1"#).bind(id).fetch_one(&mut *tx).await?;
    if assigned > 0 {
        return Err(AppError::Conflict("Cannot delete a role that is assigned to users".into()));
    }
    sqlx::query(r#"DELETE FROM "roles" WHERE "id" = ?1"#).bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
