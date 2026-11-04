//! D52: ending a user's sign-ins. A signed token cannot be taken back, so each user has a
//! "valid from" time: tokens issued (`iat`) before it are refused. Set when the password or 2FA
//! is reset and when the user is switched off, so a stolen or left-open session ends with it.

use app_core::AppResult;
use database::Db;
use sqlx::SqliteConnection;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Ends every sign-in of the user made before now.
pub async fn end_all_in(conn: &mut SqliteConnection, user_id: &str) -> AppResult<()> {
    sqlx::query(
        r#"INSERT INTO "session_cutoffs" ("userId","validFrom") VALUES (?1,?2)
           ON CONFLICT ("userId") DO UPDATE SET "validFrom" = excluded."validFrom""#,
    )
    .bind(user_id)
    .bind(now_secs())
    .execute(conn)
    .await?;
    Ok(())
}

/// Whether a token issued at `iat` (Unix seconds) for the user is still accepted.
pub async fn still_valid(db: &Db, user_id: &str, iat: u64) -> AppResult<bool> {
    let from: Option<i64> = sqlx::query_scalar(r#"SELECT "validFrom" FROM "session_cutoffs" WHERE "userId" = ?1"#)
        .bind(user_id)
        .fetch_optional(db.read())
        .await?;
    Ok(from.is_none_or(|from| i64::try_from(iat).unwrap_or(i64::MAX) >= from))
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_core::AppError;

    #[tokio::test]
    async fn tokens_issued_before_the_cutoff_are_refused() -> AppResult<()> {
        let dir = tempfile::tempdir().map_err(AppError::internal)?;
        let db = Db::open(&dir.path().join("t.db")).await?;
        let user = crate::users::create_first_admin(&db, "owner", "old-password").await?.expect("test value");
        let now = u64::try_from(now_secs()).expect("test value");
        assert!(still_valid(&db, &user.id, now - 3600).await?, "no cutoff yet");
        let mut tx = db.begin().await?;
        end_all_in(&mut tx, &user.id).await?;
        tx.commit().await?;
        assert!(!still_valid(&db, &user.id, now - 3600).await?, "an hour-old sign-in has ended");
        assert!(still_valid(&db, &user.id, now + 1).await?, "a new sign-in works");
        Ok(())
    }
}
