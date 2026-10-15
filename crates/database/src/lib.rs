//! SQLite access: connection pools, per-connection pragmas, migrations and the settings
//! repository.
//!
//! Concurrency model (one office PC serving LAN clients through the API):
//! * one **writer** connection — every write goes through it, so writes are serialised inside
//!   the process and a business document plus its journal entry commit atomically;
//! * a pool of **read-only** connections — WAL lets readers run alongside the writer.
//!
//! Every connection gets `foreign_keys=ON`, `journal_mode=WAL`, `synchronous=FULL` and a
//! 5-second busy timeout. `verify_pragmas` proves it in tests and at startup.

pub mod counters;
pub mod settings;

use app_core::{AppError, AppResult};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::path::Path;
use std::time::Duration;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations/sqlite");

const READ_POOL_SIZE: u32 = 4;

#[derive(Clone, Debug)]
pub struct Db {
    writer: SqlitePool,
    reader: SqlitePool,
}

pub type Tx = Transaction<'static, Sqlite>;

fn base_options(path: &Path) -> SqliteConnectOptions {
    SqliteConnectOptions::new()
        .filename(path)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Full)
        .busy_timeout(Duration::from_secs(5))
}

impl Db {
    /// Opens (creating if needed) the database at `path` and applies pending migrations.
    pub async fn open(path: &Path) -> AppResult<Db> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(AppError::internal)?;
        }
        let writer = SqlitePoolOptions::new()
            .max_connections(1)
            .min_connections(1)
            .connect_with(base_options(path).create_if_missing(true))
            .await?;
        MIGRATOR.run(&writer).await?;

        let reader = SqlitePoolOptions::new()
            .max_connections(READ_POOL_SIZE)
            .connect_with(base_options(path).read_only(true))
            .await?;

        let db = Db { writer, reader };
        db.verify_pragmas().await?;
        Ok(db)
    }

    /// Pool for queries that never write.
    pub fn read(&self) -> &SqlitePool {
        &self.reader
    }

    /// Starts a write transaction on the single writer connection.
    pub async fn begin(&self) -> AppResult<Tx> {
        Ok(self.writer.begin().await?)
    }

    /// The writer pool, for single-statement writes that need no transaction.
    pub fn write(&self) -> &SqlitePool {
        &self.writer
    }

    /// Fails unless both pools enforce foreign keys and use WAL.
    pub async fn verify_pragmas(&self) -> AppResult<()> {
        for (name, pool) in [("writer", &self.writer), ("reader", &self.reader)] {
            let fk: i64 = sqlx::query_scalar("PRAGMA foreign_keys").fetch_one(pool).await?;
            let mode: String = sqlx::query_scalar("PRAGMA journal_mode").fetch_one(pool).await?;
            if fk != 1 || !mode.eq_ignore_ascii_case("wal") {
                return Err(AppError::internal(format!(
                    "{name} connection pragmas wrong: foreign_keys={fk}, journal_mode={mode}"
                )));
            }
        }
        Ok(())
    }

    /// If `path` holds a database that this build's migrations would upgrade, copies it to
    /// `dest_dir` first (a consistent `VACUUM INTO` snapshot) and returns the copy's path.
    /// An installer update therefore never changes the only copy of the data.
    pub async fn snapshot_before_upgrade(path: &Path, dest_dir: &Path) -> AppResult<Option<std::path::PathBuf>> {
        use sqlx::{ConnectOptions, Connection};
        if !path.exists() {
            return Ok(None);
        }
        let mut conn = base_options(path).connect().await?;
        let has_table: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='_sqlx_migrations'")
                .fetch_one(&mut conn)
                .await?;
        if has_table == 0 {
            conn.close().await?;
            return Ok(None);
        }
        let applied: Vec<i64> = sqlx::query_scalar("SELECT version FROM _sqlx_migrations").fetch_all(&mut conn).await?;
        let pending = MIGRATOR.iter().any(|m| !applied.contains(&m.version));
        if applied.is_empty() || !pending {
            conn.close().await?;
            return Ok(None);
        }
        std::fs::create_dir_all(dest_dir).map_err(AppError::internal)?;
        let from = applied.iter().max().copied().unwrap_or(0);
        let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        let dest = dest_dir.join(format!("accounting-v{from}-{stamp}.db"));
        sqlx::query("VACUUM INTO ?1").bind(dest.to_string_lossy().to_string()).execute(&mut conn).await?;
        conn.close().await?;
        Ok(Some(dest))
    }

    pub async fn close(&self) {
        self.reader.close().await;
        self.writer.close().await;
    }
}

#[cfg(test)]
mod tests;
