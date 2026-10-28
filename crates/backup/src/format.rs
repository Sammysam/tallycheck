//! The backup file format (`.qbak`, version 1).
//!
//! ```text
//! "QBAK" | u8 version | u32 LE header length | header JSON
//! chunk*  : u8 last-flag | u32 LE ciphertext length | AES-256-GCM ciphertext+tag
//! ```
//! The payload is a simple archive (see [`ArchiveWriter`]) encrypted in 1 MiB chunks with a
//! random per-backup data key (STREAM construction): nonce = 7-byte random prefix | u32 BE
//! chunk counter | last-flag byte, AAD = SHA-256 of the header. Reordered, dropped, truncated
//! or altered chunks, and a swapped header, all fail authentication. The data key is stored in
//! the header wrapped (AES-256-GCM) with a key derived from the backup password (Argon2id).

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{self, Read, Write};

pub const MAGIC: &[u8; 4] = b"QBAK";
pub const VERSION: u8 = 1;
pub const CHUNK: usize = 1024 * 1024;
const MAX_HEADER: u32 = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("not a TallyCheck backup file")]
    NotABackup,
    #[error("backup format version {0} is not supported by this version of the app")]
    UnsupportedVersion(u8),
    #[error("wrong backup password, or the backup file is damaged")]
    WrongKeyOrDamaged,
    #[error("the backup file is damaged or incomplete: {0}")]
    Damaged(String),
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("random number generator failed: {0}")]
    Random(String),
}

pub type FormatResult<T> = Result<T, FormatError>;

/// How the data key is wrapped: Argon2id over the backup password.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KeySlot {
    pub kdf: String,
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
    pub salt: String,
    pub nonce: String,
    pub wrapped_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Header {
    pub format: u8,
    pub created_at: String,
    pub app_version: String,
    pub schema_version: i64,
    pub chunk_size: usize,
    pub nonce_prefix: String,
    pub key_slot: KeySlot,
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len() / 2).map(|i| u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()).collect()
}

pub fn random(len: usize) -> FormatResult<Vec<u8>> {
    app_core::secrets::random_bytes(len).map_err(|e| FormatError::Random(e.to_string()))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub fn write_header<W: Write>(w: &mut W, header: &Header) -> FormatResult<Vec<u8>> {
    let json = serde_json::to_vec(header).map_err(|e| FormatError::Damaged(e.to_string()))?;
    let len = u32::try_from(json.len()).map_err(|_| FormatError::Damaged("header too large".into()))?;
    w.write_all(MAGIC)?;
    w.write_all(&[VERSION])?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(&json)?;
    Ok(json)
}

/// Reads and parses the header; returns it with its raw bytes (the payload's AAD source).
pub fn read_header<R: Read>(r: &mut R) -> FormatResult<(Header, Vec<u8>)> {
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic).map_err(|_| FormatError::NotABackup)?;
    if &magic != MAGIC {
        return Err(FormatError::NotABackup);
    }
    let mut v = [0u8; 1];
    r.read_exact(&mut v)?;
    if v[0] != VERSION {
        return Err(FormatError::UnsupportedVersion(v[0]));
    }
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len);
    if len > MAX_HEADER {
        return Err(FormatError::NotABackup);
    }
    let mut json = vec![0u8; len as usize];
    r.read_exact(&mut json).map_err(|_| FormatError::Damaged("header cut short".into()))?;
    let header: Header = serde_json::from_slice(&json).map_err(|_| FormatError::NotABackup)?;
    Ok((header, json))
}

fn nonce(prefix: &[u8; 7], counter: u32, last: bool) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[..7].copy_from_slice(prefix);
    n[7..11].copy_from_slice(&counter.to_be_bytes());
    n[11] = u8::from(last);
    n
}

/// Encrypts a byte stream in authenticated chunks.
pub struct EncryptWriter<W: Write> {
    inner: W,
    cipher: Aes256Gcm,
    prefix: [u8; 7],
    aad: [u8; 32],
    counter: u32,
    buf: Vec<u8>,
}

impl<W: Write> EncryptWriter<W> {
    pub fn new(inner: W, key: &[u8; 32], prefix: [u8; 7], header_json: &[u8]) -> Self {
        EncryptWriter {
            inner,
            cipher: Aes256Gcm::new(key.into()),
            prefix,
            aad: Sha256::digest(header_json).into(),
            counter: 0,
            buf: Vec::with_capacity(CHUNK),
        }
    }

    fn emit(&mut self, last: bool) -> io::Result<()> {
        let n = nonce(&self.prefix, self.counter, last);
        let ct = self
            .cipher
            .encrypt(Nonce::from_slice(&n), Payload { msg: &self.buf, aad: &self.aad })
            .map_err(|_| io::Error::other("encryption failed"))?;
        let len = u32::try_from(ct.len()).map_err(|_| io::Error::other("chunk too large"))?;
        self.inner.write_all(&[u8::from(last)])?;
        self.inner.write_all(&len.to_le_bytes())?;
        self.inner.write_all(&ct)?;
        self.counter = self.counter.checked_add(1).ok_or_else(|| io::Error::other("backup too large"))?;
        self.buf.clear();
        Ok(())
    }

    /// Writes the final chunk and returns the inner writer.
    pub fn finish(mut self) -> io::Result<W> {
        self.emit(true)?;
        self.inner.flush()?;
        Ok(self.inner)
    }
}

impl<W: Write> Write for EncryptWriter<W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let room = CHUNK - self.buf.len();
        let n = room.min(data.len());
        self.buf.extend_from_slice(&data[..n]);
        if self.buf.len() == CHUNK {
            self.emit(false)?;
        }
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Decrypts and authenticates the chunk stream; EOF before the final chunk is an error.
pub struct DecryptReader<R: Read> {
    inner: R,
    cipher: Aes256Gcm,
    prefix: [u8; 7],
    aad: [u8; 32],
    counter: u32,
    plain: Vec<u8>,
    pos: usize,
    done: bool,
}

impl<R: Read> DecryptReader<R> {
    pub fn new(inner: R, key: &[u8; 32], prefix: [u8; 7], header_json: &[u8]) -> Self {
        DecryptReader {
            inner,
            cipher: Aes256Gcm::new(key.into()),
            prefix,
            aad: Sha256::digest(header_json).into(),
            counter: 0,
            plain: Vec::new(),
            pos: 0,
            done: false,
        }
    }

    fn next_chunk(&mut self) -> io::Result<()> {
        let mut head = [0u8; 5];
        self.inner.read_exact(&mut head).map_err(|_| damaged("the file ends before its last chunk"))?;
        let last = match head[0] {
            0 => false,
            1 => true,
            _ => return Err(damaged("bad chunk marker")),
        };
        let len = u32::from_le_bytes([head[1], head[2], head[3], head[4]]) as usize;
        if len > CHUNK + 16 {
            return Err(damaged("oversized chunk"));
        }
        let mut ct = vec![0u8; len];
        self.inner.read_exact(&mut ct).map_err(|_| damaged("chunk cut short"))?;
        let n = nonce(&self.prefix, self.counter, last);
        self.plain = self
            .cipher
            .decrypt(Nonce::from_slice(&n), Payload { msg: &ct, aad: &self.aad })
            .map_err(|_| damaged("a chunk failed authentication"))?;
        self.pos = 0;
        self.counter = self.counter.checked_add(1).ok_or_else(|| damaged("too many chunks"))?;
        if last {
            self.done = true;
            let mut extra = [0u8; 1];
            if self.inner.read(&mut extra)? != 0 {
                return Err(damaged("data after the final chunk"));
            }
        }
        Ok(())
    }
}

fn damaged(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what.to_string())
}

impl<R: Read> Read for DecryptReader<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        while self.pos == self.plain.len() {
            if self.done {
                return Ok(0);
            }
            self.next_chunk()?;
        }
        let n = out.len().min(self.plain.len() - self.pos);
        out[..n].copy_from_slice(&self.plain[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

// ─── Archive inside the encrypted stream ────────────────────────────────────

/// `'F' | u16 name length | name | u64 size | bytes` per entry, then `'E'`.
pub struct ArchiveWriter<W: Write> {
    inner: W,
}

impl<W: Write> ArchiveWriter<W> {
    pub fn new(inner: W) -> Self {
        ArchiveWriter { inner }
    }

    /// Streams `size` bytes from `src` and returns their SHA-256.
    pub fn entry<R: Read>(&mut self, name: &str, size: u64, src: &mut R) -> io::Result<String> {
        let name_len = u16::try_from(name.len()).map_err(|_| io::Error::other("entry name too long"))?;
        self.inner.write_all(b"F")?;
        self.inner.write_all(&name_len.to_le_bytes())?;
        self.inner.write_all(name.as_bytes())?;
        self.inner.write_all(&size.to_le_bytes())?;
        let mut hasher = Sha256::new();
        let mut left = size;
        let mut buf = vec![0u8; 64 * 1024];
        while left > 0 {
            let want = usize::try_from(left.min(buf.len() as u64)).unwrap_or(buf.len());
            let n = src.read(&mut buf[..want])?;
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, format!("{name} changed while being read")));
            }
            hasher.update(&buf[..n]);
            self.inner.write_all(&buf[..n])?;
            left -= n as u64;
        }
        Ok(hex(&hasher.finalize()))
    }

    pub fn finish(mut self) -> io::Result<W> {
        self.inner.write_all(b"E")?;
        Ok(self.inner)
    }
}

/// One entry read back: its name, size and the reader positioned on its bytes.
pub struct ArchiveReader<R: Read> {
    inner: R,
}

impl<R: Read> ArchiveReader<R> {
    pub fn new(inner: R) -> Self {
        ArchiveReader { inner }
    }

    /// The next entry's name and size, or `None` at the end marker.
    pub fn next_entry(&mut self) -> io::Result<Option<(String, u64)>> {
        let mut tag = [0u8; 1];
        self.inner.read_exact(&mut tag)?;
        match tag[0] {
            b'E' => return Ok(None),
            b'F' => {}
            _ => return Err(damaged("bad archive entry")),
        }
        let mut l = [0u8; 2];
        self.inner.read_exact(&mut l)?;
        let mut name = vec![0u8; u16::from_le_bytes(l) as usize];
        self.inner.read_exact(&mut name)?;
        let mut s = [0u8; 8];
        self.inner.read_exact(&mut s)?;
        let name = String::from_utf8(name).map_err(|_| damaged("bad entry name"))?;
        Ok(Some((name, u64::from_le_bytes(s))))
    }

    /// Copies the current entry's bytes to `out`, returning their SHA-256.
    pub fn copy_entry<W: Write>(&mut self, size: u64, out: &mut W) -> io::Result<String> {
        let mut hasher = Sha256::new();
        let mut left = size;
        let mut buf = vec![0u8; 64 * 1024];
        while left > 0 {
            let want = usize::try_from(left.min(buf.len() as u64)).unwrap_or(buf.len());
            self.inner.read_exact(&mut buf[..want])?;
            hasher.update(&buf[..want]);
            out.write_all(&buf[..want])?;
            left -= want as u64;
        }
        Ok(hex(&hasher.finalize()))
    }

    pub fn into_inner(self) -> R {
        self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(data: &[u8]) -> io::Result<Vec<u8>> {
        let key = [7u8; 32];
        let prefix = [1u8; 7];
        let header = b"{\"h\":1}";
        let mut w = EncryptWriter::new(Vec::new(), &key, prefix, header);
        w.write_all(data)?;
        let sealed = w.finish()?;
        let mut out = Vec::new();
        DecryptReader::new(sealed.as_slice(), &key, prefix, header).read_to_end(&mut out)?;
        Ok(out)
    }

    #[test]
    fn stream_round_trips_across_chunk_boundaries() -> io::Result<()> {
        for size in [0, 1, CHUNK - 1, CHUNK, CHUNK + 1, 3 * CHUNK + 17] {
            let data: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
            assert_eq!(roundtrip(&data)?, data, "size {size}");
        }
        Ok(())
    }

    #[test]
    fn truncation_reordering_tampering_and_a_swapped_header_are_detected() -> io::Result<()> {
        let key = [7u8; 32];
        let prefix = [1u8; 7];
        let header = b"{\"h\":1}";
        let data: Vec<u8> = (0..2 * CHUNK + 10).map(|i| (i % 13) as u8).collect();
        let mut w = EncryptWriter::new(Vec::new(), &key, prefix, header);
        w.write_all(&data)?;
        let sealed = w.finish()?;
        let read = |bytes: &[u8], h: &[u8]| {
            let mut out = Vec::new();
            DecryptReader::new(bytes, &key, prefix, h).read_to_end(&mut out).map(|_| out)
        };
        assert!(read(&sealed, header).is_ok());
        // Drop the final chunk.
        let first_two = 2 * (5 + CHUNK + 16);
        assert!(read(&sealed[..first_two], header).is_err(), "truncated");
        // Flip a byte.
        let mut bad = sealed.clone();
        bad[100] ^= 1;
        assert!(read(&bad, header).is_err(), "tampered");
        // Swap the first two chunks.
        let c = 5 + CHUNK + 16;
        let mut swapped = sealed[c..2 * c].to_vec();
        swapped.extend_from_slice(&sealed[..c]);
        swapped.extend_from_slice(&sealed[2 * c..]);
        assert!(read(&swapped, header).is_err(), "reordered");
        assert!(read(&sealed, b"{\"h\":2}").is_err(), "different header");
        // Extra bytes after the end.
        let mut longer = sealed.clone();
        longer.push(0);
        assert!(read(&longer, header).is_err(), "trailing data");
        Ok(())
    }

    #[test]
    fn archive_entries_round_trip_with_hashes() -> io::Result<()> {
        let mut a = ArchiveWriter::new(Vec::new());
        let h1 = a.entry("a.txt", 5, &mut &b"hello"[..])?;
        let h2 = a.entry("dir/b.bin", 0, &mut &b""[..])?;
        let bytes = a.finish()?;
        let mut r = ArchiveReader::new(bytes.as_slice());
        let (name, size) = r.next_entry()?.ok_or_else(|| io::Error::other("entry"))?;
        let mut out = Vec::new();
        assert_eq!((name.as_str(), r.copy_entry(size, &mut out)?), ("a.txt", h1));
        assert_eq!(out, b"hello");
        let (name, size) = r.next_entry()?.ok_or_else(|| io::Error::other("entry"))?;
        assert_eq!((name.as_str(), r.copy_entry(size, &mut Vec::new())?), ("dir/b.bin", h2));
        assert!(r.next_entry()?.is_none());
        Ok(())
    }
}
