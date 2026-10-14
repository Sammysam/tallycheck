use auth::throttle::LoginThrottle;
use auth::JwtKeys;
use database::Db;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub jwt: JwtKeys,
    pub throttle: Arc<LoginThrottle>,
    /// When false, only loopback `Host` headers are accepted (DNS-rebinding defence).
    pub lan_enabled: bool,
    pub port: u16,
}

impl AppState {
    pub fn new(db: Db, jwt: JwtKeys, lan_enabled: bool, port: u16) -> Self {
        AppState { db, jwt, throttle: Arc::new(LoginThrottle::default()), lan_enabled, port }
    }
}
