//! The store: one writer thread, one flusher thread, and reads that touch only the
//! projection.

use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        mpsc::{self, Sender, SyncSender},
        Arc, Condvar, Mutex, MutexGuard,
    },
    thread::JoinHandle,
    time::Duration,
};

use crate::{
    change::{Bytes, Change, Clock, Operation, Outcome, Refusal},
    changes::{ChangeStream, Changes, Resync},
    custody::{JournalHead, OfficeIdentity, WriterAuthority},
    disk::{Disk, RealDisk},
    engine::{CoreRead, Cursor, Engine, Projector},
    error::StoreError,
    files::Files,
    flusher,
    journal::{AnchorFacts, Entry, Journal, Recovered},
    rebuild::{materialize, rebuild_with, Projectors},
    sequences::{FeedPosition, Sequences},
    writer::{self, UnitOfWork},
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
    /// Entries the change stream keeps for late subscribers.
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
            clock,
            flush_interval: Duration::from_millis(50),
            opening: Opening::Existing,
            limits: Limits::default(),
            disk: Arc::new(RealDisk),
            hooks: Arc::new(NoHooks),
        }
    }
}

/// Where the writer and the flusher can be made to die, for crash tests. Production
/// composes [`NoHooks`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Site {
    DuringDecide,
    DuringApply,
    AfterAppend,
    AfterCommit,
    AfterCommittedReply,
    BeforeMark,
    DuringRebuild,
}

pub(crate) trait Hooks: Send + Sync {
    /// `true`: the process dies here. The caller does nothing more.
    fn crash_at(&self, site: Site) -> bool;
}

pub(crate) struct NoHooks;

impl Hooks for NoHooks {
    fn crash_at(&self, _site: Site) -> bool {
        false
    }
}

/// The answer to an admitted operation.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reply {
    pub outcome: Outcome,
    pub bytes: Bytes,
}

pub(crate) type Decide<E> =
    Box<dyn FnMut(&mut UnitOfWork<'_, E>) -> Result<Change, Refusal> + Send + 'static>;

pub(crate) struct Job<E: Engine> {
    pub(crate) op: Option<Operation>,
    pub(crate) durable: bool,
    pub(crate) decide: Decide<E>,
    pub(crate) to: SyncSender<Result<Reply, StoreError>>,
}

/// A durable reply waiting for the flush that covers its ordinal.
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
    pub(crate) confirmed: Option<JournalHead>,
    /// Durable replies waiting for the flush that covers their ordinal.
    pub(crate) waiters: Vec<Waiter>,
    /// Ordinal and size of each appended entry not yet flushed.
    pub(crate) unflushed: VecDeque<(u64, u64)>,
    pub(crate) unflushed_bytes: u64,
    /// The same for entries flushed and not yet materialized.
    pub(crate) unmaterialized: VecDeque<(u64, u64)>,
    pub(crate) unmaterialized_bytes: u64,
}

impl Flow {
    /// The journal is flushed through `ordinal`: answer every durable waiter it covers.
    pub(crate) fn flushed_through(&mut self, ordinal: u64) {
        self.flushed = self.flushed.max(ordinal);
        while self.unflushed.front().is_some_and(|(o, _)| *o <= ordinal) {
            let entry = self.unflushed.pop_front().expect("checked");
            self.unflushed_bytes -= entry.1;
            self.unmaterialized_bytes += entry.1;
            self.unmaterialized.push_back(entry);
        }
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
    flow: Mutex<Flow>,
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

    /// Accounts for a batch that is now in the journal.
    pub(crate) fn appended(&self, entries: &[Arc<Entry>]) {
        let mut flow = self.flow();
        for entry in entries {
            let bytes = entry.stored().len() as u64;
            flow.unflushed.push_back((entry.ordinal(), bytes));
            flow.unflushed_bytes += bytes;
        }
        self.wake.notify_all();
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

/// What the store reports about itself.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Status {
    /// Why the store stopped accepting, if it has.
    pub stopped: Option<StoreError>,
    /// The writer is holding writes back until the flusher catches up.
    pub holding_back: bool,
    /// Operations submitted and not yet taken into a batch.
    pub queued: usize,
    /// Durable replies waiting for their flush.
    pub waiting_durable: usize,
    pub flushed: u64,
    pub materialized: u64,
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
        let StoreParts {
            durable,
            identity,
            custody,
            engine,
            projectors,
            clock,
            flush_interval,
            opening,
            limits,
            disk,
            hooks,
        } = parts;
        let files = Files::new(&durable, Arc::clone(&disk))?;
        let (journal, recovered) = match opening {
            Opening::Create(facts) => {
                let confirmed = custody
                    .checkpoint(&identity)
                    .map_err(|e| StoreError::Custody(e.0))?;
                let journal = Journal::create_on(disk, &durable, &identity, &facts)?;
                let recovered = Recovered {
                    confirmed,
                    entries: Vec::new(),
                    truncated: None,
                };
                (journal, recovered)
            }
            Opening::Existing => Journal::open_on(disk, &durable, &identity, custody.as_ref())?,
        };
        journal.set_segment_bytes(limits.segment_bytes);
        let projectors = Projectors::new(projectors);
        if let Some(through) = materialize(&files, &journal)? {
            journal.set_materialized(through)?;
        }

        let head = journal.head();
        let base = journal.base();
        let cursor = engine.snapshot()?.cursor()?;
        let on_chain = cursor.as_ref().is_some_and(|cursor| {
            let held = if cursor.ordinal == base.ordinal {
                Some(base.sha256)
            } else {
                let entry = recovered
                    .entries
                    .iter()
                    .find(|e| e.ordinal() == cursor.ordinal);
                entry.map(|e| e.sha256)
            };
            cursor.ordinal <= head.ordinal && held == Some(cursor.sha256)
        });
        if !on_chain {
            rebuild_with(
                &files,
                &journal,
                engine.as_ref(),
                &projectors,
                hooks.as_ref(),
            )?;
        }
        let core = Core {
            engine,
            projectors,
            files,
            journal,
            custody,
            clock,
            hooks,
            limits,
            flush_interval,
            changes: Changes::new(head.feed, limits.change_ring),
            flow: Mutex::new(Flow {
                fatal: None,
                dead: false,
                closing: false,
                writer_done: false,
                sync_wanted: false,
                holding_back: false,
                queued: 0,
                flushed: head.ordinal,
                materialized: head.ordinal,
                confirmed: recovered.confirmed.clone(),
                waiters: Vec::new(),
                unflushed: VecDeque::new(),
                unflushed_bytes: 0,
                unmaterialized: VecDeque::new(),
                unmaterialized_bytes: 0,
            }),
            wake: Condvar::new(),
        };
        if let Some(cursor) = cursor.filter(|cursor| on_chain && cursor.ordinal < head.ordinal) {
            let after = recovered
                .entries
                .iter()
                .filter(|e| e.ordinal() > cursor.ordinal);
            let after: Vec<_> = after.cloned().collect();
            core.replay(&after, &head.cursor())?;
        }
        let flushed = head.head();
        if recovered.confirmed.as_ref() != Some(&flushed) {
            core.custody
                .confirm(
                    core.journal.identity(),
                    recovered.confirmed.as_ref(),
                    &flushed,
                )
                .map_err(|e| StoreError::Custody(e.0))?;
            core.flow().confirmed = Some(flushed);
        }

        let core = Arc::new(core);
        let (jobs, queue) = mpsc::channel();
        let sequences = Sequences {
            high_waters: head.high_waters,
            feed: head.feed,
        };
        let spawn = |name: &str, body: Box<dyn FnOnce() + Send>| {
            let thread = std::thread::Builder::new().name(name.to_owned());
            thread
                .spawn(body)
                .map_err(|e| StoreError::Internal(format!("cannot start the {name} thread: {e}")))
        };
        let for_writer = Arc::clone(&core);
        let writer = spawn(
            "store-writer",
            Box::new(move || writer::run(for_writer, queue, sequences)),
        )?;
        let for_flusher = Arc::clone(&core);
        let flusher = match spawn("store-flusher", Box::new(move || flusher::run(for_flusher))) {
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
        let (to, answer) = mpsc::sync_channel(1);
        let job = Job {
            op,
            durable,
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

    /// Follows the changes committed after `after`.
    pub fn subscribe(&self, after: FeedPosition) -> Result<ChangeStream, Resync> {
        self.core.changes.subscribe(after)
    }

    pub fn status(&self) -> Status {
        status(&self.core.flow())
    }

    /// Waits until the store's status satisfies `ready`.
    pub fn wait_for(&self, ready: impl Fn(&Status) -> bool) -> Status {
        let mut flow = self.core.flow();
        loop {
            let now = status(&flow);
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

fn status(flow: &Flow) -> Status {
    Status {
        stopped: flow.fatal.clone(),
        holding_back: flow.holding_back,
        queued: flow.queued,
        waiting_durable: flow.waiters.len(),
        flushed: flow.flushed,
        materialized: flow.materialized,
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
