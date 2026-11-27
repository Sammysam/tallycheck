//! D78 updates of the Microsoft Store build: the Store's own update check (`StoreContext`) behind
//! the server's `/api/system/update` routes, like the direct build's updater (D45). Only the
//! Microsoft Store is contacted, only when an Administrator opens the app or asks, and with no
//! internet the check fails quietly. The Store downloads the update (its own window may ask to
//! confirm); then the server and database are closed, the Store installs it and TallyCheck opens
//! the new version. A copy not installed from the Store (a test package) finds no update.

use api_server::updates::{UpdateFuture, UpdateInfo, Updater};
use tauri::Manager;

pub struct StoreUpdater {
    pub app: tauri::AppHandle,
}

/// The main window as a number (it crosses threads); the Store shows its windows over it.
fn window(app: &tauri::AppHandle) -> isize {
    app.get_webview_window("main").and_then(|w| w.hwnd().ok()).map(|h| h.0 as isize).unwrap_or_default()
}

async fn blocking<T: Send + 'static>(job: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(job).await.unwrap_or_else(|e| {
        tracing::error!("Store update task failed: {e}");
        Err("The Microsoft Store could not be asked about updates. Try again.".into())
    })
}

impl Updater for StoreUpdater {
    fn check(&self) -> UpdateFuture<Option<UpdateInfo>> {
        let hwnd = window(&self.app);
        Box::pin(blocking(move || imp::check(hwnd)))
    }

    fn install(&self) -> UpdateFuture<UpdateInfo> {
        let app = self.app.clone();
        let hwnd = window(&app);
        Box::pin(async move {
            // Downloaded while TallyCheck still runs; nothing is installed yet.
            let found = blocking(move || imp::download(hwnd)).await?;
            // Let the answer reach the page, close the database, then let the Store install it.
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(1500));
                crate::stop_server(&app);
                match imp::install(hwnd) {
                    Ok(()) => {
                        imp::open_new_version();
                        app.exit(0);
                    }
                    Err(e) => {
                        tracing::error!("Store update install: {e}");
                        app.restart();
                    }
                }
            });
            Ok(found)
        })
    }
}

#[allow(unsafe_code)]
mod imp {
    use api_server::updates::UpdateInfo;
    use std::ffi::c_void;
    use windows::core::{Interface, PCWSTR};
    use windows::ApplicationModel::Package;
    use windows::Services::Store::{
        StoreContext, StorePackageUpdate, StorePackageUpdateResult, StorePackageUpdateState,
    };
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Recovery::{RegisterApplicationRestart, REGISTER_APPLICATION_RESTART_FLAGS};
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
    use windows::Win32::UI::Shell::IInitializeWithWindow;
    use windows_collections::{IIterable, IVectorView};

    const UNREACHABLE: &str = "Could not reach the Microsoft Store. Check the internet connection and try again.";

    fn context(hwnd: isize) -> windows::core::Result<StoreContext> {
        // SAFETY: initialises WinRT for this worker thread; "already initialised" is fine.
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        let ctx = StoreContext::GetDefault()?;
        if hwnd != 0 {
            // A desktop app tells the Store which window its own windows belong to.
            let init: IInitializeWithWindow = ctx.cast()?;
            // SAFETY: `hwnd` is TallyCheck's own main window.
            unsafe { init.Initialize(HWND(hwnd as *mut c_void))? };
        }
        Ok(ctx)
    }

    fn pending(ctx: &StoreContext) -> windows::core::Result<IVectorView<StorePackageUpdate>> {
        ctx.GetAppAndOptionalStorePackageUpdatesAsync()?.join()
    }

    /// "1.2.0" from the package version 1.2.0.0 (the Store build's revision is always 0).
    fn info(updates: &IVectorView<StorePackageUpdate>) -> windows::core::Result<UpdateInfo> {
        let v = updates.GetAt(0)?.Package()?.Id()?.Version()?;
        let version = if v.Revision == 0 {
            format!("{}.{}.{}", v.Major, v.Minor, v.Build)
        } else {
            format!("{}.{}.{}.{}", v.Major, v.Minor, v.Build, v.Revision)
        };
        Ok(UpdateInfo { version, notes: None, date: None })
    }

    fn outcome(result: &StorePackageUpdateResult) -> Result<(), String> {
        match result.OverallState().map_err(|e| e.to_string())? {
            StorePackageUpdateState::Completed => Ok(()),
            StorePackageUpdateState::Canceled => Err("The update was cancelled.".into()),
            StorePackageUpdateState::OtherError | StorePackageUpdateState::ErrorLowBattery => {
                Err("The Microsoft Store could not update TallyCheck. Try again later.".into())
            }
            StorePackageUpdateState::ErrorWiFiRecommended | StorePackageUpdateState::ErrorWiFiRequired => {
                Err("The Microsoft Store wants a Wi-Fi connection for this update.".into())
            }
            other => Err(format!("The Microsoft Store did not finish the update ({other:?}).")),
        }
    }

    pub fn check(hwnd: isize) -> Result<Option<UpdateInfo>, String> {
        let found = (|| {
            let ups = pending(&context(hwnd)?)?;
            if ups.Size()? == 0 {
                Ok(None)
            } else {
                info(&ups).map(Some)
            }
        })();
        found.map_err(|e: windows::core::Error| {
            tracing::warn!("Store update check: {e}");
            UNREACHABLE.to_string()
        })
    }

    pub fn download(hwnd: isize) -> Result<UpdateInfo, String> {
        let log = |e: windows::core::Error| {
            tracing::warn!("Store update download: {e}");
            UNREACHABLE.to_string()
        };
        let ctx = context(hwnd).map_err(log)?;
        let ups = pending(&ctx).map_err(log)?;
        if ups.Size().map_err(log)? == 0 {
            return Err("TallyCheck is up to date.".into());
        }
        let found = info(&ups).map_err(log)?;
        let list: IIterable<StorePackageUpdate> = ups.cast().map_err(log)?;
        let result = ctx.RequestDownloadStorePackageUpdatesAsync(&list).map_err(log)?.join().map_err(log)?;
        outcome(&result)?;
        Ok(found)
    }

    pub fn install(hwnd: isize) -> Result<(), String> {
        // If Windows closes TallyCheck to replace it, it opens it again afterwards.
        // SAFETY: no command line and no flags.
        let _ = unsafe { RegisterApplicationRestart(PCWSTR::null(), REGISTER_APPLICATION_RESTART_FLAGS(0)) };
        let ctx = context(hwnd).map_err(|e| e.to_string())?;
        let ups = pending(&ctx).map_err(|e| e.to_string())?;
        if ups.Size().map_err(|e| e.to_string())? == 0 {
            return Ok(());
        }
        let list: IIterable<StorePackageUpdate> = ups.cast().map_err(|e| e.to_string())?;
        let silent = ctx.CanSilentlyDownloadStorePackageUpdates().unwrap_or(false);
        let op = if silent {
            ctx.TrySilentDownloadAndInstallStorePackageUpdatesAsync(&list)
        } else {
            ctx.RequestDownloadAndInstallStorePackageUpdatesAsync(&list)
        };
        let result = op.and_then(|op| op.join()).map_err(|e| e.to_string())?;
        outcome(&result)
    }

    /// Starts the installed (new) version through its Start-menu entry.
    pub fn open_new_version() {
        let family = Package::Current().and_then(|p| p.Id()).and_then(|id| id.FamilyName());
        match family {
            Ok(family) => {
                let target = format!("shell:AppsFolder\\{family}!TallyCheck");
                if let Err(e) = std::process::Command::new("explorer.exe").arg(target).spawn() {
                    tracing::warn!("could not open the updated TallyCheck: {e}");
                }
            }
            Err(e) => tracing::warn!("no package identity to open the updated TallyCheck: {e}"),
        }
    }
}
