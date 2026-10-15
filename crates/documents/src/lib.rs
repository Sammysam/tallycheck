//! Encrypted file storage for employee photos, documents and filed payslips
//! (reference `backend/src/common/storage`).
//!
//! * Format, byte for byte the reference's `encryption.ts`:
//!   `"QBE1"(4) | IV(12) | TAG(16) | AES-256-GCM ciphertext`, raw 32-byte key, no AAD.
//!   Files without the magic are legacy plaintext and are returned as stored (as the reference
//!   did for uploads made before encryption was switched on).
//! * Files live under `<data_dir>/files/<key>` with a `<key>.meta` sidecar holding the content
//!   type — the reference's local driver layout, so its storage keys carry over unchanged.
//! * The key is never written in plain text: the app keeps it DPAPI-protected
//!   (`app_core::secrets`) and an administrator can export it for an offline recovery copy.

pub mod crypto;
pub mod store;

pub use crypto::{decrypt, encrypt, is_encrypted, parse_key, CryptoError, StorageKey};
pub use store::{FileStore, StoredFile};
