//! Password hashing.
//!
//! New hashes are Argon2id (PHC string, crate defaults). Hashes imported from the reference are
//! bcrypt (cost 12); they still verify, and the caller rehashes them to Argon2id after a
//! successful login. Plaintext passwords are never stored or logged.

use app_core::{AppError, AppResult};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use std::sync::OnceLock;

#[derive(Debug, PartialEq, Eq)]
pub enum Verified {
    /// Correct password; `needs_rehash` is true for legacy (bcrypt) hashes.
    Match {
        needs_rehash: bool,
    },
    Mismatch,
}

pub fn hash(password: &str) -> AppResult<String> {
    let random = app_core::secrets::random_bytes(16).map_err(AppError::internal)?;
    let salt = SaltString::encode_b64(&random).map_err(|e| AppError::internal(format!("salt: {e}")))?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::internal(format!("argon2 hash failed: {e}")))
}

pub fn verify(password: &str, stored: &str) -> Verified {
    if stored.starts_with("$2") {
        return match bcrypt::verify(password, stored) {
            Ok(true) => Verified::Match { needs_rehash: true },
            Ok(false) => Verified::Mismatch,
            Err(e) => {
                tracing::warn!("stored bcrypt hash is malformed: {e}");
                Verified::Mismatch
            }
        };
    }
    match PasswordHash::new(stored) {
        Ok(parsed) => match Argon2::default().verify_password(password.as_bytes(), &parsed) {
            Ok(()) => Verified::Match { needs_rehash: false },
            Err(_) => Verified::Mismatch,
        },
        Err(e) => {
            tracing::warn!("stored password hash is malformed: {e}");
            Verified::Mismatch
        }
    }
}

/// Spends the same effort as a real check, so an unknown username is not revealed by timing.
pub fn burn_equivalent_time(password: &str) {
    static DUMMY: OnceLock<Option<String>> = OnceLock::new();
    if let Some(h) = DUMMY.get_or_init(|| hash("timing-equaliser").ok()) {
        let _ = verify(password, h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argon2_round_trip() -> AppResult<()> {
        let h = hash("correct horse")?;
        assert!(h.starts_with("$argon2id$"));
        assert!(!h.contains("correct horse"));
        assert_eq!(verify("correct horse", &h), Verified::Match { needs_rehash: false });
        assert_eq!(verify("wrong", &h), Verified::Mismatch);
        Ok(())
    }

    #[test]
    fn legacy_bcrypt_hash_verifies_and_asks_for_rehash() -> AppResult<()> {
        // Cost 12, like the reference's bcrypt.hash(password, 12).
        let legacy = bcrypt::hash("Passw0rd!", 12).map_err(AppError::internal)?;
        assert_eq!(verify("Passw0rd!", &legacy), Verified::Match { needs_rehash: true });
        assert_eq!(verify("passw0rd!", &legacy), Verified::Mismatch);
        Ok(())
    }

    #[test]
    fn hash_from_reference_node_bcrypt_verifies() {
        // Produced by the reference's own node_modules: bcrypt.hashSync('admin12345', 12).
        let node = "$2b$12$vILP5kfyj4QnB5EZkf2zxexAARY6Z4MaBJ4yKptNWs6MDBxqjERMm";
        assert_eq!(verify("admin12345", node), Verified::Match { needs_rehash: true });
        assert_eq!(verify("admin1234", node), Verified::Mismatch);
    }

    #[test]
    fn malformed_hash_never_matches() {
        assert_eq!(verify("x", "not-a-hash"), Verified::Mismatch);
        assert_eq!(verify("x", "$2b$12$short"), Verified::Mismatch);
    }
}
