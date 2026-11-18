use std::{
    fs,
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
};

use super::*;
use crate::{
    change::{Ack, DurablePath, Sha256, Timestamp},
    engine::Cursor,
    journal::JournalError,
    receipts::receipt_path,
    sequences::Stream,
    testkit::{
        id, identity, with_faults, CrashPoint, DiskOp, Dump, FakeCustody, Faults, FixedClock,
        FlushSite, MemoryEngine, Probe,
    },
};

type TestStore = Store<MemoryEngine>;
type Answer = Result<Reply, StoreError>;

struct Office {
    _dir: tempfile::TempDir,
    root: PathBuf,
    faults: Arc<Faults>,
    custody: Arc<FakeCustody>,
    engine: Arc<MemoryEngine>,
    probe: Probe,
    limits: Limits,
}

impl Office {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap().join("durable");
        Self {
            _dir: dir,
            root,
            faults: Faults::new(),
            custody: Arc::new(FakeCustody::new()),
            engine: Arc::new(MemoryEngine::new()),
            probe: Probe::new(&identity().office_id),
            limits: Limits::default(),
        }
    }

    /// Flushes happen only when a durable request, a sync or back-pressure asks: the
    /// interval never fires, so every test is deterministic.
    fn parts(&self, opening: Opening) -> StoreParts<MemoryEngine> {
        let mut parts = StoreParts::new(
            self.root.clone(),
            identity(),
            Arc::clone(&self.custody) as Arc<dyn WriterAuthority>,
            Arc::clone(&self.engine),
            vec![self.probe.projector()],
            Arc::new(FixedClock(Timestamp(1_700_000_000_000))),
        );
        parts.opening = opening;
        parts.flush_interval = Duration::from_secs(3_600);
        parts.limits = self.limits;
        with_faults(parts, &self.faults)
    }

    fn create(&self) -> Arc<TestStore> {
        Arc::new(Store::open(self.parts(Opening::Create(AnchorFacts::default()))).unwrap())
    }

    /// A new process on the same disk and projection.
    fn reopen(&self) -> Arc<TestStore> {
        self.faults.revive();
        Arc::new(Store::open(self.parts(Opening::Existing)).unwrap())
    }

    fn hash(&self, store: &TestStore, name: &str) -> Option<Sha256> {
        let path = self.probe.path(name);
        store.read(|s| Ok(s.source_hash(&path)?)).unwrap().0
    }

    fn file(&self, name: &str) -> Option<Vec<u8>> {
        fs::read(self.root.join(self.probe.path(name).as_str())).ok()
    }

    fn put(&self, store: &TestStore, name: &str, ack: Ack) -> Answer {
        store.submit(None, false, self.probe.put(name, name.as_bytes(), ack))
    }

    /// A put with operation ID `n`.
    fn put_as(&self, store: &TestStore, n: u64, name: &str, ack: Ack) -> Answer {
        let op = self
            .probe
            .operation(id(n), "probe.put", name, name.as_bytes());
        store.submit(Some(op), false, self.probe.put(name, name.as_bytes(), ack))
    }

    /// Submits on another thread, returning once the request is queued behind `ahead`
    /// others.
    fn queue(
        &self,
        store: &Arc<TestStore>,
        ahead: usize,
        submit: impl FnOnce(&TestStore) -> Answer + Send + 'static,
    ) -> JoinHandle<Answer> {
        let worker = Arc::clone(store);
        let handle = thread::spawn(move || submit(&worker));
        store.wait_for(|s| s.queued > ahead);
        handle
    }
}

fn cursor(store: &TestStore) -> Cursor {
    store.read(|s| Ok(s.cursor()?)).unwrap().0.unwrap()
}

fn segment_calls(faults: &Faults, op: DiskOp) -> usize {
    let paths = faults.paths(op);
    paths
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "log"))
        .count()
}

/// Blocks the writer inside a decision until the returned sender is used or dropped.
fn gate(office: &Office, store: &Arc<TestStore>) -> (JoinHandle<Answer>, Sender<()>) {
    let (entered, has_entered) = mpsc::channel();
    let (release, released): (Sender<()>, Receiver<()>) = mpsc::channel();
    let path = office.probe.path("gate");
    let worker = Arc::clone(store);
    let handle = thread::spawn(move || {
        worker.submit(None, false, move |uow| {
            let _ = entered.send(());
            let _ = released.recv();
            uow.next(Stream::Work)?;
            Ok(Change::new(Ack::Committed).put(path.clone(), b"gate".to_vec()))
        })
    });
    has_entered.recv().unwrap();
    (handle, release)
}

#[test]
fn one_failing_request_in_a_batch_fails_alone_and_the_batch_is_one_write_and_one_flush() {
    let office = Office::new();
    let store = office.create();
    let (gate, release) = gate(&office, &store);
    let probe = office.probe.clone();
    let a = office.queue(&store, 0, {
        let probe = probe.clone();
        move |s| s.submit(None, false, probe.put("a", b"a", Ack::Durable))
    });
    let b = office.queue(&store, 1, {
        let probe = probe.clone();
        move |s| {
            s.submit(
                None,
                false,
                probe.put("b", crate::testkit::probe::REFUSED, Ack::Durable),
            )
        }
    });
    let c = office.queue(&store, 2, move |s| {
        s.submit(None, false, probe.put("c", b"c", Ack::Durable))
    });
    office.faults.clear_log();
    release.send(()).unwrap();
    gate.join().unwrap().unwrap();

    assert!(a.join().unwrap().is_ok());
    assert!(matches!(b.join().unwrap(), Err(StoreError::Projection(_))));
    assert!(c.join().unwrap().is_ok());
    assert!(office.hash(&store, "a").is_some() && office.hash(&store, "c").is_some());
    assert_eq!(office.hash(&store, "b"), None);
    // The gate's batch and the batch of three: one write each. Both durable replies of
    // the second batch came from one flush.
    assert_eq!(segment_calls(&office.faults, DiskOp::Write), 2);
    assert_eq!(segment_calls(&office.faults, DiskOp::Flush), 1);
    // The failed operation took no ordinal and gave its position back.
    let cursor = cursor(&store);
    assert_eq!((cursor.ordinal, cursor.high_waters.work), (3, 3));
}

#[test]
fn when_the_engine_ends_the_transaction_the_other_requests_are_run_again() {
    let office = Office::new();
    let store = office.create();
    let (gate, release) = gate(&office, &store);
    let names = ["a", "b", "c"];
    let queued: Vec<_> = names
        .into_iter()
        .enumerate()
        .map(|(ahead, name)| {
            let probe = office.probe.clone();
            office.queue(&store, ahead, move |s| {
                s.submit(
                    None,
                    false,
                    probe.put(name, name.as_bytes(), Ack::Committed),
                )
            })
        })
        .collect();
    // Writes from now: the gate's file, then a's, then b's, which ends the transaction.
    office.engine.end_transaction_at_write(3);
    release.send(()).unwrap();
    gate.join().unwrap().unwrap();
    let answers: Vec<_> = queued.into_iter().map(|q| q.join().unwrap()).collect();
    assert!(answers[0].is_ok() && answers[2].is_ok(), "{answers:?}");
    assert!(
        matches!(&answers[1], Err(StoreError::Engine(e)) if e.transaction_ended),
        "{answers:?}"
    );
    // `a` decided twice (before its entry was appended), `b` once, `c` once.
    assert_eq!(office.probe.decisions(), 4);
    assert!(office.hash(&store, "a").is_some() && office.hash(&store, "c").is_some());
    assert_eq!(office.hash(&store, "b"), None);
    assert_eq!(cursor(&store).ordinal, 3);
}

#[test]
fn a_failed_commit_after_the_append_is_replayed_from_the_journal_and_never_reported() {
    let office = Office::new();
    let store = office.create();
    office.engine.fail_commits(1);
    let first = office.put_as(&store, 1, "x", Ack::Committed).unwrap();
    assert_eq!(office.probe.decisions(), 1);
    assert!(office.hash(&store, "x").is_some());

    // The replay fails too: the store stops, and the answer is "unknown", not a refusal.
    office.engine.fail_commits(2);
    let unknown = office.put_as(&store, 2, "y", Ack::Committed);
    assert!(
        matches!(unknown, Err(StoreError::Stopped { .. })),
        "{unknown:?}"
    );
    assert!(store.status().stopped.is_some());
    assert!(matches!(
        office.put(&store, "z", Ack::Committed),
        Err(StoreError::Stopped { .. })
    ));
    // Reads continue, on the projection as it last committed.
    assert!(office.hash(&store, "x").is_some());
    assert_eq!(office.hash(&store, "y"), None);
    drop(store);

    // The entry was appended, so the reopened office holds it, with its receipt.
    let store = office.reopen();
    assert!(office.hash(&store, "y").is_some());
    let decisions = office.probe.decisions();
    let again = office.put_as(&store, 2, "y", Ack::Committed).unwrap();
    assert_eq!(&*again.bytes, b"put y at work 2");
    assert_eq!(
        office.put_as(&store, 1, "x", Ack::Committed).unwrap(),
        first
    );
    assert_eq!(office.probe.decisions(), decisions);
}

#[test]
fn a_panicking_request_is_reported_and_leaves_the_writer_alive() {
    let office = Office::new();
    let store = office.create();
    let panicked = store.submit(None, false, |uow| {
        uow.next(Stream::Work)?;
        panic!("a decision that panics (expected in this test)")
    });
    assert_eq!(panicked, Err(StoreError::Panicked));
    office.put(&store, "a", Ack::Committed).unwrap();
    // The position the panicking decision took was given back.
    assert_eq!(cursor(&store).high_waters.work, 1);
}

#[test]
fn readers_run_while_the_writer_is_busy() {
    let office = Office::new();
    let store = office.create();
    office.put(&store, "a", Ack::Committed).unwrap();
    let (gate, release) = gate(&office, &store);
    assert!(office.hash(&store, "a").is_some());
    assert_eq!(office.hash(&store, "gate"), None);
    release.send(()).unwrap();
    gate.join().unwrap().unwrap();
    assert!(office.hash(&store, "gate").is_some());
}

/// STORE-4.
#[test]
fn a_committed_write_is_answered_while_the_flush_is_held_and_a_durable_one_is_not() {
    let office = Office::new();
    let store = office.create();
    office.faults.hold_flushes();
    office.put(&store, "c", Ack::Committed).unwrap();

    let (answered, answer) = mpsc::channel();
    let worker = Arc::clone(&store);
    let probe = office.probe.clone();
    let durable = thread::spawn(move || {
        let result = worker.submit(None, false, probe.put("d", b"d", Ack::Durable));
        let _ = answered.send(());
        result
    });
    office.faults.wait_for_held_flush();
    // The flusher is inside the held flush; nothing else answers a durable request.
    assert!(answer.try_recv().is_err());
    assert_eq!(store.status().waiting_durable, 1);
    // Both are readable from their projection commit...
    assert!(office.hash(&store, "c").is_some() && office.hash(&store, "d").is_some());
    // ...and no file exists before the flush that covers its entry.
    assert_eq!(office.file("c"), None);
    assert_eq!(office.file("d"), None);
    assert_eq!(store.status().flushed, 0);

    office.faults.release_flushes();
    durable.join().unwrap().unwrap();
    store.sync().unwrap();
    assert_eq!(office.file("c").unwrap(), b"c");
    assert_eq!(office.file("d").unwrap(), b"d");
    assert_eq!(office.custody.head().unwrap().ordinal, 2);
}

/// STORE-5.
#[test]
fn reads_answer_with_the_disk_failing_and_the_host_gone_and_see_every_acknowledged_change() {
    let office = Office::new();
    let store = office.create();
    office.put(&store, "a", Ack::Committed).unwrap();
    office.put(&store, "b", Ack::Durable).unwrap();
    store.sync().unwrap();
    office.faults.fail_everything();
    office.custody.close();
    office.faults.clear_log();
    let (seen, feed) = store
        .read(|s| {
            let a = s.source_hash(&office.probe.path("a"))?;
            let b = s.source_hash(&office.probe.path("b"))?;
            Ok((a, b))
        })
        .unwrap();
    assert_eq!(seen, (Some(Sha256::of(b"a")), Some(Sha256::of(b"b"))));
    assert_eq!(feed, FeedPosition(2));
    // A read made no disk call at all.
    assert!(office.faults.log().is_empty());
}

/// STORE-8, the flush half.
#[test]
fn a_flush_error_stops_the_store_without_a_retry_and_reopening_holds_what_the_journal_holds() {
    // (site, which matching call, whether the durable request was already answered)
    let cases = [
        (FlushSite::Segment, 1, false),
        (FlushSite::Directory, 1, false),
        (FlushSite::File, 1, true),
        (FlushSite::Directory, 2, true),
    ];
    for (site, nth, answered) in cases {
        let office = Office::new();
        let store = office.create();
        office.faults.arm(CrashPoint::FlushError(site), nth);
        let reply = office.put_as(&store, 1, "x", Ack::Durable);
        assert_eq!(reply.is_ok(), answered, "{site:?} {nth}: {reply:?}");
        let status = store.wait_for(|s| s.stopped.is_some());
        match status.stopped {
            Some(StoreError::Stopped { path: Some(_), .. }) => {}
            other => panic!("{site:?} {nth}: stopped with {other:?}"),
        }
        // It accepts nothing more, and reads continue.
        assert!(
            office.put(&store, "y", Ack::Committed).is_err(),
            "{site:?} {nth}"
        );
        assert!(office.hash(&store, "x").is_some());
        // The last flush the disk saw is the one that failed, and nothing is flushed
        // again, whatever is asked of the store afterwards.
        let flushes = || {
            let log = office.faults.log();
            let flushes = log
                .into_iter()
                .filter(|(op, _)| matches!(op, DiskOp::Flush | DiskOp::SyncDir));
            flushes.collect::<Vec<_>>()
        };
        let before = flushes();
        let (op, path) = before.last().unwrap();
        let segment = path.extension().is_some_and(|e| e == "log");
        let matches_site = match site {
            FlushSite::Segment => *op == DiskOp::Flush && segment,
            FlushSite::File => *op == DiskOp::Flush && !segment,
            FlushSite::Directory => *op == DiskOp::SyncDir,
        };
        assert!(
            matches_site,
            "{site:?} {nth}: last flush was {op:?} {path:?}"
        );
        assert!(store.sync().is_err());
        assert!(office.put(&store, "z", Ack::Durable).is_err());
        assert_eq!(flushes(), before, "{site:?} {nth}");
        drop(store);

        // The entry is in the journal on the drive, so the reopened office has it whole:
        // the projection row, the file, and the receipt that answers the retry.
        let store = office.reopen();
        assert!(office.hash(&store, "x").is_some(), "{site:?} {nth}");
        assert_eq!(office.file("x").unwrap(), b"x");
        let decisions = office.probe.decisions();
        let retried = office.put_as(&store, 1, "x", Ack::Durable).unwrap();
        assert_eq!(&*retried.bytes, b"put x at work 1");
        assert_eq!(office.probe.decisions(), decisions);
    }
}

/// STORE-8, the append half.
#[test]
fn a_failed_or_short_append_fails_its_batch_alone_and_a_failed_truncation_stops_the_store() {
    for point in [CrashPoint::AppendError, CrashPoint::ShortAppend] {
        let office = Office::new();
        let store = office.create();
        office.put(&store, "before", Ack::Durable).unwrap();
        office.faults.arm(point, 1);
        let failed = office.put_as(&store, 1, "x", Ack::Durable);
        assert!(
            matches!(failed, Err(StoreError::Append(_))),
            "{point:?}: {failed:?}"
        );
        // Nothing of the batch is acknowledged or visible, and no position was kept.
        assert_eq!(office.hash(&store, "x"), None);
        assert_eq!(store.status().stopped, None);
        let next = office.put_as(&store, 2, "y", Ack::Durable).unwrap();
        assert_eq!(&*next.bytes, b"put y at work 2", "{point:?}");
        // The same operation ID decides afresh: the failed batch left no receipt.
        office.put_as(&store, 1, "x", Ack::Durable).unwrap();
        Arc::try_unwrap(store).ok().unwrap().close().unwrap();
        let store = office.reopen();
        assert_eq!(cursor(&store).ordinal, 3, "{point:?}");
    }

    let office = Office::new();
    let store = office.create();
    office.faults.arm(CrashPoint::TruncateError, 1);
    assert!(matches!(
        office.put(&store, "x", Ack::Durable),
        Err(StoreError::Append(_))
    ));
    assert!(matches!(
        store.status().stopped,
        Some(StoreError::Journal(JournalError::Broken { .. }))
    ));
    assert!(office.put(&store, "y", Ack::Committed).is_err());
    assert_eq!(office.hash(&store, "x"), None);
    drop(store);
    let store = office.reopen();
    assert_eq!(cursor(&store).ordinal, 0);
    office.put(&store, "y", Ack::Durable).unwrap();
}

#[test]
fn a_retry_gets_the_original_reply_and_another_actor_kind_or_fingerprint_conflicts() {
    let office = Office::new();
    let store = office.create();
    let first = office.put_as(&store, 1, "x", Ack::Durable).unwrap();
    assert_eq!(first.outcome, Outcome::Completed);
    assert_eq!(&*first.bytes, b"put x at work 1");
    office.put(&store, "other", Ack::Committed).unwrap();
    let decisions = office.probe.decisions();
    assert_eq!(office.put_as(&store, 1, "x", Ack::Durable).unwrap(), first);
    assert_eq!(office.probe.decisions(), decisions);

    let op = office.probe.operation(id(1), "probe.put", "x", b"x");
    let fingerprint = Operation {
        fingerprint: Sha256::of(b"another request"),
        ..op.clone()
    };
    let actor = Operation {
        actor: crate::change::Actor::Employee(id(77)),
        ..op.clone()
    };
    let kind = Operation {
        kind: "probe.create",
        ..op
    };
    for other in [fingerprint, actor, kind] {
        let conflict = store.submit(
            Some(other),
            false,
            office.probe.put("x", b"x", Ack::Durable),
        );
        assert_eq!(conflict, Err(StoreError::Conflict));
    }
    assert_eq!(office.probe.decisions(), decisions);
    // The receipt is a file like any other, materialized with its operation.
    store.sync().unwrap();
    assert!(office.root.join(receipt_path(&id(1)).as_str()).is_file());
}

#[test]
fn two_operations_with_one_id_in_one_batch_decide_once_and_a_durable_retry_waits_for_its_flush() {
    let office = Office::new();
    let store = office.create();
    let (gate, release) = gate(&office, &store);
    let twins: Vec<_> = (0..2)
        .map(|ahead| {
            let probe = office.probe.clone();
            office.queue(&store, ahead, move |s| {
                let op = probe.operation(id(5), "probe.put", "x", b"x");
                s.submit(Some(op), false, probe.put("x", b"x", Ack::Committed))
            })
        })
        .collect();
    release.send(()).unwrap();
    gate.join().unwrap().unwrap();
    let answers: Vec<_> = twins
        .into_iter()
        .map(|t| t.join().unwrap().unwrap())
        .collect();
    assert_eq!(answers[0], answers[1]);
    assert_eq!(office.probe.decisions(), 1);

    // A retry of a durable operation whose entry is not yet flushed waits like the original.
    office.faults.hold_flushes();
    let submit = |store: &Arc<TestStore>| {
        let (worker, probe) = (Arc::clone(store), office.probe.clone());
        thread::spawn(move || {
            let op = probe.operation(id(6), "probe.put", "d", b"d");
            worker.submit(Some(op), false, probe.put("d", b"d", Ack::Durable))
        })
    };
    let original = submit(&store);
    office.faults.wait_for_held_flush();
    let retry = submit(&store);
    store.wait_for(|s| s.waiting_durable == 2);
    assert!(!original.is_finished() && !retry.is_finished());
    office.faults.release_flushes();
    assert_eq!(
        original.join().unwrap().unwrap(),
        retry.join().unwrap().unwrap()
    );
    assert_eq!(office.probe.decisions(), 2);
}

#[test]
fn a_rejection_leaves_a_receipt_only_entry_and_an_invalid_request_leaves_nothing() {
    let office = Office::new();
    let store = office.create();
    let create = |n: u64| {
        let op = office.probe.operation(id(n), "probe.create", "x", b"x");
        store.submit(Some(op), false, office.probe.create("x", b"x"))
    };
    let created = create(1).unwrap();
    assert_eq!(&*created.bytes, b"created x at generation 1");
    let rejected = create(2).unwrap();
    assert_eq!(rejected.outcome, Outcome::Rejected);
    assert_eq!(&*rejected.bytes, b"x exists");
    // The rejection took an ordinal and a feed position, and gave the generation back.
    let after = cursor(&store);
    assert_eq!((after.ordinal, after.feed), (2, FeedPosition(2)));
    assert_eq!(after.high_waters.organization, 1);
    // A retry gets the same rejection without deciding again.
    let decisions = office.probe.decisions();
    assert_eq!(create(2).unwrap(), rejected);
    assert_eq!(office.probe.decisions(), decisions);
    store.sync().unwrap();
    let receipt = fs::read_to_string(office.root.join(receipt_path(&id(2)).as_str())).unwrap();
    assert!(receipt.contains("\"outcome\":\"rejected\""), "{receipt}");

    // Refused before admission: no ordinal, no receipt, and the same ID may be used again.
    let op = office.probe.operation(id(3), "probe.remove", "none", b"");
    let invalid = store.submit(Some(op.clone()), false, office.probe.remove("none"));
    assert!(matches!(invalid, Err(StoreError::Invalid(_))));
    assert_eq!(cursor(&store), after);
    assert_eq!(store.read(|s| Ok(s.receipt(&id(3))?)).unwrap().0, None);
    // A rejection without an operation ID is answered and records nothing.
    let anonymous = store
        .submit(None, false, office.probe.create("x", b"x"))
        .unwrap();
    assert_eq!(anonymous.outcome, Outcome::Rejected);
    assert_eq!(cursor(&store), after);

    // Only the store writes receipts, and a tree is not a file.
    let forged = receipt_path(&id(9));
    let refused = store.submit(None, false, move |_| {
        Ok(Change::new(Ack::Committed).put(forged.clone(), b"{}".to_vec()))
    });
    assert!(matches!(refused, Err(StoreError::Invalid(_))));
    let unowned = store.submit(None, false, |_| {
        let path = DurablePath::parse("state/nobody/owns.yaml").unwrap();
        Ok(Change::new(Ack::Committed).put(path, b"x".to_vec()))
    });
    assert!(matches!(unowned, Err(StoreError::Projection(_))));
    assert_eq!(cursor(&store), after);
}

/// STORE-1, and the crash table of the design: at each point the operation is wholly
/// absent or wholly present after reopening, and no decision runs after its append.
#[test]
fn a_crash_at_each_point_reopens_to_wholly_absent_or_wholly_present() {
    let cases = [
        (CrashPoint::DuringDecide, false),
        (CrashPoint::DuringApply, false),
        (CrashPoint::MidAppend(9), false),
        (CrashPoint::AfterAppend, true),
        (CrashPoint::AfterCommit, true),
        (CrashPoint::AfterCommittedReply, true),
        (CrashPoint::MaterializeTempFlushed, true),
        (CrashPoint::MaterializeRenamed, true),
        (CrashPoint::BeforeMark, true),
    ];
    for (point, present) in cases {
        let office = Office::new();
        let store = office.create();
        let kept = office.put_as(&store, 1, "kept", Ack::Durable).unwrap();
        store.sync().unwrap();
        office.faults.arm(point, 1);
        let first = office.put_as(&store, 2, "x", Ack::Durable);
        // The threads are halted and joined before the next process starts.
        store.wait_for(|_| office.faults.dead());
        drop(store);

        let store = office.reopen();
        assert_eq!(office.hash(&store, "x").is_some(), present, "{point:?}");
        assert_eq!(
            cursor(&store).ordinal,
            if present { 2 } else { 1 },
            "{point:?}"
        );
        store.sync().unwrap();
        assert_eq!(office.file("x").is_some(), present, "{point:?}");
        assert_eq!(office.file("kept").unwrap(), b"kept", "{point:?}");

        // The same operation ID again: the original reply and no second decision when the
        // entry was appended; a fresh decision when it was not.
        let decisions = office.probe.decisions();
        let again = office.put_as(&store, 2, "x", Ack::Durable).unwrap();
        assert_eq!(&*again.bytes, b"put x at work 2", "{point:?}");
        assert_eq!(
            office.probe.decisions() - decisions,
            usize::from(!present),
            "{point:?}"
        );
        if let Ok(first) = first {
            assert!(present, "{point:?} acknowledged an operation that is gone");
            assert_eq!(first, again, "{point:?}");
        }
        assert_eq!(
            office.put_as(&store, 1, "kept", Ack::Durable).unwrap(),
            kept
        );
    }
}

#[test]
fn a_power_cut_after_a_committed_reply_takes_back_only_what_was_not_flushed() {
    for wipe in [false, true] {
        let office = Office::new();
        let store = office.create();
        office.put_as(&store, 1, "durable", Ack::Durable).unwrap();
        office.faults.arm(CrashPoint::AfterCommittedReply, 1);
        office
            .put_as(&store, 2, "committed", Ack::Committed)
            .unwrap();
        store.wait_for(|_| office.faults.dead());
        drop(store);
        office.faults.power_cut().unwrap();
        // The projection is ahead of the journal, or gone with the machine.
        if wipe {
            office.engine.wipe();
        }
        let store = office.reopen();
        assert!(office.hash(&store, "durable").is_some(), "wipe {wipe}");
        assert_eq!(office.hash(&store, "committed"), None, "wipe {wipe}");
        assert_eq!(cursor(&store).ordinal, 1);
        // The number the cut took back was never seen outside; it is issued again.
        let again = office.put_as(&store, 2, "committed", Ack::Durable).unwrap();
        assert_eq!(&*again.bytes, b"put committed at work 2");
    }
}

#[test]
fn the_change_stream_follows_commits_in_order_and_answers_resync_outside_its_ring() {
    let mut office = Office::new();
    office.limits.change_ring = 2;
    let store = office.create();
    let (_, start) = store.read(|_| Ok(())).unwrap();
    assert_eq!(start, FeedPosition(0));
    let mut stream = store.subscribe(start).unwrap();
    assert!(stream.try_recv().is_none());
    office.put_as(&store, 1, "a", Ack::Committed).unwrap();
    office.put(&store, "b", Ack::Committed).unwrap();
    let first = stream.recv().unwrap();
    assert_eq!((first.feed, first.ordinal), (FeedPosition(1), 1));
    assert_eq!(first.kind.as_deref(), Some("probe.put"));
    assert_eq!(first.paths, [office.probe.path("a"), receipt_path(&id(1))]);
    let second = stream.recv().unwrap();
    assert_eq!((second.feed, second.kind.clone()), (FeedPosition(2), None));

    // A late subscriber inside the ring replays, then follows.
    let mut late = store.subscribe(FeedPosition(1)).unwrap();
    assert_eq!(late.try_recv().unwrap().feed, FeedPosition(2));
    office.put(&store, "c", Ack::Committed).unwrap();
    assert_eq!(late.recv().unwrap().feed, FeedPosition(3));
    // Older than the ring, or never issued.
    assert_eq!(store.subscribe(FeedPosition(0)).err(), Some(Resync));
    assert_eq!(store.subscribe(FeedPosition(9)).err(), Some(Resync));
    assert!(store.subscribe(FeedPosition(1)).is_ok());

    // Every position from before a restart is outside the ring.
    Arc::try_unwrap(store).ok().unwrap().close().unwrap();
    assert!(late.recv().is_none());
    let store = office.reopen();
    assert_eq!(store.subscribe(FeedPosition(2)).err(), Some(Resync));
    let (_, now) = store.read(|_| Ok(())).unwrap();
    assert_eq!(now, FeedPosition(3));
    assert!(store.subscribe(now).is_ok());
}

#[test]
fn back_pressure_holds_writes_until_the_flusher_catches_up_and_reads_continue() {
    let mut office = Office::new();
    office.limits.unflushed_bytes = 1;
    let store = office.create();
    office.faults.hold_flushes();
    office.put(&store, "a", Ack::Committed).unwrap();
    let probe = office.probe.clone();
    let worker = Arc::clone(&store);
    let held =
        thread::spawn(move || worker.submit(None, false, probe.put("b", b"b", Ack::Committed)));
    let status = store.wait_for(|s| s.holding_back && s.queued == 1);
    assert_eq!(status.stopped, None);
    assert!(office.hash(&store, "a").is_some());
    assert_eq!(office.hash(&store, "b"), None);
    assert!(!held.is_finished());
    office.faults.release_flushes();
    held.join().unwrap().unwrap();
    assert!(office.hash(&store, "b").is_some());
}

#[test]
fn a_decision_reads_source_bytes_that_are_accepted_and_not_yet_written() {
    let office = Office::new();
    let store = office.create();
    let append = |suffix: &[u8]| {
        let reply = store.submit(None, false, office.probe.append("n", suffix));
        reply.unwrap().bytes.to_vec()
    };
    assert_eq!(append(b"ab"), b"ab");
    // No flush has run: the bytes come from the entry, not from a file.
    assert_eq!(office.file("n"), None);
    assert_eq!(append(b"cd"), b"abcd");
    store.sync().unwrap();
    assert_eq!(office.file("n").unwrap(), b"abcd");
    assert_eq!(append(b"ef"), b"abcdef");
    // A removal not yet written reads as no file.
    store.submit(None, false, office.probe.remove("n")).unwrap();
    assert_eq!(append(b"z"), b"z");
}

/// STORE-2 on the memory engine.
#[test]
fn reopening_and_deleting_the_projection_both_give_the_same_projection() {
    let office = Office::new();
    let store = office.create();
    for (n, name) in ["a", "b", "c", "a"].into_iter().enumerate() {
        office
            .put_as(&store, n as u64 + 1, name, Ack::Committed)
            .unwrap();
    }
    store.submit(None, false, office.probe.remove("b")).unwrap();
    assert_eq!(cursor(&store).high_waters.work, 4);
    Arc::try_unwrap(store).ok().unwrap().close().unwrap();
    let before = office.engine.dump();
    assert!(before.len() > 6, "{before:?}");

    Arc::try_unwrap(office.reopen())
        .ok()
        .unwrap()
        .close()
        .unwrap();
    assert_eq!(office.engine.dump(), before);
    office.engine.wipe();
    let store = office.reopen();
    assert_eq!(office.engine.dump(), before);
    // The generation and every position survive the rebuild: they come from the journal.
    assert_eq!(cursor(&store).high_waters.work, 4);
    assert_eq!(
        &*office.put(&store, "d", Ack::Durable).unwrap().bytes,
        b"put d at work 5"
    );
}

#[test]
fn a_second_creation_a_relative_root_and_a_failed_commit_check_are_refused() {
    let office = Office::new();
    let store = office.create();
    let again = Store::open(office.parts(Opening::Create(AnchorFacts::default())));
    assert!(matches!(
        again.err(),
        Some(StoreError::Journal(JournalError::Exists))
    ));
    let mut relative = office.parts(Opening::Existing);
    relative.durable = PathBuf::from("durable");
    assert!(Store::open(relative).is_err());

    // A reference the engine would only refuse at commit fails its operation alone, as
    // an internal error, before anything is appended.
    office.engine.fail_checks(1);
    let failed = office.put(&store, "x", Ack::Committed);
    assert!(matches!(failed, Err(StoreError::Internal(_))), "{failed:?}");
    assert_eq!(office.hash(&store, "x"), None);
    office.put(&store, "x", Ack::Committed).unwrap();
    assert_eq!(cursor(&store).ordinal, 1);
}

/// A segment rolled just before a kill was never flushed by the process that wrote it.
/// Opening flushes every segment it keeps before it confirms the head to the host.
#[test]
fn opening_makes_every_kept_segment_durable_before_it_confirms_the_head() {
    let mut office = Office::new();
    office.limits.segment_bytes = 1;
    let store = office.create();
    office.put_as(&store, 1, "a", Ack::Committed).unwrap();
    office.put_as(&store, 2, "b", Ack::Committed).unwrap();
    office.faults.kill();
    drop(store);

    let store = office.reopen();
    assert_eq!(office.custody.head().unwrap().ordinal, 2);
    drop(store);
    office.faults.power_cut().unwrap();
    let store = office.reopen();
    assert_eq!(cursor(&store).ordinal, 2);
    assert!(office.hash(&store, "a").is_some() && office.hash(&store, "b").is_some());
    // A path no rebuild walks is refused before it is accepted.
    let elsewhere = store.submit(None, false, |_| {
        let path = DurablePath::parse("local/offices/another-office/probe/x").unwrap();
        Ok(Change::new(Ack::Committed).put(path, b"x".to_vec()))
    });
    assert!(matches!(elsewhere, Err(StoreError::Projection(_))));
}

/// The flusher may flush a batch the moment it is appended; the accounting must still
/// drain, or closing would never return.
#[test]
fn closing_returns_after_many_small_batches_raced_against_the_flusher() {
    let office = Office::new();
    let store = office.create();
    for n in 0..200u64 {
        let ack = if n.is_multiple_of(3) {
            Ack::Durable
        } else {
            Ack::Committed
        };
        office.put(&store, "x", ack).unwrap();
        if n.is_multiple_of(7) {
            store.sync().unwrap();
        }
    }
    store.sync().unwrap();
    let status = store.status();
    assert_eq!((status.flushed, status.materialized), (200, 200));
    Arc::try_unwrap(store).ok().unwrap().close().unwrap();
}
