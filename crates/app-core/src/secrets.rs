//! Installation secrets (JWT signing key now; the file-encryption key in W4), stored
//! encrypted with the Windows Data Protection API (DPAPI, current-user scope) so the raw key
//! never sits on disk next to the database. On macOS (D53) the files are encrypted with
//! AES-256-GCM under a key kept in the user's login Keychain instead.
//!
//! DPAPI (and the Keychain) ties the ciphertext to the user account: a copied file cannot be
//! decrypted on another PC or by another user. Keys that must survive a reinstall or a new PC (the W4
//! file-encryption key) additionally need an offline recovery copy — that is handled by the
//! backup design, not here.

use std::fs;
use std::io;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("secret file I/O: {0}")]
    Io(#[from] io::Error),
    #[error("random number generator failed: {0}")]
    Random(String),
    #[error("DPAPI failed: {0}")]
    Dpapi(String),
    #[error("macOS Keychain failed: {0}")]
    Keychain(String),
    #[error("secret file is damaged or was made on another computer")]
    Damaged,
    #[error("secret file has the wrong length (expected {expected} bytes, got {actual})")]
    WrongLength { expected: usize, actual: usize },
}

/// Application-specific entropy mixed into DPAPI so other programs running as the same user
/// cannot decrypt the blob by accident.
const ENTROPY: &[u8] = b"accounting-desktop/secret/v1";

/// File names of the installation secrets inside `<data_dir>/secrets`.
pub const JWT_SECRET_FILE: &str = "jwt-secret.dpapi";
pub const STORAGE_KEY_FILE: &str = "storage-key.dpapi";

pub fn random_bytes(len: usize) -> Result<Vec<u8>, SecretError> {
    let mut buf = vec![0u8; len];
    getrandom::getrandom(&mut buf).map_err(|e| SecretError::Random(e.to_string()))?;
    Ok(buf)
}

/// Loads the secret at `path`, creating a new random one of `len` bytes on first run.
pub fn load_or_create(path: &Path, len: usize) -> Result<Vec<u8>, SecretError> {
    if path.exists() {
        let blob = fs::read(path)?;
        let secret = dpapi::unprotect(&blob, ENTROPY, dir_of(path))?;
        if secret.len() != len {
            return Err(SecretError::WrongLength { expected: len, actual: secret.len() });
        }
        return Ok(secret);
    }
    let secret = random_bytes(len)?;
    store(path, &secret)?;
    Ok(secret)
}

/// D39: the secret at `path` (an integration's access key), or None when there is none.
pub fn load(path: &Path) -> Result<Option<Vec<u8>>, SecretError> {
    if !path.exists() {
        return Ok(None);
    }
    let blob = fs::read(path)?;
    Ok(Some(dpapi::unprotect(&blob, ENTROPY, dir_of(path))?))
}

/// D39: forgets a stored secret (disconnecting an integration).
pub fn remove(path: &Path) -> Result<(), SecretError> {
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

/// Protects `secret` with DPAPI and writes it to `path`, replacing any existing file
/// (used when an administrator restores a key from its offline recovery copy).
pub fn store(path: &Path, secret: &[u8]) -> Result<(), SecretError> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let blob = dpapi::protect(secret, ENTROPY, dir_of(path))?;
    // Write then rename so a crash never leaves a truncated key file behind.
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, &blob)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// The folder holding the secret files (`<data_dir>/secrets`).
fn dir_of(path: &Path) -> &Path {
    path.parent().unwrap_or_else(|| Path::new("."))
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod dpapi {
    use super::SecretError;
    use std::ptr;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    fn blob(data: &[u8]) -> Result<CRYPT_INTEGER_BLOB, SecretError> {
        Ok(CRYPT_INTEGER_BLOB {
            cbData: u32::try_from(data.len()).map_err(|_| SecretError::Dpapi("input too large".into()))?,
            pbData: data.as_ptr().cast_mut(),
        })
    }

    /// Copies the DPAPI-allocated output and frees it.
    fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        if out.pbData.is_null() {
            return Vec::new();
        }
        // SAFETY: on success DPAPI returns a LocalAlloc'd buffer of exactly cbData bytes; we
        // copy it before releasing it with LocalFree, exactly once.
        unsafe {
            let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            LocalFree(out.pbData.cast());
            v
        }
    }

    pub fn protect(data: &[u8], entropy: &[u8], _dir: &std::path::Path) -> Result<Vec<u8>, SecretError> {
        let input = blob(data)?;
        let ent = blob(entropy)?;
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: ptr::null_mut() };
        // SAFETY: all pointers reference live, correctly sized buffers for the duration of
        // the call; DPAPI does not retain them. `out` is released by `take`.
        let ok = unsafe {
            CryptProtectData(&input, ptr::null(), &ent, ptr::null(), ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        };
        if ok == 0 {
            return Err(SecretError::Dpapi(std::io::Error::last_os_error().to_string()));
        }
        Ok(take(out))
    }

    pub fn unprotect(data: &[u8], entropy: &[u8], _dir: &std::path::Path) -> Result<Vec<u8>, SecretError> {
        let input = blob(data)?;
        let ent = blob(entropy)?;
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: ptr::null_mut() };
        // SAFETY: as in `protect`.
        let ok = unsafe {
            CryptUnprotectData(
                &input,
                ptr::null_mut(),
                &ent,
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        };
        if ok == 0 {
            return Err(SecretError::Dpapi(std::io::Error::last_os_error().to_string()));
        }
        Ok(take(out))
    }
}

/// D53, macOS: AES-256-GCM under a 32-byte key in the login Keychain (generic password, service
/// `KEYCHAIN_SERVICE`). Each data folder has its own Keychain item, named by a random id kept in
/// `secrets/keychain-id`, so moving the folder keeps working while a copy on another Mac (or for
/// another user) cannot be opened, as with DPAPI. File: `TCK1` | nonce(12) | ciphertext+tag;
/// the entropy is the associated data.
#[cfg(target_os = "macos")]
mod dpapi {
    use super::{random_bytes, SecretError};
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use aes_gcm::{Aes256Gcm, Nonce};
    use security_framework::passwords::{get_generic_password, set_generic_password};
    use std::path::Path;

    const KEYCHAIN_SERVICE: &str = "TallyCheck data protection";
    const ID_FILE: &str = "keychain-id";
    const MAGIC: &[u8; 4] = b"TCK1";
    /// errSecItemNotFound
    const NOT_FOUND: i32 = -25300;

    fn key_id(dir: &Path, create: bool) -> Result<String, SecretError> {
        let file = dir.join(ID_FILE);
        match std::fs::read_to_string(&file) {
            Ok(id) if id.trim().len() == 32 && id.trim().bytes().all(|b| b.is_ascii_hexdigit()) => {
                Ok(id.trim().to_string())
            }
            Ok(_) => Err(SecretError::Damaged),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && create => {
                let id: String = random_bytes(16)?.iter().map(|b| format!("{b:02x}")).collect();
                std::fs::create_dir_all(dir)?;
                std::fs::write(&file, &id)?;
                Ok(id)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(SecretError::Damaged),
            Err(e) => Err(e.into()),
        }
    }

    fn master_key(dir: &Path, create: bool) -> Result<Vec<u8>, SecretError> {
        let id = key_id(dir, create)?;
        match get_generic_password(KEYCHAIN_SERVICE, &id) {
            Ok(key) if key.len() == 32 => Ok(key),
            Ok(_) => Err(SecretError::Damaged),
            Err(e) if e.code() == NOT_FOUND && create => {
                let key = random_bytes(32)?;
                set_generic_password(KEYCHAIN_SERVICE, &id, &key).map_err(|e| SecretError::Keychain(e.to_string()))?;
                Ok(key)
            }
            Err(e) if e.code() == NOT_FOUND => Err(SecretError::Damaged),
            Err(e) => Err(SecretError::Keychain(e.to_string())),
        }
    }

    fn cipher(key: &[u8]) -> Result<Aes256Gcm, SecretError> {
        Aes256Gcm::new_from_slice(key).map_err(|_| SecretError::Damaged)
    }

    pub fn protect(data: &[u8], entropy: &[u8], dir: &Path) -> Result<Vec<u8>, SecretError> {
        let key = master_key(dir, true)?;
        let nonce = random_bytes(12)?;
        let sealed = cipher(&key)?
            .encrypt(Nonce::from_slice(&nonce), Payload { msg: data, aad: entropy })
            .map_err(|_| SecretError::Keychain("encryption failed".into()))?;
        let mut out = Vec::with_capacity(4 + 12 + sealed.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&sealed);
        Ok(out)
    }

    pub fn unprotect(blob: &[u8], entropy: &[u8], dir: &Path) -> Result<Vec<u8>, SecretError> {
        if blob.len() < 4 + 12 + 16 || &blob[..4] != MAGIC {
            return Err(SecretError::Damaged);
        }
        let key = master_key(dir, false)?;
        cipher(&key)?
            .decrypt(Nonce::from_slice(&blob[4..16]), Payload { msg: &blob[16..], aad: entropy })
            .map_err(|_| SecretError::Damaged)
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod dpapi {
    use super::SecretError;

    pub fn protect(_: &[u8], _: &[u8], _: &std::path::Path) -> Result<Vec<u8>, SecretError> {
        Err(SecretError::Dpapi("protected key storage is only available on Windows and macOS".into()))
    }

    pub fn unprotect(_: &[u8], _: &[u8], _: &std::path::Path) -> Result<Vec<u8>, SecretError> {
        Err(SecretError::Dpapi("protected key storage is only available on Windows and macOS".into()))
    }
}

#[cfg(all(test, any(windows, target_os = "macos")))]
mod tests {
    use super::*;

    #[test]
    fn secret_is_created_once_and_never_stored_in_plaintext() -> Result<(), SecretError> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("secrets").join("jwt.dpapi");
        let first = load_or_create(&path, 64)?;
        assert_eq!(first.len(), 64);
        let on_disk = fs::read(&path)?;
        assert!(!on_disk.windows(first.len()).any(|w| w == first.as_slice()));
        let second = load_or_create(&path, 64)?;
        assert_eq!(first, second);
        Ok(())
    }

    #[test]
    fn tampered_blob_is_rejected() -> Result<(), SecretError> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("k.dpapi");
        load_or_create(&path, 32)?;
        let mut blob = fs::read(&path)?;
        let last = blob.len() - 1;
        blob[last] ^= 0xff;
        fs::write(&path, &blob)?;
        assert!(load_or_create(&path, 32).is_err());
        Ok(())
    }
}
