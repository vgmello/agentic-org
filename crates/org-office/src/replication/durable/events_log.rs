//! The organization events' append-only log `events.v1.ndjson`, beside `state.v1.json`
//! (`TASK-140`). Engine-independent: every engine leaves the same two files.
//!
//! The state file stays the single commit point. It names the log's committed extent
//! ([`StoredEventsLog`]): a commit that only adds events appends their lines and syncs the
//! log (and, for the log's first lines, its folder), then writes the small state file
//! atomically, so a commit costs what it adds rather than the whole history. Bytes past the
//! committed extent are an append whose state file never committed (a crash in between, or a
//! failed write): they are never read and are cut before the next append, and at open once
//! the state file has decoded. Any other change of the events (a snapshot install or
//! a re-enrollment clears them) commits by writing them inline into the state file first, then
//! moves them to a freshly written log; until that move succeeds the inline copy is the
//! authority and the log is ignored. A state file from before the log (inline `events`, no
//! `events_log`) is read the same way and migrated by its next commit.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::error::OfficeError;
use crate::private_fs::{full_sync, private_file, sync_dir, write_private_atomic};
use crate::replication::{
    memory::ReplicationCoreData, ReplicationStateFile, StoredEventsLog, StoredOrgEvent,
};

use super::appended_events;
use super::state_file::{encode_state_file, stored_event};

/// The log's file name in the office's `replication/` folder.
pub(super) const EVENTS_FILE: &str = "events.v1.ndjson";

/// The committed log as the store knows it.
pub(super) struct EventsLog {
    path: PathBuf,
    /// The extent the committed state file names, or `None` while the committed state file
    /// holds the events inline.
    committed: Option<StoredEventsLog>,
}

const EMPTY: StoredEventsLog = StoredEventsLog {
    bytes: 0,
    count: 0,
    last_digest_hex: None,
};

fn invalid(what: &'static str) -> OfficeError {
    OfficeError::Invalid(what)
}

impl EventsLog {
    /// The log of a new state: empty, and not created before its first event.
    pub(super) fn empty(dir: &Path) -> (Self, StoredEventsLog) {
        let log = Self {
            path: dir.join(EVENTS_FILE),
            committed: Some(EMPTY),
        };
        (log, EMPTY)
    }

    /// Reads the events the committed state file `file` names, from the log or inline. Reads
    /// only: the caller cuts any uncommitted tail ([`Self::cut_uncommitted_tail`]) once the
    /// state file and these events have decoded, so a state file that fails to decode never
    /// changes the log.
    pub(super) fn open(
        dir: &Path,
        file: &ReplicationStateFile,
    ) -> Result<(Self, Vec<StoredOrgEvent>), OfficeError> {
        let path = dir.join(EVENTS_FILE);
        let Some(extent) = file.events_log.clone() else {
            // Inline: the state file is the authority and any log beside it is stale.
            let log = Self {
                path,
                committed: None,
            };
            return Ok((log, file.events.clone()));
        };
        if !file.events.is_empty() {
            return Err(invalid("malformed durable replication state"));
        }
        let events = read_committed(&path, &extent)?;
        let log = Self {
            path,
            committed: Some(extent),
        };
        Ok((log, events))
    }

    /// Cuts the log back to its committed extent: what lies past it never committed. Nothing
    /// to cut while the committed state holds the events inline (the log is not read then,
    /// and the next commit rewrites it).
    pub(super) fn cut_uncommitted_tail(&self) -> Result<(), OfficeError> {
        match &self.committed {
            Some(extent) => truncate_to(&self.path, extent.bytes),
            None => Ok(()),
        }
    }

    /// Commits `after` (the store holds `before`): the one durable commit of a mutation. On
    /// `Ok` the state file names `after`; on `Err` the committed state is still `before`'s.
    pub(super) fn commit(
        &mut self,
        state_path: &Path,
        before: &ReplicationCoreData,
        after: &ReplicationCoreData,
    ) -> Result<(), OfficeError> {
        match (self.committed.clone(), appended_events(before, after)) {
            (Some(extent), Some(added)) if added.is_empty() => {
                write_state(state_path, after, Some(&extent))
            }
            (Some(extent), Some(added)) => {
                let mut lines = Vec::new();
                for event in &added {
                    encode_line(&mut lines, &stored_event(event))?;
                }
                // Durable before the state file names it: the lines, and the log's folder
                // entry when this append created it.
                self.append_at(extent.bytes, &lines)?;
                let next = StoredEventsLog {
                    bytes: extent.bytes.saturating_add(lines.len() as u64),
                    count: extent.count.saturating_add(added.len() as u64),
                    last_digest_hex: added.last().map(|e| e.digest.to_hex()),
                };
                write_state(state_path, after, Some(&next))?;
                self.committed = Some(next);
                Ok(())
            }
            _ if after.events.is_empty() => {
                write_state(state_path, after, Some(&EMPTY))?;
                self.committed = Some(EMPTY);
                // Tidying only: nothing past the committed extent (none) is ever read.
                let _ = truncate_to(&self.path, 0);
                Ok(())
            }
            _ => {
                write_state(state_path, after, None)?;
                self.committed = None;
                // Committed inline; moving the events to the log is retried by the next
                // commit if it fails here.
                let _ = self.move_inline_events_to_log(state_path, after);
                Ok(())
            }
        }
    }

    /// Writes all of `data`'s events as a fresh log, then the state file naming it.
    fn move_inline_events_to_log(
        &mut self,
        state_path: &Path,
        data: &ReplicationCoreData,
    ) -> Result<(), OfficeError> {
        let mut lines = Vec::new();
        for event in data.events.values() {
            encode_line(&mut lines, &stored_event(event))?;
        }
        write_private_atomic(&self.path, &lines).map_err(OfficeError::storage)?;
        let extent = StoredEventsLog {
            bytes: lines.len() as u64,
            count: data.events.len() as u64,
            last_digest_hex: data.events.values().next_back().map(|e| e.digest.to_hex()),
        };
        write_state(state_path, data, Some(&extent))?;
        self.committed = Some(extent);
        Ok(())
    }

    /// Writes `lines` at the committed end `at`, first cutting any uncommitted tail, and
    /// syncs the log. When the log is new (created here, or its committed extent is empty)
    /// its folder is synced too, so the state file's rename can never become durable before
    /// the log's folder entry. A log shorter than its committed extent fails closed.
    fn append_at(&self, at: u64, lines: &[u8]) -> Result<(), OfficeError> {
        let created = !self.path.exists();
        let mut file = private_file(&self.path).map_err(OfficeError::storage)?;
        let len = file.metadata().map_err(OfficeError::storage)?.len();
        if len < at {
            return Err(invalid(
                "durable replication events log is shorter than its committed state",
            ));
        }
        if len > at {
            file.set_len(at).map_err(OfficeError::storage)?;
        }
        file.seek(SeekFrom::Start(at))
            .map_err(OfficeError::storage)?;
        file.write_all(lines).map_err(OfficeError::storage)?;
        full_sync(&file).map_err(OfficeError::storage)?;
        if created || at == 0 {
            let parent = self
                .path
                .parent()
                .ok_or_else(|| invalid("durable replication events log has no folder"))?;
            sync_dir(parent).map_err(OfficeError::storage)?;
        }
        Ok(())
    }
}

fn write_state(
    state_path: &Path,
    data: &ReplicationCoreData,
    extent: Option<&StoredEventsLog>,
) -> Result<(), OfficeError> {
    let encoded = encode_state_file(data, extent)?;
    write_private_atomic(state_path, &encoded).map_err(OfficeError::storage)
}

fn encode_line(out: &mut Vec<u8>, event: &StoredOrgEvent) -> Result<(), OfficeError> {
    serde_json::to_writer(&mut *out, event).map_err(OfficeError::storage)?;
    out.push(b'\n');
    Ok(())
}

/// Reads exactly the committed extent and checks it: whole lines, their count and the last
/// one's digest. A missing, short or different log fails closed.
fn read_committed(
    path: &Path,
    extent: &StoredEventsLog,
) -> Result<Vec<StoredOrgEvent>, OfficeError> {
    if extent.bytes == 0 {
        if extent.count != 0 || extent.last_digest_hex.is_some() {
            return Err(invalid("malformed durable replication state"));
        }
        return Ok(Vec::new());
    }
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(invalid("durable replication events log is missing"));
        }
        Err(e) => return Err(OfficeError::storage(e)),
    };
    if !file.metadata().map_err(OfficeError::storage)?.is_file() {
        return Err(invalid("durable replication events log is not a file"));
    }
    let mut bytes = Vec::new();
    file.take(extent.bytes)
        .read_to_end(&mut bytes)
        .map_err(OfficeError::storage)?;
    if bytes.len() as u64 != extent.bytes {
        return Err(invalid(
            "durable replication events log is shorter than its committed state",
        ));
    }
    let Some(body) = bytes.strip_suffix(b"\n") else {
        return Err(invalid("torn durable replication events log"));
    };
    let mut events = Vec::new();
    for line in body.split(|b| *b == b'\n') {
        let event: StoredOrgEvent = serde_json::from_slice(line)
            .map_err(|_| invalid("malformed durable replication event"))?;
        events.push(event);
    }
    if events.len() as u64 != extent.count
        || events.last().map(|e| &e.digest_hex) != extent.last_digest_hex.as_ref()
    {
        return Err(invalid(
            "durable replication events log does not match its committed state",
        ));
    }
    Ok(events)
}

/// Cuts the log back to `committed` bytes: what lies past it never committed. A missing log
/// has nothing to cut.
fn truncate_to(path: &Path, committed: u64) -> Result<(), OfficeError> {
    let file = match OpenOptions::new().write(true).open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(OfficeError::storage(e)),
    };
    if file.metadata().map_err(OfficeError::storage)?.len() > committed {
        file.set_len(committed).map_err(OfficeError::storage)?;
        full_sync(&file).map_err(OfficeError::storage)?;
    }
    Ok(())
}
