//! D45 in-app updates: Tauri's updater behind the server's `/api/system/update` routes, so the
//! page needs no native access. The feed is a `latest.json` on the release page; every update
//! is signed with Cenango's update key and refused unless it verifies against the public key in
//! `tauri.conf.json` (`plugins.updater.pubkey`). The private key never leaves the release PC.

use api_server::updates::{UpdateFuture, UpdateInfo, Updater};
use tauri::{AppHandle, Url};
use tauri_plugin_updater::{Update, UpdaterExt};

/// Where released versions are announced: the `latest.json` of the newest GitHub release.
/// `ACCOUNTING_UPDATE_URL` overrides it (tests and staging); an update must still carry a valid
/// signature either way.
const UPDATE_FEED: Option<&str> = Some("https://github.com/Sammysam/tallycheck/releases/latest/download/latest.json");

pub fn feed_url() -> Option<Url> {
    let raw = std::env::var("ACCOUNTING_UPDATE_URL").ok().filter(|v| !v.trim().is_empty());
    let raw = raw.as_deref().or(UPDATE_FEED)?;
    match raw.trim().parse() {
        Ok(url) => Some(url),
        Err(e) => {
            tracing::error!("update feed address is not a URL: {e}");
            None
        }
    }
}

pub struct TauriUpdater {
    pub app: AppHandle,
    pub feed: Url,
}

fn info(u: &Update) -> UpdateInfo {
    UpdateInfo { version: u.version.clone(), notes: u.body.clone(), date: u.date.map(|d| d.date().to_string()) }
}

/// The error as a sentence for the user; the detail goes to the log.
fn explain(e: tauri_plugin_updater::Error) -> String {
    tracing::warn!("update: {e}");
    use tauri_plugin_updater::Error as E;
    match e {
        E::Reqwest(_) | E::Network(_) => {
            "Could not reach the update server. Check the internet connection and try again.".into()
        }
        E::Minisign(_) | E::Base64(_) | E::SignatureUtf8(_) => {
            "The update could not be verified as coming from Cenango, so it was not installed.".into()
        }
        E::ReleaseNotFound => "No update information was found on the update server.".into(),
        other => format!("The update failed: {other}"),
    }
}

async fn find(app: &AppHandle, feed: &Url) -> Result<Option<Update>, String> {
    let updater = app.updater_builder().endpoints(vec![feed.clone()]).map_err(explain)?.build().map_err(explain)?;
    updater.check().await.map_err(explain)
}

impl Updater for TauriUpdater {
    fn check(&self) -> UpdateFuture<Option<UpdateInfo>> {
        let (app, feed) = (self.app.clone(), self.feed.clone());
        Box::pin(async move { Ok(find(&app, &feed).await?.as_ref().map(info)) })
    }

    fn install(&self) -> UpdateFuture<UpdateInfo> {
        let (app, feed) = (self.app.clone(), self.feed.clone());
        Box::pin(async move {
            let update = find(&app, &feed).await?.ok_or_else(|| "TallyCheck is up to date.".to_string())?;
            // `download` checks the signature; nothing runs unless it verifies.
            let bytes = update.download(|_, _| {}, || {}).await.map_err(explain)?;
            let found = info(&update);
            // Let the answer reach the page, close the database, then hand over to the
            // installer, which exits this process and opens the new version when it is done.
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(1500));
                crate::stop_server(&app);
                if let Err(e) = update.install(bytes) {
                    tracing::error!("could not start the update installer: {e}");
                    app.restart();
                }
            });
            Ok(found)
        })
    }
}
