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
    fmt, fs, io,
    ops::Range,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, MutexGuard,
    },
};

use serde::{Deserialize, Serialize};

use crate::{
    change::{Ack, Actor, Bytes, DurablePath, Id, Outcome, Sha256, Timestamp, TEMP_PREFIX},
    custody::{JournalHead, OfficeIdentity, WriterAuthority},
    disk::{write_all, Disk, DiskFile, RealDisk},
    engine::Cursor,
    files::{FileOp, FileVersion},
    sequences::{successor, FeedPosition, HighWaters, Stream, MAX_POSITION},
};

/// The largest framed entry.
pub const MAX_ENTRY_BYTES: usize = 16 * 1024 * 1024;
/// A segment is rolled before an append would take it past this.
pub const SEGMENT_BYTES: u64 = 64 * 1024 * 1024;
const FORMAT: &str = "acceptance-journal/1";
const TRAILER: usize = 32;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum JournalError {
    Io {
        path: String,
        action: &'static str,
        message: String,
    },
    /// A full flush failed. Never retried in this process.
    Flush {
        path: String,
        message: String,
    },
    /// A bad entry at or below the head the host confirmed, or an entry that is whole but
    /// does not continue the chain. Never truncated.
    Corrupt {
        ordinal: u64,
        reason: String,
    },
    /// The journal ends before the head the host confirmed.
    Behind {
        journal: u64,
        custody: u64,
    },
    /// The entry at the confirmed ordinal is not the one the host confirmed.
    Fork {
        ordinal: u64,
    },
    /// The entries before this ordinal are neither present nor covered by the checkpoint,
    /// or the entry at this ordinal, which the host confirmed, is no longer held.
    MissingPredecessor {
        ordinal: u64,
    },
    UnknownFormat(String),
    /// The journal belongs to another office, incarnation or writer.
    IdentityMismatch,
    /// `create` found a journal already there.
    Exists,
    Custody(String),
    Exhausted(&'static str),
    TooLarge {
        bytes: usize,
    },
    /// The append failed or was short. The segment is back at its last good entry.
    Append(String),
    /// A failed append could not be truncated away. The journal accepts nothing more.
    Broken {
        path: String,
        message: String,
    },
    Internal(String),
}

impl fmt::Display for JournalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                path,
                action,
                message,
            } => write!(f, "journal: {action} {path}: {message}"),
            Self::Flush { path, message } => write!(f, "journal: flush {path}: {message}"),
            Self::Corrupt { ordinal, reason } => {
                write!(f, "journal entry {ordinal} is corrupt: {reason}")
            }
            Self::Behind { journal, custody } => write!(
                f,
                "the journal ends at {journal}, behind the confirmed head {custody}"
            ),
            Self::Fork { ordinal } => {
                write!(
                    f,
                    "journal entry {ordinal} is not the one the host confirmed"
                )
            }
            Self::MissingPredecessor { ordinal } => {
                write!(f, "the journal entries before {ordinal} are missing")
            }
            Self::UnknownFormat(format) => write!(f, "unknown journal format {format:?}"),
            Self::IdentityMismatch => f.write_str("the journal belongs to another writer"),
            Self::Exists => f.write_str("a journal already exists here"),
            Self::Custody(message) => write!(f, "writer custody: {message}"),
            Self::Exhausted(what) => write!(f, "{what} exhausted"),
            Self::TooLarge { bytes } => write!(f, "a journal entry of {bytes} bytes is too large"),
            Self::Append(message) => write!(f, "journal append failed: {message}"),
            Self::Broken { path, message } => {
                write!(f, "journal segment {path} could not be repaired: {message}")
            }
            Self::Internal(message) => write!(f, "journal: {message}"),
        }
    }
}

impl std::error::Error for JournalError {}

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

impl Entry {
    pub fn frame(draft: EntryDraft) -> Result<Entry, JournalError> {
        let mut files = Vec::with_capacity(draft.files.len());
        for file in &draft.files {
            if !file.path.names_a_file() {
                return Err(JournalError::Internal(format!(
                    "{} is a tree, not a file",
                    file.path
                )));
            }
            if file.bytes.is_none() != (file.op == FileOp::Remove) {
                return Err(JournalError::Internal(
                    "a removal has no bytes and every other change has".into(),
                ));
            }
            files.push(FileRecord {
                path: file.path.clone(),
                op: file.op,
                length: file.bytes.as_ref().map_or(0, |b| b.len() as u64),
                sha256: file.bytes.as_ref().map(|b| Sha256::of(b)),
                previous_sha256: file.previous_sha256,
            });
        }
        let header = Header {
            ordinal: draft.ordinal,
            previous_sha256: draft.previous_sha256,
            office_id: draft.office_id,
            journal_incarnation: draft.journal_incarnation,
            at: draft.at,
            ack: draft.ack,
            feed: draft.feed,
            operation: draft.operation,
            positions: draft.positions,
            files,
        };
        let head =
            serde_json::to_vec(&header).map_err(|e| JournalError::Internal(e.to_string()))?;
        let body = head.len()
            + header
                .files
                .iter()
                .map(|f| f.length as usize)
                .sum::<usize>();
        let total = 4 + body + TRAILER;
        if total > MAX_ENTRY_BYTES {
            return Err(JournalError::TooLarge { bytes: total });
        }
        let mut frame = Vec::with_capacity(total);
        frame.extend_from_slice(&(body as u32).to_be_bytes());
        frame.extend_from_slice(&head);
        let mut payloads = Vec::with_capacity(draft.files.len());
        for file in &draft.files {
            let start = frame.len();
            if let Some(bytes) = &file.bytes {
                frame.extend_from_slice(bytes);
            }
            payloads.push(start..frame.len());
        }
        let sha256 = Sha256::of(&frame[4..]);
        frame.extend_from_slice(sha256.as_bytes());
        Ok(Entry {
            header,
            sha256,
            frame: frame.into(),
            payloads,
        })
    }

    /// Reads one whole frame whose length and hash are already verified.
    fn parse(frame: &[u8], sha256: Sha256) -> Result<Entry, String> {
        let body = &frame[4..frame.len() - TRAILER];
        let mut stream = serde_json::Deserializer::from_slice(body).into_iter::<Header>();
        let header = match stream.next() {
            Some(Ok(header)) => header,
            Some(Err(e)) => return Err(format!("unreadable header: {e}")),
            None => return Err("no header".into()),
        };
        let mut at = 4 + stream.byte_offset();
        let mut payloads = Vec::with_capacity(header.files.len());
        for file in &header.files {
            if (file.op == FileOp::Remove) != file.sha256.is_none()
                || (file.op == FileOp::Remove && file.length != 0)
            {
                return Err(format!(
                    "{} has the wrong shape for its operation",
                    file.path
                ));
            }
            let end = (at as u64)
                .checked_add(file.length)
                .filter(|end| *end <= (frame.len() - TRAILER) as u64)
                .ok_or("a file runs past the entry")? as usize;
            payloads.push(at..end);
            at = end;
        }
        if at != frame.len() - TRAILER {
            return Err("the entry holds bytes no file accounts for".into());
        }
        Ok(Entry {
            header,
            sha256,
            frame: frame.into(),
            payloads,
        })
    }

    pub fn ordinal(&self) -> u64 {
        self.header.ordinal
    }

    /// The stored bytes of the whole frame.
    pub fn stored(&self) -> &[u8] {
        &self.frame
    }

    /// The bytes of file `index`; empty for a removal.
    pub fn payload(&self, index: usize) -> &[u8] {
        &self.frame[self.payloads[index].clone()]
    }

    /// The entry's files as versions to materialize, in order.
    pub fn versions(&self) -> impl Iterator<Item = FileVersion<'_>> {
        self.header
            .files
            .iter()
            .enumerate()
            .map(|(i, file)| FileVersion {
                path: &file.path,
                op: file.op,
                bytes: (file.op != FileOp::Remove).then(|| self.payload(i)),
                sha256: file.sha256,
                previous_sha256: file.previous_sha256,
            })
    }
}

/// The positions an operation took, as an entry records them.
pub fn positions_between(before: &HighWaters, after: &HighWaters) -> BTreeMap<String, [u64; 2]> {
    Stream::ALL
        .into_iter()
        .filter(|stream| after.get(*stream) > before.get(*stream))
        .map(|stream| {
            (
                stream.name().to_owned(),
                [before.get(stream) + 1, after.get(stream)],
            )
        })
        .collect()
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

impl JournalPoint {
    pub fn head(&self) -> JournalHead {
        JournalHead {
            ordinal: self.ordinal,
            sha256: self.sha256,
            high_waters: self.high_waters,
        }
    }

    pub fn cursor(&self) -> Cursor {
        Cursor {
            ordinal: self.ordinal,
            sha256: self.sha256,
            feed: self.feed,
            high_waters: self.high_waters,
        }
    }

    /// The point after `entry`, or why `entry` does not continue the chain from here.
    fn after(&self, entry: &Entry, identity: &OfficeIdentity) -> Result<JournalPoint, String> {
        let header = &entry.header;
        if header.ordinal != self.ordinal + 1 {
            return Err(format!(
                "ordinal {} follows {}",
                header.ordinal, self.ordinal
            ));
        }
        if header.previous_sha256 != self.sha256 {
            return Err("it does not chain to its predecessor".into());
        }
        if header.office_id != identity.office_id
            || header.journal_incarnation != identity.journal_incarnation
        {
            return Err("it belongs to another journal".into());
        }
        if header.feed.0 != self.feed.0 + 1 || header.feed.0 > MAX_POSITION {
            return Err("its feed position does not follow".into());
        }
        let mut high_waters = self.high_waters;
        for (name, [first, last]) in &header.positions {
            let stream = Stream::from_name(name).ok_or("an unknown origin stream")?;
            if *first != high_waters.get(stream) + 1 || last < first || *last > MAX_POSITION {
                return Err(format!("its {name} positions do not follow"));
            }
            high_waters.set(stream, *last);
        }
        Ok(JournalPoint {
            ordinal: header.ordinal,
            sha256: entry.sha256,
            high_waters,
            feed: header.feed,
        })
    }
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
    /// Starts a new journal incarnation: writes its immutable anchor. Refuses when one
    /// exists.
    pub fn create(
        durable: &Path,
        identity: &OfficeIdentity,
        facts: &AnchorFacts,
    ) -> Result<Journal, JournalError> {
        Self::create_on(Arc::new(RealDisk), durable, identity, facts)
    }

    pub(crate) fn create_on(
        disk: Arc<dyn Disk>,
        durable: &Path,
        identity: &OfficeIdentity,
        facts: &AnchorFacts,
    ) -> Result<Journal, JournalError> {
        if !durable.is_absolute() {
            return Err(JournalError::Internal(
                "the durable root must be absolute".into(),
            ));
        }
        if !facts.high_waters.valid() || facts.feed.0 > MAX_POSITION {
            return Err(JournalError::Exhausted("origin position"));
        }
        let dir = journal_dir(durable, identity);
        if dir.join("anchor.json").exists() {
            return Err(JournalError::Exists);
        }
        let segments = dir.join("segments");
        let mut current = durable.to_path_buf();
        let below = segments.strip_prefix(durable).expect("built from the root");
        for component in std::iter::once(None).chain(below.components().map(Some)) {
            if let Some(component) = component {
                current.push(component);
            }
            match fs::symlink_metadata(&current) {
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => {
                    let not_dir = io::Error::new(io::ErrorKind::InvalidInput, "not a directory");
                    return Err(io_error(&current, "open", &not_dir));
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => disk
                    .create_dir(&current)
                    .map_err(|e| io_error(&current, "create directory", &e))?,
                Err(e) => return Err(io_error(&current, "open", &e)),
            }
            if let Some(parent) = current.parent() {
                disk.sync_dir(parent).map_err(|e| flush_error(parent, &e))?;
            }
        }
        let anchor = Anchor {
            format: FORMAT.to_owned(),
            identity: identity.clone(),
            high_waters: facts.high_waters,
            feed: facts.feed,
            predecessor: facts.predecessor.clone(),
        };
        let bytes =
            serde_json::to_vec(&anchor).map_err(|e| JournalError::Internal(e.to_string()))?;
        write_small(disk.as_ref(), &dir, "anchor.json", &bytes, true)?;
        let base = JournalPoint {
            ordinal: 0,
            sha256: Sha256::of(&bytes),
            high_waters: facts.high_waters,
            feed: facts.feed,
        };
        Ok(Journal {
            dir,
            disk,
            identity: identity.clone(),
            segment_bytes: AtomicU64::new(SEGMENT_BYTES),
            release_lock: Mutex::new(()),
            inner: Mutex::new(Inner {
                segments: VecDeque::new(),
                active: None,
                unflushed: Vec::new(),
                directory_unflushed: false,
                head: base.clone(),
                base,
                flushed: 0,
                materialized: 0,
                retained: VecDeque::new(),
                broken: None,
            }),
        })
    }

    /// Opens an existing journal: asks the custody for the confirmed head, verifies the
    /// chain from the anchor or the checkpoint, applies the torn-tail rule, and flushes
    /// what it kept. Everything it returns is on the drive.
    pub fn open(
        durable: &Path,
        identity: &OfficeIdentity,
        custody: &dyn WriterAuthority,
    ) -> Result<(Journal, Recovered), JournalError> {
        Self::open_on(Arc::new(RealDisk), durable, identity, custody, None)
    }

    pub(crate) fn open_on(
        disk: Arc<dyn Disk>,
        durable: &Path,
        identity: &OfficeIdentity,
        custody: &dyn WriterAuthority,
        min_consumer_watermark: Option<u64>,
    ) -> Result<(Journal, Recovered), JournalError> {
        if !durable.is_absolute() {
            return Err(JournalError::Internal(
                "the durable root must be absolute".into(),
            ));
        }
        let confirmed = custody
            .checkpoint(identity)
            .map_err(|e| JournalError::Custody(e.0))?;
        let dir = journal_dir(durable, identity);
        let anchor_bytes = read_small(&dir.join("anchor.json"))?
            .ok_or(JournalError::MissingPredecessor { ordinal: 0 })?;
        let anchor: Anchor = serde_json::from_slice(&anchor_bytes)
            .map_err(|e| JournalError::UnknownFormat(format!("anchor: {e}")))?;
        if anchor.format != FORMAT {
            return Err(JournalError::UnknownFormat(anchor.format));
        }
        if anchor.identity != *identity {
            return Err(JournalError::IdentityMismatch);
        }
        let mut base = JournalPoint {
            ordinal: 0,
            sha256: Sha256::of(&anchor_bytes),
            high_waters: anchor.high_waters,
            feed: anchor.feed,
        };
        if let Some(bytes) = read_small(&dir.join("checkpoint.json"))? {
            let checkpoint: Checkpoint = serde_json::from_slice(&bytes)
                .map_err(|e| JournalError::UnknownFormat(format!("checkpoint: {e}")))?;
            if checkpoint.format != FORMAT {
                return Err(JournalError::UnknownFormat(checkpoint.format));
            }
            base = JournalPoint {
                ordinal: checkpoint.ordinal,
                sha256: checkpoint.sha256,
                high_waters: checkpoint.high_waters,
                feed: checkpoint.feed,
            };
        }
        if base.ordinal > MAX_POSITION || base.feed.0 > MAX_POSITION || !base.high_waters.valid() {
            return Err(JournalError::Corrupt {
                ordinal: base.ordinal,
                reason: "the anchor or checkpoint holds a position past the limit".into(),
            });
        }
        let mut materialized = base.ordinal;
        if let Some(bytes) = read_small(&dir.join("materialized.json"))? {
            let mark: Materialized = serde_json::from_slice(&bytes)
                .map_err(|e| JournalError::UnknownFormat(format!("materialized mark: {e}")))?;
            materialized = materialized.max(mark.ordinal);
        }

        // Segments, by first ordinal. One wholly at or below the checkpoint was released
        // and only its unlink was lost.
        let segments_dir = dir.join("segments");
        let mut firsts = Vec::new();
        let listing =
            fs::read_dir(&segments_dir).map_err(|e| io_error(&segments_dir, "list", &e))?;
        for entry in listing {
            let entry = entry.map_err(|e| io_error(&segments_dir, "list", &e))?;
            let name = entry.file_name();
            let first = name
                .to_str()
                .and_then(|n| n.strip_suffix(".log"))
                .filter(|n| n.len() == 20)
                .and_then(|n| n.parse::<u64>().ok());
            if let Some(first) = first {
                firsts.push(first);
            }
        }
        firsts.sort_unstable();
        // Nothing on the drive is changed until the confirmed head has been compared:
        // an opening that refuses leaves every byte where it found it.
        let mut repairs: Vec<Repair> = Vec::new();
        while firsts.len() > 1 && firsts[1] <= base.ordinal + 1 {
            let path = segments_dir.join(segment_name(firsts.remove(0)));
            repairs.push(Repair::Unlink(path));
        }
        // The last of the released segments has no successor's name to prove it: a cut
        // that lost its unlink can also have lost the entry of the segment after it. It
        // is told by what it holds. One that ends at the checkpoint with another entry
        // than the checkpoint's is another history: refused, every byte left.
        if let Some(first) = firsts.first().copied().filter(|f| *f <= base.ordinal) {
            let path = segments_dir.join(segment_name(first));
            let bytes = fs::read(&path).map_err(|e| io_error(&path, "read", &e))?;
            if ends_at_or_below(&bytes, first, &base)? {
                firsts.remove(0);
                repairs.push(Repair::Unlink(path));
            }
        }

        let mut point = base.clone();
        let mut entries: Vec<Arc<Entry>> = Vec::new();
        let mut segments = VecDeque::new();
        let mut truncated = None;
        let mut active = None;
        let mut kept: Vec<(PathBuf, Arc<dyn DiskFile>)> = Vec::new();
        for first in firsts.iter().copied() {
            let path = segments_dir.join(segment_name(first));
            if truncated.is_some() {
                // Everything after a torn entry was never acknowledged durable.
                repairs.push(Repair::Unlink(path));
                continue;
            }
            if first != point.ordinal + 1 {
                // A gap before this segment. A power cut leaves one when it takes the
                // unflushed end of the segment before, or all of it: a segment is rolled
                // without a flush. Every entry in such a gap is above the confirmed head,
                // so this segment and the later ones are a torn tail. A gap that reaches
                // down to the confirmed head, or a segment that starts at an ordinal the
                // chain already holds, is never a cut.
                let above_confirmed = confirmed
                    .as_ref()
                    .is_none_or(|confirmed| point.ordinal >= confirmed.ordinal);
                if first <= point.ordinal || !above_confirmed {
                    return Err(JournalError::MissingPredecessor { ordinal: first });
                }
                truncated = Some(point.ordinal + 1);
                repairs.push(Repair::Unlink(path));
                continue;
            }
            let bytes = fs::read(&path).map_err(|e| io_error(&path, "read", &e))?;
            let good =
                scan(&bytes, &mut point, identity, &mut entries).or_else(|bad| match bad {
                    Bad::Chain { ordinal, reason } => {
                        Err(JournalError::Corrupt { ordinal, reason })
                    }
                    Bad::Torn {
                        offset,
                        ordinal,
                        reason,
                    } => {
                        if confirmed.as_ref().is_some_and(|c| ordinal <= c.ordinal) {
                            return Err(JournalError::Corrupt { ordinal, reason });
                        }
                        truncated = Some(ordinal);
                        Ok(offset)
                    }
                })?;
            // A rolled segment's tail may never have been flushed by the process that
            // wrote it: every kept segment is opened here and flushed below.
            let file = disk
                .open_append(&path)
                .map_err(|e| io_error(&path, "open", &e))?;
            if good < bytes.len() {
                repairs.push(Repair::Truncate {
                    path: path.clone(),
                    file: Arc::clone(&file),
                    length: good as u64,
                });
            }
            kept.push((path.clone(), Arc::clone(&file)));
            active = Some(file);
            segments.push_back(Segment {
                first,
                path,
                length: good as u64,
                end: point.clone(),
            });
        }

        if let Some(confirmed) = &confirmed {
            if confirmed.ordinal > point.ordinal {
                return Err(JournalError::Behind {
                    journal: point.ordinal,
                    custody: confirmed.ordinal,
                });
            }
            // The confirmed entry must be here to be compared: one the journal no longer
            // holds (released before a head the host has since lost) is never a pass.
            let held = if confirmed.ordinal == base.ordinal {
                base.sha256
            } else {
                let found = entries.iter().find(|e| e.ordinal() == confirmed.ordinal);
                found
                    .map(|e| e.sha256)
                    .ok_or(JournalError::MissingPredecessor {
                        ordinal: confirmed.ordinal,
                    })?
            };
            if held != confirmed.sha256 {
                return Err(JournalError::Fork {
                    ordinal: confirmed.ordinal,
                });
            }
        }

        // The journal is this writer's and holds the confirmed head: only now is the
        // torn tail cut away and what a release left behind removed.
        for repair in repairs {
            match repair {
                Repair::Unlink(path) => disk
                    .unlink(&path)
                    .map_err(|e| io_error(&path, "remove", &e))?,
                Repair::Truncate { path, file, length } => file
                    .truncate(length)
                    .map_err(|e| io_error(&path, "truncate", &e))?,
            }
        }
        // Make what was kept durable before anything is built on it.
        disk.sync_dir(&segments_dir)
            .map_err(|e| flush_error(&segments_dir, &e))?;
        for (path, file) in &kept {
            file.flush_full().map_err(|e| flush_error(path, &e))?;
        }
        let materialized = materialized.min(point.ordinal);
        let retained = build_retained_queue(
            &entries,
            base.ordinal,
            materialized,
            point.ordinal,
            min_consumer_watermark,
        )?;
        let journal = Journal {
            dir,
            disk,
            identity: identity.clone(),
            segment_bytes: AtomicU64::new(SEGMENT_BYTES),
            release_lock: Mutex::new(()),
            inner: Mutex::new(Inner {
                segments,
                active,
                unflushed: Vec::new(),
                directory_unflushed: false,
                base,
                flushed: point.ordinal,
                head: point,
                materialized,
                retained,
                broken: None,
            }),
        };
        let recovered = Recovered {
            confirmed,
            entries,
            truncated,
        };
        Ok((journal, recovered))
    }

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

    /// Appends a batch in one write. On an error or a short write nothing of the batch is
    /// kept: the segment is truncated back to its last good entry, and that is flushed,
    /// before this returns. If the truncation fails the journal accepts nothing more.
    pub fn append(&self, entries: &[Arc<Entry>]) -> Result<(), JournalError> {
        let mut inner = self.inner();
        if let Some(broken) = &inner.broken {
            return Err(broken.clone());
        }
        let Some(first) = entries.first() else {
            return Ok(());
        };
        let mut point = inner.head.clone();
        let mut bytes = Vec::with_capacity(entries.iter().map(|e| e.frame.len()).sum());
        for entry in entries {
            if entry.ordinal() > MAX_POSITION {
                return Err(JournalError::Exhausted("journal ordinal"));
            }
            point = point
                .after(entry, &self.identity)
                .map_err(JournalError::Internal)?;
            bytes.extend_from_slice(&entry.frame);
        }
        let limit = self.segment_bytes.load(Ordering::Relaxed);
        let roll = match inner.segments.back() {
            Some(segment) => segment.length > 0 && segment.length + bytes.len() as u64 > limit,
            None => true,
        };
        if roll || inner.active.is_none() {
            let path = match inner.segments.back() {
                Some(segment) if !roll => segment.path.clone(),
                _ => self
                    .dir
                    .join("segments")
                    .join(segment_name(first.ordinal())),
            };
            let file = self
                .disk
                .open_append(&path)
                .map_err(|e| JournalError::Append(format!("open {}: {e}", path.display())))?;
            if roll {
                if let (Some(old), Some(segment)) = (inner.active.take(), inner.segments.back()) {
                    let rolled = (segment.path.clone(), old);
                    inner.unflushed.push(rolled);
                }
                let end = inner.head.clone();
                inner.segments.push_back(Segment {
                    first: first.ordinal(),
                    path,
                    length: 0,
                    end,
                });
                inner.directory_unflushed = true;
            }
            inner.active = Some(file);
        }
        let file = Arc::clone(inner.active.as_ref().expect("opened above"));
        let segment = inner.segments.back_mut().expect("pushed above");
        let failure = match file.write(&bytes) {
            Ok(written) if written == bytes.len() => None,
            Ok(written) => Some(format!("{written} of {} bytes written", bytes.len())),
            Err(e) => Some(e.to_string()),
        };
        if let Some(failure) = failure {
            let repaired = file
                .truncate(segment.length)
                .and_then(|()| file.flush_full());
            if let Err(e) = repaired {
                let broken = JournalError::Broken {
                    path: segment.path.display().to_string(),
                    message: e.to_string(),
                };
                inner.broken = Some(broken.clone());
                return Err(broken);
            }
            return Err(JournalError::Append(failure));
        }
        segment.length += bytes.len() as u64;
        segment.end = point.clone();
        inner.head = point;
        inner.retained.extend(entries.iter().cloned());
        Ok(())
    }

    /// Fully flushes the journal through the head it had when the call began, and returns
    /// that head. Appends continue meanwhile. An error is final for this process.
    ///
    /// Only the store's flusher calls this, and the two calls after it: there is one
    /// flusher by visibility, not by convention.
    pub(crate) fn flush(&self) -> Result<JournalPoint, JournalError> {
        let (target, rolled, active, directory) = {
            let mut inner = self.inner();
            if let Some(broken) = &inner.broken {
                return Err(broken.clone());
            }
            let active = match (&inner.active, inner.segments.back()) {
                (Some(file), Some(segment)) => Some((segment.path.clone(), Arc::clone(file))),
                _ => None,
            };
            let directory = std::mem::take(&mut inner.directory_unflushed);
            let rolled = std::mem::take(&mut inner.unflushed);
            (inner.head.clone(), rolled, active, directory)
        };
        let flushed: Result<(), JournalError> = (|| {
            if directory {
                let segments = self.dir.join("segments");
                self.disk
                    .sync_dir(&segments)
                    .map_err(|e| flush_error(&segments, &e))?;
            }
            for (path, file) in rolled.iter().chain(active.iter()) {
                file.flush_full().map_err(|e| flush_error(path, &e))?;
            }
            Ok(())
        })();
        let mut inner = self.inner();
        if let Err(error) = flushed {
            // After a failed flush the kernel may have dropped the pages: a later flush
            // could succeed without the data being on the drive. Nothing more is flushed
            // or appended in this process.
            inner.broken.get_or_insert(error.clone());
            return Err(error);
        }
        inner.flushed = inner.flushed.max(target.ordinal);
        Ok(target)
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

    /// Reconstructs in-memory `retained` entries from on-disk segments starting at `base`
    /// when a consumer registers after earlier entries were evicted from RAM (`TASK-52` M-15).
    pub(crate) fn reconstruct_retained_from_segments(&self) -> Result<(), JournalError> {
        let _release_guard = self.release_lock.lock().unwrap_or_else(|e| e.into_inner());
        let (base, seg_paths, already_starts_at_base) = {
            let inner = self.inner();
            let base = inner.base.clone();
            let head_ord = inner.head.ordinal;
            let starts = head_ord == base.ordinal
                || inner
                    .retained
                    .front()
                    .is_some_and(|e| e.ordinal() == base.ordinal + 1);
            let paths: Vec<PathBuf> = inner.segments.iter().map(|s| s.path.clone()).collect();
            (base, paths, starts)
        };
        if already_starts_at_base || seg_paths.is_empty() {
            return Ok(());
        }
        let mut point = base;
        let mut scanned = Vec::new();
        for path in seg_paths {
            let bytes = fs::read(&path).map_err(|e| io_error(&path, "read", &e))?;
            let _ = scan(&bytes, &mut point, &self.identity, &mut scanned).map_err(
                |bad| match bad {
                    Bad::Chain { ordinal, reason }
                    | Bad::Torn {
                        ordinal, reason, ..
                    } => JournalError::Corrupt { ordinal, reason },
                },
            )?;
        }
        let mut inner = self.inner();
        let mut merged: VecDeque<Arc<Entry>> = scanned.into_iter().collect();
        let last_scanned = merged.back().map_or(inner.base.ordinal, |e| e.ordinal());
        for existing in &inner.retained {
            if existing.ordinal() > last_scanned {
                merged.push_back(Arc::clone(existing));
            }
        }
        inner.retained = merged;
        Ok(())
    }

    /// Records that every file through `ordinal` is written and flushed, and lets go of
    /// retained entries up to `ordinal`.
    pub(crate) fn set_materialized(&self, ordinal: u64) -> Result<(), JournalError> {
        self.set_materialized_with_release_cap(ordinal, ordinal)
    }

    /// Records that every file through `ordinal` is written and flushed, and lets go of
    /// retained entries up to `release_cap` (which equals `ordinal` when no external journal
    /// consumer is registered, or `ordinal.min(min_consumer_watermark)` under `ARCH-03`).
    pub(crate) fn set_materialized_with_release_cap(
        &self,
        ordinal: u64,
        release_cap: u64,
    ) -> Result<(), JournalError> {
        {
            let inner = self.inner();
            if ordinal > inner.flushed {
                return Err(JournalError::Internal(
                    "an entry is materialized only after its flush".into(),
                ));
            }
            if ordinal <= inner.materialized {
                return Ok(());
            }
        }
        let bytes = serde_json::to_vec(&Materialized { ordinal })
            .map_err(|e| JournalError::Internal(e.to_string()))?;
        write_small(
            self.disk.as_ref(),
            &self.dir,
            "materialized.json",
            &bytes,
            false,
        )?;
        let mut inner = self.inner();
        inner.materialized = inner.materialized.max(ordinal);
        let evict_through = ordinal.min(release_cap);
        while inner
            .retained
            .front()
            .is_some_and(|e| e.ordinal() <= evict_through)
        {
            inner.retained.pop_front();
        }
        Ok(())
    }

    /// Evicts in-memory retained entries through `ordinal.min(materialized)` once all
    /// registered journal consumers have advanced past them (`ARCH-03`).
    pub(crate) fn evict_retained_through(&self, ordinal: u64) {
        let mut inner = self.inner();
        let limit = ordinal.min(inner.materialized);
        while inner.retained.front().is_some_and(|e| e.ordinal() <= limit) {
            inner.retained.pop_front();
        }
    }

    /// Drops every rolled segment whose entries are all at or below `ordinal` and the
    /// materialized mark. `checkpoint.json` is written and flushed first, so the chain
    /// always has a verifiable start; and the segment directory is flushed before that,
    /// so the segment the checkpoint leads to is on the drive by name before the
    /// checkpoint names it. Returns how many segments were dropped.
    pub(crate) fn release_through(&self, ordinal: u64) -> Result<usize, JournalError> {
        let _release_guard = self.release_lock.lock().unwrap_or_else(|e| e.into_inner());
        let (drop, base) = {
            let inner = self.inner();
            let limit = ordinal.min(inner.materialized);
            let rolled = inner.segments.len().saturating_sub(1);
            let passed = inner.segments.iter().take(rolled);
            let drop: Vec<_> = passed
                .take_while(|s| s.end.ordinal <= limit)
                .map(|s| (s.path.clone(), s.end.clone()))
                .collect();
            let Some((_, base)) = drop.last().cloned() else {
                return Ok(0);
            };
            (drop, base)
        };
        let checkpoint = Checkpoint {
            format: FORMAT.to_owned(),
            ordinal: base.ordinal,
            sha256: base.sha256,
            high_waters: base.high_waters,
            feed: base.feed,
        };
        let bytes =
            serde_json::to_vec(&checkpoint).map_err(|e| JournalError::Internal(e.to_string()))?;
        // The segment kept after the released ones may have been rolled since the last
        // flush: its directory entry must not be losable once the checkpoint is.
        let segments = self.dir.join("segments");
        self.disk
            .sync_dir(&segments)
            .map_err(|e| flush_error(&segments, &e))?;
        write_small(
            self.disk.as_ref(),
            &self.dir,
            "checkpoint.json",
            &bytes,
            false,
        )?;
        {
            let mut inner = self.inner();
            for (path, _) in &drop {
                if inner.segments.front().is_some_and(|s| &s.path == path) {
                    inner.segments.pop_front();
                }
            }
            if base.ordinal > inner.base.ordinal {
                inner.base = base;
            }
        }
        for (path, _) in &drop {
            self.disk
                .unlink(path)
                .map_err(|e| io_error(path, "remove", &e))?;
        }
        self.disk
            .sync_dir(&segments)
            .map_err(|e| flush_error(&segments, &e))?;
        Ok(drop.len())
    }

    /// The first ordinal of every segment the journal holds.
    pub fn segment_firsts(&self) -> Vec<u64> {
        self.inner().segments.iter().map(|s| s.first).collect()
    }
}

fn build_retained_queue(
    entries: &[Arc<Entry>],
    base_ordinal: u64,
    materialized: u64,
    head_ordinal: u64,
    min_consumer_watermark: Option<u64>,
) -> Result<VecDeque<Arc<Entry>>, JournalError> {
    if let Some(wm) = min_consumer_watermark {
        if wm < base_ordinal {
            return Err(JournalError::MissingPredecessor { ordinal: wm });
        }
    }
    let retain_floor = min_consumer_watermark.map_or(materialized, |wm| materialized.min(wm));
    let retained: VecDeque<Arc<Entry>> = entries
        .iter()
        .filter(|e| e.ordinal() > retain_floor)
        .cloned()
        .collect();
    if retain_floor < head_ordinal {
        let mut expected = retain_floor + 1;
        for entry in &retained {
            if entry.ordinal() != expected {
                return Err(JournalError::MissingPredecessor { ordinal: expected });
            }
            expected += 1;
        }
        if expected != head_ordinal + 1 {
            return Err(JournalError::MissingPredecessor { ordinal: expected });
        }
    }
    Ok(retained)
}

/// A change opening makes to the drive, held back until the opening is known to pass.
enum Repair {
    Unlink(PathBuf),
    Truncate {
        path: PathBuf,
        file: Arc<dyn DiskFile>,
        length: u64,
    },
}

enum Bad {
    /// A bad length or hash: what a cut write leaves.
    Torn {
        offset: usize,
        ordinal: u64,
        reason: String,
    },
    /// A whole entry, correctly hashed, that does not continue the chain.
    Chain { ordinal: u64, reason: String },
}

/// Verifies the entries of one segment from `point`, advancing it. Returns the length of
/// the good prefix.
fn scan(
    bytes: &[u8],
    point: &mut JournalPoint,
    identity: &OfficeIdentity,
    entries: &mut Vec<Arc<Entry>>,
) -> Result<usize, Bad> {
    let mut offset = 0;
    while offset < bytes.len() {
        let ordinal = point.ordinal + 1;
        let torn = |reason: &str| Bad::Torn {
            offset,
            ordinal,
            reason: reason.to_owned(),
        };
        let (frame, sha256) = whole_frame(&bytes[offset..]).map_err(torn)?;
        let chain = |reason: String| Bad::Chain { ordinal, reason };
        let entry = Entry::parse(frame, sha256).map_err(chain)?;
        *point = point.after(&entry, identity).map_err(chain)?;
        entries.push(Arc::new(entry));
        offset += frame.len();
    }
    Ok(offset)
}

/// The first frame of `rest`, if it is whole and its hash matches, or what a cut left.
fn whole_frame(rest: &[u8]) -> Result<(&[u8], Sha256), &'static str> {
    let Some(length) = rest.first_chunk::<4>() else {
        return Err("a cut length");
    };
    let length = u32::from_be_bytes(*length) as usize;
    let total = 4 + length + TRAILER;
    if length == 0 || total > MAX_ENTRY_BYTES {
        return Err("an impossible length");
    }
    if rest.len() < total {
        return Err("a cut entry");
    }
    let sha256 = Sha256::of(&rest[4..4 + length]);
    if sha256.as_bytes()[..] != rest[4 + length..total] {
        return Err("its hash does not match its bytes");
    }
    Ok((&rest[..total], sha256))
}

/// Whether a segment named `first` holds only whole entries, numbered from `first`
/// without a gap, the last of them at or below `base`: a segment a release had passed.
/// Its chain cannot be verified (its predecessor is gone) and need not be: nothing in it
/// is read again. The one entry that can be compared is: a last entry at `base`'s own
/// ordinal is the entry the checkpoint was written from, so another hash there is
/// corruption, not a segment to drop.
fn ends_at_or_below(bytes: &[u8], first: u64, base: &JournalPoint) -> Result<bool, JournalError> {
    let (mut offset, mut next) = (0, first);
    let mut last = None;
    while offset < bytes.len() {
        let Ok((frame, sha256)) = whole_frame(&bytes[offset..]) else {
            return Ok(false);
        };
        if next > base.ordinal || Entry::parse(frame, sha256).map(|e| e.ordinal()) != Ok(next) {
            return Ok(false);
        }
        last = Some((next, sha256));
        next += 1;
        offset += frame.len();
    }
    match last {
        Some((ordinal, sha256)) if ordinal == base.ordinal && sha256 != base.sha256 => {
            Err(JournalError::Corrupt {
                ordinal,
                reason: "the entry at the checkpoint is not the one the checkpoint names".into(),
            })
        }
        _ => Ok(true),
    }
}

fn read_small(path: &Path) -> Result<Option<Vec<u8>>, JournalError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_error(path, "read", &e)),
    }
}

/// Publishes one small file of the journal: written and fully flushed under a temporary
/// name, then linked (`exclusive`) or renamed into place, then the directory and one more
/// full flush.
fn write_small(
    disk: &dyn Disk,
    dir: &Path,
    name: &str,
    bytes: &[u8],
    exclusive: bool,
) -> Result<(), JournalError> {
    let path = dir.join(name);
    let temp = dir.join(format!("{TEMP_PREFIX}{name}"));
    let file = match disk.create_new(&temp) {
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            disk.unlink(&temp)
                .map_err(|e| io_error(&temp, "remove", &e))?;
            disk.create_new(&temp)
        }
        other => other,
    }
    .map_err(|e| io_error(&temp, "create", &e))?;
    write_all(file.as_ref(), bytes).map_err(|e| io_error(&temp, "write", &e))?;
    file.flush_full().map_err(|e| flush_error(&temp, &e))?;
    if exclusive {
        disk.link(&temp, &path).map_err(|e| match e.kind() {
            io::ErrorKind::AlreadyExists => JournalError::Exists,
            _ => io_error(&path, "link", &e),
        })?;
        disk.unlink(&temp)
            .map_err(|e| io_error(&temp, "remove", &e))?;
    } else {
        disk.rename(&temp, &path)
            .map_err(|e| io_error(&path, "rename", &e))?;
    }
    disk.sync_dir(dir).map_err(|e| flush_error(dir, &e))?;
    file.flush_full().map_err(|e| flush_error(&path, &e))
}

#[cfg(test)]
mod tests;
