//! Document number sequences (reference `CounterService`): `JE-000001`, `INV-9000`, …
//!
//! The counter is incremented inside the caller's write transaction, so a posting that fails
//! and rolls back also gives its number back — numbers never skip because of an error (the
//! reference took the number in a separate statement and could leave gaps).

use crate::Tx;
use app_core::AppResult;

/// Atomically increments `key` and returns the new value (1 for a new key).
pub async fn next(tx: &mut Tx, key: &str) -> AppResult<i64> {
    let value: i64 = sqlx::query_scalar(
        r#"INSERT INTO "counters" ("key", "value") VALUES (?1, 1)
           ON CONFLICT ("key") DO UPDATE SET "value" = "counters"."value" + 1
           RETURNING "value""#,
    )
    .bind(key)
    .fetch_one(&mut **tx)
    .await?;
    Ok(value)
}

/// `PREFIX-000001`: JavaScript `String(n).padStart(pad, '0')`, which never truncates.
pub async fn next_formatted(tx: &mut Tx, prefix: &str, pad: usize) -> AppResult<String> {
    let n = next(tx, prefix).await?;
    Ok(format!("{prefix}-{n:0pad$}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Db;
    use app_core::AppError;

    #[tokio::test]
    async fn sequences_start_at_one_and_rollback_returns_the_number() -> AppResult<()> {
        let dir = tempfile::tempdir().map_err(AppError::internal)?;
        let db = Db::open(&dir.path().join("t.db")).await?;

        let mut tx = db.begin().await?;
        assert_eq!(next_formatted(&mut tx, "JE", 6).await?, "JE-000001");
        assert_eq!(next_formatted(&mut tx, "JE", 6).await?, "JE-000002");
        tx.commit().await?;

        let mut tx = db.begin().await?;
        assert_eq!(next_formatted(&mut tx, "JE", 6).await?, "JE-000003");
        tx.rollback().await?;

        let mut tx = db.begin().await?;
        assert_eq!(next_formatted(&mut tx, "JE", 6).await?, "JE-000003");
        // Seeded by the baseline migration: invoices start at INV-9000, 4 digits.
        assert_eq!(next_formatted(&mut tx, "INV", 4).await?, "INV-9000");
        tx.commit().await?;
        Ok(())
    }

    #[tokio::test]
    async fn padding_never_truncates() -> AppResult<()> {
        let dir = tempfile::tempdir().map_err(AppError::internal)?;
        let db = Db::open(&dir.path().join("t.db")).await?;
        sqlx::query(r#"INSERT INTO "counters" ("key","value") VALUES ('PRUN', 99999)"#).execute(db.write()).await?;
        let mut tx = db.begin().await?;
        assert_eq!(next_formatted(&mut tx, "PRUN", 4).await?, "PRUN-100000");
        tx.commit().await?;
        Ok(())
    }
}
