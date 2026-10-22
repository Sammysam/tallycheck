//! D41: keys for Claude (the MCP connector) and other programs on this PC.
//!
//! A key acts as the user who created it — that user's role and permissions, and nothing when the
//! user is deactivated. It is read-only unless `canWrite`, and even then may only create drafts
//! (enforced where requests are authenticated). The key is `cak_` + 40 hex characters; only its
//! SHA-256 is stored, and it is shown once. Revoking is immediate and final.

use app_core::ids::new_id;
use app_core::time::now_iso;
use app_core::{AppError, AppResult};
use database::Db;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::Row;

pub const PREFIX: &str = "cak_";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKey {
    pub id: String,
    pub name: String,
    pub user_id: String,
    pub username: Option<String>,
    /// The first characters, to tell keys apart ("cak_3f9a…").
    pub prefix: String,
    pub can_write: bool,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub revoked_at: Option<String>,
}

pub fn hash(key: &str) -> String {
    Sha256::digest(key.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

pub fn looks_like_key(token: &str) -> bool {
    token.starts_with(PREFIX)
}

const SELECT: &str = r#"SELECT k."id", k."name", k."userId", u."username", k."prefix", k."canWrite", k."createdAt", k."lastUsedAt",
       k."revokedAt" FROM "api_keys" k LEFT JOIN "users" u ON u."id" = k."userId""#;

fn from_row(r: &sqlx::sqlite::SqliteRow) -> AppResult<ApiKey> {
    Ok(ApiKey {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        user_id: r.try_get("userId")?,
        username: r.try_get("username")?,
        prefix: r.try_get("prefix")?,
        can_write: r.try_get::<i64, _>("canWrite")? == 1,
        created_at: r.try_get("createdAt")?,
        last_used_at: r.try_get("lastUsedAt")?,
        revoked_at: r.try_get("revokedAt")?,
    })
}

pub async fn list(db: &Db) -> AppResult<Vec<ApiKey>> {
    let rows = sqlx::query(&format!(r#"{SELECT} ORDER BY k."revokedAt" IS NOT NULL, k."createdAt" DESC"#))
        .fetch_all(db.read())
        .await?;
    rows.iter().map(from_row).collect()
}

async fn find(db: &Db, id: &str) -> AppResult<ApiKey> {
    let row = sqlx::query(&format!(r#"{SELECT} WHERE k."id" = ?1"#)).bind(id).fetch_optional(db.read()).await?;
    row.as_ref().map(from_row).transpose()?.ok_or_else(|| AppError::NotFound(format!("Key with ID {id} not found")))
}

/// Creates a key for `user_id`; returns it with the key text (shown once).
pub async fn create(db: &Db, user_id: &str, name: &str, can_write: bool) -> AppResult<(ApiKey, String)> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(AppError::bad_request("Name the key, e.g. \"Claude on the office PC\""));
    }
    let random = app_core::secrets::random_bytes(20).map_err(|e| AppError::internal(e.to_string()))?;
    let key = format!("{PREFIX}{}", random.iter().map(|b| format!("{b:02x}")).collect::<String>());
    let id = new_id();
    sqlx::query(
        r#"INSERT INTO "api_keys" ("id","name","userId","prefix","keyHash","canWrite","createdAt") VALUES (?1,?2,?3,?4,?5,?6,?7)"#,
    )
    .bind(&id)
    .bind(name)
    .bind(user_id)
    .bind(&key[..12])
    .bind(hash(&key))
    .bind(can_write)
    .bind(now_iso())
    .execute(db.write())
    .await?;
    Ok((find(db, &id).await?, key))
}

pub async fn update(db: &Db, id: &str, name: Option<&str>, can_write: Option<bool>) -> AppResult<ApiKey> {
    let k = find(db, id).await?;
    if k.revoked_at.is_some() {
        return Err(AppError::bad_request("This key was revoked"));
    }
    let name = name.map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&k.name).to_string();
    sqlx::query(r#"UPDATE "api_keys" SET "name" = ?1, "canWrite" = ?2 WHERE "id" = ?3"#)
        .bind(&name)
        .bind(can_write.unwrap_or(k.can_write))
        .bind(id)
        .execute(db.write())
        .await?;
    find(db, id).await
}

pub async fn revoke(db: &Db, id: &str) -> AppResult<ApiKey> {
    find(db, id).await?;
    sqlx::query(r#"UPDATE "api_keys" SET "revokedAt" = COALESCE("revokedAt", ?1) WHERE "id" = ?2"#)
        .bind(now_iso())
        .bind(id)
        .execute(db.write())
        .await?;
    find(db, id).await
}

/// The live key with this text, noting when it was used (at most once a minute).
pub async fn verify(db: &Db, key: &str) -> AppResult<Option<ApiKey>> {
    if !looks_like_key(key) {
        return Ok(None);
    }
    let row = sqlx::query(&format!(r#"{SELECT} WHERE k."keyHash" = ?1 AND k."revokedAt" IS NULL"#))
        .bind(hash(key))
        .fetch_optional(db.read())
        .await?;
    let Some(k) = row.as_ref().map(from_row).transpose()? else { return Ok(None) };
    let now = now_iso();
    let a_minute_ago =
        (chrono::Utc::now() - chrono::Duration::seconds(60)).format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    if let Err(e) = sqlx::query(
        r#"UPDATE "api_keys" SET "lastUsedAt" = ?1 WHERE "id" = ?2 AND ("lastUsedAt" IS NULL OR "lastUsedAt" < ?3)"#,
    )
    .bind(&now)
    .bind(&k.id)
    .bind(&a_minute_ago)
    .execute(db.write())
    .await
    {
        tracing::warn!("could not note a key's use: {e}");
    }
    Ok(Some(k))
}

/// What a write key may do: create drafts (and the customers / vendors they need) — never post,
/// pay, void, delete or change settings.
pub fn draft_only_allows(method: &str, path: &str) -> bool {
    let path = path.split('?').next().unwrap_or(path).trim_end_matches('/');
    method == "POST" && matches!(path, "/api/invoices" | "/api/bills" | "/api/customers" | "/api/vendors")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_keys_only_create_drafts() {
        assert!(draft_only_allows("POST", "/api/invoices"));
        assert!(draft_only_allows("POST", "/api/bills/"));
        assert!(!draft_only_allows("POST", "/api/invoices/1/post"));
        assert!(!draft_only_allows("PATCH", "/api/invoices/1"));
        assert!(!draft_only_allows("POST", "/api/payments"));
        assert!(!draft_only_allows("DELETE", "/api/customers/1"));
        assert_eq!(hash("cak_x").len(), 64);
    }
}
