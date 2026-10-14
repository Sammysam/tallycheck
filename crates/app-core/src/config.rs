//! Runtime configuration.
//!
//! Resolution order (later wins): built-in defaults → `<data_dir>/config.toml` → environment.
//!
//! | Setting | config.toml | Environment | Default |
//! |---|---|---|---|
//! | data directory | — | `ACCOUNTING_DATA_DIR` | `%LOCALAPPDATA%\CenangoAccounting` |
//! | bind address | `bind_address` | `ACCOUNTING_BIND` | `127.0.0.1` |
//! | port | `port` | `ACCOUNTING_PORT` | `47800` |
//! | LAN access | `lan_enabled` | `ACCOUNTING_LAN` | `false` |
//! | frontend build dir | `frontend_dir` | `ACCOUNTING_FRONTEND_DIR` | none |
//!
//! Binding to anything but a loopback address requires `lan_enabled = true`.

use serde::Deserialize;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

pub const DEFAULT_PORT: u16 = 47800;
const APP_DIR_NAME: &str = "CenangoAccounting";

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot determine the data directory: set ACCOUNTING_DATA_DIR")]
    NoDataDir,
    #[error("cannot read {path}: {source}")]
    Read { path: PathBuf, source: std::io::Error },
    #[error("invalid {path}: {source}")]
    Parse { path: PathBuf, source: Box<toml::de::Error> },
    #[error("invalid value for {name}: {value}")]
    InvalidValue { name: &'static str, value: String },
    #[error("bind address {0} is not loopback; set lan_enabled = true in config.toml to allow LAN access")]
    LanDisabled(IpAddr),
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    bind_address: Option<String>,
    port: Option<u16>,
    lan_enabled: Option<bool>,
    frontend_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub data_dir: PathBuf,
    pub bind: SocketAddr,
    pub lan_enabled: bool,
    pub frontend_dir: Option<PathBuf>,
}

impl AppConfig {
    pub fn database_path(&self) -> PathBuf {
        self.data_dir.join("accounting.db")
    }

    pub fn secrets_dir(&self) -> PathBuf {
        self.data_dir.join("secrets")
    }

    pub fn log_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    /// Loads configuration from the environment and `<data_dir>/config.toml`.
    pub fn load() -> Result<Self, ConfigError> {
        let data_dir = match std::env::var_os("ACCOUNTING_DATA_DIR") {
            Some(d) if !d.is_empty() => PathBuf::from(d),
            _ => default_data_dir().ok_or(ConfigError::NoDataDir)?,
        };
        let env = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        Self::resolve(
            data_dir,
            env("ACCOUNTING_BIND"),
            env("ACCOUNTING_PORT"),
            env("ACCOUNTING_LAN"),
            env("ACCOUNTING_FRONTEND_DIR"),
        )
    }

    /// Pure resolution step, separated from the process environment for testing.
    pub fn resolve(
        data_dir: PathBuf,
        bind_env: Option<String>,
        port_env: Option<String>,
        lan_env: Option<String>,
        frontend_env: Option<String>,
    ) -> Result<Self, ConfigError> {
        let file = read_file_config(&data_dir.join("config.toml"))?;

        let bind_text = bind_env.or(file.bind_address);
        let ip = match bind_text {
            Some(t) => {
                t.trim().parse::<IpAddr>().map_err(|_| ConfigError::InvalidValue { name: "bind_address", value: t })?
            }
            None => IpAddr::V4(Ipv4Addr::LOCALHOST),
        };
        let port = match port_env {
            Some(p) => p.trim().parse::<u16>().map_err(|_| ConfigError::InvalidValue { name: "port", value: p })?,
            None => file.port.unwrap_or(DEFAULT_PORT),
        };
        let lan_enabled = match lan_env {
            Some(v) => parse_bool(&v).ok_or(ConfigError::InvalidValue { name: "lan_enabled", value: v })?,
            None => file.lan_enabled.unwrap_or(false),
        };
        if !lan_enabled && !ip.is_loopback() {
            return Err(ConfigError::LanDisabled(ip));
        }
        let frontend_dir = frontend_env.map(PathBuf::from).or(file.frontend_dir);

        Ok(AppConfig { data_dir, bind: SocketAddr::new(ip, port), lan_enabled, frontend_dir })
    }
}

fn parse_bool(v: &str) -> Option<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn read_file_config(path: &Path) -> Result<FileConfig, ConfigError> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            toml::from_str(&text).map_err(|source| ConfigError::Parse { path: path.into(), source: Box::new(source) })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(FileConfig::default()),
        Err(source) => Err(ConfigError::Read { path: path.into(), source }),
    }
}

fn default_data_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()).map(|base| PathBuf::from(base).join(APP_DIR_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_loopback() -> Result<(), ConfigError> {
        let dir = tempfile::tempdir().map_err(|_| ConfigError::NoDataDir)?;
        let c = AppConfig::resolve(dir.path().into(), None, None, None, None)?;
        assert_eq!(c.bind, SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), DEFAULT_PORT));
        assert!(!c.lan_enabled);
        Ok(())
    }

    #[test]
    fn lan_bind_requires_explicit_opt_in() -> Result<(), ConfigError> {
        let dir = tempfile::tempdir().map_err(|_| ConfigError::NoDataDir)?;
        let refused = AppConfig::resolve(dir.path().into(), Some("0.0.0.0".into()), None, None, None);
        assert!(matches!(refused, Err(ConfigError::LanDisabled(_))));
        let allowed = AppConfig::resolve(dir.path().into(), Some("0.0.0.0".into()), None, Some("true".into()), None)?;
        assert!(allowed.lan_enabled);
        Ok(())
    }

    #[test]
    fn file_config_is_read_and_env_wins() -> Result<(), ConfigError> {
        let dir = tempfile::tempdir().map_err(|_| ConfigError::NoDataDir)?;
        std::fs::write(dir.path().join("config.toml"), "port = 5000\n")
            .map_err(|source| ConfigError::Read { path: dir.path().into(), source })?;
        let c = AppConfig::resolve(dir.path().into(), None, None, None, None)?;
        assert_eq!(c.bind.port(), 5000);
        let c = AppConfig::resolve(dir.path().into(), None, Some("6000".into()), None, None)?;
        assert_eq!(c.bind.port(), 6000);
        Ok(())
    }

    #[test]
    fn unknown_config_keys_are_rejected() -> Result<(), ConfigError> {
        let dir = tempfile::tempdir().map_err(|_| ConfigError::NoDataDir)?;
        std::fs::write(dir.path().join("config.toml"), "lan = true\n")
            .map_err(|source| ConfigError::Read { path: dir.path().into(), source })?;
        let r = AppConfig::resolve(dir.path().into(), None, None, None, None);
        assert!(matches!(r, Err(ConfigError::Parse { .. })));
        Ok(())
    }
}
