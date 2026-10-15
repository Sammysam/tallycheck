//! Verifying and restoring a backup.

use crate::format::{read_header, sha256_hex, unhex, ArchiveReader, DecryptReader, Header};
use crate::keys::{self, Kek, KeyError};
use crate::{latest_schema_version, BackupError, BackupResult, Manifest, DB_ENTRY, MANIFEST_ENTRY, STORAGE_KEY_ENTRY};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Connection};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, BufReader, Write};
use std::path::{Component, Path, PathBuf};

/// How to open a backup: the key saved on this PC, or the backup password.
#[derive(Debug, Clone)]
pub enum BackupKey {
    Kek(Kek),
    Password(String),
}

pub const STAGING_DIR: &str = "restore-staging";
const READY: &str = "READY";
const APPLYING: &str = "APPLYING";

fn open(path: &Path, key: &BackupKey) -> BackupResult<(Header, ArchiveReader<DecryptReader<BufReader<File>>>)> {
    let mut f = BufReader::new(File::open(path)?);
    let (header, json) = read_header(&mut f)?;
    let kek = match key {
        BackupKey::Kek(k) => k.clone(),
        BackupKey::Password(p) => Kek::from_password_for(p, &header.key_slot)?,
    };
    let data_key = kek.unwrap(&header.key_slot)?;
    let prefix: [u8; 7] = unhex(&header.nonce_prefix)
        .and_then(|p| p.try_into().ok())
        .ok_or_else(|| BackupError::Verify("bad nonce prefix".into()))?;
    Ok((header, ArchiveReader::new(DecryptReader::new(f, &data_key, prefix, &json))))
}

/// Reads the header only (no key needed): when and by which version a backup was made.
pub fn peek(path: &Path) -> BackupResult<Header> {
    Ok(read_header(&mut BufReader::new(File::open(path)?))?.0)
}

/// Picks the key for `path`: the saved key if it made this backup, otherwise the password.
pub fn key_for(path: &Path, secrets: &Path, password: Option<String>) -> BackupResult<BackupKey> {
    if let Some(p) = password.filter(|p| !p.is_empty()) {
        return Ok(BackupKey::Password(p));
    }
    let header = peek(path)?;
    match keys::load(secrets)? {
        Some(k) if k.params.salt == header.key_slot.salt => Ok(BackupKey::Kek(k)),
        _ => Err(BackupError::Key(KeyError::WrongPassword)),
    }
}

/// A key inside the archive must be a plain relative path (no `..`, no drive, no root).
fn safe_relative(name: &str) -> Option<PathBuf> {
    let p = Path::new(name);
    let ok = !name.is_empty() && !name.contains('\\') && p.components().all(|c| matches!(c, Component::Normal(_)));
    ok.then(|| p.to_path_buf())
}

struct Sink<'a> {
    staging: Option<&'a Path>,
}

impl Sink<'_> {
    fn writer(&self, name: &str) -> BackupResult<Box<dyn Write>> {
        let Some(root) = self.staging else { return Ok(Box::new(io::sink())) };
        if name == STORAGE_KEY_ENTRY {
            // Never written raw: collected in memory and stored DPAPI-protected by the caller.
            return Ok(Box::new(io::sink()));
        }
        let rel = safe_relative(name).ok_or_else(|| BackupError::Verify(format!("unsafe entry name {name:?}")))?;
        let path = root.join(rel);
        if let Some(d) = path.parent() {
            fs::create_dir_all(d)?;
        }
        Ok(Box::new(io::BufWriter::new(File::create(path)?)))
    }
}

/// Reads every entry, checks each SHA-256 against the manifest and returns the manifest
/// (plus the storage key bytes, if any). With `staging`, entries are written there.
fn read_all(path: &Path, key: &BackupKey, staging: Option<&Path>) -> BackupResult<(Header, Manifest, Option<Vec<u8>>)> {
    let (header, mut archive) = open(path, key)?;
    let sink = Sink { staging };
    let mut seen: BTreeMap<String, (u64, String)> = BTreeMap::new();
    let mut manifest: Option<Manifest> = None;
    let mut storage_key = None;
    while let Some((name, size)) = archive.next_entry().map_err(|e| BackupError::Verify(e.to_string()))? {
        if name == MANIFEST_ENTRY {
            let mut json = Vec::new();
            archive.copy_entry(size, &mut json).map_err(|e| BackupError::Verify(e.to_string()))?;
            manifest = Some(serde_json::from_slice(&json).map_err(|e| BackupError::Verify(format!("manifest: {e}")))?);
            continue;
        }
        if name == STORAGE_KEY_ENTRY {
            let mut k = Vec::new();
            let sha = archive.copy_entry(size, &mut k).map_err(|e| BackupError::Verify(e.to_string()))?;
            seen.insert(name, (size, sha));
            storage_key = Some(k);
            continue;
        }
        let mut w = sink.writer(&name)?;
        let sha = archive.copy_entry(size, &mut w).map_err(|e| BackupError::Verify(e.to_string()))?;
        w.flush()?;
        seen.insert(name, (size, sha));
    }
    let manifest = manifest.ok_or_else(|| BackupError::Verify("the backup has no manifest".into()))?;
    if manifest.entries.len() != seen.len() {
        return Err(BackupError::Verify(format!(
            "manifest lists {} entries, the file holds {}",
            manifest.entries.len(),
            seen.len()
        )));
    }
    for e in &manifest.entries {
        match seen.get(&e.name) {
            Some((size, sha)) if *size == e.size && *sha == e.sha256 => {}
            _ => return Err(BackupError::Verify(format!("{} does not match the manifest", e.name))),
        }
    }
    if !seen.contains_key(DB_ENTRY) {
        return Err(BackupError::Verify("the backup holds no database".into()));
    }
    if let (Some(k), Some(fp)) = (&storage_key, &manifest.storage_key_fingerprint) {
        if &sha256_hex(k)[..16] != fp.as_str() {
            return Err(BackupError::Verify("file-encryption key does not match its fingerprint".into()));
        }
    }
    Ok((header, manifest, storage_key))
}

/// Decrypts and checks the whole backup without writing anything.
pub async fn verify_backup(path: &Path, key: &BackupKey) -> BackupResult<Manifest> {
    let (path, key) = (path.to_path_buf(), key.clone());
    tokio::task::spawn_blocking(move || read_all(&path, &key, None).map(|(_, m, _)| m))
        .await
        .map_err(|e| BackupError::Refused(format!("verification task failed: {e}")))?
}

async fn check_database(path: &Path) -> BackupResult<()> {
    let mut conn = SqliteConnectOptions::new().filename(path).read_only(true).connect().await?;
    let ok: String = sqlx::query_scalar("PRAGMA integrity_check").fetch_one(&mut conn).await?;
    let fk: Vec<(String,)> =
        sqlx::query_as("SELECT \"table\" FROM pragma_foreign_key_check").fetch_all(&mut conn).await?;
    conn.close().await?;
    if ok != "ok" {
        return Err(BackupError::Verify(format!("database integrity check: {ok}")));
    }
    if !fk.is_empty() {
        return Err(BackupError::Verify(format!("{} foreign key problems in the database", fk.len())));
    }
    Ok(())
}

/// Verifies `path` and unpacks it into `<data_dir>/restore-staging`, ready to be swapped in at
/// the next start. Nothing live is touched.
pub async fn stage_restore(path: &Path, key: &BackupKey, data_dir: &Path) -> BackupResult<Manifest> {
    let staging = data_dir.join(STAGING_DIR);
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging)?;
    let result = async {
        let (p, k, s) = (path.to_path_buf(), key.clone(), staging.clone());
        let (header, manifest, storage_key) =
            tokio::task::spawn_blocking(move || read_all(&p, &k, Some(&s)))
                .await
                .map_err(|e| BackupError::Refused(format!("restore task failed: {e}")))??;
        if header.schema_version > latest_schema_version() {
            return Err(BackupError::Refused(format!(
                "this backup was made by a newer version of the app (database version {}); update the app first",
                header.schema_version
            )));
        }
        let db = staging.join(DB_ENTRY);
        check_database(&db).await?;
        fs::rename(&db, staging.join("accounting.db"))?;
        let _ = fs::remove_dir(staging.join("database"));
        if let Some(k) = storage_key {
            app_core::secrets::store(&staging.join("secrets").join(app_core::secrets::STORAGE_KEY_FILE), &k)?;
        }
        let manifest_json = serde_json::to_vec_pretty(&manifest).map_err(|e| BackupError::Verify(e.to_string()))?;
        fs::write(staging.join("manifest.json"), manifest_json)?;
        fs::write(staging.join(READY), path.to_string_lossy().as_bytes())?;
        Ok(manifest)
    }
    .await;
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

/// Drops a staged restore that has not been applied yet.
pub fn cancel_staged_restore(data_dir: &Path) -> io::Result<bool> {
    let staging = data_dir.join(STAGING_DIR);
    if staging.join(READY).exists() && !staging.join(APPLYING).exists() {
        fs::remove_dir_all(staging)?;
        return Ok(true);
    }
    Ok(false)
}

pub fn staged_manifest(data_dir: &Path) -> Option<Manifest> {
    let staging = data_dir.join(STAGING_DIR);
    if !staging.join(READY).exists() {
        return None;
    }
    serde_json::from_slice(&fs::read(staging.join("manifest.json")).ok()?).ok()
}

/// Items swapped by a restore, in order. WAL files of the old database go first and have no
/// staged counterpart.
fn items() -> Vec<String> {
    vec![
        "accounting.db-wal".into(),
        "accounting.db-shm".into(),
        "accounting.db".into(),
        "files".into(),
        format!("secrets/{}", app_core::secrets::STORAGE_KEY_FILE),
    ]
}

/// Swaps a staged restore in. Must run before the database is opened. Safe to call when
/// nothing is staged, and resumes a swap that was interrupted. Returns the folder holding the
/// replaced data, if a restore was applied.
pub fn apply_pending_restore(data_dir: &Path) -> io::Result<Option<PathBuf>> {
    let staging = data_dir.join(STAGING_DIR);
    if !staging.join(READY).exists() {
        return Ok(None);
    }
    let marker = staging.join(APPLYING);
    let previous = match fs::read_to_string(&marker) {
        Ok(dir) => data_dir.join(dir.trim()),
        Err(_) => {
            let name = format!("pre-restore-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S"));
            fs::write(&marker, &name)?;
            data_dir.join(name)
        }
    };
    fs::create_dir_all(previous.join("secrets"))?;
    for item in items() {
        let live = data_dir.join(&item);
        let staged = staging.join(&item);
        let kept = previous.join(&item);
        let has_staged = staged.exists();
        if live.exists() && !kept.exists() && (has_staged || item.starts_with("accounting.db-")) {
            fs::rename(&live, &kept)?;
        }
        if has_staged {
            if let Some(d) = live.parent() {
                fs::create_dir_all(d)?;
            }
            fs::rename(&staged, &live)?;
        }
    }
    let done = data_dir.join(format!(
        "restore-applied-{}",
        previous.file_name().and_then(|n| n.to_str()).unwrap_or("x").trim_start_matches("pre-restore-")
    ));
    fs::rename(&staging, &done)?;
    let _ = fs::remove_file(done.join(APPLYING));
    let _ = fs::remove_file(done.join(READY));
    Ok(Some(previous))
}
