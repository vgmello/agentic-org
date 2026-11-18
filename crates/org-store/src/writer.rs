//! The single writer: it alone decides, applies to the projection and appends to the
//! journal. Appending a batch is the commit point; nothing decides again after it.

use std::{
    collections::BTreeMap,
    ops::ControlFlow,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{mpsc::Receiver, Arc},
};

use crate::{
    change::{
        Ack, Bytes, Change, DurablePath, FileChange, Outcome, Receipt, Refusal, Sha256, Timestamp,
    },
    engine::{CorePort, CoreRead, Engine, Mark},
    error::StoreError,
    files::{FileOp, Files},
    journal::{
        positions_between, Entry, EntryDraft, FileDraft, JournalError, JournalPoint,
        OperationRecord,
    },
    receipts::{encode_receipt, receipt_path, ReceiptProjector},
    sequences::{successor, Sequences, Stream},
    store::{Core, Job, Reply, Site},
};

/// What a decision may see and take. Everything else is out of its reach.
pub struct UnitOfWork<'a, E: Engine> {
    tx: &'a E::Tx,
    sequences: &'a mut Sequences,
    batch: &'a BTreeMap<DurablePath, Option<Bytes>>,
    overlay: &'a Overlay,
    files: &'a Files,
    now: Timestamp,
}

/// Path to the latest bytes in an entry whose files are not yet written; `None` is a
/// removal. Each value carries its entry's ordinal.
type Overlay = BTreeMap<DurablePath, (u64, Option<Bytes>)>;

impl<E: Engine> UnitOfWork<'_, E> {
    /// The projection at head, earlier operations of this batch included.
    pub fn view(&self) -> &E::Tx {
        self.tx
    }

    /// The current bytes of a source file, accepted changes not yet written to the file
    /// included. For source-preserving edits.
    pub fn source(&self, path: &DurablePath) -> Result<Option<Bytes>, StoreError> {
        if let Some(bytes) = self.batch.get(path) {
            return Ok(bytes.clone());
        }
        if let Some((_, bytes)) = self.overlay.get(path) {
            return Ok(bytes.clone());
        }
        Ok(self.files.read(path)?)
    }

    /// The next position of one of this office's origin streams; recorded in the entry.
    /// A refusal gives it back.
    pub fn next(&mut self, stream: Stream) -> Result<u64, StoreError> {
        self.sequences.next(stream)
    }

    /// The writer's clock, recorded in the entry. A decision reads no other clock.
    pub fn now(&self) -> Timestamp {
        self.now
    }
}

enum Slot {
    /// Answered without an entry of its own to wait for.
    Done(Result<Reply, StoreError>),
    /// Answered from the entry at `ordinal`: its own, or the one its receipt names.
    Appended {
        ordinal: u64,
        reply: Reply,
        durable: bool,
    },
}

enum Abort {
    /// The engine ended the transaction: this operation fails, the others run again.
    Ended(StoreError),
    /// A crash point: the process is dead.
    Crash,
}

/// One run of the batch in one transaction.
struct Attempt {
    sequences: Sequences,
    point: JournalPoint,
    entries: Vec<Arc<Entry>>,
    files: BTreeMap<DurablePath, Option<Bytes>>,
}

struct Writer<E: Engine> {
    core: Arc<Core<E>>,
    jobs: Receiver<Job<E>>,
    sequences: Sequences,
    overlay: Overlay,
}

pub(crate) fn run<E: Engine>(core: Arc<Core<E>>, jobs: Receiver<Job<E>>, sequences: Sequences) {
    let mut writer = Writer {
        core,
        jobs,
        sequences,
        overlay: Overlay::new(),
    };
    // Set on every exit, a panic outside a decision included, so closing never waits for
    // a writer that is gone.
    struct Done<E: Engine>(Arc<Core<E>>);
    impl<E: Engine> Drop for Done<E> {
        fn drop(&mut self) {
            self.0.flow().writer_done = true;
            self.0.wake.notify_all();
        }
    }
    let _done = Done(Arc::clone(&writer.core));
    writer.serve();
}

impl<E: Engine> Writer<E> {
    fn serve(&mut self) {
        loop {
            if !self.wait_for_room() {
                return;
            }
            let Ok(first) = self.jobs.recv() else {
                return;
            };
            let mut jobs = vec![first];
            while jobs.len() < self.core.limits.max_batch {
                match self.jobs.try_recv() {
                    Ok(job) => jobs.push(job),
                    Err(_) => break,
                }
            }
            let stopped = {
                let mut flow = self.core.flow();
                flow.queued -= jobs.len();
                self.core.wake.notify_all();
                if flow.dead {
                    return;
                }
                flow.fatal.clone()
            };
            if let Some(fatal) = stopped {
                for job in jobs {
                    let _ = job.to.send(Err(fatal.clone()));
                }
                continue;
            }
            if self.batch(jobs).is_break() {
                self.core.die();
                return;
            }
        }
    }

    /// Back-pressure: no new batch while either tail is at its bound. `false` when dead.
    fn wait_for_room(&self) -> bool {
        let limits = self.core.limits;
        let mut flow = self.core.flow();
        loop {
            if flow.dead {
                return false;
            }
            let full = flow.unflushed_bytes >= limits.unflushed_bytes
                || flow.unmaterialized_bytes >= limits.unmaterialized_bytes;
            if flow.holding_back != (full && flow.fatal.is_none()) {
                flow.holding_back = !flow.holding_back;
                self.core.wake.notify_all();
            }
            if !flow.holding_back {
                return true;
            }
            flow = self.core.wake.wait(flow).unwrap_or_else(|e| e.into_inner());
        }
    }

    fn batch(&mut self, mut jobs: Vec<Job<E>>) -> ControlFlow<()> {
        let core = Arc::clone(&self.core);
        let start = core.journal.head();
        let materialized = core.journal.materialized();
        self.overlay
            .retain(|_, (ordinal, _)| *ordinal > materialized);

        let mut slots: Vec<Option<Slot>> = jobs.iter().map(|_| None).collect();
        // Operations that ended a transaction have failed for good; the rest run again.
        let mut settled = vec![false; jobs.len()];
        let (tx, attempt) = loop {
            let mut tx = match core.engine.begin() {
                Ok(tx) => tx,
                Err(e) => {
                    for (slot, settled) in slots.iter_mut().zip(&settled) {
                        if !settled {
                            *slot = Some(Slot::Done(Err(e.clone().into())));
                        }
                    }
                    self.answer(jobs, slots);
                    return ControlFlow::Continue(());
                }
            };
            let mut attempt = Attempt {
                sequences: self.sequences,
                point: start.clone(),
                entries: Vec::new(),
                files: BTreeMap::new(),
            };
            let mut ended = false;
            for (index, job) in jobs.iter_mut().enumerate() {
                if settled[index] {
                    continue;
                }
                match self.one(&mut tx, job, &mut attempt) {
                    Ok(slot) => slots[index] = Some(slot),
                    Err(Abort::Ended(error)) => {
                        slots[index] = Some(Slot::Done(Err(error)));
                        settled[index] = true;
                        ended = true;
                        break;
                    }
                    Err(Abort::Crash) => return ControlFlow::Break(()),
                }
            }
            if !ended {
                break (tx, attempt);
            }
        };

        if attempt.entries.is_empty() {
            drop(tx);
            self.answer(jobs, slots);
            return ControlFlow::Continue(());
        }
        // The commit point. One write for the whole batch.
        if let Err(error) = core.append(&attempt.entries) {
            drop(tx);
            let failure = StoreError::Append(error.to_string());
            if matches!(error, JournalError::Broken { .. }) {
                core.stop(StoreError::Journal(error));
            }
            fail_appended(&mut slots, start.ordinal, &failure);
            self.answer(jobs, slots);
            return ControlFlow::Continue(());
        }
        self.sequences = attempt.sequences;
        if core.hooks.crash_at(Site::AfterAppend) {
            return ControlFlow::Break(());
        }
        // From here nothing decides again and no reply reports failure.
        let cursor = attempt.point.cursor();
        if let Err(first) = core.engine.commit(tx, &cursor) {
            if let Err(second) = core.replay(&attempt.entries, &cursor) {
                let fatal = StoreError::stopped(format!(
                    "recovery required: the projection did not commit ({first}) and the replay \
                     from the journal failed ({second})"
                ));
                core.stop(fatal.clone());
                fail_appended(&mut slots, start.ordinal, &fatal);
                self.answer(jobs, slots);
                return ControlFlow::Continue(());
            }
        }
        for entry in &attempt.entries {
            for (index, file) in entry.header.files.iter().enumerate() {
                let bytes = (file.op != FileOp::Remove).then(|| Bytes::from(entry.payload(index)));
                self.overlay
                    .insert(file.path.clone(), (entry.ordinal(), bytes));
            }
            core.changes.publish(entry);
        }
        if core.hooks.crash_at(Site::AfterCommit) {
            return ControlFlow::Break(());
        }
        self.answer(jobs, slots);
        if core.hooks.crash_at(Site::AfterCommittedReply) {
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    }

    /// The four steps of one operation: receipt, decide, frame, apply.
    fn one(&self, tx: &mut E::Tx, job: &mut Job<E>, attempt: &mut Attempt) -> Result<Slot, Abort> {
        let core = &self.core;
        let engine_failed = |error: crate::error::EngineError| {
            if error.transaction_ended {
                Err(Abort::Ended(error.into()))
            } else {
                Ok(Slot::Done(Err(error.into())))
            }
        };
        // 1. A retry is matched before any validation of current state.
        if let Some(op) = &job.op {
            match tx.receipt(&op.id) {
                Err(error) => return engine_failed(error),
                Ok(Some(receipt)) => {
                    if receipt.actor != op.actor
                        || receipt.fingerprint != op.fingerprint
                        || receipt.kind != op.kind
                    {
                        return Ok(Slot::Done(Err(StoreError::Conflict)));
                    }
                    return Ok(Slot::Appended {
                        ordinal: receipt.ordinal,
                        durable: job.durable || receipt.ack == Ack::Durable,
                        reply: Reply {
                            outcome: receipt.outcome,
                            bytes: receipt.reply,
                        },
                    });
                }
                Ok(None) => {}
            }
        }
        // 2. Decide, on the projection at head.
        let mark = match tx.mark() {
            Ok(mark) => mark,
            Err(error) => return engine_failed(error),
        };
        let before = attempt.sequences;
        let now = core.clock.now();
        let decided = {
            let mut uow = UnitOfWork {
                tx: &*tx,
                sequences: &mut attempt.sequences,
                batch: &attempt.files,
                overlay: &self.overlay,
                files: &core.files,
                now,
            };
            catch_unwind(AssertUnwindSafe(|| (job.decide)(&mut uow)))
        };
        if core.hooks.crash_at(Site::DuringDecide) {
            return Err(Abort::Crash);
        }
        let (change, outcome) = match decided {
            Err(_panic) => return fail(tx, mark, attempt, before, StoreError::Panicked),
            Ok(Err(Refusal::Invalid(error))) => return fail(tx, mark, attempt, before, error),
            Ok(Err(Refusal::Rejected(reply))) => {
                attempt.sequences = before;
                if job.op.is_none() {
                    let outcome = Outcome::Rejected;
                    return Ok(Slot::Done(Ok(Reply {
                        outcome,
                        bytes: reply,
                    })));
                }
                (Change::new(Ack::Committed).reply(reply), Outcome::Rejected)
            }
            Ok(Ok(change)) => (change, Outcome::Completed),
        };
        // 3. Frame the entry.
        let framed = self.frame(tx, job, attempt, change, outcome, now);
        let (entry, reply, durable) = match framed {
            Ok(framed) => framed,
            Err(error) => return fail(tx, mark, attempt, before, error),
        };
        // 4. Apply it to the projection, and make sure the transaction could still commit.
        let applied = core.projectors.apply_entry(tx, &entry).and_then(|()| {
            tx.check().map_err(|e| match e.transaction_ended {
                true => StoreError::Engine(e),
                false => StoreError::Internal(format!(
                    "the projection would not commit after this operation: {}",
                    e.message
                )),
            })
        });
        if let Err(error) = applied {
            return fail(tx, mark, attempt, before, error);
        }
        if core.hooks.crash_at(Site::DuringApply) {
            return Err(Abort::Crash);
        }
        for (index, file) in entry.header.files.iter().enumerate() {
            let bytes = (file.op != FileOp::Remove).then(|| Bytes::from(entry.payload(index)));
            attempt.files.insert(file.path.clone(), bytes);
        }
        attempt.point = JournalPoint {
            ordinal: entry.ordinal(),
            sha256: entry.sha256,
            high_waters: attempt.sequences.high_waters,
            feed: entry.header.feed,
        };
        let ordinal = entry.ordinal();
        attempt.entries.push(Arc::new(entry));
        Ok(Slot::Appended {
            ordinal,
            reply,
            durable,
        })
    }

    fn frame(
        &self,
        tx: &E::Tx,
        job: &Job<E>,
        attempt: &mut Attempt,
        change: Change,
        outcome: Outcome,
        now: Timestamp,
    ) -> Result<(Entry, Reply, bool), StoreError> {
        let identity = self.core.journal.identity();
        let mut files = Vec::with_capacity(change.files.len() + 1);
        for file in &change.files {
            let path = file.path();
            if !path.names_a_file() {
                return Err(StoreError::invalid(format!("{path} is a tree, not a file")));
            }
            if ReceiptProjector::owns_path(path) {
                return Err(StoreError::invalid("only the store writes receipts"));
            }
            if files
                .iter()
                .any(|earlier: &FileDraft| earlier.path == *path)
            {
                return Err(StoreError::invalid(format!(
                    "{path} is changed twice in one operation"
                )));
            }
            self.core.projectors.owned(path)?;
            self.placeable(tx, attempt, &files, path)?;
            let previous_sha256 = tx.source_hash(path)?;
            let (op, bytes) = match file {
                FileChange::Create(_, bytes) => {
                    if previous_sha256.is_some_and(|held| held != Sha256::of(bytes)) {
                        return Err(StoreError::invalid(format!(
                            "{path} exists with other bytes and is immutable"
                        )));
                    }
                    (FileOp::Create, Some(bytes.clone()))
                }
                FileChange::Put(_, bytes) => (FileOp::Put, Some(bytes.clone())),
                FileChange::Remove(_) => (FileOp::Remove, None),
            };
            files.push(FileDraft {
                path: path.clone(),
                op,
                bytes,
                previous_sha256,
            });
        }
        let ordinal = successor(attempt.point.ordinal, "journal ordinal")?;
        let durable = job.durable || change.ack == Ack::Durable;
        let ack = if durable {
            Ack::Durable
        } else {
            Ack::Committed
        };
        let operation = job.op.as_ref().map(|op| OperationRecord {
            id: op.id.clone(),
            kind: op.kind.to_owned(),
            actor: op.actor.clone(),
            fingerprint: op.fingerprint,
            outcome,
        });
        if let Some(op) = &operation {
            let receipt = Receipt {
                id: op.id.clone(),
                kind: op.kind.clone(),
                actor: op.actor.clone(),
                fingerprint: op.fingerprint,
                outcome,
                ack,
                ordinal,
                at: now,
                reply: change.reply.clone(),
            };
            files.push(FileDraft {
                path: receipt_path(&op.id),
                op: FileOp::Create,
                bytes: Some(encode_receipt(&receipt)),
                previous_sha256: None,
            });
        }
        let positions =
            positions_between(&attempt.point.high_waters, &attempt.sequences.high_waters);
        let feed = attempt.sequences.next_feed()?;
        let entry = Entry::frame(EntryDraft {
            ordinal,
            previous_sha256: attempt.point.sha256,
            office_id: identity.office_id.clone(),
            journal_incarnation: identity.journal_incarnation.clone(),
            at: now,
            ack,
            feed,
            operation,
            positions,
            files,
        })
        .map_err(|e| match e {
            JournalError::TooLarge { bytes } => {
                StoreError::invalid(format!("the change is {bytes} bytes, over the entry limit"))
            }
            other => StoreError::Journal(other),
        })?;
        let reply = Reply {
            outcome,
            bytes: change.reply,
        };
        Ok((entry, reply, durable))
    }

    /// An accepted entry must always be materializable, so a path is refused when it
    /// could not stand as a file beside the others: a file is not a directory.
    ///
    /// - No ancestor of it is a source (in the projection at head, which holds the
    ///   earlier operations of this batch, or in this operation).
    /// - It is not an ancestor of a source, in the same places.
    /// - It is not a directory already: publication creates the directories above every
    ///   path it writes or removes and never removes one, so a path that ever had a file
    ///   below it stays a directory. Those are the paths below it in the entries not yet
    ///   written (this batch and the overlay) and what stands on the disk.
    fn placeable(
        &self,
        tx: &E::Tx,
        attempt: &Attempt,
        drafts: &[FileDraft],
        path: &DurablePath,
    ) -> Result<(), StoreError> {
        let below = |other: &DurablePath| other != path && other.is_under(path);
        let refuse_above = |file: &DurablePath| {
            StoreError::invalid(format!("{path} lies below {file}, which is a file"))
        };
        let refuse_below =
            || StoreError::invalid(format!("{path} is a directory: files lie below it"));
        for draft in drafts {
            if path.is_under(&draft.path) {
                return Err(refuse_above(&draft.path));
            }
            if below(&draft.path) {
                return Err(refuse_below());
            }
        }
        let mut ancestor = path.parent();
        while let Some(above) = ancestor.filter(|above| above.names_a_file()) {
            if tx.source_hash(&above)?.is_some() {
                return Err(refuse_above(&above));
            }
            ancestor = above.parent();
        }
        // Keys below `path` follow it in byte order, after only the keys that extend its
        // last component (`a/b-c` sorts between `a/b` and `a/b/c`).
        fn any_below<V>(map: &BTreeMap<DurablePath, V>, path: &DurablePath) -> bool {
            let after = (std::ops::Bound::Excluded(path), std::ops::Bound::Unbounded);
            map.range::<DurablePath, _>(after)
                .map(|(other, _)| other)
                .take_while(|other| other.as_str().starts_with(path.as_str()))
                .any(|other| other.is_under(path))
        }
        if tx.sources_under(path)?.iter().any(below)
            || any_below(&attempt.files, path)
            || any_below(&self.overlay, path)
            || self.core.files.is_directory(path)?
        {
            return Err(refuse_below());
        }
        Ok(())
    }

    /// Sends every answer the batch has. A durable one whose entry is not yet flushed and
    /// confirmed by the host goes to the flusher instead.
    fn answer(&self, jobs: Vec<Job<E>>, slots: Vec<Option<Slot>>) {
        let mut flow = self.core.flow();
        for (job, slot) in jobs.into_iter().zip(slots) {
            match slot {
                None => {
                    let lost = StoreError::Internal("an operation was left undecided".into());
                    let _ = job.to.send(Err(lost));
                }
                Some(Slot::Done(result)) => {
                    let _ = job.to.send(result);
                }
                Some(Slot::Appended {
                    ordinal,
                    reply,
                    durable,
                }) => {
                    if !durable || flow.durable_at(ordinal) {
                        let _ = job.to.send(Ok(reply));
                    } else if let Some(fatal) = &flow.fatal {
                        let _ = job.to.send(Err(fatal.clone()));
                    } else {
                        flow.waiters.push((ordinal, job.to, reply));
                    }
                }
            }
        }
        self.core.wake.notify_all();
    }
}

/// Fails one operation alone: gives back its positions and undoes to its mark.
fn fail<T: CorePort>(
    tx: &mut T,
    mark: Mark,
    attempt: &mut Attempt,
    before: Sequences,
    error: StoreError,
) -> Result<Slot, Abort> {
    if error.transaction_ended() {
        return Err(Abort::Ended(error));
    }
    attempt.sequences = before;
    match tx.undo_to(mark) {
        Ok(()) => Ok(Slot::Done(Err(error))),
        // A transaction that cannot be undone cannot be trusted: start another.
        Err(undo) => Err(Abort::Ended(StoreError::Engine(undo))),
    }
}

/// Every operation answered from an entry of this batch gets `error` instead.
fn fail_appended(slots: &mut [Option<Slot>], start: u64, error: &StoreError) {
    for slot in slots {
        if matches!(slot, Some(Slot::Appended { ordinal, .. }) if *ordinal > start) {
            *slot = Some(Slot::Done(Err(error.clone())));
        }
    }
}
