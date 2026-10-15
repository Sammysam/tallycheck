//! The backup key: derived from the administrator's backup password with Argon2id.
//!
//! The password itself is never stored. The derived key (KEK) is kept DPAPI-protected in
//! `secrets/backup-kek.dpapi` so scheduled backups run unattended on this PC; its salt and
//! Argon2 parameters sit next to it in `backup-kek.json` and are copied into every backup's
//! header, so the same password re-derives the key on any other PC.

use crate::format::{hex, random, unhex, KeySlot};
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MIN_PASSWORD_CHARS: usize = 10;
const KEK_FILE: &str = "backup-kek.dpapi";
const PARAMS_FILE: &str = "backup-kek.json";

/// Argon2id cost: 64 MiB, 3 passes (OWASP-recommended range for interactive use).
const MEMORY_KIB: u32 = 64 * 1024;
const ITERATIONS: u32 = 3;
const PARALLELISM: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    #[error("no backup password has been set; set one before making backups")]
    NotSet,
    #[error("the backup password must be at least {MIN_PASSWORD_CHARS} characters")]
    TooShort,
    #[error("wrong backup password, or the backup file is damaged")]
    WrongPassword,
    #[error("key derivation failed: {0}")]
    Kdf(String),
    #[error("{0}")]
    Secret(#[from] app_core::secrets::SecretError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Format(#[from] crate::format::FormatError),
}

/// The key-encryption key plus the parameters that derive it from the password.
#[derive(Clone)]
pub struct Kek {
    key: [u8; 32],
    pub params: KdfParams,
}

impl std::fmt::Debug for Kek {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Kek(<redacted>)")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KdfParams {
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
    pub salt: String,
}

fn derive(password: &str, p: &KdfParams) -> Result<[u8; 32], KeyError> {
    let salt = unhex(&p.salt).ok_or(KeyError::WrongPassword)?;
    let params =
        Params::new(p.memory_kib, p.iterations, p.parallelism, Some(32)).map_err(|e| KeyError::Kdf(e.to_string()))?;
    let mut out = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), &salt, &mut out)
        .map_err(|e| KeyError::Kdf(e.to_string()))?;
    Ok(out)
}

impl Kek {
    /// A new key from a new password, with a fresh salt.
    pub fn from_new_password(password: &str) -> Result<Kek, KeyError> {
        if password.chars().count() < MIN_PASSWORD_CHARS {
            return Err(KeyError::TooShort);
        }
        let params = KdfParams {
            memory_kib: MEMORY_KIB,
            iterations: ITERATIONS,
            parallelism: PARALLELISM,
            salt: hex(&random(16)?),
        };
        Ok(Kek { key: derive(password, &params)?, params })
    }

    /// Re-derives the key a backup was made with.
    pub fn from_password_for(password: &str, slot: &KeySlot) -> Result<Kek, KeyError> {
        let params = KdfParams {
            memory_kib: slot.memory_kib,
            iterations: slot.iterations,
            parallelism: slot.parallelism,
            salt: slot.salt.clone(),
        };
        Ok(Kek { key: derive(password, &params)?, params })
    }

    /// Wraps a backup's data key.
    pub fn wrap(&self, data_key: &[u8; 32]) -> Result<KeySlot, KeyError> {
        let nonce = random(12)?;
        let wrapped = Aes256Gcm::new((&self.key).into())
            .encrypt(Nonce::from_slice(&nonce), data_key.as_slice())
            .map_err(|_| KeyError::Kdf("key wrapping failed".into()))?;
        Ok(KeySlot {
            kdf: "argon2id".into(),
            memory_kib: self.params.memory_kib,
            iterations: self.params.iterations,
            parallelism: self.params.parallelism,
            salt: self.params.salt.clone(),
            nonce: hex(&nonce),
            wrapped_key: hex(&wrapped),
        })
    }

    /// Unwraps a backup's data key; fails on a wrong password or a different salt.
    pub fn unwrap(&self, slot: &KeySlot) -> Result<[u8; 32], KeyError> {
        if slot.kdf != "argon2id" || slot.salt != self.params.salt {
            return Err(KeyError::WrongPassword);
        }
        let nonce = unhex(&slot.nonce).filter(|n| n.len() == 12).ok_or(KeyError::WrongPassword)?;
        let wrapped = unhex(&slot.wrapped_key).ok_or(KeyError::WrongPassword)?;
        let key = Aes256Gcm::new((&self.key).into())
            .decrypt(Nonce::from_slice(&nonce), wrapped.as_slice())
            .map_err(|_| KeyError::WrongPassword)?;
        key.try_into().map_err(|_| KeyError::WrongPassword)
    }
}

fn kek_path(secrets: &Path) -> PathBuf {
    secrets.join(KEK_FILE)
}

/// Saves the key for unattended backups on this PC (DPAPI) with its parameters.
pub fn save(secrets: &Path, kek: &Kek) -> Result<(), KeyError> {
    let params = serde_json::to_vec_pretty(&kek.params).map_err(|e| KeyError::Kdf(e.to_string()))?;
    std::fs::create_dir_all(secrets)?;
    let tmp = secrets.join(format!("{PARAMS_FILE}.tmp"));
    std::fs::write(&tmp, params)?;
    app_core::secrets::store(&kek_path(secrets), &kek.key)?;
    std::fs::rename(tmp, secrets.join(PARAMS_FILE))?;
    Ok(())
}

/// The saved key, if a backup password has been set on this PC.
pub fn load(secrets: &Path) -> Result<Option<Kek>, KeyError> {
    let params_path = secrets.join(PARAMS_FILE);
    if !params_path.exists() || !kek_path(secrets).exists() {
        return Ok(None);
    }
    let params: KdfParams =
        serde_json::from_slice(&std::fs::read(params_path)?).map_err(|e| KeyError::Kdf(e.to_string()))?;
    let key = app_core::secrets::load_or_create(&kek_path(secrets), 32)?;
    let key: [u8; 32] = key.try_into().map_err(|_| KeyError::NotSet)?;
    Ok(Some(Kek { key, params }))
}

/// True if `password` is the one currently set (re-derived with the saved salt).
pub fn verify(secrets: &Path, password: &str) -> Result<bool, KeyError> {
    let Some(kek) = load(secrets)? else { return Ok(false) };
    Ok(derive(password, &kek.params)? == kek.key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_wraps_and_unwraps_the_data_key_anywhere() -> Result<(), KeyError> {
        let dir = tempfile::tempdir()?;
        assert!(matches!(Kek::from_new_password("short"), Err(KeyError::TooShort)));
        let kek = Kek::from_new_password("correct horse battery")?;
        save(dir.path(), &kek)?;
        assert!(verify(dir.path(), "correct horse battery")?);
        assert!(!verify(dir.path(), "correct horse batterY")?);

        let data_key = [42u8; 32];
        let slot = kek.wrap(&data_key)?;
        // Same PC: the saved key opens it without the password.
        let saved = load(dir.path())?.ok_or(KeyError::NotSet)?;
        assert_eq!(saved.unwrap(&slot)?, data_key);
        // Another PC: the password re-derives the key from the slot's parameters.
        let elsewhere = Kek::from_password_for("correct horse battery", &slot)?;
        assert_eq!(elsewhere.unwrap(&slot)?, data_key);
        let wrong = Kek::from_password_for("wrong password!!", &slot)?;
        assert!(matches!(wrong.unwrap(&slot), Err(KeyError::WrongPassword)));
        assert!(!format!("{saved:?}").contains("42"));
        Ok(())
    }
}
