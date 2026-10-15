//! Backup, restore and recovery (W6).
//!
//! * A backup is one encrypted `.qbak` file (see [`format`]) holding a consistent snapshot of
//!   the database, every encrypted employee file, the file-encryption key and a manifest with
//!   the SHA-256 of each entry.
//! * Consistency: all writes go through the single writer connection. While a backup holds that
//!   connection it takes `VACUUM INTO` and hard-links the stored files (they are immutable:
//!   written once under a new name, deleted only after a commit). No commit can happen in
//!   between, so every file the snapshot references is captured. Packing and encryption then
//!   run without blocking the app.
//! * A backup is written as `*.qbak.partial`, read back and fully verified, and only then
//!   renamed. An interrupted backup never looks like a finished one; leftovers are removed on
//!   the next run.
//! * Restore verifies everything into a staging folder first. The swap happens at the next
//!   start, before the database opens; the replaced data is kept in `pre-restore-<time>`, and
//!   an interrupted swap resumes on the following start.

pub mod create;
pub mod format;
pub mod keys;
pub mod restore;
pub mod schedule;

use serde::{Deserialize, Serialize};

pub use create::{create_backup, BackupInfo};
pub use keys::{Kek, KeyError};
pub use restore::{apply_pending_restore, stage_restore, verify_backup, BackupKey};

/// Entry names inside the archive.
pub const DB_ENTRY: &str = "database/accounting.db";
pub const STORAGE_KEY_ENTRY: &str = "secrets/storage-key";
pub const MANIFEST_ENTRY: &str = "manifest.json";
pub const FILES_PREFIX: &str = "files/";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestEntry {
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format: u8,
    pub created_at: String,
    pub app_version: String,
    pub schema_version: i64,
    /// First 16 hex digits of SHA-256 of the file-encryption key, to recognise the key.
    pub storage_key_fingerprint: Option<String>,
    pub counts: std::collections::BTreeMap<String, i64>,
    /// Files the database refers to that were not on disk when the backup was taken.
    pub missing_files: Vec<String>,
    pub entries: Vec<ManifestEntry>,
}

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("{0}")]
    Key(#[from] KeyError),
    #[error("{0}")]
    Format(#[from] format::FormatError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("database: {0}")]
    Db(String),
    #[error("verification failed: {0}")]
    Verify(String),
    #[error("{0}")]
    Refused(String),
}

impl From<sqlx::Error> for BackupError {
    fn from(e: sqlx::Error) -> Self {
        BackupError::Db(e.to_string())
    }
}

impl From<app_core::AppError> for BackupError {
    fn from(e: app_core::AppError) -> Self {
        BackupError::Db(e.to_string())
    }
}

impl From<app_core::secrets::SecretError> for BackupError {
    fn from(e: app_core::secrets::SecretError) -> Self {
        BackupError::Key(KeyError::Secret(e))
    }
}

pub type BackupResult<T> = Result<T, BackupError>;

/// Tables counted in the manifest and the restore summary.
pub const COUNTED_TABLES: &[&str] = &[
    "users",
    "accounts",
    "journal_entries",
    "journal_lines",
    "customers",
    "vendors",
    "invoices",
    "bills",
    "expenses",
    "payments",
    "employees",
    "employee_documents",
    "employee_salaries",
    "payroll_runs",
    "payroll_lines",
    "audit_logs",
];

/// The newest migration this build knows; a backup from a newer build cannot be restored.
pub fn latest_schema_version() -> i64 {
    database::MIGRATOR.iter().map(|m| m.version).max().unwrap_or(0)
}

#[cfg(test)]
mod tests;
