//! Runtime configuration.
//!
//! Resolution order (later wins): built-in defaults → `<data_dir>/config.toml` → environment.
//!
//! | Setting | config.toml | Environment | Default |
//! |---|---|---|---|
//! | data directory | — | `ACCOUNTING_DATA_DIR` | `%LOCALAPPDATA%\TallyCheckData` (or the older `CenangoAccounting`, D46); Microsoft Store build: `%USERPROFILE%\TallyCheckData` (D55); macOS: `~/Library/Application Support/TallyCheckData` (D53) |
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
/// The data folder under `%LOCALAPPDATA%` (D46). Not `TallyCheck`: that is where the installer
/// puts the program, and an uninstall or update would take the books with it. Installations from
/// before the rename keep their `CenangoAccounting` folder: it is used as it is, never moved or
/// copied.
pub const APP_DIR_NAME: &str = "TallyCheckData";
pub const LEGACY_APP_DIR_NAME: &str = "CenangoAccounting";
/// Set at *build* time only, for package tests: the test build uses this folder alone, so a test
/// install never opens a real company's books.
const TEST_APP_DIR_NAME: Option<&str> = option_env!("ACCOUNTING_APP_DIR_NAME");
/// D55: set at *build* time by `tools/build_msix.py`. A Microsoft Store (MSIX) app's writes to
/// AppData go to a private copy that Windows deletes on uninstall, and the Store refused the
/// restricted capability that would have excluded the data folder. The Store build therefore
/// keeps the books in the user's own folder (`%USERPROFILE%\TallyCheckData`), which MSIX does
/// not redirect: they survive updates and uninstalling. The direct installer is unchanged.
#[cfg(windows)]
const STORE_EDITION: bool = option_env!("ACCOUNTING_STORE_EDITION").is_some();

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
    #[error(
        "the data folder {0} is on a network drive; keep it on this PC's own disk (other PCs connect through LAN mode)"
    )]
    NetworkDataDir(PathBuf),
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    bind_address: Option<String>,
    port: Option<u16>,
    lan_enabled: Option<bool>,
    frontend_dir: Option<PathBuf>,
    /// LAN clients must come from one of these networks (CIDR). Default: private ranges.
    lan_allowed_networks: Option<Vec<String>>,
    /// Extra names LAN clients may use in the address bar (besides IPs and this PC's name).
    lan_host_names: Option<Vec<String>>,
}

/// An IP network in CIDR form, e.g. `192.168.1.0/24`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Network {
    addr: IpAddr,
    prefix: u8,
}

impl Network {
    pub fn parse(text: &str) -> Option<Network> {
        let (a, p) = text.trim().split_once('/').unwrap_or((text.trim(), ""));
        let addr: IpAddr = a.parse().ok()?;
        let max = if addr.is_ipv4() { 32 } else { 128 };
        let prefix = if p.is_empty() { max } else { p.parse().ok().filter(|n| *n <= max)? };
        Some(Network { addr, prefix })
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        let ip = match ip {
            IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(ip),
            v4 => v4,
        };
        match (self.addr, ip) {
            (IpAddr::V4(n), IpAddr::V4(a)) => {
                let mask = if self.prefix == 0 { 0 } else { u32::MAX << (32 - u32::from(self.prefix)) };
                u32::from(n) & mask == u32::from(a) & mask
            }
            (IpAddr::V6(n), IpAddr::V6(a)) => {
                let mask = if self.prefix == 0 { 0 } else { u128::MAX << (128 - u32::from(self.prefix)) };
                u128::from(n) & mask == u128::from(a) & mask
            }
            _ => false,
        }
    }
}

/// Private and link-local ranges: the office network, never the internet.
pub const DEFAULT_LAN_NETWORKS: &[&str] =
    &["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "169.254.0.0/16", "fc00::/7", "fe80::/10"];

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub data_dir: PathBuf,
    pub bind: SocketAddr,
    pub lan_enabled: bool,
    pub frontend_dir: Option<PathBuf>,
    pub lan_allowed: Vec<Network>,
    pub lan_host_names: Vec<String>,
}

impl AppConfig {
    pub fn database_path(&self) -> PathBuf {
        self.data_dir.join("accounting.db")
    }

    pub fn secrets_dir(&self) -> PathBuf {
        self.data_dir.join("secrets")
    }

    /// Refuses a data folder on a network drive or share: SQLite must never be opened by
    /// several PCs over the network (LAN clients use the main PC's API instead).
    pub fn check_local_data_dir(&self) -> Result<(), ConfigError> {
        if is_network_path(&self.data_dir) {
            return Err(ConfigError::NetworkDataDir(self.data_dir.clone()));
        }
        Ok(())
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
        let networks =
            file.lan_allowed_networks.unwrap_or_else(|| DEFAULT_LAN_NETWORKS.iter().map(|s| s.to_string()).collect());
        let lan_allowed = networks
            .into_iter()
            .map(|n| Network::parse(&n).ok_or(ConfigError::InvalidValue { name: "lan_allowed_networks", value: n }))
            .collect::<Result<Vec<_>, _>>()?;
        let lan_host_names = file.lan_host_names.unwrap_or_default();

        Ok(AppConfig {
            data_dir,
            bind: SocketAddr::new(ip, port),
            lan_enabled,
            frontend_dir,
            lan_allowed,
            lan_host_names,
        })
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
    Some(choose_data_dir(&data_base()?, TEST_APP_DIR_NAME))
}

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// The per-user folder for application data (the user's own folder for the Store build, D55).
#[cfg(windows)]
fn data_base() -> Option<PathBuf> {
    windows_base(STORE_EDITION, env_dir("LOCALAPPDATA"), env_dir("USERPROFILE"))
}

#[cfg(windows)]
fn windows_base(store: bool, local_app_data: Option<PathBuf>, profile: Option<PathBuf>) -> Option<PathBuf> {
    if store {
        profile
    } else {
        local_app_data
    }
}

/// D53: `~/Library/Application Support`, where macOS keeps each user's application data.
#[cfg(target_os = "macos")]
fn data_base() -> Option<PathBuf> {
    Some(env_dir("HOME")?.join("Library").join("Application Support"))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn data_base() -> Option<PathBuf> {
    env_dir("XDG_DATA_HOME").or_else(|| Some(env_dir("HOME")?.join(".local").join("share")))
}

/// `TallyCheckData`, unless only the older `CenangoAccounting` exists there (D46); a test build's
/// own folder name always wins.
fn choose_data_dir(base: &Path, test_name: Option<&str>) -> PathBuf {
    if let Some(name) = test_name {
        return base.join(name);
    }
    let (current, legacy) = (base.join(APP_DIR_NAME), base.join(LEGACY_APP_DIR_NAME));
    if !current.exists() && legacy.is_dir() {
        legacy
    } else {
        current
    }
}

/// UNC shares (`\\server\share`) and mapped network drives.
pub fn is_network_path(path: &Path) -> bool {
    let text = path.to_string_lossy();
    let verbatim_local = text.starts_with("\\\\?\\") && text.chars().nth(4).is_some_and(|c| c.is_ascii_alphabetic());
    if text.starts_with("\\\\") && !verbatim_local {
        return true;
    }
    drive::is_remote(path)
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod drive {
    use std::path::{Component, Path, Prefix};
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;

    const DRIVE_REMOTE: u32 = 4;

    pub fn is_remote(path: &Path) -> bool {
        let letter = match path.components().next() {
            Some(Component::Prefix(p)) => match p.kind() {
                Prefix::Disk(l) | Prefix::VerbatimDisk(l) => l,
                Prefix::UNC(..) | Prefix::VerbatimUNC(..) => return true,
                _ => return false,
            },
            _ => return false,
        };
        let root: Vec<u16> = format!("{}:\\", char::from(letter)).encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: `root` is a NUL-terminated UTF-16 string that outlives the call.
        unsafe { GetDriveTypeW(root.as_ptr()) == DRIVE_REMOTE }
    }
}

/// D53: a folder on a network volume (SMB, AFP, NFS under `/Volumes`) has no `MNT_LOCAL` flag.
/// A folder that does not exist yet is judged by its nearest existing parent.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod drive {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    const MNT_LOCAL: u32 = 0x0000_1000;

    pub fn is_remote(path: &Path) -> bool {
        let Some(existing) = path.ancestors().find(|p| p.exists()) else {
            return false;
        };
        let Ok(c_path) = CString::new(existing.as_os_str().as_bytes()) else {
            return false;
        };
        // SAFETY: `c_path` is a NUL-terminated path that outlives the call, and `stat` is a
        // plain C struct that statfs fills in completely on success.
        unsafe {
            let mut stat: libc::statfs = std::mem::zeroed();
            if libc::statfs(c_path.as_ptr(), &mut stat) != 0 {
                return false;
            }
            stat.f_flags & MNT_LOCAL == 0
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod drive {
    pub fn is_remote(_: &std::path::Path) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// D55: the Store build keeps the books outside AppData; the installer keeps LOCALAPPDATA.
    #[cfg(windows)]
    #[test]
    fn store_build_uses_the_profile_folder() {
        let (local, profile) = (Some(PathBuf::from(r"C:\U\AppData\Local")), Some(PathBuf::from(r"C:\U")));
        assert_eq!(windows_base(false, local.clone(), profile.clone()), local);
        assert_eq!(windows_base(true, local, profile.clone()), profile);
        let base = windows_base(true, None, profile).expect("profile");
        assert_eq!(choose_data_dir(&base, None), PathBuf::from(r"C:\U\TallyCheckData"));
    }

    /// D46: a new PC gets `TallyCheckData`; a PC from before the rename keeps using its
    /// `CenangoAccounting` folder in place, even next to the installer's own `TallyCheck` program
    /// folder; once `TallyCheckData` exists it wins; a test build's folder is used alone.
    #[test]
    fn data_folder_keeps_older_installations_in_place() -> Result<(), Box<dyn std::error::Error>> {
        let base = tempfile::tempdir()?;
        let b = base.path();
        assert_eq!(choose_data_dir(b, None), b.join("TallyCheckData"));
        std::fs::create_dir(b.join("TallyCheck"))?; // the installed program, never the data
        assert_eq!(choose_data_dir(b, None), b.join("TallyCheckData"));
        std::fs::create_dir(b.join("CenangoAccounting"))?;
        assert_eq!(choose_data_dir(b, None), b.join("CenangoAccounting"));
        assert_eq!(choose_data_dir(b, Some("TallyCheckStoreTest")), b.join("TallyCheckStoreTest"));
        std::fs::create_dir(b.join("TallyCheckData"))?;
        assert_eq!(choose_data_dir(b, None), b.join("TallyCheckData"));
        Ok(())
    }

    #[test]
    fn lan_networks_parse_and_match() -> Result<(), ConfigError> {
        let net = Network::parse("192.168.1.0/24").ok_or(ConfigError::NoDataDir)?;
        assert!(net.contains("192.168.1.77".parse().map_err(|_| ConfigError::NoDataDir)?));
        assert!(!net.contains("192.168.2.1".parse().map_err(|_| ConfigError::NoDataDir)?));
        assert!(net.contains("::ffff:192.168.1.5".parse().map_err(|_| ConfigError::NoDataDir)?), "IPv4-mapped");
        assert!(Network::parse("10.0.0.0/33").is_none());
        assert!(Network::parse("nonsense").is_none());
        let dir = tempfile::tempdir().map_err(|_| ConfigError::NoDataDir)?;
        let c = AppConfig::resolve(dir.path().into(), None, None, None, None)?;
        let public: IpAddr = "8.8.8.8".parse().map_err(|_| ConfigError::NoDataDir)?;
        assert!(!c.lan_allowed.iter().any(|n| n.contains(public)), "the internet is never a LAN client");
        assert!(super::is_network_path(Path::new(r"\\server\share\data")));
        assert!(!super::is_network_path(Path::new(r"\\?\C:\data")));
        Ok(())
    }

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
