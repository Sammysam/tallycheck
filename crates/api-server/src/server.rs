//! Router assembly, startup and logging.

use crate::errors::{nest_errors, unknown_route};
use crate::middleware::{audit_log, host_guard, security_headers};
use crate::routes::api_router;
use crate::state::AppState;
use anyhow::Context;
use app_core::config::AppConfig;
use auth::JwtKeys;
use axum::middleware::from_fn;
use axum::middleware::from_fn_with_state;
use axum::Router;
use database::Db;
use documents::{FileStore, StorageKey};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

const JWT_SECRET_FILE: &str = "jwt-secret.dpapi";
const JWT_SECRET_LEN: usize = 64;
/// AES-256 key for employee photos and documents, DPAPI-protected like the JWT secret.
pub use app_core::secrets::STORAGE_KEY_FILE;

/// The encrypted file store under `<data_dir>/files`, creating its key on first run.
pub fn open_file_store(config: &AppConfig) -> anyhow::Result<FileStore> {
    let key = app_core::secrets::load_or_create(&config.secrets_dir().join(STORAGE_KEY_FILE), 32)
        .context("loading the file-encryption key")?;
    let key = StorageKey::from_bytes(&key).map_err(|e| anyhow::anyhow!("file-encryption key: {e}"))?;
    Ok(FileStore::new(config.data_dir.join("files"), key))
}

/// The whole application: `/api` plus, when a frontend build is given, the static app with
/// SPA fallback to `index.html`.
pub fn build_router(state: AppState, frontend_dir: Option<&Path>) -> Router {
    let mut router = Router::new()
        .nest("/api", api_router())
        .route("/system/backup", axum::routing::get(crate::routes::system::page));
    router = match frontend_dir {
        Some(dir) => {
            let index = dir.join("index.html");
            // Always revalidate (cheap 304s locally) so an upgraded install never runs a stale
            // cached index.html that points at asset files which no longer exist.
            let files = tower::ServiceBuilder::new()
                .layer(SetResponseHeaderLayer::overriding(
                    axum::http::header::CACHE_CONTROL,
                    axum::http::HeaderValue::from_static("no-cache"),
                ))
                .service(ServeDir::new(dir).fallback(ServeFile::new(index)));
            router.fallback_service(files)
        }
        None => router.fallback(unknown_route),
    };
    let mut router = router
        .layer(from_fn_with_state(state.clone(), audit_log))
        .layer(from_fn(nest_errors))
        // Method, URL, status and latency only: headers (Authorization) and bodies are never logged.
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO).include_headers(false))
                .on_response(DefaultOnResponse::new().level(Level::INFO).include_headers(false)),
        );
    for (name, value) in security_headers() {
        router = router.layer(SetResponseHeaderLayer::overriding(name, value));
    }
    router.layer(from_fn_with_state(state.clone(), host_guard)).with_state(state)
}

pub struct ServerHandle {
    pub addr: SocketAddr,
    pub db: Db,
    shutdown: Option<oneshot::Sender<()>>,
    join: JoinHandle<()>,
    scheduler: JoinHandle<()>,
    /// Held for the server's lifetime: one server per data folder.
    _lock: std::fs::File,
}

impl ServerHandle {
    /// Stops accepting connections, lets in-flight requests finish, and closes the database.
    pub async fn shutdown(mut self) {
        self.scheduler.abort();
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Err(e) = (&mut self.join).await {
            tracing::error!("server task ended abnormally: {e}");
        }
        self.db.close().await;
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

/// Opens the database (running migrations), loads the DPAPI-protected JWT secret, creates the
/// first admin from `ADMIN_USERNAME` / `ADMIN_PASSWORD` if set (reference `onModuleInit`),
/// binds the configured address and serves until `shutdown`.
pub async fn start(config: &AppConfig) -> anyhow::Result<ServerHandle> {
    config.check_local_data_dir()?;
    std::fs::create_dir_all(&config.data_dir).context("creating the data folder")?;
    let lock = lock_data_dir(&config.data_dir)?;
    if let Some(previous) = backup::apply_pending_restore(&config.data_dir).context("finishing a restore")? {
        tracing::warn!(kept = %previous.display(), "restore applied; the replaced data was kept");
    }
    if let Some(copy) = Db::snapshot_before_upgrade(&config.database_path(), &config.data_dir.join("pre-upgrade"))
        .await
        .context("copying the database before upgrading it")?
    {
        tracing::info!(copy = %copy.display(), "database copied before the schema upgrade");
    }
    let db = Db::open(&config.database_path()).await.context("opening the database")?;
    let secret = app_core::secrets::load_or_create(&config.secrets_dir().join(JWT_SECRET_FILE), JWT_SECRET_LEN)
        .context("loading the JWT signing key")?;
    let files = open_file_store(config)?;
    // D29: the company's currency decides whether amounts are kept in whole units.
    if let Some(code) = database::settings::find_all(&db).await?.get("region.currency") {
        app_core::money::set_currency(code);
    }
    let added = seed_fresh_install(&db).await.context("creating the chart of accounts")?;
    if added > 0 {
        tracing::info!(added, "fresh install: default chart of accounts created");
    }
    bootstrap_admin_from_env(&db).await?;

    let listener = TcpListener::bind(config.bind).await.with_context(|| format!("binding {}", config.bind))?;
    let addr = listener.local_addr()?;
    let state = AppState::new(db.clone(), JwtKeys::new(&secret), files, config.lan_enabled, addr.port())
        .with_data_dir(config.data_dir.clone())
        .with_lan_policy(config.lan_allowed.clone(), config.lan_host_names.clone());
    let frontend = resolve_frontend_dir(config.frontend_dir.as_deref());
    match &frontend {
        Some(d) => tracing::info!("serving frontend from {}", d.display()),
        None => tracing::warn!("no frontend build found; only /api is served"),
    }
    let app = build_router(state, frontend.as_deref());

    let (tx, rx) = oneshot::channel::<()>();
    let join = tokio::spawn(async move {
        let result = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await;
        if let Err(e) = result {
            tracing::error!("server error: {e}");
        }
    });
    let scheduler = backup::schedule::spawn(db.clone(), config.data_dir.clone());
    tracing::info!(%addr, lan = config.lan_enabled, "accounting server listening");
    Ok(ServerHandle { addr, db, shutdown: Some(tx), join, scheduler, _lock: lock })
}

/// An exclusive lock on `<data_dir>/.lock`: a second server (or a CLI restore) on the same
/// data folder is refused instead of sharing the database.
pub fn lock_data_dir(dir: &Path) -> anyhow::Result<std::fs::File> {
    let file = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(".lock"))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(std::fs::TryLockError::WouldBlock) => {
            anyhow::bail!("Cenango Accounting is already running with the data folder {}", dir.display())
        }
        Err(std::fs::TryLockError::Error(e)) => Err(e.into()),
    }
}

/// D20: a fresh install (no users yet) gets the full default chart of accounts — 1000 Cash,
/// 1100 AR, 2000 AP, 4000 Revenue, … — so invoices, bills and payments work from the first
/// day without anyone pressing "Seed". Only missing codes are added, and never once the app is
/// in use: an imported database or a chart someone has changed is left alone.
pub async fn seed_fresh_install(db: &Db) -> app_core::AppResult<usize> {
    if auth::users::any_users(db).await? {
        return Ok(0);
    }
    ledger::accounts::seed(db).await
}

async fn bootstrap_admin_from_env(db: &Db) -> anyhow::Result<()> {
    let (Ok(username), Ok(password)) = (std::env::var("ADMIN_USERNAME"), std::env::var("ADMIN_PASSWORD")) else {
        return Ok(());
    };
    if username.is_empty() || password.is_empty() {
        return Ok(());
    }
    if auth::users::bootstrap_admin(db, &username, &password).await? {
        tracing::info!(username, "admin user created");
    }
    Ok(())
}

fn resolve_frontend_dir(configured: Option<&Path>) -> Option<PathBuf> {
    let candidates: Vec<PathBuf> = match configured {
        Some(p) => vec![p.to_path_buf()],
        None => {
            let mut c = Vec::new();
            if let Ok(exe) = std::env::current_exe() {
                if let Some(dir) = exe.parent() {
                    c.push(dir.join("frontend"));
                }
            }
            c.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../frontend/dist"));
            c
        }
    };
    candidates.into_iter().find(|d| d.join("index.html").is_file())
}

/// Logs to stderr and to daily files under `log_dir`. Request bodies are never logged.
pub fn init_logging(log_dir: &Path) -> anyhow::Result<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::{fmt, EnvFilter};

    std::fs::create_dir_all(log_dir)?;
    let file = tracing_appender::rolling::daily(log_dir, "accounting.log");
    let (writer, guard) = tracing_appender::non_blocking(file);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,tower_http=info,sqlx=warn"));
    tracing_subscriber::registry()
        .with(filter)
        // Plain text on both outputs: formatted span fields are shared between layers, so ANSI
        // colour codes on the console layer would also leak into the log file.
        .with(fmt::layer().with_ansi(false).with_writer(std::io::stderr))
        .with(fmt::layer().with_ansi(false).with_writer(writer))
        .try_init()
        .map_err(|e| anyhow::anyhow!("logging already initialised: {e}"))?;
    Ok(guard)
}
