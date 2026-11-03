use app_core::config::{Network, DEFAULT_LAN_NETWORKS};
use auth::throttle::LoginThrottle;
use auth::JwtKeys;
use database::Db;
use documents::FileStore;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    /// Encrypted store for employee photos and documents.
    pub files: FileStore,
    pub jwt: JwtKeys,
    pub throttle: Arc<LoginThrottle>,
    /// D49: failed password-recovery attempts, counted apart from sign-in failures.
    pub recovery_throttle: Arc<LoginThrottle>,
    /// When false, only loopback `Host` headers are accepted (DNS-rebinding defence).
    pub lan_enabled: bool,
    pub port: u16,
    /// Networks LAN clients may connect from, and the host names they may use.
    pub lan_allowed: Arc<Vec<Network>>,
    pub lan_host_names: Arc<Vec<String>>,
    /// The installation's data folder (backups, restore staging). `None` in some tests.
    pub data_dir: Option<PathBuf>,
    /// D45: the desktop shell's updater; `None` when the server runs on its own.
    pub updater: Option<Arc<dyn crate::updates::Updater>>,
    /// D47: a Microsoft Store copy is updated by the Store, never by our updater.
    pub update_channel: crate::updates::UpdateChannel,
    /// D49: the desktop shell's Windows verification; `None` when the server runs on its own.
    pub windows_verifier: Option<Arc<dyn crate::local_verify::WindowsVerifier>>,
}

impl AppState {
    pub fn new(db: Db, jwt: JwtKeys, files: FileStore, lan_enabled: bool, port: u16) -> Self {
        AppState {
            db,
            files,
            jwt,
            throttle: Arc::new(LoginThrottle::default()),
            recovery_throttle: Arc::new(LoginThrottle::default()),
            lan_enabled,
            port,
            lan_allowed: Arc::new(DEFAULT_LAN_NETWORKS.iter().filter_map(|n| Network::parse(n)).collect()),
            lan_host_names: Arc::new(Vec::new()),
            data_dir: None,
            updater: None,
            update_channel: crate::updates::UpdateChannel::Direct,
            windows_verifier: None,
        }
    }

    pub fn with_data_dir(mut self, dir: PathBuf) -> Self {
        self.data_dir = Some(dir);
        self
    }

    pub fn with_updater(
        mut self,
        updater: Option<Arc<dyn crate::updates::Updater>>,
        channel: crate::updates::UpdateChannel,
    ) -> Self {
        self.updater = updater;
        self.update_channel = channel;
        self
    }

    pub fn with_windows_verifier(mut self, verifier: Option<Arc<dyn crate::local_verify::WindowsVerifier>>) -> Self {
        self.windows_verifier = verifier;
        self
    }

    pub fn with_lan_policy(mut self, allowed: Vec<Network>, host_names: Vec<String>) -> Self {
        self.lan_allowed = Arc::new(allowed);
        self.lan_host_names = Arc::new(host_names);
        self
    }
}
