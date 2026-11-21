//! Crashes, failing disks and a changed schema, through `open_store` on each engine.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc,
};

use org_store::{
    testkit::{CrashPoint, FlushSite},
    Ack, DurablePath, Id, JournalError, Sha256,
};

use super::*;
use crate::organization::features::{create_team, Feature, TeamCreated};

/// `create_team`, counting its decisions: a retry answered from its receipt adds none.
struct CountedTeam {
    op: u64,
    team: u64,
    decisions: Arc<AtomicUsize>,
}

impl StoreOp for CountedTeam {
    type Out = Result<Reply, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let (here, by) = (identity(), Actor::Employee(id(CREATOR)));
        let command = team_command(self.team, TOP_TEAM);
        let operation = command.operation(id(self.op), &by);
        let decisions = self.decisions;
        store.submit(Some(operation), false, move |uow| {
            decisions.fetch_add(1, Ordering::SeqCst);
            create_team::decide(uow, &here, &id(CREATOR), &by, &command)
        })
    }
}

/// A probe file, in the committed class unless it says otherwise.
struct Put<'a> {
    probe: &'a Probe,
    name: &'a str,
    ack: Ack,
}

impl StoreOp for Put<'_> {
    type Out = Result<Reply, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let decide = self
            .probe
            .put::<E>(self.name, self.name.as_bytes(), self.ack);
        store.submit(None, false, decide)
    }
}

struct HasReceipt(Id);

impl ReadOp for HasReceipt {
    type Out = bool;

    fn read<S: MessagingRead>(self, snapshot: &S) -> Result<bool, StoreError> {
        Ok(snapshot.receipt(&self.0)?.is_some())
    }
}

struct Source(DurablePath);

impl ReadOp for Source {
    type Out = Option<Sha256>;

    fn read<S: MessagingRead>(self, snapshot: &S) -> Result<Option<Sha256>, StoreError> {
        Ok(snapshot.source_hash(&self.0)?)
    }
}

/// The teams a test creates, and the operation that creates each.
const TEAMS: [(u64, u64); 2] = [(2, 0x21), (3, 0x22)];

struct Office {
    place: Place,
    engine: EngineChoice,
    decisions: Arc<AtomicUsize>,
}

impl Office {
    /// A new office with its organization, and the store it is open on.
    fn created(engine: EngineChoice) -> (Self, OfficeStore) {
        let place = Place::new();
        let store = place.create(engine);
        let created = create_organization(&store, 1).expect("an organization");
        assert_eq!(created.outcome, Outcome::Completed);
        let decisions = Arc::new(AtomicUsize::new(0));
        let office = Self {
            place,
            engine,
            decisions,
        };
        (office, store)
    }

    fn team(&self, store: &OfficeStore, op: u64, team: u64) -> Result<Reply, StoreError> {
        store.run(CountedTeam {
            op,
            team,
            decisions: Arc::clone(&self.decisions),
        })
    }

    fn decided(&self) -> usize {
        self.decisions.load(Ordering::SeqCst)
    }

    /// The process dies and a new one opens the same office.
    fn reopened(&self, store: OfficeStore) -> OfficeStore {
        self.place.faults.kill();
        drop(store);
        self.place.faults.revive();
        self.place.reopen(self.engine)
    }

    /// Whether the team's file is on the drive.
    fn team_file(&self, team: u64) -> bool {
        let files = tree(&self.place.durable().join("state"));
        let name = format!("{}/team.yaml", id(team).as_str());
        files.keys().any(|path| path.ends_with(&name))
    }

    /// After a reopening: each team is wholly there or wholly not, an acknowledged one is
    /// there, and submitting its operation again answers what it answered, undecided.
    fn check(
        &self,
        store: &OfficeStore,
        first: &[Option<Result<Reply, StoreError>>],
        context: &str,
    ) {
        for ((op, team), first) in TEAMS.into_iter().zip(first) {
            let seen = directory(store);
            let team_id = id(team);
            let present = seen.teams.iter().any(|t| t.id == team_id);
            let parts = [
                seen.allocations.iter().any(|a| a.team_id == team_id),
                seen.refs.iter().any(|r| r.id == team_id),
                store.read(HasReceipt(id(op))).unwrap().0,
            ];
            assert_eq!(
                parts, [present; 3],
                "{context}: team {team:x} is there in part"
            );
            if matches!(first, Some(Ok(_))) {
                assert!(present, "{context}: acknowledged team {team:x} is gone");
            }
            let before = self.decided();
            let again = self.team(store, op, team);
            match (present, first) {
                (true, Some(Ok(first))) => {
                    assert_eq!(again.as_ref(), Ok(first), "{context}: a retry differs");
                    assert_eq!(self.decided(), before, "{context}: decided again");
                }
                // Appended, never answered: the retry is its first answer.
                (true, _) => {
                    assert!(again.is_ok(), "{context}: {again:?}");
                    assert_eq!(self.decided(), before, "{context}: decided again");
                }
                // Never appended: it decides afresh, once.
                (false, _) => {
                    assert!(again.is_ok(), "{context}: {again:?}");
                    assert_eq!(self.decided(), before + 1, "{context}");
                }
            }
        }
        let seen = directory(store);
        assert_eq!(seen.teams.len(), 3, "{context}");
        assert_eq!(seen.counter.map(|c| c.last_number), Some(3), "{context}");
        assert_eq!(seen.generation, 3, "{context}");
        store.sync().expect("files written");
        for (_, team) in TEAMS {
            assert!(self.team_file(team), "{context}: team {team:x} has no file");
        }
    }
}

#[test]
fn a_crash_at_every_point_reopens_to_teams_wholly_absent_or_wholly_present() {
    let mut idle = Vec::new();
    for engine in ENGINES {
        for point in CrashPoint::ALL {
            if point == CrashPoint::DuringRebuild {
                continue;
            }
            let context = format!("{engine:?} {point:?}");
            let (office, store) = Office::created(engine);
            office.place.faults.arm(point, 1);
            let mut first = Vec::new();
            for (op, team) in TEAMS {
                let dead = office.place.faults.dead();
                first.push((!dead).then(|| office.team(&store, op, team)));
            }
            if !office.place.faults.fired() {
                idle.push(context.clone());
            }
            let store = office.reopened(store);
            office.check(&store, &first, &context);
            store.close().expect("an orderly close");

            // A projection rebuilt from nothing equals the one that lived through it.
            let store = office.place.reopen(engine);
            let lived = store.dump();
            store.close().expect("an orderly close");
            let _ = std::fs::remove_dir_all(office.place.runtime());
            let store = office.place.reopen(engine);
            assert_eq!(store.dump(), lived, "{context}: rebuild differs");
            store.close().expect("an orderly close");
        }
    }
    assert!(idle.is_empty(), "armed points that never acted: {idle:?}");
}

#[test]
fn a_rebuild_that_dies_leaves_nothing_and_the_next_one_completes() {
    for engine in ENGINES {
        let (office, store) = Office::created(engine);
        office.team(&store, 2, 0x21).unwrap();
        store.sync().unwrap();
        let (dump, read) = (store.dump(), directory(&store));
        store.close().unwrap();
        let durable = tree(&office.place.durable());

        let _ = std::fs::remove_dir_all(office.place.runtime());
        office.place.faults.arm(CrashPoint::DuringRebuild, 1);
        let died = open_store(&office.place.config(engine, Opening::Existing));
        assert!(
            died.is_err(),
            "{engine:?}: a rebuild survived its crash point"
        );
        drop(died);
        office.place.faults.revive();
        let store = office.place.reopen(engine);
        assert_eq!(store.dump(), dump, "{engine:?}");
        assert_eq!(directory(&store), read, "{engine:?}");
        store.close().unwrap();
        assert_eq!(tree(&office.place.durable()), durable, "{engine:?}");
    }
}

/// STORE-3, part 2: a projection made from other DDL is replaced and rebuilt, and no
/// durable byte changes for it.
#[test]
fn another_schema_digest_rebuilds_the_projection_and_changes_no_journal_or_file_byte() {
    let (office, store) = Office::created(EngineChoice::Sqlite);
    office.team(&store, 2, 0x21).unwrap();
    store.sync().unwrap();
    let (dump, read) = (store.dump(), directory(&store));
    let digest = store.schema_digest();
    store.close().unwrap();
    let durable = tree(&office.place.durable());
    assert!(durable.keys().any(|path| path.ends_with(".log")));

    // Another build: this one's schema and one more index.
    let mut other = office.place.config(EngineChoice::Sqlite, Opening::Existing);
    other.testing.extra_ddl = Some(crate::store::sqlite::ANOTHER_BUILDS_DDL);
    let store = open_store(&other).expect("the store on the other schema");
    assert_ne!(store.schema_digest(), digest);
    assert_eq!(directory(&store), read);
    assert_eq!(store.dump(), dump);
    store.close().unwrap();
    assert_eq!(tree(&office.place.durable()), durable);

    // And back to this build's.
    let store = office.place.reopen(EngineChoice::Sqlite);
    assert_eq!(store.schema_digest(), digest);
    assert_eq!(store.dump(), dump);
    store.close().unwrap();
    assert_eq!(tree(&office.place.durable()), durable);
}

/// Releases held flushes when dropped, also while a failed assertion unwinds.
struct ReleaseFlushes<'a>(&'a org_store::testkit::Faults);

impl Drop for ReleaseFlushes<'_> {
    fn drop(&mut self) {
        self.0.release_flushes();
    }
}

/// STORE-4 on SQLite.
#[test]
fn on_sqlite_a_committed_write_is_answered_while_the_flush_is_held_and_a_team_is_not() {
    let (office, store) = Office::created(EngineChoice::Sqlite);
    let probe = &office.place.probe;
    store.sync().unwrap();
    let flushed = store.status().flushed;
    office.place.faults.hold_flushes();
    let put = Put {
        probe,
        name: "c",
        ack: Ack::Committed,
    };
    store
        .run(put)
        .expect("a committed answer with the flush held");

    let (answered, answer) = mpsc::channel();
    std::thread::scope(|scope| {
        // Released however this closure ends: a failed assertion below must not leave
        // the team's thread waiting on a held flush, or the scope would never join it
        // and the test would hang instead of failing.
        let held = ReleaseFlushes(&office.place.faults);
        let team = scope.spawn(|| {
            let reply = office.team(&store, 2, 0x21);
            let _ = answered.send(());
            reply
        });
        office.place.faults.wait_for_held_flush();
        // The flusher is inside the held flush: a team, durable by class, is not answered.
        assert!(answer.try_recv().is_err());
        assert_eq!(store.status().waiting_durable, 1);
        // Both are readable from their projection commit...
        let seen = directory(&store);
        assert!(seen.teams.iter().any(|team| team.id == id(0x21)));
        assert_eq!(seen.generation, 2);
        assert!(store.read(Source(probe.path("c"))).unwrap().0.is_some());
        // ...and no file exists before the flush that covers its entry.
        assert!(!office.team_file(0x21));
        assert_eq!(store.status().flushed, flushed);

        drop(held);
        let reply = team.join().expect("the submitting thread").expect("a team");
        let created: TeamCreated = serde_json::from_slice(&reply.bytes).unwrap();
        assert_eq!((created.number, created.generation), (2, 2));
    });
    store.sync().unwrap();
    assert!(office.team_file(0x21));
    store.close().unwrap();
}

/// STORE-5 on SQLite.
#[test]
fn on_sqlite_reads_answer_with_the_disk_failing_and_the_host_gone() {
    let (office, store) = Office::created(EngineChoice::Sqlite);
    let probe = &office.place.probe;
    office.team(&store, 2, 0x21).unwrap();
    let put = Put {
        probe,
        name: "a",
        ack: Ack::Committed,
    };
    store.run(put).unwrap();
    store.sync().unwrap();
    office.place.faults.fail_everything();
    office.place.custody.close();
    office.place.faults.clear_log();

    let (seen, feed) = store.read(ReadDirectory).unwrap();
    assert!(seen.teams.iter().any(|team| team.id == id(0x21)));
    assert_eq!(seen.generation, 2);
    assert_eq!(
        store.read(Source(probe.path("a"))).unwrap().0,
        Some(Sha256::of(b"a"))
    );
    assert_eq!(feed, org_store::FeedPosition(3));
    assert!(store.read(HasReceipt(id(2))).unwrap().0);
    // No read made a call to the store's disk.
    assert!(office.place.faults.log().is_empty());
}

/// STORE-8 on SQLite, the flush half. `org-store`'s own test of it also asserts, per
/// case, whether the durable request was already answered when the flush failed (its
/// `answered` column); this one cannot, because every operation of the organization
/// family is of the committed class and is answered before any flush.
#[test]
fn on_sqlite_a_flush_error_stops_the_store_and_reopening_holds_what_the_journal_holds() {
    let cases = [
        (FlushSite::Segment, 1),
        (FlushSite::File, 1),
        (FlushSite::Directory, 1),
        (FlushSite::Directory, 2),
    ];
    for (site, nth) in cases {
        let context = format!("{site:?} {nth}");
        let (office, store) = Office::created(EngineChoice::Sqlite);
        store.sync().unwrap();
        office.place.faults.arm(CrashPoint::FlushError(site), nth);
        let first = office.team(&store, 2, 0x21);
        // The flusher may still be on its way to the failing call when the team answers.
        let _ = store.sync();
        match store.status().stopped {
            Some(StoreError::Stopped { path: Some(_), .. }) => {}
            other => panic!("{context}: stopped with {other:?}"),
        }
        // It accepts nothing more, and reads continue: the entry was appended.
        assert!(office.team(&store, 3, 0x22).is_err(), "{context}");
        assert!(store.sync().is_err(), "{context}");
        let seen = directory(&store);
        assert!(
            seen.teams.iter().any(|team| team.id == id(0x21)),
            "{context}"
        );

        // The entry is in the journal on the drive, so the reopened office has it whole.
        let store = office.reopened(store);
        office.check(&store, &[Some(first), None], &context);
        store.close().unwrap();
    }
}

/// STORE-8 on SQLite, the append half.
#[test]
fn on_sqlite_a_failed_append_fails_its_batch_alone_and_a_failed_truncation_stops_the_store() {
    let created = |reply: Result<Reply, StoreError>| -> TeamCreated {
        serde_json::from_slice(&reply.expect("a team").bytes).expect("a team reply")
    };
    for point in [CrashPoint::AppendError, CrashPoint::ShortAppend] {
        let (office, store) = Office::created(EngineChoice::Sqlite);
        office.place.faults.arm(point, 1);
        let failed = office.team(&store, 2, 0x21);
        assert!(
            matches!(failed, Err(StoreError::Append(_))),
            "{point:?}: {failed:?}"
        );
        // Nothing of the batch is visible, and neither the number nor the position is kept.
        let seen = directory(&store);
        assert_eq!((seen.teams.len(), seen.generation), (1, 1), "{point:?}");
        assert!(!store.read(HasReceipt(id(2))).unwrap().0, "{point:?}");
        assert_eq!(store.status().stopped, None, "{point:?}");
        let next = created(office.team(&store, 3, 0x22));
        assert_eq!((next.number, next.generation), (2, 2), "{point:?}");
        // The same operation ID decides afresh: the failed batch left no receipt.
        let again = created(office.team(&store, 2, 0x21));
        assert_eq!((again.number, again.generation), (3, 3), "{point:?}");
        store.close().unwrap();
        let store = office.place.reopen(EngineChoice::Sqlite);
        assert_eq!(directory(&store).generation, 3, "{point:?}");
        store.close().unwrap();
    }

    let (office, store) = Office::created(EngineChoice::Sqlite);
    office.place.faults.arm(CrashPoint::TruncateError, 1);
    let failed = office.team(&store, 2, 0x21);
    assert!(matches!(failed, Err(StoreError::Append(_))), "{failed:?}");
    assert!(matches!(
        store.status().stopped,
        Some(StoreError::Journal(JournalError::Broken { .. }))
    ));
    assert!(office.team(&store, 3, 0x22).is_err());
    assert_eq!(directory(&store).teams.len(), 1);
}

// ---- a real SIGKILL, in a subprocess ----

const KILL_CHILD: &str = "store::open::tests::crashes::sigkill_child";
const KILL_ROOT: &str = "ORG_TEST_OFFICE_STORE_KILL_ROOT";
const KILL_POINT: &str = "ORG_TEST_OFFICE_STORE_KILL_POINT";

/// The three points a kill is tried at, by the name the child is given.
const KILL_POINTS: [(&str, CrashPoint); 3] = [
    ("during_apply", CrashPoint::DuringApply),
    ("after_append", CrashPoint::AfterAppend),
    ("after_committed_reply", CrashPoint::AfterCommittedReply),
];

/// The child of the test below; alone it does nothing. It creates an office on SQLite,
/// acknowledges one team, then sends itself SIGKILL from the writer's thread at the
/// selected point of a second team's creation and never goes past it.
#[test]
fn sigkill_child() {
    let (Some(root), Ok(selected)) = (std::env::var_os(KILL_ROOT), std::env::var(KILL_POINT))
    else {
        return;
    };
    let point = KILL_POINTS.iter().find(|(name, _)| *name == selected);
    let point = point.expect("a known kill point").1;
    let office = Office {
        place: Place::at(Path::new(&root)),
        engine: EngineChoice::Sqlite,
        decisions: Arc::new(AtomicUsize::new(0)),
    };
    let store = office.place.create(EngineChoice::Sqlite);
    create_organization(&store, 1).expect("an organization");
    office.team(&store, 2, 0x21).expect("an acknowledged team");
    office.place.faults.on_site_crash(|| {
        let me = std::process::id().to_string();
        let _ = std::process::Command::new("kill")
            .args(["-KILL", &me])
            .status();
        // The signal ends the process; this thread goes no further meanwhile.
        loop {
            std::thread::park();
        }
    });
    office.place.faults.arm(point, 1);
    let _ = office.team(&store, 3, 0x22);
    loop {
        std::thread::park();
    }
}

#[test]
fn a_real_sigkill_at_three_points_reopens_to_what_the_journal_holds() {
    use std::os::unix::process::ExitStatusExt;
    for (name, point) in KILL_POINTS {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([KILL_CHILD, "--exact", "--nocapture", "--test-threads=1"])
            .env(KILL_ROOT, dir.path())
            .env(KILL_POINT, name)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("the child process");
        // A child that never reaches its point is ended here, and the test fails.
        let (done, wait) = mpsc::channel();
        let pid = child.id().to_string();
        let waiter = std::thread::spawn(move || done.send(child.wait()));
        let status = wait.recv_timeout(std::time::Duration::from_secs(120));
        if status.is_err() {
            let _ = std::process::Command::new("kill")
                .args(["-KILL", &pid])
                .status();
        }
        let _ = waiter.join();
        let status = status
            .expect("the child reached its kill point")
            .expect("its status");
        assert_eq!(
            status.signal(),
            Some(9),
            "{name}: the child ended with {status:?}"
        );

        let office = Office {
            place: Place::at(dir.path()),
            engine: EngineChoice::Sqlite,
            decisions: Arc::new(AtomicUsize::new(0)),
        };
        let store = office.place.reopen(EngineChoice::Sqlite);
        let seen = directory(&store);
        let has = |team: u64| seen.teams.iter().any(|t| t.id == id(team));
        // The acknowledged team is there. The other is there exactly when its entry was
        // appended before the kill: the journal decides, not the projection.
        assert!(has(0x21), "{name}: the acknowledged team is gone");
        let appended = point != CrashPoint::DuringApply;
        assert_eq!(has(0x22), appended, "{name}");
        assert_eq!(seen.generation, if appended { 3 } else { 2 }, "{name}");
        assert_eq!(store.read(HasReceipt(id(3))).unwrap().0, appended, "{name}");
        office.check(&store, &[None, None], name);
        // The appended one answered from its receipt; the other decided now, once.
        assert_eq!(office.decided(), usize::from(!appended), "{name}");
        let lived = store.dump();
        store.close().expect("an orderly close");
        std::fs::remove_dir_all(office.place.runtime()).expect("the runtime folder");
        let store = office.place.reopen(EngineChoice::Sqlite);
        assert_eq!(store.dump(), lived, "{name}: rebuild differs");
        store.close().expect("an orderly close");
    }
}
