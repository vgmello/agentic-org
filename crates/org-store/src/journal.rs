//! The acceptance journal: append-only segments of framed, chained entries under an
//! immutable anchor. Appending an entry is the commit point. Nothing rebuilds the journal:
//! for the entries it still holds it is the only copy.
//!
//! ```text
//! <durable>/recovery/offices/<office-id>/journals/acceptance/<journal-incarnation>/
//!   anchor.json            immutable; the first entry chains to its hash; ordinal zero
//!   checkpoint.json        where the chain starts once earlier segments are released
//!   materialized.json      the ordinal through which files are written
//!   segments/<first-ordinal>.log
//! entry = length:u32 ‖ header ‖ payloads ‖ sha256(header ‖ payloads)
//! ```

use std::{
    collections::{BTreeMap, VecDeque},
    io,
    ops::Range,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, MutexGuard,
    },
};

use serde::{Deserialize, Serialize};

use crate::{
    change::{Ack, Actor, Bytes, DurablePath, Id, Outcome, Sha256, Timestamp},
    custody::{JournalHead, OfficeIdentity},
    disk::{Disk, DiskFile},
    error::JournalError,
    files::FileOp,
    sequences::{successor, FeedPosition, HighWaters},
};

mod append;
mod entry;
mod open;
mod segment;

pub use entry::positions_between;

/// The largest framed entry.
pub const MAX_ENTRY_BYTES: usize = 16 * 1024 * 1024;
/// A segment is rolled before an append would take it past this.
pub const SEGMENT_BYTES: u64 = 64 * 1024 * 1024;
const FORMAT: &str = "acceptance-journal/1";
const TRAILER: usize = 32;

fn io_error(path: &Path, action: &'static str, error: &io::Error) -> JournalError {
    JournalError::Io {
        path: path.display().to_string(),
        action,
        message: error.to_string(),
    }
}

fn flush_error(path: &Path, error: &io::Error) -> JournalError {
    JournalError::Flush {
        path: path.display().to_string(),
        message: error.to_string(),
    }
}

/// The receipt identity an entry records.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationRecord {
    pub id: Id,
    pub kind: String,
    pub actor: Actor,
    pub fingerprint: Sha256,
    pub outcome: Outcome,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileRecord {
    pub path: DurablePath,
    pub op: FileOp,
    pub length: u64,
    /// The hash of the file's bytes; `null` for a removal.
    pub sha256: Option<Sha256>,
    /// The hash of the version this one replaces; `null` when there was no file.
    pub previous_sha256: Option<Sha256>,
}

/// An entry's header: JSON with this field order, hashed as stored.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    pub ordinal: u64,
    pub previous_sha256: Sha256,
    pub office_id: Id,
    pub journal_incarnation: Id,
    pub at: Timestamp,
    pub ack: Ack,
    pub feed: FeedPosition,
    pub operation: Option<OperationRecord>,
    /// The origin positions this entry took: stream name to `[first, last]`.
    pub positions: BTreeMap<String, [u64; 2]>,
    pub files: Vec<FileRecord>,
}

/// One file of an entry about to be framed.
#[derive(Clone, Debug)]
pub struct FileDraft {
    pub path: DurablePath,
    pub op: FileOp,
    /// `None` exactly when `op` is `Remove`.
    pub bytes: Option<Bytes>,
    pub previous_sha256: Option<Sha256>,
}

/// An entry about to be framed.
#[derive(Clone, Debug)]
pub struct EntryDraft {
    pub ordinal: u64,
    pub previous_sha256: Sha256,
    pub office_id: Id,
    pub journal_incarnation: Id,
    pub at: Timestamp,
    pub ack: Ack,
    pub feed: FeedPosition,
    pub operation: Option<OperationRecord>,
    pub positions: BTreeMap<String, [u64; 2]>,
    pub files: Vec<FileDraft>,
}

/// One accepted operation, exactly as stored.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    pub header: Header,
    /// The hash of the header and payloads as stored; what the next entry chains to.
    pub sha256: Sha256,
    frame: Bytes,
    payloads: Vec<Range<usize>>,
}

/// A position in the journal with everything recovered up to it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct JournalPoint {
    /// Zero is the anchor.
    pub ordinal: u64,
    pub sha256: Sha256,
    pub high_waters: HighWaters,
    pub feed: FeedPosition,
}

/// What a new journal incarnation starts from.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct AnchorFacts {
    /// The origin high-waters it continues: a new incarnation never reuses a position.
    pub high_waters: HighWaters,
    pub feed: FeedPosition,
    pub predecessor: Option<Predecessor>,
}

/// The head of the incarnation this one follows.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Predecessor {
    pub journal_incarnation: Id,
    pub ordinal: u64,
    pub sha256: Sha256,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Anchor {
    format: String,
    identity: OfficeIdentity,
    high_waters: HighWaters,
    feed: FeedPosition,
    predecessor: Option<Predecessor>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    format: String,
    ordinal: u64,
    sha256: Sha256,
    high_waters: HighWaters,
    feed: FeedPosition,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Materialized {
    ordinal: u64,
}

/// What opening found.
#[derive(Debug)]
pub struct Recovered {
    /// The head the host had confirmed, if any.
    pub confirmed: Option<JournalHead>,
    /// Every entry the journal still holds, in order.
    pub entries: Vec<Arc<Entry>>,
    /// The ordinal a torn tail was truncated from, if one was.
    pub truncated: Option<u64>,
}

struct Segment {
    first: u64,
    path: PathBuf,
    length: u64,
    /// The point after its last entry.
    end: JournalPoint,
}

struct Inner {
    segments: VecDeque<Segment>,
    /// The handle of the last segment, once one is open.
    active: Option<Arc<dyn DiskFile>>,
    /// Rolled segments whose tail is not yet flushed.
    unflushed: Vec<(PathBuf, Arc<dyn DiskFile>)>,
    directory_unflushed: bool,
    base: JournalPoint,
    head: JournalPoint,
    flushed: u64,
    materialized: u64,
    /// Entries after the materialized mark.
    retained: VecDeque<Arc<Entry>>,
    broken: Option<JournalError>,
}

pub struct Journal {
    dir: PathBuf,
    disk: Arc<dyn Disk>,
    identity: OfficeIdentity,
    segment_bytes: AtomicU64,
    release_lock: Mutex<()>,
    inner: Mutex<Inner>,
}

/// Where an office's journal incarnation lives under the durable root.
pub fn journal_dir(durable: &Path, identity: &OfficeIdentity) -> PathBuf {
    durable
        .join("recovery/offices")
        .join(identity.office_id.as_str())
        .join("journals/acceptance")
        .join(identity.journal_incarnation.as_str())
}

fn segment_name(first: u64) -> String {
    format!("{first:020}.log")
}

impl Journal {
    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn identity(&self) -> &OfficeIdentity {
        &self.identity
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The last appended entry.
    pub fn head(&self) -> JournalPoint {
        self.inner().head.clone()
    }

    /// Where the chain starts: the anchor, or the checkpoint once segments were released.
    pub fn base(&self) -> JournalPoint {
        self.inner().base.clone()
    }

    /// The ordinal through which the journal is fully flushed.
    pub fn flushed(&self) -> u64 {
        self.inner().flushed
    }

    /// The ordinal through which files are written.
    pub fn materialized(&self) -> u64 {
        self.inner().materialized
    }

    /// The ordinal the next entry takes. Exhaustion stops writes; it is never a wrap.
    pub fn next_ordinal(&self) -> Result<u64, JournalError> {
        successor(self.inner().head.ordinal, "journal ordinal")
            .map_err(|_| JournalError::Exhausted("journal ordinal"))
    }

    /// Rolls segments at `bytes` instead of [`SEGMENT_BYTES`].
    pub(crate) fn set_segment_bytes(&self, bytes: u64) {
        self.segment_bytes.store(bytes, Ordering::Relaxed);
    }

    /// The flushed entries after `ordinal` that are still retained, in order. An entry is
    /// never handed out before its flush: this is what materialization and an outbox read.
    pub fn after(&self, ordinal: u64) -> Vec<Arc<Entry>> {
        let inner = self.inner();
        let flushed = inner.flushed;
        let wanted = inner
            .retained
            .iter()
            .filter(|e| e.ordinal() > ordinal && e.ordinal() <= flushed);
        wanted.cloned().collect()
    }

    /// Reads flushed entries strictly after `ordinal` and verifies strict contiguity (`ordinal + 1`
    /// onward) so a consumer can never skip an evicted entry (`TASK-52` M-15).
    pub fn after_contiguous(&self, ordinal: u64) -> Result<Vec<Arc<Entry>>, JournalError> {
        let entries = self.after(ordinal);
        let mut expected = ordinal.saturating_add(1);
        for entry in &entries {
            if entry.ordinal() != expected {
                return Err(JournalError::MissingPredecessor { ordinal: expected });
            }
            expected = expected.saturating_add(1);
        }
        Ok(entries)
    }

    /// The first ordinal of every segment the journal holds.
    pub fn segment_firsts(&self) -> Vec<u64> {
        self.inner().segments.iter().map(|s| s.first).collect()
    }
}

#[cfg(test)]
mod tests;
