//! What the store's writer, flusher and readers share: its bounds, the flow of appended,
//! flushed and materialized entries under one lock, and the composed parts they all use.

use std::{
    collections::VecDeque,
    sync::{mpsc::SyncSender, Arc, Condvar, Mutex, MutexGuard},
    time::Duration,
};

use crate::{
    change::{Bytes, Clock, Outcome},
    changes::Changes,
    custody::{JournalHead, WriterAuthority},
    engine::{Cursor, Engine},
    error::{JournalError, StoreError},
    files::Files,
    hooks::Hooks,
    journal::{Entry, Journal},
    rebuild::Projectors,
};

/// The most operations one batch takes.
pub const MAX_BATCH: usize = 64;

/// The store's bounds. The defaults are the design's; B1c tunes them by measurement.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Limits {
    pub max_batch: usize,
    /// Appended, not yet flushed. At this many bytes the writer takes no new batch.
    pub unflushed_bytes: u64,
    /// Flushed, files not yet written. At this many bytes the writer takes no new batch.
    pub unmaterialized_bytes: u64,
    /// Entries the change stream keeps for late subscribers. Zero is refused at opening.
    pub change_ring: usize,
    /// A journal segment is rolled before an append would take it past this.
    pub segment_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_batch: MAX_BATCH,
            unflushed_bytes: 8 * 1024 * 1024,
            unmaterialized_bytes: 64 * 1024 * 1024,
            change_ring: 8_192,
            segment_bytes: crate::journal::SEGMENT_BYTES,
        }
    }
}

/// The answer to an admitted operation.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reply {
    pub outcome: Outcome,
    pub bytes: Bytes,
}

/// A durable reply waiting for the flush and the host confirmation that cover its ordinal.
pub(crate) type Waiter = (u64, SyncSender<Result<Reply, StoreError>>, Reply);

/// What the three threads share, under one lock.
pub(crate) struct Flow {
    /// Why the store stopped accepting. Reads continue.
    pub(crate) fatal: Option<StoreError>,
    /// The process is dead (a crash point, or the store was dropped): nothing more is done.
    pub(crate) dead: bool,
    pub(crate) closing: bool,
    pub(crate) writer_done: bool,
    pub(crate) sync_wanted: bool,
    pub(crate) holding_back: bool,
    /// Submitted, not yet taken by the writer.
    pub(crate) queued: usize,
    pub(crate) flushed: u64,
    pub(crate) materialized: u64,
    /// The flushed head the host last confirmed. A durable reply is sent only for an
    /// ordinal at or below it.
    pub(crate) confirmed: Option<JournalHead>,
    /// Durable replies waiting for the flush and the confirmation that cover their ordinal.
    pub(crate) waiters: Vec<Waiter>,
    /// Ordinal and size of each appended entry not yet flushed.
    pub(crate) unflushed: VecDeque<(u64, u64)>,
    pub(crate) unflushed_bytes: u64,
    /// The same for entries flushed and not yet materialized.
    pub(crate) unmaterialized: VecDeque<(u64, u64)>,
    pub(crate) unmaterialized_bytes: u64,
    /// Persistent consumer watermarks (`ARCH-03`, `TASK-52`): journal segments are never
    /// released past the minimum registered consumer watermark.
    pub(crate) consumer_watermarks: std::collections::BTreeMap<String, u64>,
}

impl Flow {
    /// Maximum ordinal the flusher may release from journal segments: bounded by both
    /// `materialized` and the slowest registered consumer watermark (`ARCH-03`).
    pub(crate) fn release_cap(&self, materialized: u64) -> u64 {
        self.consumer_watermarks
            .values()
            .copied()
            .min()
            .map_or(materialized, |min_wm| materialized.min(min_wm))
    }
    /// The journal is flushed through `ordinal`. Nobody is answered yet: a durable reply
    /// also needs the host's confirmation.
    pub(crate) fn flushed_through(&mut self, ordinal: u64) {
        self.flushed = self.flushed.max(ordinal);
        while self.unflushed.front().is_some_and(|(o, _)| *o <= ordinal) {
            let entry = self.unflushed.pop_front().expect("checked");
            self.unflushed_bytes -= entry.1;
            self.unmaterialized_bytes += entry.1;
            self.unmaterialized.push_back(entry);
        }
    }

    /// Whether a durable reply for `ordinal` may be sent: flushed, and confirmed by the host.
    pub(crate) fn durable_at(&self, ordinal: u64) -> bool {
        let confirmed = self.confirmed.as_ref();
        ordinal <= self.flushed && confirmed.is_some_and(|head| ordinal <= head.ordinal)
    }

    /// The host confirmed the flushed `head`: answer every durable waiter it covers.
    pub(crate) fn confirmed_through(&mut self, head: JournalHead) {
        let ordinal = head.ordinal.min(self.flushed);
        self.confirmed = Some(head);
        let waiters = std::mem::take(&mut self.waiters);
        for (waiting, to, reply) in waiters {
            if waiting <= ordinal {
                let _ = to.send(Ok(reply));
            } else {
                self.waiters.push((waiting, to, reply));
            }
        }
    }

    pub(crate) fn materialized_through(&mut self, ordinal: u64) {
        self.materialized = self.materialized.max(ordinal);
        while self
            .unmaterialized
            .front()
            .is_some_and(|(o, _)| *o <= ordinal)
        {
            let (_, bytes) = self.unmaterialized.pop_front().expect("checked");
            self.unmaterialized_bytes -= bytes;
        }
    }
}

pub(crate) struct Core<E: Engine> {
    pub(crate) engine: Arc<E>,
    pub(crate) projectors: Projectors<E>,
    pub(crate) files: Files,
    pub(crate) journal: Journal,
    pub(crate) custody: Arc<dyn WriterAuthority>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) hooks: Arc<dyn Hooks>,
    pub(crate) limits: Limits,
    pub(crate) flush_interval: Duration,
    pub(crate) changes: Changes,
    pub(crate) release_gate: Mutex<()>,
    pub(crate) materialization_lock: Mutex<()>,
    /// Taken only through [`Core::flow`], which survives a poisoned lock.
    pub(crate) flow: Mutex<Flow>,
    pub(crate) wake: Condvar,
}

impl<E: Engine> Core<E> {
    pub(crate) fn flow(&self) -> MutexGuard<'_, Flow> {
        self.flow.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Stops accepting, for good. No waiting durable request is answered as success.
    pub(crate) fn stop(&self, fatal: StoreError) {
        let mut flow = self.flow();
        let fatal = flow.fatal.get_or_insert(fatal).clone();
        for (_, to, _) in flow.waiters.drain(..) {
            let _ = to.send(Err(fatal.clone()));
        }
        self.wake.notify_all();
    }

    /// The process is dead: every thread returns at its next step and nothing is answered.
    pub(crate) fn die(&self) {
        let mut flow = self.flow();
        flow.dead = true;
        flow.waiters.clear();
        self.wake.notify_all();
    }

    /// Appends a batch and accounts for it under one lock, so the flusher never sees the
    /// journal ahead of the accounting: a flush that covers these entries moves them on.
    /// A store that stopped since the batch began appends nothing: the check is made
    /// under the same lock that stopping takes. An error means nothing was appended.
    pub(crate) fn append(&self, entries: &[Arc<Entry>]) -> Result<(), StoreError> {
        let mut flow = self.flow();
        if let Some(fatal) = &flow.fatal {
            return Err(fatal.clone());
        }
        // B1c: measure. The segment write runs under the flow lock, so a submit, a status
        // read and the flusher's accounting wait for it (review finding F8).
        if let Err(error) = self.journal.append(entries) {
            drop(flow);
            let failure = StoreError::Append(error.to_string());
            if matches!(error, JournalError::Broken { .. }) {
                self.stop(StoreError::Journal(error));
            }
            return Err(failure);
        }
        for entry in entries {
            let bytes = entry.stored().len() as u64;
            flow.unflushed.push_back((entry.ordinal(), bytes));
            flow.unflushed_bytes += bytes;
        }
        self.wake.notify_all();
        Ok(())
    }

    /// Applies appended entries to the projection in a transaction of their own.
    pub(crate) fn replay(&self, entries: &[Arc<Entry>], cursor: &Cursor) -> Result<(), StoreError> {
        let mut tx = self.engine.begin()?;
        for entry in entries {
            self.projectors.apply_entry(&mut tx, entry)?;
        }
        Ok(self.engine.commit(tx, cursor)?)
    }
}
