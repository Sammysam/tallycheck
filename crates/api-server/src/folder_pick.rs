//! D62: "Choose folder…" for backups. The page has no Tauri IPC, so the desktop shell
//! (`src-tauri`) opens the system's own folder window over TallyCheck and the server only passes
//! the request through, like the updater (D45) and the Windows sign-in check (D49). A server
//! started without the shell (a browser on another PC, the headless server) has no picker and the
//! folder is typed instead.

use std::future::Future;
use std::pin::Pin;

pub type PickFuture = Pin<Box<dyn Future<Output = Result<Option<String>, String>> + Send>>;

pub trait FolderPicker: Send + Sync {
    /// Shows the folder window titled `title`, starting in `start` when given. `Ok(None)` when
    /// the person closed it without choosing; `Err` with a sentence when it could not open.
    fn pick(&self, title: String, start: Option<String>) -> PickFuture;
}
