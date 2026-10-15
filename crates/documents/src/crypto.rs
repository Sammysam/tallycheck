//! AES-256-GCM in the reference's container format.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};

pub const MAGIC: &[u8; 4] = b"QBE1";
const IV_LEN: usize = 12;
const TAG_LEN: usize = 16;
const HEADER_LEN: usize = MAGIC.len() + IV_LEN + TAG_LEN;

#[derive(Debug)]
pub enum CryptoError {
    NotEncrypted,
    /// Wrong key, or the file was altered or truncated: the GCM tag did not verify.
    AuthenticationFailed,
    BadKey(String),
    Random(String),
}

impl std::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CryptoError::NotEncrypted => f.write_str("Object is not encrypted"),
            CryptoError::AuthenticationFailed => {
                f.write_str("Stored file failed authentication (wrong key, or the file is damaged)")
            }
            CryptoError::BadKey(m) => write!(f, "{m}"),
            CryptoError::Random(m) => write!(f, "random number generator failed: {m}"),
        }
    }
}

impl std::error::Error for CryptoError {}

/// A 32-byte AES-256 key. `Debug` never shows the bytes.
#[derive(Clone)]
pub struct StorageKey([u8; 32]);

impl std::fmt::Debug for StorageKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StorageKey(<redacted>)")
    }
}

impl StorageKey {
    pub fn from_bytes(bytes: &[u8]) -> Result<StorageKey, CryptoError> {
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| CryptoError::BadKey(format!("storage key must be 32 bytes, got {}", bytes.len())))?;
        Ok(StorageKey(arr))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Lower-case hex, for an offline recovery copy (`openssl rand -hex 32` style).
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// Reference `parseKey`: 64 hex characters, otherwise base64; either way exactly 32 bytes.
pub fn parse_key(raw: &str) -> Result<StorageKey, CryptoError> {
    let v = raw.trim();
    if v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()) {
        let bytes: Vec<u8> = (0..32)
            .map(|i| u8::from_str_radix(&v[i * 2..i * 2 + 2], 16))
            .collect::<Result<_, _>>()
            .map_err(|e| CryptoError::BadKey(e.to_string()))?;
        return StorageKey::from_bytes(&bytes);
    }
    let bytes = base64_decode(v).ok_or_else(|| {
        CryptoError::BadKey("STORAGE_ENCRYPTION_KEY must be 32 bytes: 64 hex characters or 44 base64 characters".into())
    })?;
    StorageKey::from_bytes(&bytes).map_err(|_| {
        CryptoError::BadKey("STORAGE_ENCRYPTION_KEY must be 32 bytes: 64 hex characters or 44 base64 characters".into())
    })
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let mut buf = 0u32;
    let mut bits = 0;
    for c in s.trim_end_matches('=').bytes() {
        let v = ALPHABET.iter().position(|&a| a == c)? as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

pub fn is_encrypted(data: &[u8]) -> bool {
    data.len() >= HEADER_LEN && &data[..MAGIC.len()] == MAGIC
}

fn cipher(key: &StorageKey) -> Aes256Gcm {
    Aes256Gcm::new(key.as_bytes().into())
}

/// Encrypts with a fresh random 96-bit IV for every call.
pub fn encrypt(plain: &[u8], key: &StorageKey) -> Result<Vec<u8>, CryptoError> {
    let mut iv = [0u8; IV_LEN];
    getrandom_fill(&mut iv)?;
    let sealed = cipher(key)
        .encrypt(Nonce::from_slice(&iv), plain)
        .map_err(|_| CryptoError::Random("encryption failed".into()))?;
    // aes-gcm returns ciphertext || tag; the reference stores tag before ciphertext.
    let (body, tag) = sealed.split_at(sealed.len() - TAG_LEN);
    let mut out = Vec::with_capacity(HEADER_LEN + body.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&iv);
    out.extend_from_slice(tag);
    out.extend_from_slice(body);
    Ok(out)
}

pub fn decrypt(data: &[u8], key: &StorageKey) -> Result<Vec<u8>, CryptoError> {
    if !is_encrypted(data) {
        return Err(CryptoError::NotEncrypted);
    }
    let iv = &data[MAGIC.len()..MAGIC.len() + IV_LEN];
    let tag = &data[MAGIC.len() + IV_LEN..HEADER_LEN];
    let body = &data[HEADER_LEN..];
    let mut sealed = Vec::with_capacity(body.len() + TAG_LEN);
    sealed.extend_from_slice(body);
    sealed.extend_from_slice(tag);
    cipher(key).decrypt(Nonce::from_slice(iv), sealed.as_slice()).map_err(|_| CryptoError::AuthenticationFailed)
}

fn getrandom_fill(buf: &mut [u8]) -> Result<(), CryptoError> {
    let bytes = app_core::secrets::random_bytes(buf.len()).map_err(|e| CryptoError::Random(e.to_string()))?;
    buf.copy_from_slice(&bytes);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len() / 2).filter_map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()).collect()
    }

    fn fixture() -> Value {
        serde_json::from_str(include_str!("../tests/fixtures/reference_encryption.json")).unwrap_or(Value::Null)
    }

    #[test]
    fn decrypts_files_written_by_the_reference() -> Result<(), CryptoError> {
        let fx = fixture();
        let key = parse_key(fx["keyHex"].as_str().unwrap_or_default())?;
        let cases = fx["cases"].as_array().cloned().unwrap_or_default();
        assert_eq!(cases.len(), 3);
        for c in cases {
            let cipher = unhex(c["cipher"].as_str().unwrap_or_default());
            assert_eq!(decrypt(&cipher, &key)?, unhex(c["plain"].as_str().unwrap_or_default()));
        }
        Ok(())
    }

    #[test]
    fn base64_keys_parse_like_the_reference() -> Result<(), CryptoError> {
        let fx = fixture();
        let k = parse_key(fx["keyB64"].as_str().unwrap_or_default())?;
        assert_eq!(k.to_hex(), fx["base64KeyParsesTo"].as_str().unwrap_or_default());
        assert!(matches!(parse_key("tooshort"), Err(CryptoError::BadKey(_))));
        assert!(matches!(parse_key(&"g".repeat(64)), Err(CryptoError::BadKey(_))));
        Ok(())
    }

    #[test]
    fn round_trip_uses_a_fresh_iv_and_detects_tampering() -> Result<(), CryptoError> {
        let key = StorageKey::from_bytes(&[7u8; 32])?;
        let a = encrypt(b"payslip", &key)?;
        let b = encrypt(b"payslip", &key)?;
        assert_ne!(a, b, "a fresh IV each time");
        assert_eq!(&a[..4], b"QBE1");
        assert_eq!(decrypt(&a, &key)?, b"payslip");

        let mut flipped = a.clone();
        let last = flipped.len() - 1;
        flipped[last] ^= 1;
        assert!(matches!(decrypt(&flipped, &key), Err(CryptoError::AuthenticationFailed)));
        let mut bad_tag = a.clone();
        bad_tag[20] ^= 1;
        assert!(matches!(decrypt(&bad_tag, &key), Err(CryptoError::AuthenticationFailed)));
        let other = StorageKey::from_bytes(&[8u8; 32])?;
        assert!(matches!(decrypt(&a, &other), Err(CryptoError::AuthenticationFailed)));
        assert!(matches!(decrypt(&a[..20], &key), Err(CryptoError::NotEncrypted)));
        assert!(matches!(decrypt(b"%PDF-1.4 plain", &key), Err(CryptoError::NotEncrypted)));
        assert!(!format!("{key:?}").contains('7'));
        Ok(())
    }
}

/// Cross-check in the other direction: the reference's own `decryptBuffer` must read what this
/// code writes. Runs when `REFERENCE_BACKEND` points at the reference `backend/` directory
/// (Node required); without it the test only checks the container layout.
#[cfg(test)]
mod reference_reads_our_files {
    use super::*;

    #[test]
    fn reference_decrypts_a_file_written_here() -> Result<(), Box<dyn std::error::Error>> {
        let key = parse_key("00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff")?;
        let message = "Written by the Rust port — Sinhala/Tamil safe: ශ්‍රී ලංකා";
        let sealed = encrypt(message.as_bytes(), &key)?;
        assert_eq!(&sealed[..4], b"QBE1");
        let Ok(backend) = std::env::var("REFERENCE_BACKEND") else { return Ok(()) };
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("from-rust.bin");
        std::fs::write(&file, &sealed)?;
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/gen_encryption_fixture.mjs");
        let out = std::process::Command::new("node")
            .args([script, "--verify", &backend, &key.to_hex()])
            .arg(&file)
            .output()?;
        assert!(out.status.success(), "node failed: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8(out.stdout)?, message);
        Ok(())
    }
}
