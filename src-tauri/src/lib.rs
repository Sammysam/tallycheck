//! Desktop shell: starts the embedded API server (which also serves the React app) and opens
//! a window on it. The page is a normal http://127.0.0.1 origin with no Tauri IPC access —
//! no capabilities are granted to remote URLs — so the frontend runs exactly as in a browser.

use api_server::ServerHandle;
use app_core::config::AppConfig;
use std::sync::Mutex;
use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

struct Server(Mutex<Option<ServerHandle>>);
struct LogGuard(#[allow(dead_code)] tracing_appender::non_blocking::WorkerGuard);

fn start_server(app: &tauri::App) -> anyhow::Result<String> {
    let mut config = AppConfig::load()?;
    let guard = api_server::init_logging(&config.log_dir())?;
    app.manage(LogGuard(guard));

    // An installed build ships the frontend next to the executable as a resource.
    if config.frontend_dir.is_none() {
        if let Ok(dir) = app.path().resource_dir() {
            let bundled = dir.join("frontend");
            if bundled.join("index.html").is_file() {
                config.frontend_dir = Some(bundled);
            }
        }
    }

    let handle = tauri::async_runtime::block_on(api_server::start(&config))?;
    // Always open the loopback address, even when LAN access binds 0.0.0.0.
    let url = format!("http://127.0.0.1:{}/", handle.addr.port());
    app.manage(Server(Mutex::new(Some(handle))));
    Ok(url)
}

pub fn run() {
    let app = tauri::Builder::default()
        .setup(|app| {
            let url = start_server(app).map_err(|e| {
                tracing::error!("startup failed: {e:#}");
                e
            })?;
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url.parse()?))
                .title("Cenango Accounting")
                .inner_size(1366.0, 860.0)
                .min_inner_size(1024.0, 700.0)
                .build()?;
            Ok(())
        })
        .build(tauri::generate_context!());

    let app = match app {
        Ok(app) => app,
        Err(e) => {
            eprintln!("Cenango Accounting could not start: {e}");
            std::process::exit(1);
        }
    };

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            let server = handle.state::<Server>().0.lock().ok().and_then(|mut s| s.take());
            if let Some(server) = server {
                tauri::async_runtime::block_on(server.shutdown());
                tracing::info!("server stopped, database closed");
            }
        }
    });
}
