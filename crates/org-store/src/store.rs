//! The store: one writer thread, one flusher thread, and reads that touch only the
//! projection.

use std::{
    path::PathBuf,
    sync::{
        mpsc::{self, Sender},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::Duration,
};

use crate::{
    change::{Change, Clock, Operation, Refusal},
    changes::{ChangeStream, Resync},
    custody::{OfficeIdentity, WriterAuthority},
    disk::{Disk, RealDisk},
    engine::{CoreRead, Engine, Projector},
    error::StoreError,
    flusher,
    hooks::{Hooks, NoHooks},
    journal::{AnchorFacts, Entry},
    path::DurablePath,
    sequences::FeedPosition,
    state::{Core, Flow, Limits, Reply},
    writer::{self, Job, UnitOfWork},
};

mod consumers;
mod open;

use open::{open_store_core, spawn_store_thread};

/// Whether the journal incarnation is new.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Opening {
    /// Start a new journal incarnation from these facts. Refused if one exists.
    Create(AnchorFacts),
    Existing,
}

/// Everything a store is composed from.
pub struct StoreParts<E: Engine> {
    /// `<data-root>/<organization-id>/durable`, absolute.
    pub durable: PathBuf,
    pub identity: OfficeIdentity,
    pub custody: Arc<dyn WriterAuthority>,
    pub engine: Arc<E>,
    /// One projector per family. The store adds its own receipt family.
    pub projectors: Vec<Arc<dyn Projector<E>>>,
    /// Durable subtrees owned by independent companion stores (pruned before rebuild walks).
    pub excluded_prefixes: Vec<DurablePath>,
    pub clock: Arc<dyn Clock>,
    /// How long appended entries may wait for a flush nobody asked for.
    pub flush_interval: Duration,
    pub opening: Opening,
    pub limits: Limits,
    pub(crate) disk: Arc<dyn Disk>,
    pub(crate) hooks: Arc<dyn Hooks>,
}

impl<E: Engine> StoreParts<E> {
    pub fn new(
        durable: PathBuf,
        identity: OfficeIdentity,
        custody: Arc<dyn WriterAuthority>,
        engine: Arc<E>,
        projectors: Vec<Arc<dyn Projector<E>>>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            durable,
            identity,
            custody,
            engine,
            projectors,
            excluded_prefixes: Vec::new(),
            clock,
            flush_interval: Duration::from_millis(50),
            opening: Opening::Existing,
            limits: Limits::default(),
            disk: Arc::new(RealDisk),
            hooks: Arc::new(NoHooks),
        }
    }
}

/// What the store reports about itself.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Status {
    /// Why the store stopped accepting, if it has.
    pub stopped: Option<StoreError>,
    /// The writer is holding writes back until the flusher catches up.
    pub holding_back: bool,
    /// Operations submitted and not yet taken into a batch.
    pub queued: usize,
    /// Durable replies waiting for their flush and its confirmation by the host.
    pub waiting_durable: usize,
    pub flushed: u64,
    /// The ordinal of the flushed head the host last confirmed.
    pub confirmed: u64,
    pub materialized: u64,
    /// Non-fatal persistent engine upkeep warning (for example repeated SQLite WAL checkpoint
    /// failures), cleared once upkeep succeeds again.
    pub upkeep_warning: Option<String>,
}

pub struct Store<E: Engine> {
    core: Arc<Core<E>>,
    jobs: Mutex<Option<Sender<Job<E>>>>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

impl<E: Engine> Store<E> {
    /// Opens the office's store: the custody checkpoint and the journal (chain verified,
    /// torn tail truncated), the files of every retained entry, then the projection,
    /// replayed from its cursor or rebuilt when it is missing, ahead of the journal or
    /// not on its chain. The head is confirmed to the host before anything is accepted.
    pub fn open(parts: StoreParts<E>) -> Result<Store<E>, StoreError> {
        let (core, sequences) = open_store_core(parts)?;
        let (jobs, queue) = mpsc::channel();
        let writer = spawn_store_thread("store-writer", {
            let core = Arc::clone(&core);
            move || writer::run(core, queue, sequences)
        })?;
        let flusher = match spawn_store_thread("store-flusher", {
            let core = Arc::clone(&core);
            move || flusher::run(core)
        }) {
            Ok(flusher) => flusher,
            Err(error) => {
                core.die();
                drop(jobs);
                let _ = writer.join();
                return Err(error);
            }
        };
        Ok(Store {
            core,
            jobs: Mutex::new(Some(jobs)),
            threads: Mutex::new(vec![writer, flusher]),
        })
    }

    /// Runs `decide` on the single writer and answers in the class its change declares,
    /// or durable when `durable` is set. `FnMut`: a decision may run again until its
    /// entry is appended, never after.
    ///
    /// With an operation ID, a retry gets the original reply without deciding again, and
    /// the same ID with another actor, kind or fingerprint conflicts.
    pub fn submit<F>(
        &self,
        op: Option<Operation>,
        durable: bool,
        decide: F,
    ) -> Result<Reply, StoreError>
    where
        F: FnMut(&mut UnitOfWork<'_, E>) -> Result<Change, Refusal> + Send + 'static,
    {
        self.enqueue(op, durable, false, decide)
    }

    /// Runs `decide`, entry framing, projector application and engine validation on the
    /// single writer, then unconditionally undoes the transaction to its savepoint.
    /// Produces zero journal entries, zero files, zero receipts, and zero sequence or
    /// feed advancement.
    pub fn check<F>(&self, decide: F) -> Result<Reply, StoreError>
    where
        F: FnMut(&mut UnitOfWork<'_, E>) -> Result<Change, Refusal> + Send + 'static,
    {
        self.enqueue(None, false, true, decide)
    }

    fn enqueue<F>(
        &self,
        op: Option<Operation>,
        durable: bool,
        dry_run: bool,
        decide: F,
    ) -> Result<Reply, StoreError>
    where
        F: FnMut(&mut UnitOfWork<'_, E>) -> Result<Change, Refusal> + Send + 'static,
    {
        let (to, answer) = mpsc::sync_channel(1);
        let job = Job {
            op,
            durable,
            dry_run,
            decide: Box::new(decide),
            to,
        };
        {
            let jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
            let mut flow = self.core.flow();
            if let Some(fatal) = &flow.fatal {
                return Err(fatal.clone());
            }
            let sent = jobs.as_ref().is_some_and(|jobs| jobs.send(job).is_ok());
            if flow.dead || !sent {
                return Err(StoreError::stopped("the store is closed"));
            }
            flow.queued += 1;
            self.core.wake.notify_all();
        }
        let lost =
            || StoreError::stopped("the store stopped before answering; retry by operation ID");
        answer.recv().unwrap_or_else(|_| Err(lost()))
    }

    /// One consistent snapshot of the projection, with the feed position it stands at.
    /// Touches no file, no journal, no writer and no host.
    pub fn read<R>(
        &self,
        f: impl FnOnce(&E::Snapshot) -> Result<R, StoreError>,
    ) -> Result<(R, FeedPosition), StoreError> {
        let snapshot = self.core.engine.snapshot()?;
        let feed = snapshot.cursor()?.map(|c| c.feed).unwrap_or_default();
        Ok((f(&snapshot)?, feed))
    }

    /// Reads all retained journal entries strictly after `ordinal` (used by replication outbox
    /// consumers, `ARCH-03` / `TASK-52`).
    pub fn entries_after(&self, ordinal: u64) -> Vec<Arc<Entry>> {
        self.core.journal.after(ordinal)
    }

    /// Reads retained journal entries strictly after `ordinal`, verifying strict contiguity
    /// (`ordinal + 1` onward) so consumers reject gaps rather than skipping entries (`TASK-52` M-15).
    pub fn entries_after_contiguous(&self, ordinal: u64) -> Result<Vec<Arc<Entry>>, StoreError> {
        self.core
            .journal
            .after_contiguous(ordinal)
            .map_err(StoreError::Journal)
    }

    /// Syncs the store and runs `f(materialized_ordinal)` while holding `materialization_lock` so
    /// no files under `durable/state/` are mutated while capturing a consistent snapshot (`TASK-52` H-11).
    pub fn with_consistent_materialized<R>(
        &self,
        f: impl FnOnce(u64) -> Result<R, StoreError>,
    ) -> Result<R, StoreError> {
        self.sync()?;
        let _mat = self
            .core
            .materialization_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mat = self.core.flow().materialized;
        f(mat)
    }

    /// Follows the changes committed after `after`.
    pub fn subscribe(&self, after: FeedPosition) -> Result<ChangeStream, Resync> {
        self.core.changes.subscribe(after)
    }

    pub fn status(&self) -> Status {
        status(&self.core.flow(), self.core.engine.as_ref())
    }

    /// Waits until the store's status satisfies `ready`.
    pub fn wait_for(&self, ready: impl Fn(&Status) -> bool) -> Status {
        let mut flow = self.core.flow();
        loop {
            let now = status(&flow, self.core.engine.as_ref());
            if ready(&now) {
                return now;
            }
            flow = self.core.wake.wait(flow).unwrap_or_else(|e| e.into_inner());
        }
    }

    /// Flushes everything appended so far and writes its files, then returns.
    pub fn sync(&self) -> Result<(), StoreError> {
        let target = self.core.journal.head().ordinal;
        let mut flow = self.core.flow();
        flow.sync_wanted = true;
        self.core.wake.notify_all();
        loop {
            if let Some(fatal) = &flow.fatal {
                return Err(fatal.clone());
            }
            if flow.dead {
                return Err(StoreError::stopped("the store is closed"));
            }
            if flow.flushed >= target && flow.materialized >= target {
                return Ok(());
            }
            flow = self.core.wake.wait(flow).unwrap_or_else(|e| e.into_inner());
        }
    }

    /// Asks the flusher to flush every entry appended so far now instead of at the end of
    /// the flush interval, and returns at once: it waits for nothing (`TASK-140`). A
    /// buffered write another office is waiting on (a cross-office message, a relay) uses
    /// it so the entry is confirmed in milliseconds rather than after the group-flush
    /// interval. A no-op once the store has stopped, and when nothing is waiting to be
    /// flushed: the flusher is not woken for nothing.
    pub fn request_flush(&self) {
        let mut flow = self.core.flow();
        if flow.dead || flow.fatal.is_some() || flow.unflushed.is_empty() {
            return;
        }
        flow.flush_now = true;
        self.core.wake.notify_all();
    }

    /// An orderly stop: takes no more operations, finishes the queued ones, flushes,
    /// writes the files, and joins both threads.
    pub fn close(self) -> Result<(), StoreError> {
        drop(self.jobs.lock().unwrap_or_else(|e| e.into_inner()).take());
        {
            let mut flow = self.core.flow();
            flow.closing = true;
            self.core.wake.notify_all();
        }
        self.join();
        match self.core.flow().fatal.clone() {
            Some(fatal) => Err(fatal),
            None => Ok(()),
        }
    }

    fn join(&self) {
        let threads = std::mem::take(&mut *self.threads.lock().unwrap_or_else(|e| e.into_inner()));
        for thread in threads {
            let _ = thread.join();
        }
    }
}

fn status<E: Engine>(flow: &Flow, engine: &E) -> Status {
    Status {
        stopped: flow.fatal.clone(),
        holding_back: flow.holding_back,
        queued: flow.queued,
        waiting_durable: flow.waiters.len(),
        flushed: flow.flushed,
        confirmed: flow.confirmed.as_ref().map_or(0, |head| head.ordinal),
        materialized: flow.materialized,
        upkeep_warning: engine.upkeep_error().map(|error| error.to_string()),
    }
}

/// Dropping a store does nothing durable: no flush, no file, no marker. Both threads stop
/// at their next step, so a drop is what a process kill is, and the next opening recovers
/// from the journal alone.
impl<E: Engine> Drop for Store<E> {
    fn drop(&mut self) {
        if self
            .threads
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
        {
            return;
        }
        self.core.die();
        drop(self.jobs.lock().unwrap_or_else(|e| e.into_inner()).take());
        self.join();
    }
}

#[cfg(test)]
mod tests;
