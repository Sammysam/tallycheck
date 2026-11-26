//! D62: the system's own folder window (Windows Explorer's "Select Folder", the macOS open panel)
//! for choosing where backups go. Opened over the TallyCheck window from Rust; the page reaches it
//! only through the server (`POST /api/system/backup/choose-folder`), never through Tauri IPC.

use api_server::folder_pick::{FolderPicker, PickFuture};
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

pub struct SystemFolderPicker {
    pub app: tauri::AppHandle,
}

impl FolderPicker for SystemFolderPicker {
    fn pick(&self, title: String, start: Option<String>) -> PickFuture {
        let app = self.app.clone();
        Box::pin(async move {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let mut dialog = app.dialog().file().set_title(title);
            if let Some(dir) = start {
                dialog = dialog.set_directory(dir);
            }
            if let Some(window) = app.get_webview_window("main") {
                dialog = dialog.set_parent(&window);
            }
            dialog.pick_folder(move |chosen| {
                let _ = tx.send(chosen);
            });
            match rx.await {
                Ok(Some(path)) => path
                    .into_path()
                    .map(|p| Some(p.to_string_lossy().into_owned()))
                    .map_err(|_| "That folder cannot be used; choose a folder on this computer".to_string()),
                Ok(None) => Ok(None),
                Err(_) => Err("The folder window could not be opened".to_string()),
            }
        })
    }
}
