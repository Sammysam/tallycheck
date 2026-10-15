//! Employee records (reference W4 modules `employees` and the salary half of `payroll`):
//! profiles, profile photos, versioned documents, dated salary structures and bank details.
//!
//! Files go through `documents::FileStore` (AES-256-GCM at rest). A file is written before its
//! row and removed again if the row cannot be saved, so a failed request never leaves a row
//! pointing at a missing file. Old files are removed only after the row change committed.

pub mod docs;
pub mod records;
pub mod salaries;

use app_core::AppResult;
use documents::FileStore;

/// A file received from a multipart upload (multer's `file`).
#[derive(Debug, Clone)]
pub struct Upload {
    pub original_name: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

/// Who performed an upload or created a salary row.
#[derive(Debug, Clone, Default)]
pub struct Actor {
    pub id: Option<String>,
    pub username: Option<String>,
}

pub const MAX_DOCUMENT_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_PHOTO_BYTES: usize = 5 * 1024 * 1024;

pub const DOCUMENT_MIMES: &[&str] = &[
    "application/pdf",
    "image/jpeg",
    "image/png",
    "image/webp",
    "application/msword",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
];

pub const PHOTO_MIMES: &[&str] = &["image/jpeg", "image/png", "image/webp"];

/// Reference `extension()`: the lower-cased extension of the original name when it is short and
/// plain (`^\.[a-z0-9]{1,5}$`), otherwise nothing.
pub fn extension(original_name: &str) -> String {
    // Node's (POSIX) path.extname: the last '.' of the base name, unless the name starts with it.
    let base = original_name.rsplit('/').next().unwrap_or("");
    let ext = match base.rfind('.') {
        Some(i) if i > 0 => base[i..].to_lowercase(),
        _ => String::new(),
    };
    let ok = ext.len() >= 2 && ext.len() <= 6 && ext[1..].bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    if ok {
        ext
    } else {
        String::new()
    }
}

/// Deletes files after the database change they belong to has committed. A failure only leaves
/// an orphaned encrypted file behind, so it is logged (key only) rather than reported.
pub(crate) fn delete_quietly(store: &FileStore, keys: &[String]) {
    for k in keys {
        if let Err(e) = store.delete(k) {
            tracing::warn!(key = %k, "could not delete stored file: {e}");
        }
    }
}

/// Stores a new file, runs the database work, and removes the file again if that work fails.
pub(crate) async fn with_new_file<T, F, Fut>(store: &FileStore, key: &str, upload: &Upload, work: F) -> AppResult<T>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = AppResult<T>>,
{
    store.put(key, &upload.bytes, &upload.mime_type)?;
    match work().await {
        Ok(v) => Ok(v),
        Err(e) => {
            delete_quietly(store, &[key.to_string()]);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests;
