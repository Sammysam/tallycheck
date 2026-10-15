use crate::create::{create_backup, remove_partials};
use crate::keys::{self, Kek};
use crate::restore::{apply_pending_restore, stage_restore, verify_backup, BackupKey};
use crate::schedule;
use crate::{BackupError, BackupResult};
use database::Db;
use documents::{FileStore, StorageKey};
use std::fs;
use std::path::Path;

const PASSWORD: &str = "office backup 2026";

struct Install {
    dir: tempfile::TempDir,
    db: Db,
    files: FileStore,
}

impl Install {
    fn data(&self) -> &Path {
        self.dir.path()
    }
}

async fn install() -> BackupResult<Install> {
    let dir = tempfile::tempdir()?;
    let db = Db::open(&dir.path().join("accounting.db")).await?;
    let raw =
        app_core::secrets::load_or_create(&dir.path().join("secrets").join(app_core::secrets::STORAGE_KEY_FILE), 32)?;
    let key = StorageKey::from_bytes(&raw).map_err(|e| BackupError::Refused(e.to_string()))?;
    let files = FileStore::new(dir.path().join("files"), key);
    Ok(Install { dir, db, files })
}

async fn add_employee_with_document(i: &Install, code: &str) -> BackupResult<String> {
    let e = employees::records::create(
        &i.db,
        employees::records::EmployeeFields {
            employee_id: Some(code.into()),
            name: Some(format!("Person {code}")),
            ..Default::default()
        },
    )
    .await?;
    employees::docs::add(
        &i.db,
        &i.files,
        &e.id,
        employees::docs::DocFields { title: Some("Contract".into()), ..Default::default() },
        Some(employees::Upload {
            original_name: "contract.pdf".into(),
            mime_type: "application/pdf".into(),
            bytes: format!("%PDF-1.4 contract of {code}").into_bytes(),
        }),
        &employees::Actor::default(),
    )
    .await?;
    Ok(e.id)
}

async fn count(db: &Db, table: &str) -> BackupResult<i64> {
    Ok(sqlx::query_scalar(&format!("SELECT COUNT(*) FROM \"{table}\"")).fetch_one(db.read()).await?)
}

#[tokio::test]
async fn a_backup_restores_on_a_new_pc_with_only_the_password() -> BackupResult<()> {
    let old = install().await?;
    let emp = add_employee_with_document(&old, "CEN-001").await?;
    let kek = Kek::from_new_password(PASSWORD)?;
    keys::save(&old.data().join("secrets"), &kek)?;
    let dest = tempfile::tempdir()?;
    let info = create_backup(&old.db, old.data(), dest.path(), &kek).await?;
    assert_eq!(info.files, 2, "the document and its content-type sidecar");
    assert!(info.missing_files.is_empty());
    let raw = fs::read(&info.file)?;
    assert_eq!(&raw[..4], b"QBAK");
    assert!(!raw.windows(8).any(|w| w == b"CEN-001\0" || w == b"Person C"), "no plaintext in the backup");
    assert!(fs::read_dir(dest.path())?.flatten().all(|e| !e.file_name().to_string_lossy().ends_with(".partial")));

    // A new PC: different DPAPI key store, empty data folder, only the file and the password.
    let new_pc = tempfile::tempdir()?;
    let wrong =
        stage_restore(Path::new(&info.file), &BackupKey::Password("not the password".into()), new_pc.path()).await;
    assert!(matches!(wrong, Err(BackupError::Key(keys::KeyError::WrongPassword))));
    let manifest = stage_restore(Path::new(&info.file), &BackupKey::Password(PASSWORD.into()), new_pc.path()).await?;
    assert_eq!(manifest.counts.get("employees"), Some(&1));
    assert!(apply_pending_restore(new_pc.path())?.is_some());
    assert!(apply_pending_restore(new_pc.path())?.is_none(), "applies once");

    let db = Db::open(&new_pc.path().join("accounting.db")).await?;
    assert_eq!(count(&db, "employee_documents").await?, 1);
    let raw = app_core::secrets::load_or_create(
        &new_pc.path().join("secrets").join(app_core::secrets::STORAGE_KEY_FILE),
        32,
    )?;
    let store = FileStore::new(
        new_pc.path().join("files"),
        StorageKey::from_bytes(&raw).map_err(|e| BackupError::Refused(e.to_string()))?,
    );
    let doc =
        employees::docs::list(&db, &emp).await?.into_iter().next().ok_or_else(|| BackupError::Refused("doc".into()))?;
    assert_eq!(
        store.get(&doc.file_key)?.bytes,
        b"%PDF-1.4 contract of CEN-001",
        "documents decrypt with the restored key"
    );
    Ok(())
}

#[tokio::test]
async fn restore_replaces_live_data_and_keeps_the_old_copy() -> BackupResult<()> {
    let i = install().await?;
    add_employee_with_document(&i, "CEN-001").await?;
    let kek = Kek::from_new_password(PASSWORD)?;
    let dest = tempfile::tempdir()?;
    let info = create_backup(&i.db, i.data(), dest.path(), &kek).await?;
    add_employee_with_document(&i, "CEN-002").await?;
    assert_eq!(count(&i.db, "employees").await?, 2);

    stage_restore(Path::new(&info.file), &BackupKey::Kek(kek.clone()), i.data()).await?;
    i.db.close().await;
    let previous = apply_pending_restore(i.data())?.ok_or_else(|| BackupError::Refused("not applied".into()))?;
    let db = Db::open(&i.data().join("accounting.db")).await?;
    assert_eq!(count(&db, "employees").await?, 1, "back to the backup's state");
    assert!(previous.join("accounting.db").exists(), "the replaced database is kept");
    Ok(())
}

#[tokio::test]
async fn damaged_backups_are_refused_and_nothing_is_staged() -> BackupResult<()> {
    let i = install().await?;
    add_employee_with_document(&i, "CEN-001").await?;
    let kek = Kek::from_new_password(PASSWORD)?;
    let dest = tempfile::tempdir()?;
    let info = create_backup(&i.db, i.data(), dest.path(), &kek).await?;
    let mut bytes = fs::read(&info.file)?;
    let n = bytes.len();
    bytes[n - 40] ^= 0x55;
    let bad = dest.path().join("bad.qbak");
    fs::write(&bad, &bytes)?;
    assert!(verify_backup(&bad, &BackupKey::Kek(kek.clone())).await.is_err());
    let cut = dest.path().join("cut.qbak");
    fs::write(&cut, &bytes[..n / 2])?;
    assert!(verify_backup(&cut, &BackupKey::Kek(kek.clone())).await.is_err());
    let target = tempfile::tempdir()?;
    assert!(stage_restore(&bad, &BackupKey::Kek(kek.clone()), target.path()).await.is_err());
    assert!(!target.path().join(crate::restore::STAGING_DIR).exists());
    assert!(apply_pending_restore(target.path())?.is_none());
    Ok(())
}

#[tokio::test]
async fn interrupted_backups_and_restores_recover() -> BackupResult<()> {
    let i = install().await?;
    add_employee_with_document(&i, "CEN-001").await?;
    let kek = Kek::from_new_password(PASSWORD)?;
    let dest = tempfile::tempdir()?;
    fs::write(dest.path().join("cenango-accounting-20260101-000000.qbak.partial"), b"half")?;
    let info = create_backup(&i.db, i.data(), dest.path(), &kek).await?;
    assert_eq!(remove_partials(dest.path())?, 0, "the stale partial was removed by the backup run");

    // Simulate a crash half-way through the swap: marker written, database already moved.
    let target = tempfile::tempdir()?;
    stage_restore(Path::new(&info.file), &BackupKey::Kek(kek.clone()), target.path()).await?;
    let staging = target.path().join(crate::restore::STAGING_DIR);
    fs::write(staging.join("APPLYING"), "pre-restore-crash")?;
    fs::rename(staging.join("accounting.db"), target.path().join("accounting.db"))?;
    let previous = apply_pending_restore(target.path())?;
    assert_eq!(previous.as_deref(), Some(target.path().join("pre-restore-crash").as_path()));
    let db = Db::open(&target.path().join("accounting.db")).await?;
    assert_eq!(count(&db, "employees").await?, 1);
    assert!(target.path().join("files").exists(), "the rest of the swap was completed");
    Ok(())
}

#[tokio::test]
async fn scheduled_backups_need_a_password_and_keep_the_newest() -> BackupResult<()> {
    let i = install().await?;
    let dest = tempfile::tempdir()?;
    schedule::save_settings(
        i.data(),
        &schedule::Settings { enabled: true, folder: Some(dest.path().to_string_lossy().into()), keep: 2 },
    )?;
    assert!(matches!(schedule::run_if_due(&i.db, i.data()).await, Some(Err(_))), "no password yet");
    assert!(schedule::load_status(i.data()).last_error.is_some());
    keys::save(&i.data().join("secrets"), &Kek::from_new_password(PASSWORD)?)?;
    assert!(matches!(schedule::run_if_due(&i.db, i.data()).await, Some(Ok(_))));
    assert!(schedule::run_if_due(&i.db, i.data()).await.is_none(), "not due again within a day");
    let dir = dest.path().join(schedule::SCHEDULED_DIR);
    for stamp in ["20200101-000000", "20200102-000000"] {
        fs::write(dir.join(format!("cenango-accounting-{stamp}.qbak")), b"old")?;
    }
    fs::write(dir.join("my-own-copy.qbak"), b"keep me")?;
    let removed = schedule::prune(&dir, 2)?;
    assert_eq!(removed.len(), 1);
    assert!(dir.join("my-own-copy.qbak").exists(), "files made by hand are never deleted");
    assert!(!dir.join("cenango-accounting-20200101-000000.qbak").exists(), "oldest goes first");
    Ok(())
}
