//! Engine-independent ADR-0014 acknowledgement class tests (`ACK-2`, `ACK-3`, `ACK-4`,
//! `ACK-5`, `ACK-6`, `ACK-7`) for organization operations and office store durability (`TASK-33` / B1c).
//!
//! Every engine-independent test runs identically against both `EngineChoice::Sqlite` and
//! `EngineChoice::Memory` without referring to any SQLite-specific mechanism (`ACK-6`).

use std::sync::mpsc;
use std::time::{Duration, Instant};

use org_store::testkit::{CrashPoint, FlushSite};
use org_store::{
    Ack, Actor, DurablePath, Id, Opening, Operation, Outcome, Reply, Store, StoreError,
};

use super::*;
use org_office::organization_family::features::{create_team, Feature};

struct CommittedWrite<'a> {
    probe: &'a Probe,
    name: &'a str,
    body: Vec<u8>,
    op_id: Option<Id>,
}

impl StoreOp for CommittedWrite<'_> {
    type Out = Result<Reply, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let op = self.op_id.map(|id| Operation {
            id,
            kind: "probe.committed",
            actor: Actor::Employee(id_creator()),
            fingerprint: org_store::Sha256::of(&self.body),
        });
        let decide = self.probe.put::<E>(self.name, &self.body, Ack::Committed);
        store.submit(op, false, decide)
    }
}

struct CommittedMultiFile<'a> {
    probe: &'a Probe,
    a: &'a str,
    b: &'a str,
    op_id: Id,
}

impl StoreOp for CommittedMultiFile<'_> {
    type Out = Result<Reply, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let a_path = self.probe.path(self.a);
        let b_path = self.probe.path(self.b);
        let op = Operation {
            id: self.op_id,
            kind: "probe.multi",
            actor: Actor::Employee(id_creator()),
            fingerprint: org_store::Sha256::of(format!("{}:{}", self.a, self.b).as_bytes()),
        };
        store.submit(Some(op), false, move |_uow| {
            Ok(org_store::Change::new(Ack::Committed)
                .put(a_path.clone(), b"file-a".to_vec())
                .put(b_path.clone(), b"file-b".to_vec())
                .reply(b"ok".to_vec()))
        })
    }
}

struct ReadSourceExists(DurablePath);

impl ReadOp for ReadSourceExists {
    type Out = bool;

    fn read<S: TasksRead>(self, snapshot: &S) -> Result<bool, StoreError> {
        Ok(snapshot.source_hash(&self.0)?.is_some())
    }
}

struct HasReceiptOp(Id);

impl ReadOp for HasReceiptOp {
    type Out = bool;

    fn read<S: TasksRead>(self, snapshot: &S) -> Result<bool, StoreError> {
        Ok(snapshot.receipt(&self.0)?.is_some())
    }
}

fn id_creator() -> Id {
    id(CREATOR)
}

/// ACK-2: A committed-class write is present after the office process is killed immediately
/// after the acknowledgement (without waiting for a full flush).
#[test]
fn ack_2_committed_write_survives_immediate_process_kill_on_both_engines() {
    for engine in engines() {
        let place = Place::new();
        let store = place.create(engine.clone());
        create_organization(&store, 1).expect("org created");
        store.sync().expect("baseline synced");
        let flushed_before = store.status().flushed;

        // Submit a committed-class write while flush_interval is 1 hour (no flush triggered).
        let op_id = id(0xA201);
        let reply = store
            .run(CommittedWrite {
                probe: &place.probe,
                name: "ack2-item",
                body: b"committed-payload".to_vec(),
                op_id: Some(op_id.clone()),
            })
            .expect("committed write acknowledged");
        assert_eq!(reply.outcome, Outcome::Completed);
        assert_eq!(
            store.status().flushed,
            flushed_before,
            "{engine:?}: committed ack must not wait for flush"
        );

        // Kill immediately without orderly close (`drop` simulates SIGKILL / crash).
        drop(store);

        // Reopen: journal replay recovers the committed write and its receipt intact.
        let reopened = place.reopen(engine.clone());
        assert!(!reopened.stopped_cleanly(), "{engine:?}");
        let exists = reopened
            .read(ReadSourceExists(place.probe.path("ack2-item")))
            .expect("read")
            .0;
        assert!(
            exists,
            "{engine:?}: committed write must survive process kill"
        );
        let has_receipt = reopened.read(HasReceiptOp(op_id)).expect("receipt read").0;
        assert!(
            has_receipt,
            "{engine:?}: committed receipt must survive kill"
        );
        reopened.close().expect("clean close");
    }
}

/// ACK-3: A durable-class organization operation is never acknowledged before its full flush;
/// injecting a fault between commit and flush yields no success acknowledgement.
#[test]
fn ack_3_durable_org_operation_never_acknowledges_when_flush_fails() {
    for engine in engines() {
        let place = Place::new();
        let store = place.create(engine.clone());
        create_organization(&store, 1).expect("org created");
        store.sync().expect("baseline synced");

        place
            .faults
            .arm(CrashPoint::FlushError(FlushSite::Segment), 1);
        let op_id = id(0xA301);
        let team_cmd = team_command(0x310, TOP_TEAM);
        let (tx, rx) = mpsc::channel();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let outcome = store.run(CreateTeamOp {
                    operation_id: op_id.clone(),
                    here: identity(),
                    owner: id(CREATOR),
                    by: Actor::Employee(id(CREATOR)),
                    command: team_cmd,
                });
                let _ = tx.send(outcome);
            });
            let res = rx
                .recv_timeout(Duration::from_secs(5))
                .expect("durable submit must resolve on flush failure");
            assert!(
                res.is_err(),
                "{engine:?}: durable op MUST NOT acknowledge when flush fails, got {res:?}"
            );
        });
        assert!(
            store.fatal().is_some(),
            "{engine:?}: failed flush stops the store"
        );
        drop(store);

        place.faults.revive();
        let reopened = place.reopen(engine.clone());
        assert!(reopened.fatal().is_none(), "{engine:?}");
        reopened.close().expect("clean close");
    }
}

/// ACK-5: The bounded flush interval is enforced *while* continuous committed-class writes keep
/// arriving (no idle pause before asserting flush progress), with a generous outer timeout for
/// slow macOS `F_FULLFSYNC`.
#[test]
fn ack_5_bounded_flush_interval_enforced_under_continuous_committed_load() {
    for engine in engines() {
        let place = Place::new();
        let mut cfg = place.config(engine.clone(), Opening::Create(AnchorFacts::default()));
        cfg.flush_interval = Duration::from_millis(25);
        let store = open_store(&cfg).expect("open store");
        create_organization(&store, 1).expect("org created");
        store.sync().expect("baseline synced");
        let start_flushed = store.status().flushed;

        // Keep submitting committed writes continuously until `status().flushed` advances
        // DURING the load loop itself (catches timer-reset starvation bugs).
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut count = 0u64;
        let mut advanced_during_load = false;
        while Instant::now() < deadline {
            count += 1;
            let name = format!("load-{count}");
            let reply = store
                .run(CommittedWrite {
                    probe: &place.probe,
                    name: &name,
                    body: format!("payload-{count}").into_bytes(),
                    op_id: None,
                })
                .expect("committed load write");
            assert_eq!(reply.outcome, Outcome::Completed);
            if store.status().flushed > start_flushed {
                advanced_during_load = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            advanced_during_load,
            "{engine:?}: bounded flush interval did not advance flushed ordinal during continuous committed load (start={start_flushed}, now={}, writes={count})",
            store.status().flushed
        );
        store.close().expect("clean close");
    }
}

/// ACK-6 & ACK-7: A committed-class multi-file change killed at every crash point actually fires
/// its crash point, recovers wholly present (if acknowledged) or wholly absent (if pre-append),
/// keeps files + receipt consistent, and handles idempotent retry identically on both engines.
#[test]
fn ack_6_and_ack_7_committed_multi_file_change_is_atomic_across_crash_points() {
    let points = [
        CrashPoint::DuringDecide,
        CrashPoint::DuringApply,
        CrashPoint::MidAppend(9),
        CrashPoint::AfterAppend,
        CrashPoint::AfterCommit,
        CrashPoint::AfterCommittedReply,
        CrashPoint::BeforeMark,
    ];
    for engine in engines() {
        for (idx, point) in points.into_iter().enumerate() {
            let place = Place::new();
            let store = place.create(engine.clone());
            create_organization(&store, 1).expect("org created");
            store.sync().expect("baseline synced");

            place.faults.arm(point, 1);
            let op_id = id(0xA700 + idx as u64);
            let first = store.run(CommittedMultiFile {
                probe: &place.probe,
                a: "part-a",
                b: "part-b",
                op_id: op_id.clone(),
            });
            if point == CrashPoint::BeforeMark {
                let _ = store.sync();
            } else if point == CrashPoint::AfterCommittedReply {
                // Wait briefly for the writer thread to execute the post-reply crash hook.
                let wait_until = Instant::now() + Duration::from_secs(2);
                while !place.faults.fired() && Instant::now() < wait_until {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
            assert!(
                place.faults.fired(),
                "{engine:?} {point:?}: armed crash point never fired"
            );
            place.faults.kill();
            drop(store);

            place.faults.revive();
            let reopened = place.reopen(engine.clone());
            let has_a = reopened
                .read(ReadSourceExists(place.probe.path("part-a")))
                .expect("read a")
                .0;
            let has_b = reopened
                .read(ReadSourceExists(place.probe.path("part-b")))
                .expect("read b")
                .0;
            let has_receipt = reopened
                .read(HasReceiptOp(op_id.clone()))
                .expect("receipt")
                .0;
            assert_eq!(
                has_a, has_b,
                "{engine:?} {point:?}: multi-file committed change must never be half-applied"
            );
            assert_eq!(
                has_a, has_receipt,
                "{engine:?} {point:?}: files and receipt must agree after recovery"
            );
            if first.is_ok() {
                assert!(
                    has_a,
                    "{engine:?} {point:?}: acknowledged committed write must survive recovery"
                );
            }
            if matches!(
                point,
                CrashPoint::DuringDecide | CrashPoint::DuringApply | CrashPoint::MidAppend(_)
            ) {
                assert!(
                    !has_a,
                    "{engine:?} {point:?}: pre-append crash must leave write wholly absent"
                );
            }

            // Idempotent retry after recovery always succeeds and leaves both files + receipt intact.
            let retry = reopened
                .run(CommittedMultiFile {
                    probe: &place.probe,
                    a: "part-a",
                    b: "part-b",
                    op_id: op_id.clone(),
                })
                .expect("retry after recovery");
            assert_eq!(retry.outcome, Outcome::Completed);
            assert!(
                reopened
                    .read(ReadSourceExists(place.probe.path("part-a")))
                    .unwrap()
                    .0
            );
            assert!(
                reopened
                    .read(ReadSourceExists(place.probe.path("part-b")))
                    .unwrap()
                    .0
            );
            assert!(reopened.read(HasReceiptOp(op_id)).unwrap().0);
            reopened.close().expect("clean close");
        }
    }
}

/// Verifies `create_team` always declares `Ack::Durable` on both engines (`ACK-4` / `ACK-6`).
#[test]
fn ack_4_organization_mutation_always_declares_durable_class() {
    for engine in engines() {
        let place = Place::new();
        let store = place.create(engine.clone());
        create_organization(&store, 1).expect("org created");
        let command = team_command(0x410, TOP_TEAM);
        let actor = Actor::Employee(id(CREATOR));
        let op = command.operation(id(0xA401), &actor);
        let here = identity();
        let reply = store
            .run(TeamDurableCheck {
                op,
                here,
                actor,
                command,
            })
            .expect("check");
        assert_eq!(reply.outcome, Outcome::Completed);
        store.close().expect("close");
    }
}

struct TeamDurableCheck {
    op: Operation,
    here: OfficeIdentity,
    actor: Actor,
    command: CreateTeam,
}

impl StoreOp for TeamDurableCheck {
    type Out = Result<Reply, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let TeamDurableCheck {
            op,
            here,
            actor,
            command,
        } = self;
        store.submit(Some(op), false, move |uow| {
            let change = create_team::decide(uow, &here, &id(CREATOR), &actor, &command)?;
            assert_eq!(
                change.ack,
                Ack::Durable,
                "organization mutations must declare Ack::Durable"
            );
            Ok(change)
        })
    }
}

/// Holds a read snapshot open on a worker thread until signalled, proving real SQLite WAL
/// checkpoint blocks surface through `OfficeStore::status().upkeep_warning` (`TASK-33` AC #3).
struct HoldReadOp {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}

impl ReadOp for HoldReadOp {
    type Out = ();

    fn read<S: TasksRead>(self, _snapshot: &S) -> Result<(), StoreError> {
        let _ = self.entered.send(());
        let _ = self.release.recv_timeout(Duration::from_secs(5));
        Ok(())
    }
}

#[test]
fn sqlite_persistent_checkpoint_block_surfaces_in_store_status_upkeep_warning() {
    let place = Place::new();
    let store = place.create(EngineChoice::Sqlite(org_office_sqlite::sqlite_backend()));
    create_organization(&store, 1).expect("org created");
    store.sync().expect("baseline synced");
    assert!(store.status().upkeep_warning.is_none());

    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    std::thread::scope(|scope| {
        let reader = scope.spawn(|| {
            store
                .read(HoldReadOp {
                    entered: entered_tx,
                    release: release_rx,
                })
                .expect("held snapshot")
        });
        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("reader acquired snapshot");

        // Run 3 materialized rounds while the snapshot holds the WAL open -> triggers 3 consecutive
        // `PRAGMA wal_checkpoint(PASSIVE)` busy stalls -> surfaces `status().upkeep_warning`.
        for i in 1..=3u64 {
            let name = format!("blocked-wal-{i}");
            store
                .run(CommittedWrite {
                    probe: &place.probe,
                    name: &name,
                    body: format!("wal-frame-{i}").into_bytes(),
                    op_id: None,
                })
                .expect("write");
            store.sync().expect("sync round");
        }
        let warning = store
            .status()
            .upkeep_warning
            .expect("3 consecutive reader-blocked WAL checkpoints must surface upkeep_warning");
        assert!(
            warning.contains("3 consecutive"),
            "unexpected warning text: {warning}"
        );

        // Release the reader and run one more round -> warning clears cleanly.
        let _ = release_tx.send(());
        let _ = reader.join().expect("reader thread");
    });

    store
        .run(CommittedWrite {
            probe: &place.probe,
            name: "wal-recovered",
            body: b"recovered".to_vec(),
            op_id: None,
        })
        .expect("post-release write");
    store.sync().expect("post-release sync");
    assert!(
        store.status().upkeep_warning.is_none(),
        "upkeep_warning must clear once WAL checkpoint succeeds"
    );
    store.close().expect("clean close");
}
