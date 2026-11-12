//! Private call lifecycle; frozen contract agents d8db96d (revision 6).

use std::collections::BTreeSet;
use std::fmt;
use std::sync::{mpsc, Arc, Mutex, PoisonError};

use org_proto::work::{EventRef, WorkEvent, WorkPosition};
use org_proto::Position;
use org_work::{NumberScope, StoreError, StoredItem, StoredResult};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};

use crate::store::origin::{self, OriginRef};
use crate::store::{Store, WriteJob};
use crate::OfficeError;

pub(super) const ROLLBACK: &str = "internal work call rollback";

pub(super) enum WorkCallError {
    Unrecorded,
    AlreadyRecorded,
    NothingToRecord,
    InvalidRefs,
    RefsMismatch,
    DuplicatePut,
    CallFailed,
    ResultBeforeRecord,
    Port(StoreError),
    Store(OfficeError),
}

impl fmt::Display for WorkCallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unrecorded => "work call finish: unrecorded writes",
            Self::AlreadyRecorded => "work call write: already recorded",
            Self::NothingToRecord => "work call record_change: no puts",
            Self::InvalidRefs => "work call record_change: invalid refs",
            Self::RefsMismatch => "work call record_change: refs mismatch",
            Self::DuplicatePut => "work call put_item: duplicate put",
            Self::CallFailed => "work call write: call failed",
            Self::ResultBeforeRecord => "work call result: changes not recorded",
            Self::Port(_) => "work call callback failed",
            Self::Store(_) => "work call storage step failed",
        })
    }
}

impl fmt::Debug for WorkCallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Retain the typed cause for diagnostics without echoing row/SQL content.
        fmt::Display::fmt(self, f)
    }
}

impl std::error::Error for WorkCallError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::Port(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Open,
    Writing,
    Finalized,
    Failed,
}

pub(super) struct WorkCall<'a> {
    db: &'a Connection,
    feed: Option<u64>,
    puts: Vec<OriginRef>,
    keys: BTreeSet<(String, u64)>,
    state: State,
    result_written: bool,
}

pub(super) struct Reply<T> {
    answer: mpsc::Receiver<Result<T, OfficeError>>,
    slot: Arc<Mutex<Option<WorkCallError>>>,
}

impl<T> Reply<T> {
    pub(super) fn recv(self) -> Result<T, WorkCallError> {
        let result = self
            .answer
            .recv()
            .unwrap_or_else(|_| Err(crate::store::failure("the storage writer stopped")));
        finish(result, &self.slot)
    }
}

pub(super) fn transact<T: Send + 'static>(
    store: &Store,
    callback: impl Fn(&mut WorkCall<'_>) -> Result<T, WorkCallError> + Send + 'static,
) -> Result<T, WorkCallError> {
    transact_with_abort(store, callback, |_| false)
}

pub(super) fn transact_with_abort<T: Send + 'static>(
    store: &Store,
    callback: impl Fn(&mut WorkCall<'_>) -> Result<T, WorkCallError> + Send + 'static,
    abort_batch: fn(&OfficeError) -> bool,
) -> Result<T, WorkCallError> {
    let (job, reply) = transact_job(move |db| run(db, &callback), abort_batch);
    store.send_job(job).map_err(WorkCallError::Store)?;
    reply.recv()
}

pub(super) fn transact_job<T: Send + 'static>(
    callback: impl Fn(&Connection) -> Result<T, WorkCallError> + Send + 'static,
    abort_batch: fn(&OfficeError) -> bool,
) -> (WriteJob, Reply<T>) {
    let slot = Arc::new(Mutex::new(None));
    let attempt_slot = Arc::clone(&slot);
    let (job, answer) = Store::job_with_batch_abort(
        move |db| bridge(&attempt_slot, || callback(db)).map(|value| (value, None)),
        abort_batch,
    );
    (job, Reply { answer, slot })
}

/// Error bridge shared by the writer attempt and its eventual committed reply.
pub(super) fn attempt<T>(
    db: &Connection,
    slot: &Mutex<Option<WorkCallError>>,
    callback: impl FnOnce(&mut WorkCall<'_>) -> Result<T, WorkCallError>,
) -> Result<T, OfficeError> {
    bridge(slot, || run(db, callback))
}

fn bridge<T>(
    slot: &Mutex<Option<WorkCallError>>,
    callback: impl FnOnce() -> Result<T, WorkCallError>,
) -> Result<T, OfficeError> {
    *slot.lock().unwrap_or_else(PoisonError::into_inner) = None;
    match callback() {
        Ok(value) => Ok(value),
        Err(WorkCallError::Store(error)) => Err(error),
        Err(error) => {
            *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(error);
            Err(OfficeError::Invalid(ROLLBACK))
        }
    }
}

pub(super) fn finish<T>(
    result: Result<T, OfficeError>,
    slot: &Mutex<Option<WorkCallError>>,
) -> Result<T, WorkCallError> {
    match result {
        Err(error) if matches!(error, OfficeError::Invalid(ROLLBACK)) => Err(slot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .unwrap_or(WorkCallError::Store(error))),
        other => other.map_err(WorkCallError::Store),
    }
}

pub(super) fn run<T>(
    db: &Connection,
    callback: impl FnOnce(&mut WorkCall<'_>) -> Result<T, WorkCallError>,
) -> Result<T, WorkCallError> {
    if db.is_autocommit() {
        return Err(WorkCallError::Store(OfficeError::Invalid(
            "work call requires a writer transaction",
        )));
    }
    let mut call = WorkCall {
        db,
        feed: None,
        puts: Vec::new(),
        keys: BTreeSet::new(),
        state: State::Open,
        result_written: false,
    };
    let value = callback(&mut call)?;
    match call.state {
        State::Writing => Err(WorkCallError::Unrecorded),
        State::Failed => Err(WorkCallError::CallFailed),
        _ => Ok(value),
    }
}

impl WorkCall<'_> {
    pub(super) fn conn(&self) -> &Connection {
        self.db
    }

    fn writable(&self) -> Result<(), WorkCallError> {
        if self.result_written {
            return Err(WorkCallError::AlreadyRecorded);
        }
        match self.state {
            State::Finalized => Err(WorkCallError::AlreadyRecorded),
            State::Failed => Err(WorkCallError::CallFailed),
            _ => Ok(()),
        }
    }

    pub(super) fn allocate_number(&mut self, scope: &NumberScope) -> Result<u64, WorkCallError> {
        self.writable()?;
        let value = super::metadata::allocate_number(self.db, scope).map_err(|error| {
            self.state = State::Failed;
            WorkCallError::Store(error)
        })?;
        self.state = State::Writing;
        Ok(value)
    }

    pub(super) fn put_change_result(
        &mut self,
        id: &str,
        result: &StoredResult,
    ) -> Result<(), WorkCallError> {
        if self.result_written {
            return Err(WorkCallError::AlreadyRecorded);
        }
        match self.state {
            State::Writing => return Err(WorkCallError::ResultBeforeRecord),
            State::Failed => return Err(WorkCallError::CallFailed),
            _ => {}
        }
        super::metadata::put_change_result(self.db, id, result).map_err(|error| {
            self.state = State::Failed;
            WorkCallError::Store(error)
        })?;
        self.result_written = true;
        Ok(())
    }

    pub(super) fn put_item(
        &mut self,
        item: &StoredItem,
        events: &[WorkEvent],
    ) -> Result<(), WorkCallError> {
        self.writable()?;
        let mut incoming = BTreeSet::new();
        for event in events {
            let key = (event.target_id.clone(), event.version);
            if self.keys.contains(&key) || !incoming.insert(key) {
                return Err(WorkCallError::DuplicatePut);
            }
        }
        let result = (|| {
            let feed = match self.feed {
                Some(feed) => feed,
                None => {
                    let allocated: i64 = self.db.query_row(
                        "INSERT INTO work_feed(targets,change_id) VALUES ('[]',NULL) RETURNING seq",
                        [], |row| row.get(0)).map_err(OfficeError::storage)?;
                    let feed = allocated as u64; // generated positive INTEGER PRIMARY KEY
                    self.feed = Some(feed);
                    feed
                }
            };
            super::put_item(self.db, item, events, feed)
        })();
        if let Err(error) = result {
            self.state = State::Failed;
            return Err(WorkCallError::Store(error));
        }
        self.keys.extend(incoming);
        self.puts.extend(events.iter().map(|event| OriginRef {
            target_id: event.target_id.clone(),
            generation: item.generation,
            version: event.version,
        }));
        self.state = State::Writing;
        Ok(())
    }

    pub(super) fn record_change(
        &mut self,
        position: Position,
        refs: &[EventRef],
        change_id: Option<&str>,
        at_ms: u64,
    ) -> Result<WorkPosition, WorkCallError> {
        self.writable()?;
        if self.puts.is_empty() {
            return Err(WorkCallError::NothingToRecord);
        }
        let supplied: BTreeSet<_> = refs
            .iter()
            .map(|r| (r.target_id.clone(), r.version))
            .collect();
        if refs.is_empty() || supplied.len() < refs.len() {
            return Err(WorkCallError::InvalidRefs);
        }
        if supplied != self.keys {
            return Err(WorkCallError::RefsMismatch);
        }
        match self.finalize(position, change_id, at_ms) {
            Ok(position) => {
                self.state = State::Finalized;
                Ok(position)
            }
            Err(error) => {
                self.state = State::Failed;
                Err(WorkCallError::Store(error))
            }
        }
    }

    fn finalize(
        &self,
        position: Position,
        change_id: Option<&str>,
        at_ms: u64,
    ) -> Result<WorkPosition, OfficeError> {
        let feed = self.feed.expect("successful puts allocate a feed");
        origin::append(self.db, position, &self.puts, feed, at_ms)?;
        let mut seen = BTreeSet::new();
        let targets: Vec<_> = self
            .puts
            .iter()
            .filter_map(|p| {
                seen.insert(p.target_id.as_str())
                    .then_some(p.target_id.as_str())
            })
            .collect();
        let targets = serde_json::to_string(&targets).map_err(OfficeError::storage)?;
        let updated = self
            .db
            .execute(
                "UPDATE work_feed SET targets=?1,change_id=?2 WHERE seq=?3",
                params![targets, change_id, feed as i64],
            )
            .map_err(OfficeError::storage)?;
        if updated != 1 {
            return Err(OfficeError::Invalid("work feed was not finalized"));
        }
        let origin = origin::local_origin(self.db)?;
        let mut hash = Sha256::new();
        hash.update(b"agentic-org/work-feed-incarnation/v1\0");
        hash.update(origin.incarnation.as_bytes());
        let digest = hash.finalize();
        let mut first = [0; 8];
        first.copy_from_slice(&digest[..8]);
        Ok(WorkPosition {
            incarnation: u64::from_be_bytes(first),
            seq: feed,
        })
    }
}
