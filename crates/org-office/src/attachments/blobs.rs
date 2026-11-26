//! Content-addressed attachment bytes (`TASK-98.1`, ADR-0037): one engine-independent
//! [`BlobStore`] port with a disk adapter ([`FsBlobStore`]) and a memory one
//! ([`MemoryBlobStore`]).
//!
//! On disk a blob is `<root>/sha256/<aa>/<bb>/<sha256>`, written once: owner-only folders
//! (`0700`) and files (`0600`), the bytes fully synced to a temporary file and renamed into
//! place, the folder synced after. The shared root is `<durable>/blobs/` and HQ's private root
//! `<durable>/private/blobs/`; neither is a tree of the office store (`org_store::Tree::ALL`),
//! so the journal never carries them and no SQLite database holds a blob.
//!
//! A blob's digest is how the store files it, never how a caller finds it: callers address
//! attachments by `attachment_id` and the policy decides before any blob is opened.

use std::collections::BTreeMap;
use std::fmt;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use org_store::Sha256;

use crate::error::OfficeError;
use crate::private_fs::{private_dir_synced, write_private_atomic};

/// A lowercase hex SHA-256 of a blob's bytes.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentHash(String);

impl ContentHash {
    /// The digest of `bytes`.
    pub fn of(bytes: &[u8]) -> Self {
        Self(Sha256::of(bytes).to_hex())
    }

    /// A stored digest: exactly 64 lowercase hex digits.
    pub fn parse(text: &str) -> Option<Self> {
        (text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
            .then(|| Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ContentHash({})", self.0)
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where the blob `hash` is filed below `root`: `<root>/sha256/<aa>/<bb>/<sha256>`.
pub(crate) fn blob_path(root: &Path, hash: &ContentHash) -> PathBuf {
    let hex = hash.as_str();
    root.join("sha256")
        .join(&hex[0..2])
        .join(&hex[2..4])
        .join(hex)
}

/// Where attachment bytes are kept, whichever engine holds the projection.
pub trait BlobStore: Send + Sync {
    /// Stores `bytes` once under their digest and returns it. Storing bytes already there
    /// writes nothing.
    fn put(&self, bytes: &[u8]) -> Result<ContentHash, OfficeError>;
    /// The size of the blob, if it is there.
    fn size(&self, hash: &ContentHash) -> Result<Option<u64>, OfficeError>;
    /// Up to `max` bytes from `offset`; `None` when the blob is not there.
    fn read_range(
        &self,
        hash: &ContentHash,
        offset: u64,
        max: usize,
    ) -> Result<Option<Vec<u8>>, OfficeError>;
    /// The whole blob when it is at most `limit` bytes; `None` when it is not there.
    fn read_all(&self, hash: &ContentHash, limit: u64) -> Result<Option<Vec<u8>>, OfficeError>;
    /// How many blobs the store holds (tests and diagnostics).
    fn count(&self) -> Result<usize, OfficeError>;
}

/// The blobs under one root folder on disk.
pub struct FsBlobStore {
    /// The folder the root is made below with owner-only, synced folders.
    established: PathBuf,
    root: PathBuf,
}

impl FsBlobStore {
    /// The store at `root` (`<durable>/blobs` or `<durable>/private/blobs`). Folders are made
    /// lazily, owner-only, from `established` (the durable root) down.
    pub fn new(established: &Path, root: PathBuf) -> Self {
        Self {
            established: established.to_path_buf(),
            root,
        }
    }

    fn path_of(&self, hash: &ContentHash) -> PathBuf {
        blob_path(&self.root, hash)
    }

    /// Removes the blob, if it is there; `true` when it was. Only the satellite's outbox and
    /// cache ever call this: HQ's shared and private stores are write-once.
    pub(crate) fn remove(&self, hash: &ContentHash) -> Result<bool, OfficeError> {
        let path = self.path_of(hash);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(OfficeError::storage(error)),
        }
        let folder = path.parent().expect("a blob is in a folder");
        crate::private_fs::sync_dir(folder).map_err(OfficeError::storage)?;
        Ok(true)
    }

    /// Opens the blob without following a link or blocking on a FIFO, and checks the handle
    /// is a regular file.
    fn open(&self, hash: &ContentHash) -> Result<Option<(std::fs::File, u64)>, OfficeError> {
        let path = self.path_of(hash);
        let file = match org_platform::fs::open_nofollow_ro(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(OfficeError::storage(error)),
        };
        let meta = file.metadata().map_err(OfficeError::storage)?;
        if !meta.is_file() {
            return Err(OfficeError::Invalid(
                "an attachment blob is not a regular file",
            ));
        }
        Ok(Some((file, meta.len())))
    }
}

impl BlobStore for FsBlobStore {
    fn put(&self, bytes: &[u8]) -> Result<ContentHash, OfficeError> {
        let hash = ContentHash::of(bytes);
        if self.size(&hash)? == Some(bytes.len() as u64) {
            return Ok(hash);
        }
        let path = self.path_of(&hash);
        let folder = path.parent().expect("a blob is in a folder");
        private_dir_synced(&self.established, folder).map_err(OfficeError::storage)?;
        write_private_atomic(&path, bytes).map_err(OfficeError::storage)?;
        Ok(hash)
    }

    fn size(&self, hash: &ContentHash) -> Result<Option<u64>, OfficeError> {
        Ok(self.open(hash)?.map(|(_, len)| len))
    }

    fn read_range(
        &self,
        hash: &ContentHash,
        offset: u64,
        max: usize,
    ) -> Result<Option<Vec<u8>>, OfficeError> {
        let Some((mut file, len)) = self.open(hash)? else {
            return Ok(None);
        };
        if offset >= len {
            return Ok(Some(Vec::new()));
        }
        file.seek(SeekFrom::Start(offset))
            .map_err(OfficeError::storage)?;
        let wanted = (len - offset).min(max as u64);
        let mut out = Vec::with_capacity(usize::try_from(wanted).unwrap_or(0));
        file.take(wanted)
            .read_to_end(&mut out)
            .map_err(OfficeError::storage)?;
        Ok(Some(out))
    }

    fn read_all(&self, hash: &ContentHash, limit: u64) -> Result<Option<Vec<u8>>, OfficeError> {
        let Some((file, len)) = self.open(hash)? else {
            return Ok(None);
        };
        if len > limit {
            return Err(OfficeError::Invalid(
                "an attachment blob is over its size limit",
            ));
        }
        let mut out = Vec::with_capacity(usize::try_from(len).unwrap_or(0));
        // Bounded even if the file grew after the check: read at most `limit + 1`.
        file.take(limit.saturating_add(1))
            .read_to_end(&mut out)
            .map_err(OfficeError::storage)?;
        if out.len() as u64 > limit {
            return Err(OfficeError::Invalid(
                "an attachment blob is over its size limit",
            ));
        }
        Ok(Some(out))
    }

    fn count(&self) -> Result<usize, OfficeError> {
        let mut count = 0;
        let mut folders = vec![self.root.join("sha256")];
        while let Some(folder) = folders.pop() {
            let entries = match std::fs::read_dir(&folder) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(OfficeError::storage(error)),
            };
            for entry in entries {
                let entry = entry.map_err(OfficeError::storage)?;
                let kind = entry.file_type().map_err(OfficeError::storage)?;
                if kind.is_dir() {
                    folders.push(entry.path());
                } else if kind.is_file()
                    && entry
                        .file_name()
                        .to_str()
                        .and_then(ContentHash::parse)
                        .is_some()
                {
                    count += 1;
                }
            }
        }
        Ok(count)
    }
}

/// Blobs in memory: for tests of everything above the store.
#[derive(Default)]
pub struct MemoryBlobStore {
    blobs: Mutex<BTreeMap<ContentHash, Vec<u8>>>,
}

impl MemoryBlobStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<ContentHash, Vec<u8>>> {
        self.blobs.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl BlobStore for MemoryBlobStore {
    fn put(&self, bytes: &[u8]) -> Result<ContentHash, OfficeError> {
        let hash = ContentHash::of(bytes);
        self.lock()
            .entry(hash.clone())
            .or_insert_with(|| bytes.to_vec());
        Ok(hash)
    }

    fn size(&self, hash: &ContentHash) -> Result<Option<u64>, OfficeError> {
        Ok(self.lock().get(hash).map(|b| b.len() as u64))
    }

    fn read_range(
        &self,
        hash: &ContentHash,
        offset: u64,
        max: usize,
    ) -> Result<Option<Vec<u8>>, OfficeError> {
        Ok(self.lock().get(hash).map(|bytes| {
            let start = usize::try_from(offset)
                .unwrap_or(usize::MAX)
                .min(bytes.len());
            let end = start.saturating_add(max).min(bytes.len());
            bytes[start..end].to_vec()
        }))
    }

    fn read_all(&self, hash: &ContentHash, limit: u64) -> Result<Option<Vec<u8>>, OfficeError> {
        match self.lock().get(hash) {
            Some(bytes) if bytes.len() as u64 > limit => Err(OfficeError::Invalid(
                "an attachment blob is over its size limit",
            )),
            Some(bytes) => Ok(Some(bytes.clone())),
            None => Ok(None),
        }
    }

    fn count(&self) -> Result<usize, OfficeError> {
        Ok(self.lock().len())
    }
}
