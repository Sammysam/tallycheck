//! Headless server (development, LAN host service) and admin tooling.
//!
//! ```text
//! accounting-server serve                    run until Ctrl+C
//! accounting-server create-admin <username>  password from ACCOUNTING_ADMIN_PASSWORD or stdin
//! ```
//! Configuration: see `app_core::config` (ACCOUNTING_DATA_DIR, ACCOUNTING_PORT, …).

use anyhow::{bail, Context};
use app_core::config::AppConfig;
use std::io::BufRead;

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
        Some(other) => bail!("unknown command {other:?}; expected `serve` or `create-admin <username>`"),
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
