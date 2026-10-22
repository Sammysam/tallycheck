//! Installation secrets (JWT signing key now; the file-encryption key in W4), stored
//! encrypted with the Windows Data Protection API (DPAPI, current-user scope) so the raw key
//! never sits on disk next to the database.
//!
//! DPAPI ties the ciphertext to the Windows user account: a copied file cannot be decrypted on
//! another PC or by another user. Keys that must survive a reinstall or a new PC (the W4
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
        let secret = dpapi::unprotect(&blob, ENTROPY)?;
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
    Ok(Some(dpapi::unprotect(&blob, ENTROPY)?))
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
    let blob = dpapi::protect(secret, ENTROPY)?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    // Write then rename so a crash never leaves a truncated key file behind.
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, &blob)?;
    fs::rename(&tmp, path)?;
    Ok(())
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

    pub fn protect(data: &[u8], entropy: &[u8]) -> Result<Vec<u8>, SecretError> {
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

    pub fn unprotect(data: &[u8], entropy: &[u8]) -> Result<Vec<u8>, SecretError> {
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

#[cfg(not(windows))]
mod dpapi {
    use super::SecretError;

    pub fn protect(_: &[u8], _: &[u8]) -> Result<Vec<u8>, SecretError> {
        Err(SecretError::Dpapi("DPAPI is only available on Windows".into()))
    }

    pub fn unprotect(_: &[u8], _: &[u8]) -> Result<Vec<u8>, SecretError> {
        Err(SecretError::Dpapi("DPAPI is only available on Windows".into()))
    }
}

#[cfg(all(test, windows))]
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
