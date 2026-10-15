use auth::throttle::LoginThrottle;
use auth::JwtKeys;
use database::Db;
use documents::FileStore;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    /// Encrypted store for employee photos and documents.
    pub files: FileStore,
    pub jwt: JwtKeys,
    pub throttle: Arc<LoginThrottle>,
    /// When false, only loopback `Host` headers are accepted (DNS-rebinding defence).
    pub lan_enabled: bool,
    pub port: u16,
}

impl AppState {
    pub fn new(db: Db, jwt: JwtKeys, files: FileStore, lan_enabled: bool, port: u16) -> Self {
        AppState { db, files, jwt, throttle: Arc::new(LoginThrottle::default()), lan_enabled, port }
    }
}
