//! Daily backups: settings in `<data_dir>/backup-settings.json`, the outcome of the last
//! attempt in `<data_dir>/backup-status.json`.
//!
//! An office PC is not always on at a fixed hour, so "daily" means: whenever the app is
//! running, check every hour and back up if the last successful backup is more than a day old
//! (and right after start if one is overdue). Old scheduled backups beyond `keep` are deleted,
//! newest kept; files made by hand are never deleted.

use crate::create::{create_backup, BackupInfo, EXTENSION, FILE_PREFIX};
use crate::keys;
use crate::BackupResult;
use database::Db;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const SETTINGS: &str = "backup-settings.json";
const STATUS: &str = "backup-status.json";
pub const SCHEDULED_DIR: &str = "scheduled";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub enabled: bool,
    /// Folder for scheduled backups (they go into its `scheduled` subfolder).
    pub folder: Option<String>,
    /// How many scheduled backups to keep.
    pub keep: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { enabled: false, folder: None, keep: 14 }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub last_attempt_at: Option<String>,
    pub last_success_at: Option<String>,
    pub last_file: Option<String>,
    pub last_error: Option<String>,
}

fn read_json<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> T {
    fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?)?;
    fs::rename(tmp, path)
}

pub fn load_settings(data_dir: &Path) -> Settings {
    read_json(&data_dir.join(SETTINGS))
}

pub fn save_settings(data_dir: &Path, s: &Settings) -> std::io::Result<()> {
    write_json(&data_dir.join(SETTINGS), s)
}

pub fn load_status(data_dir: &Path) -> Status {
    read_json(&data_dir.join(STATUS))
}

fn save_status(data_dir: &Path, s: &Status) {
    if let Err(e) = write_json(&data_dir.join(STATUS), s) {
        tracing::error!("could not record the backup status: {e}");
    }
}

/// Records the outcome of a backup (manual or scheduled) for the status page.
pub fn record(data_dir: &Path, outcome: &Result<BackupInfo, String>) {
    let mut s = load_status(data_dir);
    let now = app_core::time::now_iso();
    s.last_attempt_at = Some(now.clone());
    match outcome {
        Ok(info) => {
            s.last_success_at = Some(now);
            s.last_file = Some(info.file.clone());
            s.last_error = None;
        }
        Err(e) => s.last_error = Some(e.clone()),
    }
    save_status(data_dir, &s);
}

fn older_than_a_day(iso: &Option<String>) -> bool {
    let Some(t) = iso.as_deref().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()) else { return true };
    chrono::Utc::now().signed_duration_since(t) > chrono::Duration::hours(24)
}

/// Deletes the oldest scheduled backups beyond `keep`.
pub fn prune(dir: &Path, keep: u32) -> std::io::Result<Vec<PathBuf>> {
    let mut found: Vec<PathBuf> = fs::read_dir(dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.starts_with(FILE_PREFIX) && name.ends_with(&format!(".{EXTENSION}"))
        })
        .collect();
    // Names carry a sortable UTC timestamp.
    found.sort();
    let excess = found.len().saturating_sub(keep.max(1) as usize);
    let removed: Vec<PathBuf> = found.into_iter().take(excess).collect();
    for p in &removed {
        fs::remove_file(p)?;
    }
    Ok(removed)
}

/// True for a network address (`\\server\share`, `//server/share`, `\\?\UNC\…`, device paths).
/// A copy of `api-server`'s check, which refuses such folders in new settings; this one also
/// keeps a folder saved before that check from being used by the schedule.
pub fn is_unc_path(text: &str) -> bool {
    let t = text.trim();
    let b = t.as_bytes();
    let sep = |c: u8| c == b'\\' || c == b'/';
    if b.len() >= 2 && sep(b[0]) && sep(b[1]) {
        // `\\?\C:\…` is a local drive written the long way.
        let verbatim_local = b.len() >= 6 && b[2] == b'?' && sep(b[3]) && b[4].is_ascii_alphabetic() && b[5] == b':';
        return !verbatim_local;
    }
    matches!(
        Path::new(t).components().next(),
        Some(std::path::Component::Prefix(p))
            if !matches!(p.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_))
    )
}

/// What Settings → Backup shows when the saved folder is a network address.
pub const NETWORK_FOLDER_REFUSED: &str = "Scheduled backup skipped: the backup folder is a network address. \
     Choose a folder on this PC (a drive letter such as C: or D:) under Settings → Backup";

/// Runs a scheduled backup if one is due. Returns `None` when nothing was due.
pub async fn run_if_due(db: &Db, data_dir: &Path) -> Option<BackupResult<BackupInfo>> {
    let settings = load_settings(data_dir);
    let folder = settings.folder.clone().filter(|f| !f.trim().is_empty())?;
    if !settings.enabled || !older_than_a_day(&load_status(data_dir).last_success_at) {
        return None;
    }
    if is_unc_path(&folder) {
        // The folder itself is not logged: it may name a server or share.
        tracing::warn!("scheduled backup skipped: the saved backup folder is a network address");
        record(data_dir, &Err(NETWORK_FOLDER_REFUSED.to_string()));
        return Some(Err(crate::BackupError::Refused(NETWORK_FOLDER_REFUSED.to_string())));
    }
    let kek = match keys::load(&data_dir.join("secrets")) {
        Ok(Some(k)) => k,
        Ok(None) => {
            let msg = "Scheduled backup skipped: no backup password has been set".to_string();
            record(data_dir, &Err(msg.clone()));
            return Some(Err(crate::BackupError::Key(keys::KeyError::NotSet)));
        }
        Err(e) => return Some(Err(e.into())),
    };
    let dest = PathBuf::from(folder).join(SCHEDULED_DIR);
    let result = create_backup(db, data_dir, &dest, &kek).await;
    record(data_dir, &result.as_ref().map(Clone::clone).map_err(ToString::to_string));
    if result.is_ok() {
        if let Err(e) = prune(&dest, settings.keep) {
            tracing::warn!("could not remove old scheduled backups: {e}");
        }
    }
    Some(result)
}

/// The background loop: shortly after start, then every hour.
pub fn spawn(db: Db, data_dir: PathBuf) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(60)).await;
        loop {
            match run_if_due(&db, &data_dir).await {
                Some(Ok(info)) => tracing::info!(file = %info.file, "scheduled backup written and verified"),
                Some(Err(e)) => tracing::error!("scheduled backup failed: {e}"),
                None => {}
            }
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    })
}

#[cfg(test)]
mod unc_tests {
    use super::*;

    #[test]
    fn network_folders_are_recognised() {
        for unc in [
            r"\\server\share\backups",
            "//server/share",
            r"\\?\UNC\server\share",
            r"\\.\pipe\x",
            r"\/server/share",
            r"  \\10.0.0.5\c$",
        ] {
            assert!(is_unc_path(unc), "{unc}");
        }
        for local in [r"C:\Users\me\Documents\Backups", r"D:\backups", r"\\?\C:\Backups"] {
            assert!(!is_unc_path(local), "{local}");
        }
    }

    #[tokio::test]
    async fn a_network_folder_saved_earlier_is_not_used() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db = Db::open(&dir.path().join("accounting.db")).await.expect("db");
        save_settings(dir.path(), &Settings { enabled: true, folder: Some(r"\\server\share".into()), keep: 3 })
            .expect("settings");
        match run_if_due(&db, dir.path()).await {
            Some(Err(crate::BackupError::Refused(m))) => assert_eq!(m, NETWORK_FOLDER_REFUSED),
            other => panic!("expected a refusal, got {other:?}"),
        }
        let status = load_status(dir.path());
        assert_eq!(status.last_error.as_deref(), Some(NETWORK_FOLDER_REFUSED));
        assert!(status.last_attempt_at.is_some() && status.last_success_at.is_none());
    }
}
