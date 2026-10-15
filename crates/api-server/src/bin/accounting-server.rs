//! Headless server (development, LAN host service) and admin tooling.
//!
//! ```text
//! accounting-server serve                    run until Ctrl+C
//! accounting-server create-admin <username>  password from ACCOUNTING_ADMIN_PASSWORD or stdin
//! accounting-server export-storage-key        print the file-encryption key (hex) for an offline copy
//! accounting-server import-storage-key        read a key (hex or base64) from stdin and install it
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
        Some(other) => bail!(
            "unknown command {other:?}; expected serve, create-admin <username>, export-storage-key or import-storage-key"
        ),
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
