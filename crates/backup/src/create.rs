//! Making a backup.

use crate::format::{hex, random, sha256_hex, write_header, ArchiveWriter, EncryptWriter, Header, CHUNK, VERSION};
use crate::keys::Kek;
use crate::restore::{verify_backup, BackupKey};
use crate::{
    BackupError, BackupResult, Manifest, ManifestEntry, COUNTED_TABLES, DB_ENTRY, FILES_PREFIX, MANIFEST_ENTRY,
    STORAGE_KEY_ENTRY,
};
use database::Db;
use serde::Serialize;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Connection};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

pub const EXTENSION: &str = "qbak";
pub const FILE_PREFIX: &str = "cenango-accounting-";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub file: String,
    pub size: u64,
    pub created_at: String,
    pub schema_version: i64,
    pub files: usize,
    pub missing_files: Vec<String>,
    pub counts: std::collections::BTreeMap<String, i64>,
}

/// Removes `*.qbak.partial` files an interrupted backup left in `dir`.
pub fn remove_partials(dir: &Path) -> std::io::Result<usize> {
    let mut n = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(&format!(".{EXTENSION}.partial")) && fs::remove_file(e.path()).is_ok() {
                n += 1;
            }
        }
    }
    Ok(n)
}

/// Every stored file under `root`, as relative `/`-separated keys (in-flight `.partial` writes
/// are not stored files yet).
fn stored_files(root: &Path) -> std::io::Result<Vec<String>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            if e.file_type()?.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|x| x.to_str()) != Some("partial") {
                if let Ok(rel) = path.strip_prefix(root) {
                    let key: Vec<String> =
                        rel.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
                    out.push(key.join("/"));
                }
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Hard-links (or copies) the stored files into `dest` — instant on NTFS, and safe because a
/// stored file is never modified in place.
fn link_files(src_root: &Path, dest_root: &Path, keys: &[String]) -> std::io::Result<()> {
    for k in keys {
        let src = src_root.join(k);
        let dest = dest_root.join(k);
        if let Some(d) = dest.parent() {
            fs::create_dir_all(d)?;
        }
        if fs::hard_link(&src, &dest).is_err() {
            fs::copy(&src, &dest)?;
        }
    }
    Ok(())
}

async fn snapshot_facts(path: &Path) -> BackupResult<(i64, std::collections::BTreeMap<String, i64>, Vec<String>)> {
    let mut conn = SqliteConnectOptions::new().filename(path).read_only(true).connect().await?;
    let ok: String = sqlx::query_scalar("PRAGMA integrity_check").fetch_one(&mut conn).await?;
    if ok != "ok" {
        return Err(BackupError::Verify(format!("database integrity check: {ok}")));
    }
    let fk: Vec<(String,)> =
        sqlx::query_as("SELECT \"table\" FROM pragma_foreign_key_check").fetch_all(&mut conn).await?;
    if !fk.is_empty() {
        return Err(BackupError::Verify(format!("{} foreign key problems in the database", fk.len())));
    }
    let schema: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations").fetch_one(&mut conn).await?;
    let mut counts = std::collections::BTreeMap::new();
    for t in COUNTED_TABLES {
        let n: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM \"{t}\"")).fetch_one(&mut conn).await?;
        counts.insert((*t).to_string(), n);
    }
    let referenced: Vec<String> = sqlx::query_scalar(
        r#"SELECT "photoKey" FROM "employees" WHERE "photoKey" IS NOT NULL
           UNION SELECT "fileKey" FROM "employee_documents""#,
    )
    .fetch_all(&mut conn)
    .await?;
    conn.close().await?;
    Ok((schema, counts, referenced))
}

/// Takes a backup of the live installation into `dest_dir` and verifies it before returning.
pub async fn create_backup(db: &Db, data_dir: &Path, dest_dir: &Path, kek: &Kek) -> BackupResult<BackupInfo> {
    fs::create_dir_all(dest_dir)?;
    remove_partials(dest_dir)?;
    let work_root = data_dir.join("backup-work");
    let _ = fs::remove_dir_all(&work_root);
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let work = work_root.join(&stamp);
    fs::create_dir_all(work.join("files"))?;
    let result = build(db, data_dir, dest_dir, kek, &work, &stamp).await;
    let _ = fs::remove_dir_all(&work_root);
    result
}

async fn build(
    db: &Db,
    data_dir: &Path,
    dest_dir: &Path,
    kek: &Kek,
    work: &Path,
    stamp: &str,
) -> BackupResult<BackupInfo> {
    let snapshot = work.join("accounting.db");
    let files_root = data_dir.join("files");
    let file_keys;
    {
        // Holding the only writer connection: no commit can happen until the files are linked.
        let mut writer = db.write().acquire().await?;
        sqlx::query("VACUUM INTO ?1").bind(snapshot.to_string_lossy().to_string()).execute(&mut *writer).await?;
        file_keys = stored_files(&files_root)?;
        link_files(&files_root, &work.join("files"), &file_keys)?;
    }
    let (schema_version, counts, referenced) = snapshot_facts(&snapshot).await?;
    let missing_files: Vec<String> = referenced.into_iter().filter(|k| file_keys.binary_search(k).is_err()).collect();
    if !missing_files.is_empty() {
        tracing::warn!(count = missing_files.len(), "backup: files referenced by the database are missing on disk");
    }

    let key_path = data_dir.join("secrets").join(app_core::secrets::STORAGE_KEY_FILE);
    let storage_key = if key_path.exists() { Some(app_core::secrets::load_or_create(&key_path, 32)?) } else { None };

    let created_at = app_core::time::now_iso();
    let name = format!("{}{stamp}.{}", crate::create::FILE_PREFIX, EXTENSION);
    let final_path = dest_dir.join(&name);
    let partial = dest_dir.join(format!("{name}.partial"));

    let data_key: [u8; 32] = random(32)?.try_into().map_err(|_| BackupError::Refused("random key".into()))?;
    let prefix: [u8; 7] = random(7)?.try_into().map_err(|_| BackupError::Refused("random nonce".into()))?;
    let header = Header {
        format: VERSION,
        created_at: created_at.clone(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        schema_version,
        chunk_size: CHUNK,
        nonce_prefix: hex(&prefix),
        key_slot: kek.wrap(&data_key)?,
    };

    let work = work.to_path_buf();
    let partial_for_task = partial.clone();
    let file_count = file_keys.len();
    let manifest_base = Manifest {
        format: VERSION,
        created_at: created_at.clone(),
        app_version: header.app_version.clone(),
        schema_version,
        storage_key_fingerprint: storage_key.as_ref().map(|k| sha256_hex(k)[..16].to_string()),
        counts: counts.clone(),
        missing_files: missing_files.clone(),
        entries: Vec::new(),
    };
    tokio::task::spawn_blocking(move || -> BackupResult<()> {
        let mut out = BufWriter::new(File::create(&partial_for_task)?);
        let header_json = write_header(&mut out, &header)?;
        let mut archive = ArchiveWriter::new(EncryptWriter::new(out, &data_key, prefix, &header_json));
        let mut entries = Vec::new();
        fn add<W: Write>(archive: &mut ArchiveWriter<W>, name: String, path: &PathBuf) -> BackupResult<ManifestEntry> {
            let size = fs::metadata(path)?.len();
            let sha256 = archive.entry(&name, size, &mut File::open(path)?)?;
            Ok(ManifestEntry { name, size, sha256 })
        }
        entries.push(add(&mut archive, DB_ENTRY.into(), &work.join("accounting.db"))?);
        if let Some(k) = &storage_key {
            let sha256 = archive.entry(STORAGE_KEY_ENTRY, k.len() as u64, &mut k.as_slice())?;
            entries.push(ManifestEntry { name: STORAGE_KEY_ENTRY.into(), size: k.len() as u64, sha256 });
        }
        for key in &file_keys {
            entries.push(add(&mut archive, format!("{FILES_PREFIX}{key}"), &work.join("files").join(key))?);
        }
        let manifest = Manifest { entries, ..manifest_base };
        let json = serde_json::to_vec_pretty(&manifest).map_err(|e| BackupError::Verify(e.to_string()))?;
        archive.entry(MANIFEST_ENTRY, json.len() as u64, &mut json.as_slice())?;
        let mut out = archive.finish()?.finish()?;
        out.flush()?;
        out.into_inner().map_err(|e| BackupError::Io(e.into_error()))?.sync_all()?;
        Ok(())
    })
    .await
    .map_err(|e| BackupError::Refused(format!("backup task failed: {e}")))??;

    // Read the whole file back before calling it a backup.
    let verified = match verify_backup(&partial, &BackupKey::Kek(kek.clone())).await {
        Ok(m) => m,
        Err(e) => {
            let _ = fs::remove_file(&partial);
            return Err(BackupError::Verify(format!("the written backup could not be read back: {e}")));
        }
    };
    fs::rename(&partial, &final_path)?;
    Ok(BackupInfo {
        file: final_path.to_string_lossy().to_string(),
        size: fs::metadata(&final_path)?.len(),
        created_at,
        schema_version,
        files: file_count,
        missing_files: verified.missing_files,
        counts,
    })
}
