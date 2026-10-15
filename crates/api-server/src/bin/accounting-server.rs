//! Headless server (development, LAN host service) and admin tooling.
//!
//! ```text
//! accounting-server serve                    run until Ctrl+C
//! accounting-server create-admin <username>  password from ACCOUNTING_ADMIN_PASSWORD or stdin
//! accounting-server export-storage-key        print the file-encryption key (hex) for an offline copy
//! accounting-server import-storage-key        read a key (hex or base64) from stdin and install it
//! accounting-server set-backup-password       set or change the backup password (stdin)
//! accounting-server backup <folder>           write a verified, encrypted backup into <folder>
//! accounting-server verify-backup <file>      check a backup completely
//! accounting-server restore <file>            restore a backup (the app must be closed)
//!
//! Backup passwords are read from ACCOUNTING_BACKUP_PASSWORD or typed on stdin.
//! ```
//! The file-encryption key is DPAPI-protected and tied to this Windows account. Without its
//! recovery copy, encrypted employee files cannot be opened on a new PC or after a reinstall.
//! Configuration: see `app_core::config` (ACCOUNTING_DATA_DIR, ACCOUNTING_PORT, …).

use anyhow::{bail, Context};
use app_core::config::AppConfig;
use std::io::{BufRead, Write};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let config = AppConfig::load()?;
    let _guard = api_server::init_logging(&config.log_dir())?;

    match args.first().map(String::as_str) {
        None | Some("serve") => serve(&config).await,
        Some("create-admin") => {
            let username = args.get(1).context("usage: accounting-server create-admin <username>")?;
            create_admin(&config, username).await
        }
        Some("export-storage-key") => export_storage_key(&config),
        Some("import-storage-key") => import_storage_key(&config),
        Some("set-backup-password") => set_backup_password(&config),
        Some("backup") => {
            let folder = args.get(1).context("usage: accounting-server backup <folder>")?;
            run_backup(&config, folder).await
        }
        Some("verify-backup") => {
            let file = args.get(1).context("usage: accounting-server verify-backup <file>")?;
            verify(&config, file).await
        }
        Some("restore") => {
            let file = args.get(1).context("usage: accounting-server restore <file>")?;
            restore(&config, file).await
        }
        Some(other) => bail!("unknown command {other:?}; see the header of accounting-server.rs for the commands"),
    }
}

async fn serve(config: &AppConfig) -> anyhow::Result<()> {
    let handle = api_server::start(config).await?;
    tracing::info!("open {} in a browser; Ctrl+C stops the server", handle.url());
    tokio::signal::ctrl_c().await?;
    tracing::info!("shutting down");
    handle.shutdown().await;
    Ok(())
}

fn read_line(prompt: &str) -> anyhow::Result<String> {
    eprintln!("{prompt}");
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

fn backup_password(prompt: &str) -> anyhow::Result<String> {
    match std::env::var("ACCOUNTING_BACKUP_PASSWORD") {
        Ok(p) if !p.is_empty() => Ok(p),
        _ => read_line(prompt),
    }
}

fn set_backup_password(config: &AppConfig) -> anyhow::Result<()> {
    let secrets = config.secrets_dir();
    if backup::keys::load(&secrets)?.is_some() {
        let current = read_line("Current backup password, then Enter:")?;
        if !backup::keys::verify(&secrets, &current)? {
            bail!("the current backup password is not correct");
        }
    }
    let password = backup_password("New backup password (at least 10 characters), then Enter:")?;
    backup::keys::save(&secrets, &backup::Kek::from_new_password(&password)?)?;
    eprintln!("Backup password saved. Write it down: without it a backup cannot be opened on another PC.");
    Ok(())
}

async fn run_backup(config: &AppConfig, folder: &str) -> anyhow::Result<()> {
    let kek =
        backup::keys::load(&config.secrets_dir())?.context("set a backup password first (set-backup-password)")?;
    let _lock = api_server::server::lock_data_dir(&config.data_dir)?;
    let db = database::Db::open(&config.database_path()).await?;
    let result = backup::create_backup(&db, &config.data_dir, std::path::Path::new(folder), &kek).await;
    db.close().await;
    backup::schedule::record(&config.data_dir, &result.as_ref().map(Clone::clone).map_err(ToString::to_string));
    let info = result?;
    eprintln!("Backup written and verified: {} ({} bytes, {} stored files)", info.file, info.size, info.files);
    if !info.missing_files.is_empty() {
        eprintln!("Warning: {} files referred to by the records were already missing.", info.missing_files.len());
    }
    Ok(())
}

fn key_for(config: &AppConfig, file: &std::path::Path) -> anyhow::Result<backup::BackupKey> {
    match backup::restore::key_for(file, &config.secrets_dir(), None) {
        Ok(k) => Ok(k),
        Err(_) => Ok(backup::BackupKey::Password(backup_password("Backup password for this file, then Enter:")?)),
    }
}

async fn verify(config: &AppConfig, file: &str) -> anyhow::Result<()> {
    let path = std::path::Path::new(file);
    let m = backup::verify_backup(path, &key_for(config, path)?).await?;
    eprintln!(
        "The backup is complete and readable: made {} by version {}, {} entries.",
        m.created_at,
        m.app_version,
        m.entries.len()
    );
    for (table, n) in &m.counts {
        eprintln!("  {table}: {n}");
    }
    Ok(())
}

async fn restore(config: &AppConfig, file: &str) -> anyhow::Result<()> {
    let path = std::path::Path::new(file);
    let key = key_for(config, path)?;
    std::fs::create_dir_all(&config.data_dir)?;
    let _lock = api_server::server::lock_data_dir(&config.data_dir)
        .context("close Cenango Accounting before restoring from the command line")?;
    let answer = match std::env::var("ACCOUNTING_RESTORE_CONFIRM") {
        Ok(v) => v,
        Err(_) => {
            read_line(&format!("This replaces all data in {}. Type RESTORE to continue:", config.data_dir.display()))?
        }
    };
    if answer.trim() != "RESTORE" {
        bail!("restore cancelled");
    }
    let m = backup::stage_restore(path, &key, &config.data_dir).await?;
    let previous = backup::apply_pending_restore(&config.data_dir)?;
    eprintln!("Restored the backup made {}.", m.created_at);
    for (table, n) in &m.counts {
        eprintln!("  {table}: {n}");
    }
    if let Some(prev) = previous {
        eprintln!("The replaced data was kept in {}", prev.display());
    }
    Ok(())
}

/// Prints the key to stdout only (never to the log), for the administrator to store offline.
fn export_storage_key(config: &AppConfig) -> anyhow::Result<()> {
    let path = config.secrets_dir().join(api_server::STORAGE_KEY_FILE);
    if !path.exists() {
        bail!("no file-encryption key yet; start the server once to create it");
    }
    let raw = app_core::secrets::load_or_create(&path, 32)?;
    let key = documents::StorageKey::from_bytes(&raw).map_err(|e| anyhow::anyhow!("{e}"))?;
    // stdout only, so it can be redirected straight to removable media; never logged.
    writeln!(std::io::stdout().lock(), "{}", key.to_hex())?;
    eprintln!("Keep this key offline (paper or a password manager). Anyone holding it can read the employee files.");
    Ok(())
}

/// Installs a recovery key, e.g. on a new PC before restoring the data folder. Refuses to replace
/// a different existing key: files encrypted with it would become unreadable.
fn import_storage_key(config: &AppConfig) -> anyhow::Result<()> {
    eprintln!("Paste the file-encryption key (64 hex characters or base64), then Enter:");
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    let key = documents::parse_key(&line).map_err(|e| anyhow::anyhow!("{e}"))?;
    let path = config.secrets_dir().join(api_server::STORAGE_KEY_FILE);
    if path.exists() {
        let current = app_core::secrets::load_or_create(&path, 32)?;
        if current.as_slice() == key.as_bytes() {
            eprintln!("That key is already installed.");
            return Ok(());
        }
        bail!("a different file-encryption key is already installed at {}; move it aside first", path.display());
    }
    app_core::secrets::store(&path, key.as_bytes())?;
    eprintln!("File-encryption key installed.");
    Ok(())
}

async fn create_admin(config: &AppConfig, username: &str) -> anyhow::Result<()> {
    let password = match std::env::var("ACCOUNTING_ADMIN_PASSWORD") {
        Ok(p) if !p.is_empty() => p,
        _ => {
            eprintln!("Password for {username} (min 8 characters), then Enter:");
            let mut line = String::new();
            std::io::stdin().lock().read_line(&mut line)?;
            line.trim_end_matches(['\r', '\n']).to_string()
        }
    };
    if password.chars().count() < 8 {
        bail!("password must be at least 8 characters");
    }
    let db = database::Db::open(&config.database_path()).await?;
    let created = auth::users::bootstrap_admin(&db, username, &password).await?;
    db.close().await;
    if created {
        eprintln!("Admin user {username} created. Two-factor setup happens at first login.");
    } else {
        eprintln!("User {username} already exists; nothing changed.");
    }
    Ok(())
}
