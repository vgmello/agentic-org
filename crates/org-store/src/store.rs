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
    change::{Bytes, Change, Clock, DurablePath, Operation, Outcome, Refusal},
    changes::{ChangeStream, Changes, Resync},
    custody::{JournalHead, OfficeIdentity, WriterAuthority},
    disk::{Disk, RealDisk},
    engine::{CoreRead, Cursor, Engine, Projector},
    error::StoreError,
    files::Files,
    flusher,
    journal::{AnchorFacts, Entry, Journal, JournalError, JournalPoint, Recovered},
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
    pub(crate) dry_run: bool,
    pub(crate) decide: Decide<E>,
    pub(crate) to: SyncSender<Result<Reply, StoreError>>,
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

    /// Registers a durable journal consumer (such as multi-office replication, `ARCH-03` /
    /// `TASK-52`) so the flusher will never release journal segments past its watermark. Returns
    /// the consumer's current persisted watermark (reconstructing unreleased retained entries
    /// from disk segments when needed, or starting at the consistent materialized boundary so a
    /// consumer can never skip an evicted entry).
    pub fn register_consumer(&self, consumer_id: &str) -> Result<u64, StoreError> {
        validate_consumer_id(consumer_id)?;
        let _gate = self
            .core
            .release_gate
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        {
            let flow = self.core.flow();
            if let Some(&existing) = flow.consumer_watermarks.get(consumer_id) {
                return Ok(existing);
            }
        }
        self.core.journal.reconstruct_retained_from_segments()?;
        let mut flow = self.core.flow();
        if let Some(&existing) = flow.consumer_watermarks.get(consumer_id) {
            return Ok(existing);
        }
        let base = self.core.journal.base().ordinal;
        let initial = if self.core.journal.after_contiguous(base).is_ok() {
            base
        } else {
            flow.materialized.max(base)
        };
        persist_consumer_watermark(self.core.journal.dir(), consumer_id, initial)?;
        flow.consumer_watermarks
            .insert(consumer_id.to_owned(), initial);
        Ok(initial)
    }

    /// Advances a registered consumer's durable watermark monotonically and releases any
    /// retained segments now covered by all consumers (`ARCH-03`), serialized through `release_gate`.
    pub fn advance_consumer(&self, consumer_id: &str, ordinal: u64) -> Result<(), StoreError> {
        validate_consumer_id(consumer_id)?;
        let _gate = self
            .core
            .release_gate
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let release_target = {
            let mut flow = self.core.flow();
            let current = flow
                .consumer_watermarks
                .get(consumer_id)
                .copied()
                .ok_or_else(|| {
                    StoreError::Invalid(format!("unknown journal consumer `{consumer_id}`"))
                })?;
            let next = current.max(ordinal);
            if next > current {
                persist_consumer_watermark(self.core.journal.dir(), consumer_id, next)?;
                flow.consumer_watermarks
                    .insert(consumer_id.to_owned(), next);
            }
            flow.release_cap(flow.materialized)
        };
        if release_target > 0 {
            self.core.journal.evict_retained_through(release_target);
            self.core.journal.release_through(release_target)?;
        }
        Ok(())
    }

    /// Returns the persisted watermark for `consumer_id`, if registered.
    pub fn consumer_watermark(&self, consumer_id: &str) -> Option<u64> {
        self.core
            .flow()
            .consumer_watermarks
            .get(consumer_id)
            .copied()
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

fn spawn_store_thread(
    name: &str,
    body: impl FnOnce() + Send + 'static,
) -> Result<JoinHandle<()>, StoreError> {
    std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(body)
        .map_err(|e| StoreError::Internal(format!("cannot start the {name} thread: {e}")))
}

fn open_store_core<E: Engine>(
    parts: StoreParts<E>,
) -> Result<(Arc<Core<E>>, Sequences), StoreError> {
    let StoreParts {
        durable,
        identity,
        custody,
        engine,
        projectors,
        excluded_prefixes,
        clock,
        flush_interval,
        opening,
        limits,
        disk,
        hooks,
    } = parts;
    if limits.change_ring == 0 {
        return Err(StoreError::Invalid(
            "the change ring must keep at least one entry".into(),
        ));
    }
    let files = Files::new(&durable, Arc::clone(&disk))?;
    let consumer_watermarks = match &opening {
        Opening::Create(_) => std::collections::BTreeMap::new(),
        Opening::Existing => {
            load_consumer_watermarks(&crate::journal::journal_dir(&durable, &identity))?
        }
    };
    let min_consumer_wm = consumer_watermarks.values().copied().min();
    let (journal, recovered) = open_or_create_journal(
        opening,
        disk,
        &durable,
        &identity,
        &custody,
        min_consumer_wm,
    )?;
    journal.set_segment_bytes(limits.segment_bytes);
    let projectors =
        Projectors::new_with_exclusions(projectors, &identity.office_id, excluded_prefixes);
    materialize_with_consumer_cap(&files, &journal, &consumer_watermarks)?;

    let head = journal.head();
    let base = journal.base();
    let cursor = engine.snapshot()?.cursor()?;
    let on_chain = cursor_on_chain(cursor.as_ref(), &base, &head, &recovered);
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
        release_gate: Mutex::new(()),
        materialization_lock: Mutex::new(()),
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
            consumer_watermarks,
        }),
        wake: Condvar::new(),
    };
    replay_and_confirm(&core, cursor, on_chain, &head, &recovered)?;
    let sequences = Sequences {
        high_waters: head.high_waters,
        feed: head.feed,
    };
    Ok((Arc::new(core), sequences))
}

fn open_or_create_journal(
    opening: Opening,
    disk: Arc<dyn Disk>,
    durable: &std::path::Path,
    identity: &OfficeIdentity,
    custody: &Arc<dyn WriterAuthority>,
    min_consumer_watermark: Option<u64>,
) -> Result<(Journal, Recovered), StoreError> {
    match opening {
        Opening::Create(facts) => {
            let confirmed = custody
                .checkpoint(identity)
                .map_err(|e| StoreError::Custody(e.0))?;
            let journal = Journal::create_on(disk, durable, identity, &facts)?;
            let recovered = Recovered {
                confirmed,
                entries: Vec::new(),
                truncated: None,
            };
            Ok((journal, recovered))
        }
        Opening::Existing => Ok(Journal::open_on(
            disk,
            durable,
            identity,
            custody.as_ref(),
            min_consumer_watermark,
        )?),
    }
}

fn materialize_with_consumer_cap(
    files: &Files,
    journal: &Journal,
    consumer_watermarks: &std::collections::BTreeMap<String, u64>,
) -> Result<(), StoreError> {
    if let Some(through) = materialize(files, journal)? {
        let release_cap = consumer_watermarks
            .values()
            .copied()
            .min()
            .map_or(through, |min| through.min(min));
        journal.set_materialized_with_release_cap(through, release_cap)?;
    }
    Ok(())
}

fn cursor_on_chain(
    cursor: Option<&Cursor>,
    base: &JournalPoint,
    head: &JournalPoint,
    recovered: &Recovered,
) -> bool {
    cursor.is_some_and(|cursor| {
        let held = if cursor.ordinal == base.ordinal {
            Some(base.sha256)
        } else {
            recovered
                .entries
                .iter()
                .find(|e| e.ordinal() == cursor.ordinal)
                .map(|e| e.sha256)
        };
        cursor.ordinal <= head.ordinal && held == Some(cursor.sha256)
    })
}

fn replay_and_confirm<E: Engine>(
    core: &Core<E>,
    cursor: Option<Cursor>,
    on_chain: bool,
    head: &JournalPoint,
    recovered: &Recovered,
) -> Result<(), StoreError> {
    if let Some(cursor) = cursor.filter(|cursor| on_chain && cursor.ordinal < head.ordinal) {
        let after: Vec<_> = recovered
            .entries
            .iter()
            .filter(|e| e.ordinal() > cursor.ordinal)
            .cloned()
            .collect();
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
    Ok(())
}

fn validate_consumer_id(consumer_id: &str) -> Result<(), StoreError> {
    if consumer_id.is_empty()
        || consumer_id.len() > 64
        || !consumer_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(StoreError::Invalid(format!(
            "invalid journal consumer id `{consumer_id}`"
        )));
    }
    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ConsumerWatermarkFile {
    v: u32,
    consumer_id: String,
    ordinal: u64,
}

fn load_consumer_watermarks(
    journal_dir: &std::path::Path,
) -> Result<std::collections::BTreeMap<String, u64>, StoreError> {
    let dir = journal_dir.join("consumers");
    let mut out = std::collections::BTreeMap::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(&dir)
        .map_err(|e| StoreError::Internal(format!("read consumers dir: {e}")))?
    {
        let entry =
            entry.map_err(|e| StoreError::Internal(format!("read consumers entry: {e}")))?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let bytes = std::fs::read(&path)
            .map_err(|e| StoreError::Internal(format!("read consumer watermark: {e}")))?;
        let parsed: ConsumerWatermarkFile = serde_json::from_slice(&bytes)
            .map_err(|e| StoreError::Internal(format!("parse consumer watermark: {e}")))?;
        validate_consumer_id(&parsed.consumer_id)?;
        out.insert(parsed.consumer_id, parsed.ordinal);
    }
    Ok(out)
}

fn persist_consumer_watermark(
    journal_dir: &std::path::Path,
    consumer_id: &str,
    ordinal: u64,
) -> Result<(), StoreError> {
    let dir = journal_dir.join("consumers");
    match org_platform::fs::create_owner_only_dir(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => {
            return Err(StoreError::Internal(format!("create consumers dir: {e}")));
        }
    }
    // Always sync the parent journal directory before publishing a consumer watermark so a retry
    // after a crash or failed fsync following `mkdir` never skips parent durability (`TASK-52` #15 / P3-#14).
    std::fs::File::open(journal_dir)
        .and_then(|f| f.sync_all())
        .map_err(|e| StoreError::Internal(format!("sync journal dir: {e}")))?;
    let bytes = serde_json::to_vec_pretty(&ConsumerWatermarkFile {
        v: 1,
        consumer_id: consumer_id.to_owned(),
        ordinal,
    })
    .map_err(|e| StoreError::Internal(format!("encode consumer watermark: {e}")))?;
    crate::files::publish_one(&dir, &format!("{consumer_id}.json"), &bytes, false)
        .map_err(StoreError::Files)
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
