//! A satellite's shared attachments waiting for HQ (`TASK-98.6`, ADR-0037): the companion store
//! `<durable>/local/offices/<office-id>/blob-outbox/` (`crate::companion_paths`), never journaled,
//! masked from every agent session and carried by backups.
//!
//! - `sha256/<aa>/<bb>/<sha256>`: the bytes, written once like any blob store's.
//! - `entries/<attachment-id>.yaml`: the attachment's canonical record, one per attachment
//!   still to ship.
//!
//! A locally created shared file is readable offline from here at once, and nothing here is
//! evicted or removed until HQ acknowledges it with a verified `BlobAck`
//! ([`BlobOutbox::release`]). Only shared files ever enter: a private file never leaves HQ.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use org_proto::attachments::AttachmentScope;
use org_store::Id;

use crate::error::OfficeError;
use crate::private_fs::{private_dir_synced, sync_dir, write_private_atomic};

use super::blobs::{BlobStore, ContentHash, FsBlobStore};
use super::records::{self, AttachmentRecord};

/// The companion folder's name below `durable/local/offices/<office-id>/`.
pub const OUTBOX_DIR: &str = crate::companion_paths::BLOB_OUTBOX_DIR_NAME;
const ENTRIES: &str = "entries";
/// The largest record file an entry may be (a record is a few hundred bytes).
const MAX_ENTRY_BYTES: u64 = 64 * 1024;

/// Serializes every change to any outbox of this process: an entry and the bytes it names
/// are written and released together, so a release never drops bytes a new entry names.
static OUTBOX_LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    OUTBOX_LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

/// One attachment waiting for HQ: its record, and the canonical bytes HQ is sent.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OutboxEntry {
    pub record: AttachmentRecord,
    pub record_yaml: Vec<u8>,
}

/// The outbox of one satellite office.
pub struct BlobOutbox {
    durable: PathBuf,
    root: PathBuf,
    blobs: FsBlobStore,
}

impl BlobOutbox {
    pub fn new(durable_root: &Path, office_id: &str) -> Self {
        let root = durable_root
            .join(crate::companion_paths::OFFICE_LOCAL_DIR)
            .join(office_id)
            .join(OUTBOX_DIR);
        Self {
            durable: durable_root.to_path_buf(),
            blobs: FsBlobStore::new(durable_root, root.clone()),
            root,
        }
    }

    /// The companion folder (tests and diagnostics).
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The bytes waiting here, by digest.
    pub fn blobs(&self) -> &dyn BlobStore {
        &self.blobs
    }

    fn entries_dir(&self) -> PathBuf {
        self.root.join(ENTRIES)
    }

    fn entry_path(&self, id: &Id) -> PathBuf {
        self.entries_dir().join(format!("{id}.yaml"))
    }

    /// Keeps `bytes` and `record` durably until HQ acknowledges them. Refuses a private file,
    /// and bytes that are not the record's.
    pub fn enqueue(&self, record: &AttachmentRecord, bytes: &[u8]) -> Result<(), OfficeError> {
        if record.scope() != AttachmentScope::Shared {
            return Err(OfficeError::Invalid(
                "a private file never enters the outbox",
            ));
        }
        if ContentHash::of(bytes) != record.content_sha256
            || bytes.len() as u64 != record.byte_count
        {
            return Err(OfficeError::Invalid(
                "outbox bytes do not match their record",
            ));
        }
        let _guard = lock();
        // The bytes first: an entry never names bytes that are not there.
        self.blobs.put(bytes)?;
        private_dir_synced(&self.durable, &self.entries_dir()).map_err(OfficeError::storage)?;
        write_private_atomic(&self.entry_path(&record.id), &records::encode(record))
            .map_err(OfficeError::storage)
    }

    fn read_entry(&self, path: &Path) -> Result<Option<OutboxEntry>, OfficeError> {
        let file = match org_platform::fs::open_nofollow_ro(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            // A link or anything else that is not a readable entry: left alone, never shipped.
            Err(_) => return Ok(None),
        };
        if !file.metadata().map_err(OfficeError::storage)?.is_file() {
            return Ok(None);
        }
        let mut yaml = Vec::new();
        file.take(MAX_ENTRY_BYTES + 1)
            .read_to_end(&mut yaml)
            .map_err(OfficeError::storage)?;
        if yaml.len() as u64 > MAX_ENTRY_BYTES {
            return Ok(None);
        }
        Ok(records::decode(&yaml).ok().map(|record| OutboxEntry {
            record,
            record_yaml: yaml,
        }))
    }

    /// Every entry waiting, oldest attachment first (IDs are UUIDv7). An entry that does not
    /// read back as a shared record named by its file is skipped, never removed.
    pub fn pending(&self) -> Result<Vec<OutboxEntry>, OfficeError> {
        let dir = self.entries_dir();
        let listing = match fs::read_dir(&dir) {
            Ok(listing) => listing,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(OfficeError::storage(error)),
        };
        let mut found = Vec::new();
        for item in listing {
            let item = item.map_err(OfficeError::storage)?;
            let name = item.file_name();
            let Some(id) = name
                .to_str()
                .and_then(|n| n.strip_suffix(".yaml"))
                .filter(|id| org_proto::is_uuid_v7(id))
            else {
                continue;
            };
            let Some(entry) = self.read_entry(&item.path())? else {
                continue;
            };
            if entry.record.id.as_str() == id && entry.record.scope() == AttachmentScope::Shared {
                found.push(entry);
            }
        }
        found.sort_by(|a, b| a.record.id.cmp(&b.record.id));
        Ok(found)
    }

    /// Whether `id` still waits for HQ.
    pub fn is_pending(&self, id: &Id) -> bool {
        self.entry_path(id).is_file()
    }

    /// Drops `id`'s entry once HQ has acknowledged it (or its binding was refused here), and
    /// its bytes once no other entry names them. Returns whether the bytes went.
    pub fn release(&self, id: &Id) -> Result<bool, OfficeError> {
        let _guard = lock();
        let path = self.entry_path(id);
        let Some(entry) = self.read_entry(&path)? else {
            return Ok(false);
        };
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(OfficeError::storage(error)),
        }
        sync_dir(&self.entries_dir()).map_err(OfficeError::storage)?;
        let hash = entry.record.content_sha256;
        if self
            .pending()?
            .iter()
            .any(|other| other.record.content_sha256 == hash)
        {
            return Ok(false);
        }
        self.blobs.remove(&hash)
    }
}
