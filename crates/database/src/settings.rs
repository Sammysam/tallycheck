//! Key/value company settings (`settings` table), as the reference `SettingsService` keeps
//! them. Payroll rates and statutory company details live here too under `payroll.*` keys.

use crate::{Db, Tx};
use app_core::time::now_iso;
use app_core::AppResult;
use std::collections::BTreeMap;

/// Fallbacks used when a key has never been saved (reference `SettingsService.DEFAULTS`).
pub const DEFAULTS: &[(&str, &str)] = &[("signatoryName", "Samir Husain"), ("signatoryTitle", "Director")];

pub type SettingsMap = BTreeMap<String, String>;

/// Every stored row merged over the defaults — what `GET /api/settings` returns.
pub async fn find_all(db: &Db) -> AppResult<SettingsMap> {
    let rows: Vec<(String, String)> =
        sqlx::query_as(r#"SELECT "key", "value" FROM "settings""#).fetch_all(db.read()).await?;
    let mut map: SettingsMap = DEFAULTS.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    map.extend(rows);
    Ok(map)
}

pub async fn upsert(tx: &mut Tx, key: &str, value: &str) -> AppResult<()> {
    sqlx::query(
        r#"INSERT INTO "settings" ("key", "value", "updatedAt") VALUES (?1, ?2, ?3)
           ON CONFLICT ("key") DO UPDATE SET "value" = excluded."value", "updatedAt" = excluded."updatedAt""#,
    )
    .bind(key)
    .bind(value)
    .bind(now_iso())
    .execute(&mut **tx)
    .await?;
    Ok(())
}
