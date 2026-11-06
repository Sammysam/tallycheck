//! Backup and restore (W6). Only an Administrator sitting at the main PC may use these: the
//! request must come from the loopback interface, so LAN clients can never read backup paths,
//! write backups to arbitrary folders or stage a restore.

use crate::body::JsonBody;
use crate::extract::CurrentUser;
use crate::state::AppState;
use app_core::{AppError, AppResult};
use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::Json;
use backup::restore::{cancel_staged_restore, key_for, staged_manifest, BackupKey};
use backup::{keys, schedule, BackupError, Kek, KeyError};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// An Administrator connected from this PC.
pub struct LocalAdmin(pub CurrentUser);

impl FromRequestParts<AppState> for LocalAdmin {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let current = CurrentUser::from_request_parts(parts, state).await?;
        let local = crate::extract::from_this_pc(parts);
        if !current.user.is_admin() || !local {
            return Err(AppError::Forbidden(
                "Backup and restore are only available to an Administrator on the main PC".into(),
            ));
        }
        Ok(LocalAdmin(current))
    }
}

fn data_dir(s: &AppState) -> AppResult<&Path> {
    s.data_dir.as_deref().ok_or_else(|| AppError::internal("no data directory configured"))
}

fn backup_error(e: BackupError) -> AppError {
    match e {
        BackupError::Key(KeyError::WrongPassword) => {
            AppError::bad_request("Wrong backup password, or the backup file is damaged")
        }
        BackupError::Key(KeyError::NotSet) => AppError::bad_request("Set a backup password first"),
        BackupError::Key(KeyError::TooShort) => AppError::bad_request(format!(
            "The backup password must be at least {} characters",
            keys::MIN_PASSWORD_CHARS
        )),
        BackupError::Format(f) => AppError::bad_request(f.to_string()),
        BackupError::Verify(m) => AppError::bad_request(format!("The backup failed verification: {m}")),
        BackupError::Refused(m) => AppError::bad_request(m),
        BackupError::Io(e) if e.kind() == std::io::ErrorKind::NotFound => {
            AppError::bad_request("File or folder not found")
        }
        BackupError::Io(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            AppError::bad_request("Windows refused access to that file or folder")
        }
        other => AppError::internal(other),
    }
}

fn text(body: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    body.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// A folder or file on another computer (`\\server\share`, `//server/share`, `\\?\UNC\…`,
/// device paths). Opening one makes Windows sign in to that computer with this user's
/// credentials, so backups and restores use local (or mapped) drives only.
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

fn local_only(path: &str) -> AppResult<()> {
    if is_unc_path(path) {
        return Err(AppError::bad_request(
            "Choose a folder on this PC (a drive letter such as C: or D:), not a network address",
        ));
    }
    Ok(())
}

/// `Documents\Cenango Backups` of the user running the app (`USERPROFILE` on Windows, `HOME`
/// on macOS, D53).
pub fn default_folder() -> Option<String> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).filter(|v| !v.is_empty())?;
    Some(PathBuf::from(home).join("Documents").join("Cenango Backups").to_string_lossy().to_string())
}

pub async fn status(State(s): State<AppState>, _a: LocalAdmin) -> AppResult<Json<Value>> {
    let dir = data_dir(&s)?;
    let password_set = keys::load(&dir.join("secrets")).map_err(|e| backup_error(e.into()))?.is_some();
    Ok(Json(json!({
        "passwordSet": password_set,
        "settings": schedule::load_settings(dir),
        "status": schedule::load_status(dir),
        "stagedRestore": staged_manifest(dir),
        "defaultFolder": default_folder(),
        "dataFolder": dir.to_string_lossy(),
    })))
}

/// Sets or changes the backup password. Changing it needs the current one; backups made
/// earlier still open with the password they were made with.
pub async fn set_password(State(s): State<AppState>, _a: LocalAdmin, body: JsonBody) -> AppResult<StatusCode> {
    let dir = data_dir(&s)?;
    let b = body.object();
    let secrets = dir.join("secrets");
    let existing = keys::load(&secrets).map_err(|e| backup_error(e.into()))?;
    if existing.is_some() {
        let current = text(&b, "currentPassword").unwrap_or_default();
        if !keys::verify(&secrets, &current).map_err(|e| backup_error(e.into()))? {
            return Err(AppError::bad_request("The current backup password is not correct"));
        }
    }
    let password = b.get("password").and_then(Value::as_str).unwrap_or_default().to_string();
    let kek = Kek::from_new_password(&password).map_err(|e| backup_error(e.into()))?;
    keys::save(&secrets, &kek).map_err(|e| backup_error(e.into()))?;
    tracing::info!("backup password set");
    Ok(StatusCode::NO_CONTENT)
}

pub async fn update_settings(State(s): State<AppState>, _a: LocalAdmin, body: JsonBody) -> AppResult<Json<Value>> {
    let dir = data_dir(&s)?;
    let b = body.object();
    let mut settings = schedule::load_settings(dir);
    if let Some(v) = b.get("enabled") {
        settings.enabled =
            v.as_bool().ok_or_else(|| AppError::validation(vec!["enabled must be a boolean value".into()]))?;
    }
    if b.contains_key("folder") {
        settings.folder = text(&b, "folder");
        if let Some(f) = &settings.folder {
            local_only(f)?;
        }
    }
    if let Some(v) = b.get("keep") {
        let keep = v.as_u64().filter(|k| (1..=365).contains(k));
        settings.keep =
            keep.ok_or_else(|| AppError::validation(vec!["keep must be a whole number from 1 to 365".into()]))? as u32;
    }
    if settings.enabled {
        let folder =
            settings.folder.clone().ok_or_else(|| AppError::bad_request("Choose a folder for the daily backups"))?;
        std::fs::create_dir_all(&folder).map_err(|e| AppError::bad_request(format!("Cannot use that folder: {e}")))?;
    }
    schedule::save_settings(dir, &settings).map_err(AppError::internal)?;
    Ok(Json(json!(settings)))
}

pub async fn run(State(s): State<AppState>, _a: LocalAdmin, body: JsonBody) -> AppResult<(StatusCode, Json<Value>)> {
    let dir = data_dir(&s)?.to_path_buf();
    let folder =
        text(&body.object(), "folder").ok_or_else(|| AppError::bad_request("Choose a folder for the backup"))?;
    local_only(&folder)?;
    let kek = keys::load(&dir.join("secrets"))
        .map_err(|e| backup_error(e.into()))?
        .ok_or_else(|| backup_error(KeyError::NotSet.into()))?;
    let outcome = backup::create_backup(&s.db, &dir, Path::new(&folder), &kek).await;
    schedule::record(&dir, &outcome.as_ref().map(Clone::clone).map_err(ToString::to_string));
    let info = outcome.map_err(backup_error)?;
    tracing::info!(file = %info.file, "backup written and verified");
    Ok((StatusCode::CREATED, Json(json!(info))))
}

fn restore_key(s: &AppState, body: &serde_json::Map<String, Value>) -> AppResult<(PathBuf, BackupKey)> {
    let dir = data_dir(s)?;
    let file = text(body, "file").ok_or_else(|| AppError::bad_request("Choose a backup file"))?;
    local_only(&file)?;
    let file = PathBuf::from(file);
    let key = key_for(&file, &dir.join("secrets"), text(body, "password")).map_err(|e| match e {
        BackupError::Key(KeyError::WrongPassword) => {
            AppError::bad_request("This backup was made with a different backup password; enter that password")
        }
        other => backup_error(other),
    })?;
    Ok((file, key))
}

/// Checks a backup completely without changing anything.
pub async fn verify(State(s): State<AppState>, _a: LocalAdmin, body: JsonBody) -> AppResult<Json<Value>> {
    let (file, key) = restore_key(&s, &body.object())?;
    let manifest = backup::verify_backup(&file, &key).await.map_err(backup_error)?;
    Ok(Json(json!(manifest)))
}

/// Verifies and unpacks a backup; it replaces the live data when the app next starts.
pub async fn stage(State(s): State<AppState>, _a: LocalAdmin, body: JsonBody) -> AppResult<(StatusCode, Json<Value>)> {
    let b = body.object();
    if b.get("confirm").and_then(Value::as_str) != Some("RESTORE") {
        return Err(AppError::bad_request("Type RESTORE to confirm that the current data will be replaced"));
    }
    let (file, key) = restore_key(&s, &b)?;
    let dir = data_dir(&s)?;
    let manifest = backup::stage_restore(&file, &key, dir).await.map_err(backup_error)?;
    tracing::warn!(file = %file.display(), "restore staged; it is applied when the app restarts");
    Ok((StatusCode::CREATED, Json(json!({ "manifest": manifest, "restartRequired": true }))))
}

pub async fn cancel(State(s): State<AppState>, _a: LocalAdmin) -> AppResult<StatusCode> {
    cancel_staged_restore(data_dir(&s)?).map_err(AppError::internal)?;
    Ok(StatusCode::NO_CONTENT)
}

/// The backup and restore page (plain HTML; it uses the token the app already holds).
/// D36: backup and restore now live in Settings → Backup & restore; the old address forwards.
pub async fn page() -> axum::response::Redirect {
    axum::response::Redirect::to("/settings?tab=backup")
}

#[cfg(test)]
mod tests {
    use super::is_unc_path;

    #[test]
    fn network_paths_are_recognised() {
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
        for local in [r"C:\Users\me\Documents\Backups", r"D:\backup.cbk", r"\\?\C:\Backups"] {
            assert!(!is_unc_path(local), "{local}");
        }
    }
}
