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

/// A desktop user never sees stderr: show why the app cannot start (already running, data
/// folder on a network drive, a restore that could not be finished, …).
fn startup_error_box(message: &str) {
    #[cfg(windows)]
    #[allow(unsafe_code)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
        let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let text = wide(&format!("Cenango Accounting could not start.\n\n{message}"));
        let title = wide("Cenango Accounting");
        // SAFETY: both buffers are NUL-terminated UTF-16 and outlive the call; no owner window.
        unsafe {
            MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR);
        }
    }
    #[cfg(not(windows))]
    let _ = message;
}

pub fn run() {
    let app = tauri::Builder::default()
        .setup(|app| {
            let url = start_server(app).map_err(|e| {
                tracing::error!("startup failed: {e:#}");
                startup_error_box(&format!("{e:#}"));
                e
            })?;
            let base: tauri::Url = url.parse()?;
            // The page draws its own menu bar, toolbar and status bar (File, Edit, View, …), so
            // the window has no native menu.
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(base))
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
            startup_error_box(&e.to_string());
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
