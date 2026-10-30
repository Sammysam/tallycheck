//! In-app updates (D45). The desktop shell (`src-tauri`) owns the updater: it checks the
//! update feed, verifies the update's signature against Cenango's public key, downloads it and
//! starts the installer. The server only passes requests through, so the page itself never needs
//! native (Tauri IPC) access. A server started without the shell (`accounting-server serve`) has
//! no updater and says so.

use std::future::Future;
use std::pin::Pin;

/// A newer version offered by the update feed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    pub version: String,
    /// The release notes from the feed.
    pub notes: Option<String>,
    /// Publication date, ISO 8601, when the feed gives one.
    pub date: Option<String>,
}

pub type UpdateFuture<T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send>>;

pub trait Updater: Send + Sync {
    /// `Ok(None)` when this version is the newest; `Err` with a sentence for the user when the
    /// feed can't be reached (no internet) or doesn't verify.
    fn check(&self) -> UpdateFuture<Option<UpdateInfo>>;

    /// Downloads and verifies the update. On `Ok` the shell closes the server cleanly, starts the
    /// installer and exits shortly after; the installer opens the new version when it is done.
    fn install(&self) -> UpdateFuture<UpdateInfo>;
}
