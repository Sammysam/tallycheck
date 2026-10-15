//! Local encrypted file store (reference `LocalStorageDriver` + `StorageService`).

use crate::crypto::{decrypt, encrypt, is_encrypted, CryptoError, StorageKey};
use app_core::{AppError, AppResult};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

#[derive(Debug)]
pub struct StoredFile {
    pub bytes: Vec<u8>,
    pub content_type: String,
}

#[derive(Debug, Clone)]
pub struct FileStore {
    root: PathBuf,
    key: StorageKey,
}

impl FileStore {
    pub fn new(root: PathBuf, key: StorageKey) -> FileStore {
        FileStore { root, key }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Refuses anything that would escape the root (absolute paths, `..`, drive prefixes).
    fn resolve(&self, key: &str) -> AppResult<PathBuf> {
        let rel = Path::new(key);
        let safe = !key.is_empty()
            && rel.components().all(|c| matches!(c, Component::Normal(_)))
            && !key.contains('\\')
            && !key.ends_with(".meta");
        if !safe {
            return Err(AppError::internal(format!("invalid storage key {key:?}")));
        }
        Ok(self.root.join(rel))
    }

    /// Encrypts and writes atomically (temp file + rename), then the content-type sidecar.
    pub fn put(&self, key: &str, plain: &[u8], content_type: &str) -> AppResult<()> {
        let path = self.resolve(key)?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(AppError::internal)?;
        }
        let sealed = encrypt(plain, &self.key).map_err(AppError::internal)?;
        let tmp = path.with_extension("partial");
        fs::write(&tmp, &sealed).map_err(AppError::internal)?;
        fs::rename(&tmp, &path).map_err(AppError::internal)?;
        fs::write(meta_path(&path), content_type).map_err(AppError::internal)?;
        Ok(())
    }

    /// Decrypts on the way out; legacy plaintext files open as they are.
    pub fn get(&self, key: &str) -> AppResult<StoredFile> {
        let path = self.resolve(key)?;
        let raw = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(AppError::NotFound("File not found in storage".into()))
            }
            Err(e) => return Err(AppError::internal(e)),
        };
        let content_type =
            fs::read_to_string(meta_path(&path)).unwrap_or_else(|_| "application/octet-stream".to_string());
        let bytes = if is_encrypted(&raw) {
            decrypt(&raw, &self.key).map_err(|e: CryptoError| {
                tracing::error!(key, "stored file could not be decrypted: {e}");
                AppError::internal(e)
            })?
        } else {
            raw
        };
        Ok(StoredFile { bytes, content_type })
    }

    /// Removes the file and its sidecar; a missing file is not an error.
    pub fn delete(&self, key: &str) -> AppResult<()> {
        let path = self.resolve(key)?;
        for p in [path.clone(), meta_path(&path)] {
            match fs::remove_file(&p) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(AppError::internal(e)),
            }
        }
        Ok(())
    }

    /// Raw stored bytes (encrypted), for backups and tests.
    pub fn raw(&self, key: &str) -> AppResult<Vec<u8>> {
        fs::read(self.resolve(key)?).map_err(AppError::internal)
    }
}

fn meta_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".meta");
    PathBuf::from(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> AppResult<(tempfile::TempDir, FileStore)> {
        let dir = tempfile::tempdir().map_err(AppError::internal)?;
        let key = StorageKey::from_bytes(&[3u8; 32]).map_err(AppError::internal)?;
        let s = FileStore::new(dir.path().join("files"), key);
        Ok((dir, s))
    }

    #[test]
    fn files_are_encrypted_at_rest_and_round_trip() -> AppResult<()> {
        let (_d, s) = store()?;
        let key = "employees/e1/documents/abc.pdf";
        s.put(key, b"%PDF-1.4 secret salary slip", "application/pdf")?;
        let raw = s.raw(key)?;
        assert_eq!(&raw[..4], b"QBE1");
        assert!(!raw.windows(6).any(|w| w == b"secret"), "no plaintext on disk");
        let back = s.get(key)?;
        assert_eq!(back.bytes, b"%PDF-1.4 secret salary slip");
        assert_eq!(back.content_type, "application/pdf");
        s.delete(key)?;
        assert!(matches!(s.get(key), Err(AppError::NotFound(_))));
        s.delete(key)?;
        Ok(())
    }

    #[test]
    fn legacy_plaintext_files_still_open() -> AppResult<()> {
        let (_d, s) = store()?;
        let path = s.root().join("employees/e1/photo/old.png");
        fs::create_dir_all(path.parent().ok_or_else(|| AppError::internal("parent"))?).map_err(AppError::internal)?;
        fs::write(&path, b"\x89PNG plain").map_err(AppError::internal)?;
        let got = s.get("employees/e1/photo/old.png")?;
        assert_eq!(got.bytes, b"\x89PNG plain");
        assert_eq!(got.content_type, "application/octet-stream");
        Ok(())
    }

    #[test]
    fn damaged_or_foreign_files_are_refused_not_returned() -> AppResult<()> {
        let (dir, s) = store()?;
        s.put("a/b.bin", b"data", "application/octet-stream")?;
        let other =
            FileStore::new(dir.path().join("files"), StorageKey::from_bytes(&[4u8; 32]).map_err(AppError::internal)?);
        assert!(matches!(other.get("a/b.bin"), Err(AppError::Internal(_))), "missing/wrong key");
        Ok(())
    }

    #[test]
    fn keys_cannot_escape_the_root() -> AppResult<()> {
        let (_d, s) = store()?;
        for bad in ["../x", "/etc/passwd", "a/../../x", "C:\\x", "a\\..\\x", "", "a/b.meta"] {
            assert!(s.put(bad, b"x", "text/plain").is_err(), "{bad}");
        }
        Ok(())
    }
}
