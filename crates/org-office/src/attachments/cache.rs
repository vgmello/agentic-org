//! A satellite's bounded cache of shared blobs pulled from HQ (`TASK-98.6`, ADR-0037):
//! `<runtime>/<office-id>/protected/blob-cache/sha256/<aa>/<bb>/<sha256>`, owner-only folders
//! (`0700`) and files (`0600`), written once like any blob store.
//!
//! It lives under the office runtime's `protected/` folder, which every agent sandbox masks
//! whole: a cached file is readable only through `attachment.read_chunk` and the policy, never
//! straight off the disk by an agent that may not read its parent.
//!
//! The cache holds at most a cap of bytes (default [`DEFAULT_BLOB_CACHE_MAX_BYTES`]) and evicts
//! the least recently used blob first. Use is recorded in each file's modification time from a
//! strictly increasing clock, so the order survives a restart and is deterministic (ties broken
//! by digest). Eviction walks this folder only: it can never touch the satellite's outbox or
//! HQ's `<durable>/blobs/`.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::OfficeError;

use super::blobs::{blob_path, BlobStore, ContentHash, FsBlobStore};

/// The cache's folder below `<runtime>/<office-id>/`.
pub const BLOB_CACHE_DIR: &str = "protected/blob-cache";
/// The default cap of a satellite's blob cache: 1 GiB.
pub const DEFAULT_BLOB_CACHE_MAX_BYTES: u64 = 1024 * 1024 * 1024;

/// Serializes inserts and evictions of every cache in this process.
static CACHE_LOCK: Mutex<()> = Mutex::new(());
/// The last use time handed out, in nanoseconds since the epoch: every use is later than the
/// one before, even within one tick of the system clock.
static LAST_USE_NANOS: AtomicU64 = AtomicU64::new(0);

fn lock() -> MutexGuard<'static, ()> {
    CACHE_LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

fn next_use_time() -> SystemTime {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
        .unwrap_or(0);
    let previous = LAST_USE_NANOS
        .try_update(Ordering::AcqRel, Ordering::Acquire, |last| {
            Some(now.max(last.saturating_add(1)))
        })
        .unwrap_or(now);
    UNIX_EPOCH + Duration::from_nanos(now.max(previous.saturating_add(1)))
}

/// One cached blob, for eviction and diagnostics.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CachedBlob {
    pub hash: ContentHash,
    pub bytes: u64,
    pub last_used: SystemTime,
}

/// The blob cache of one satellite office.
pub struct BlobCache {
    root: PathBuf,
    blobs: FsBlobStore,
}

impl BlobCache {
    pub fn new(runtime_root: &Path, office_id: &str) -> Self {
        let root = runtime_root.join(office_id).join(BLOB_CACHE_DIR);
        Self {
            blobs: FsBlobStore::new(runtime_root, root.clone()),
            root,
        }
    }

    /// The cache folder (tests and diagnostics).
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Opens a cached blob without following a link, as a regular file, and records the use.
    fn open_used(&self, hash: &ContentHash) -> Result<Option<(File, u64)>, OfficeError> {
        let file = match org_platform::fs::open_nofollow_ro(&blob_path(&self.root, hash)) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(OfficeError::storage(error)),
        };
        let meta = file.metadata().map_err(OfficeError::storage)?;
        if !meta.is_file() {
            return Err(OfficeError::Invalid("a cached blob is not a regular file"));
        }
        // Best effort: a use that cannot be recorded still serves the bytes.
        let _ = file.set_modified(next_use_time());
        Ok(Some((file, meta.len())))
    }

    /// Up to `max` bytes from `offset`, recording the use; `None` when not cached.
    pub fn read_range(
        &self,
        hash: &ContentHash,
        offset: u64,
        max: usize,
    ) -> Result<Option<Vec<u8>>, OfficeError> {
        let Some((mut file, len)) = self.open_used(hash)? else {
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

    /// The whole blob when it is at most `limit` bytes, recording the use. Read from the one
    /// handle opened, bounded even if the file grew after it was opened.
    pub fn read_all(&self, hash: &ContentHash, limit: u64) -> Result<Option<Vec<u8>>, OfficeError> {
        let Some((file, len)) = self.open_used(hash)? else {
            return Ok(None);
        };
        if len > limit {
            return Err(OfficeError::Invalid(
                "an attachment blob is over its size limit",
            ));
        }
        let mut out = Vec::with_capacity(usize::try_from(len).unwrap_or(0));
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

    /// Keeps `bytes` (whose digest must be `hash`) as just used, then evicts the least
    /// recently used other blobs until the cache holds at most `max_bytes`. The blob just kept
    /// is never evicted by its own insert, even alone over the cap. Returns what was evicted.
    pub fn insert(
        &self,
        hash: &ContentHash,
        bytes: &[u8],
        max_bytes: u64,
    ) -> Result<Vec<ContentHash>, OfficeError> {
        if ContentHash::of(bytes) != *hash {
            return Err(OfficeError::Invalid("cached bytes do not match their hash"));
        }
        let _guard = lock();
        self.blobs.put(bytes)?;
        if let Some((file, _)) = self.open_used(hash)? {
            drop(file);
        }
        self.evict_locked(max_bytes, Some(hash))
    }

    /// Every cached blob, least recently used first. Anything else found in the cache's
    /// folders (a link, a stray or half-written file) is removed: the cache is disposable.
    pub fn entries(&self) -> Result<Vec<CachedBlob>, OfficeError> {
        let _guard = lock();
        self.entries_locked()
    }

    fn entries_locked(&self) -> Result<Vec<CachedBlob>, OfficeError> {
        let mut found = Vec::new();
        let mut folders = vec![self.root.join("sha256")];
        while let Some(folder) = folders.pop() {
            let listing = match fs::read_dir(&folder) {
                Ok(listing) => listing,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(OfficeError::storage(error)),
            };
            for item in listing {
                let item = item.map_err(OfficeError::storage)?;
                let kind = item.file_type().map_err(OfficeError::storage)?;
                if kind.is_dir() {
                    folders.push(item.path());
                    continue;
                }
                let hash = item.file_name().to_str().and_then(ContentHash::parse);
                match hash {
                    Some(hash) if kind.is_file() && item.path() == blob_path(&self.root, &hash) => {
                        let meta = item.metadata().map_err(OfficeError::storage)?;
                        found.push(CachedBlob {
                            hash,
                            bytes: meta.len(),
                            last_used: meta.modified().map_err(OfficeError::storage)?,
                        });
                    }
                    _ => match fs::remove_file(item.path()) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => return Err(OfficeError::storage(error)),
                    },
                }
            }
        }
        found.sort_by(|a, b| {
            a.last_used
                .cmp(&b.last_used)
                .then_with(|| a.hash.cmp(&b.hash))
        });
        Ok(found)
    }

    fn evict_locked(
        &self,
        max_bytes: u64,
        keep: Option<&ContentHash>,
    ) -> Result<Vec<ContentHash>, OfficeError> {
        let entries = self.entries_locked()?;
        let mut total = entries
            .iter()
            .map(|e| e.bytes)
            .fold(0u64, u64::saturating_add);
        let mut evicted = Vec::new();
        for entry in entries {
            if total <= max_bytes {
                break;
            }
            if keep == Some(&entry.hash) {
                continue;
            }
            if self.blobs.remove(&entry.hash)? {
                total = total.saturating_sub(entry.bytes);
                evicted.push(entry.hash);
            }
        }
        Ok(evicted)
    }

    /// Evicts down to `max_bytes` now (a lowered cap); returns what went.
    pub fn trim(&self, max_bytes: u64) -> Result<Vec<ContentHash>, OfficeError> {
        let _guard = lock();
        self.evict_locked(max_bytes, None)
    }

    /// How many blobs the cache holds.
    pub fn count(&self) -> Result<usize, OfficeError> {
        self.blobs.count()
    }
}
