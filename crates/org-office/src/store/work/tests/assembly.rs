use super::super::tests::{fixture, reopen, task_fixture, T0};
use super::*;
use org_proto::{ApiError, ErrorCode};
use org_work::testkit::{MemOrg, TestClock};
use std::error::Error;

impl OrgSource for MemOrg {
    fn snapshot(&self) -> Result<Box<dyn OrgView + '_>, StoreError> {
        Ok(Box::new(self.clone()))
    }
}

fn setup() -> (
    tempfile::TempDir,
    Arc<Store>,
    OfficeWorkStore,
    Arc<TestClock>,
) {
    let (root, store) = fixture();
    let store = Arc::new(store);
    let wall = TestClock::at(T0);
    let now = wall.clone();
    let native = OfficeWorkStore::new(
        store.clone(),
        Arc::new(MemOrg::new("owner1")),
        Arc::new(move || now.now_ms()),
    )
    .unwrap();
    (root, store, native, wall)
}

fn noop() -> TxnOutput {
    TxnOutput::Refused(ApiError::new(ErrorCode::NotFound, "test no-op"))
}

fn saved() -> StoredResult {
    StoredResult {
        params_hash: [4; 32],
        answer: Err(ApiError::new(ErrorCode::NotFound, "stored refusal")),
        expires_at_ms: 0,
    }
}

fn call_error(error: &StoreError) -> &WorkCallError {
    error
        .source()
        .unwrap()
        .downcast_ref::<WorkCallError>()
        .unwrap()
}

#[test]
fn clock_commits_noops_and_refusals_but_rolls_back_errors_and_survives_reopen() {
    let (root, store, native, wall) = setup();
    let clock = native.clock();
    let outside = clock.clone();
    let moving = wall.clone();
    native
        .transact(Arc::new(move |_, txn| {
            assert_eq!(clock.now_ms(), T0);
            moving.set(T0 + 30);
            assert_eq!(clock.now_ms(), T0);
            let other = clock.clone();
            assert_eq!(
                std::thread::spawn(move || other.now_ms()).join().unwrap(),
                T0 + 30
            );
            txn.put_change_result("refusal", &saved())?;
            Ok(noop())
        }))
        .unwrap();
    assert_eq!(store.read(metadata::high_water).unwrap(), T0);
    native.transact(Arc::new(|_, _| Ok(noop()))).unwrap();
    assert_eq!(store.read(metadata::high_water).unwrap(), T0 + 30);
    wall.set(T0 - 1);
    assert_eq!(outside.now_ms(), T0 + 30);
    wall.set(T0 + 99);
    let error = native
        .transact(Arc::new(|_, txn| {
            txn.put_change_result("rollback", &saved())?;
            Err(StoreError::new("original port error"))
        }))
        .unwrap_err();
    assert_eq!(error.source().unwrap().to_string(), "original port error");
    assert!(error
        .source()
        .unwrap()
        .downcast_ref::<WorkCallError>()
        .is_none());
    assert_eq!(store.read(metadata::high_water).unwrap(), T0 + 30);
    drop(native);
    drop(store);
    let store = Arc::new(reopen(&root));
    assert!(store
        .read(|db| metadata::change_result(db, "rollback"))
        .unwrap()
        .is_none());
    let native = OfficeWorkStore::new(store, Arc::new(MemOrg::default()), Arc::new(|| 1)).unwrap();
    assert_eq!(native.clock().now_ms(), T0 + 30);
}

#[test]
fn allocation_and_result_gates_preserve_the_counter_and_typed_causes() {
    let (root, store, native, _) = setup();
    let error = native
        .transact(Arc::new(|_, txn| {
            assert_eq!(txn.allocate_number(&NumberScope::Requests)?, 1);
            assert!(matches!(
                call_error(&txn.put_change_result("early", &saved()).unwrap_err()),
                WorkCallError::ResultBeforeRecord
            ));
            Ok(noop())
        }))
        .unwrap_err();
    assert!(matches!(call_error(&error), WorkCallError::Unrecorded));
    native
        .transact(Arc::new(|_, txn| {
            txn.put_change_result("refusal", &saved())?;
            assert!(txn.change_result("refusal")?.is_some());
            assert!(matches!(
                call_error(&txn.put_change_result("again", &saved()).unwrap_err()),
                WorkCallError::AlreadyRecorded
            ));
            assert!(matches!(
                call_error(&txn.allocate_number(&NumberScope::Requests).unwrap_err()),
                WorkCallError::AlreadyRecorded
            ));
            let (item, events) = task_fixture(1, false);
            assert!(matches!(
                call_error(&txn.put_item(&item, &events).unwrap_err()),
                WorkCallError::AlreadyRecorded
            ));
            assert!(matches!(
                call_error(
                    &txn.record_change(Position::default(), &[], None)
                        .unwrap_err()
                ),
                WorkCallError::AlreadyRecorded
            ));
            Ok(noop())
        }))
        .unwrap();
    drop(native);
    drop(store);
    let store = reopen(&root);
    assert_eq!(
        store
            .read(|db| Ok(db
                .query_row("SELECT count(*) FROM work_number_counters", [], |r| r
                    .get::<_, i64>(0))
                .unwrap()))
            .unwrap(),
        0
    );
}

#[test]
fn panic_clears_attempt_and_recovers_a_deliberately_poisoned_clock_mutex() {
    let (_root, store, native, wall) = setup();
    native.transact(Arc::new(|_, _| Ok(noop()))).unwrap();
    wall.set(T0 + 100);
    let clock = native.clock.clone();
    assert!(native
        .transact(Arc::new(move |_, _| {
            let _poison = clock.state();
            panic!("test poison");
        }))
        .is_err());
    assert!(native.clock.state().attempt.is_none());
    assert_eq!(store.read(metadata::high_water).unwrap(), T0);
    wall.set(T0 - 1);
    let clock = native.clock();
    native
        .transact(Arc::new(move |_, _| {
            assert_eq!(clock.now_ms(), T0);
            Ok(noop())
        }))
        .unwrap();
}

use org_proto::work::{ItemKind, ItemStatus, LinkTarget};
use org_work::testkit::MemStore;
use org_work::{StoredComment, StoredLink};

fn refs(events: &[WorkEvent]) -> Vec<EventRef> {
    events
        .iter()
        .map(|e| EventRef {
            target_id: e.target_id.clone(),
            version: e.version,
        })
        .collect()
}

#[test]
fn sql_queries_match_memtxn_for_sixty_items_and_never_duplicate_live_links() {
    let (_root, store, native, _) = setup();
    super::super::tests::initialize_origin(&store);
    store
        .write(|db| {
            db.pragma_update(None, "reverse_unordered_selects", true)
                .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .unwrap();
    let memory = MemStore::new(MemOrg::default());
    let target = org_work::ulid_from_parts(T0, 601); // seed 60 is a deleted bug
    let mut rows = vec![];
    for n in (1..=60).rev() {
        let (mut item, events) = task_fixture(n, true);
        item.kind = match n % 3 {
            0 => ItemKind::Bug,
            1 => ItemKind::Task,
            _ => ItemKind::Request,
        };
        item.status = if n % 4 == 0 {
            ItemStatus::Closed
        } else {
            ItemStatus::New
        };
        item.deleted = n % 5 == 0;
        item.project_id = if n % 7 == 0 {
            None
        } else {
            Some(format!("project{}", n % 2))
        };
        item.checklist[0].removed = n % 2 == 0;
        item.links = (0..2)
            .map(|link| StoredLink {
                id: org_work::ulid_from_parts(T0, 100_000 + n * 10 + link),
                kind: if n % 2 == 0 {
                    LinkKind::Fixes
                } else {
                    LinkKind::RelatesTo
                },
                to: if n % 11 == 0 {
                    LinkTarget::Doc {
                        project_id: "project1".into(),
                        doc_key: "target".into(),
                    }
                } else {
                    LinkTarget::Item {
                        item_id: target.clone(),
                    }
                },
                created_by: "owner1".into(),
                created_at_ms: T0,
                removed: n % 13 == 0,
            })
            .collect();
        item.comments.push(StoredComment {
            id: org_work::ulid_from_parts(T0, 200_000 + n),
            kind: org_proto::work::CommentKind::Note,
            asks: None,
            answered: false,
        });
        rows.push((item, events));
    }
    let data = Arc::new(rows);
    for backend in [&native as &dyn WorkStore, &memory] {
        let data = data.clone();
        backend
            .transact(Arc::new(move |_, txn| {
                let mut all = vec![];
                for (item, events) in data.iter() {
                    txn.put_item(item, events)?;
                    all.extend(refs(events));
                }
                txn.record_change(Position::default(), &all, None)?;
                Ok(noop())
            }))
            .unwrap();
    }
    // A future field on a known doc target must not turn it into an item target.
    let extra = target.clone();
    store.write(move |db| {
        db.execute("UPDATE work_items SET data=json_set(data,'$.links[0].to.item_id',?1,'$.links[1].to.item_id',?1) WHERE json_extract(data,'$.links[0].to.type')='doc'", [&extra]).map_err(OfficeError::storage)?;
        Ok(((), None))
    }).unwrap();
    let query_results = Arc::new(Mutex::new(Vec::new()));
    let captured = query_results.clone();
    let queried = target.clone();
    memory
        .transact(Arc::new(move |_, txn| {
            let mut results = vec![];
            for kind in [
                LinkKind::Fixes,
                LinkKind::RelatesTo,
                LinkKind::PartOf,
                LinkKind::Unknown,
            ] {
                results.push(txn.linked_to(&queried, kind)?);
            }
            for project in [None, Some("project0"), Some("project1"), Some("absent")] {
                results.push(txn.open_bugs(project)?);
            }
            *captured.lock().unwrap() = results;
            Ok(noop())
        }))
        .unwrap();
    let query_results = query_results.lock().unwrap().clone();
    assert!(query_results[0].len() > 1);
    assert!(query_results[1].len() > 1);
    assert!(query_results[4].len() > 1);
    let expected = memory.snapshot();
    assert!(expected.items[&target].deleted);
    native
        .transact(Arc::new(move |_, txn| {
            for (id, item) in &expected.items {
                assert_eq!(txn.get(id)?, Some(item.clone()));
                assert!(txn.id_in_use(id)?);
                assert_eq!(txn.item_of_link(id)?, None);
                assert_eq!(txn.item_of_step(id)?, None);
                let step = &item.checklist[0].id;
                assert_eq!(txn.item_of_step(step)?, Some(item.clone()));
                assert_eq!(txn.item_of_link(step)?, None);
                assert!(txn.id_in_use(step)?);
                for link in &item.links {
                    assert_eq!(txn.item_of_link(&link.id)?, Some(item.clone()));
                    assert_eq!(txn.item_of_step(&link.id)?, None);
                    assert!(txn.id_in_use(&link.id)?);
                }
                assert!(txn.id_in_use(&item.comments[0].id)?);
                assert_eq!(txn.item_of_link(&item.comments[0].id)?, None);
                assert_eq!(txn.item_of_step(&item.comments[0].id)?, None);
                assert_eq!(txn.events_since(id, 0)?, expected.events[id]);
                assert!(txn.events_since(id, item.version)?.is_empty());
                assert!(txn.events_since(id, item.version + 1)?.is_empty());
            }
            assert!(!txn.id_in_use("absent")?);
            assert_eq!(txn.get("absent")?, None);
            for (n, kind) in [
                LinkKind::Fixes,
                LinkKind::RelatesTo,
                LinkKind::PartOf,
                LinkKind::Unknown,
            ]
            .into_iter()
            .enumerate()
            {
                assert_eq!(txn.linked_to(&target, kind)?, query_results[n]);
                assert!(txn.linked_to("absent", kind)?.is_empty());
            }
            for (n, project) in [None, Some("project0"), Some("project1"), Some("absent")]
                .into_iter()
                .enumerate()
            {
                assert_eq!(txn.open_bugs(project)?, query_results[n + 4]);
            }
            Ok(noop())
        }))
        .unwrap();
}

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

struct CountingOrg(AtomicUsize);
impl OrgSource for CountingOrg {
    fn snapshot(&self) -> Result<Box<dyn OrgView + '_>, StoreError> {
        let seq = self.0.fetch_add(1, Ordering::SeqCst) + 1;
        let mut org = MemOrg::new("owner1");
        org.position = Position {
            epoch: 4,
            seq: seq as u64,
        };
        Ok(Box::new(org))
    }
}

#[test]
fn production_job_replays_only_as_a_culprit_neighbour_with_fresh_org_clock_and_call() {
    for fail_first in [false, true] {
        let (_root, store, _, wall) = setup();
        super::super::tests::initialize_origin(&store);
        let org = Arc::new(CountingOrg(AtomicUsize::new(0)));
        let now = wall.clone();
        let native =
            OfficeWorkStore::new(store.clone(), org.clone(), Arc::new(move || now.now_ms()))
                .unwrap();
        let (started, ready) = mpsc::channel();
        let (release, held) = mpsc::channel();
        let (blocker, blocker_reply) = Store::job_with_batch_abort(
            move |_| {
                started.send(()).unwrap();
                held.recv_timeout(Duration::from_secs(10)).unwrap();
                Ok(((), None))
            },
            |_| false,
        );
        store.send_job(blocker).unwrap();
        ready.recv_timeout(Duration::from_secs(10)).unwrap();
        let attempts = Arc::new(AtomicUsize::new(0));
        let count = attempts.clone();
        let (item, events) = task_fixture(1, false);
        let clock = native.clock();
        let (job, answer) = native.transact_job(Arc::new(move |view, txn| {
            let n = count.fetch_add(1, Ordering::SeqCst);
            assert_eq!(
                view.applied_position()?,
                Position {
                    epoch: 4,
                    seq: n as u64 + 1
                }
            );
            assert_eq!(clock.now_ms(), T0 + n as u64 * 10);
            wall.set(T0 + 10);
            assert_eq!(clock.now_ms(), T0 + n as u64 * 10);
            if fail_first && n == 0 {
                return Err(StoreError::new(
                    "first attempt is discarded with its neighbour",
                ));
            }
            assert!(txn.get(&item.id)?.is_none());
            assert_eq!(txn.allocate_number(&NumberScope::Requests)?, 1);
            txn.put_item(&item, &events)?;
            txn.record_change(
                view.applied_position()?,
                &refs(&events),
                Some("replayed-job"),
            )?;
            txn.put_change_result("replayed-job", &saved())?;
            Ok(noop())
        }));
        store.send_job(job).unwrap();
        let (culprit, culprit_reply) = Store::job_with_batch_abort(
            |_| Err::<((), Option<crate::store::Undo>), _>(OfficeError::Invalid("culprit")),
            |_| true,
        );
        store.send_job(culprit).unwrap();
        release.send(()).unwrap();
        blocker_reply
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
            .unwrap();
        assert!(matches!(
            culprit_reply.recv_timeout(Duration::from_secs(10)).unwrap(),
            Err(OfficeError::Invalid("culprit"))
        ));
        answer.recv().unwrap();
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert_eq!(org.0.load(Ordering::SeqCst), 2);
        store
            .read(|db| {
                assert_eq!(
                    db.query_row("SELECT count(*) FROM work_feed", [], |r| r.get::<_, i64>(0))
                        .unwrap(),
                    1
                );
                assert_eq!(
                    db.query_row(
                        "SELECT min(org_seq), min(committed_at_ms), count(*) FROM work_origin_log",
                        [],
                        |r| Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, i64>(1)?,
                            r.get::<_, i64>(2)?
                        ))
                    )
                    .unwrap(),
                    (2, (T0 + 10) as i64, 1)
                );
                assert_eq!(metadata::high_water(db)?, T0 + 10);
                assert_eq!(
                    db.query_row(
                        "SELECT last FROM work_number_counters WHERE scope='request'",
                        [],
                        |r| r.get::<_, i64>(0)
                    )
                    .unwrap(),
                    1
                );
                Ok(())
            })
            .unwrap();
    }
}

#[test]
fn replies_received_out_of_order_never_lower_the_committed_clock() {
    let (_root, store, native, wall) = setup();
    let moving = wall.clone();
    let (first, earlier) = native.transact_job(Arc::new(move |_, _| {
        moving.set(T0 + 20);
        Ok(noop())
    }));
    store.send_job(first).unwrap();
    let (second, later) = native.transact_job(Arc::new(|_, _| Ok(noop())));
    store.send_job(second).unwrap();
    later.recv().unwrap();
    wall.set(0);
    earlier.recv().unwrap();
    assert_eq!(native.clock().now_ms(), T0 + 20);
}

#[test]
fn failed_allocations_and_result_writes_are_terminal_and_storage_cause_is_preserved() {
    for failure in ["allocation", "result", "put"] {
        let (_root, store, native, _) = setup();
        if failure == "allocation" {
            store
                .write(|db| {
                    db.execute(
                        "INSERT INTO work_number_counters VALUES ('request',?1)",
                        [i64::MAX],
                    )
                    .map_err(OfficeError::storage)?;
                    Ok(((), None))
                })
                .unwrap();
        } else if failure == "result" {
            store
                .write(|db| {
                    metadata::put_change_result(db, "conflict", &saved())?;
                    Ok(((), None))
                })
                .unwrap();
        }
        let error = native
            .transact(Arc::new(move |_, txn| {
                let error = match failure {
                    "allocation" => txn.allocate_number(&NumberScope::Requests).unwrap_err(),
                    "result" => txn.put_change_result("conflict", &saved()).unwrap_err(),
                    _ => {
                        let (item, _) = task_fixture(1, false);
                        txn.put_item(&item, &[]).unwrap_err()
                    }
                };
                assert!(matches!(call_error(&error), WorkCallError::Store(_)));
                assert!(call_error(&error)
                    .source()
                    .unwrap()
                    .downcast_ref::<OfficeError>()
                    .is_some());
                assert!(matches!(
                    call_error(
                        &txn.put_change_result("after-failure", &saved())
                            .unwrap_err()
                    ),
                    WorkCallError::CallFailed
                ));
                assert!(matches!(
                    call_error(&txn.allocate_number(&NumberScope::Requests).unwrap_err()),
                    WorkCallError::CallFailed
                ));
                let (item, events) = task_fixture(2, false);
                assert!(matches!(
                    call_error(&txn.put_item(&item, &events).unwrap_err()),
                    WorkCallError::CallFailed
                ));
                assert!(matches!(
                    call_error(
                        &txn.record_change(Position::default(), &[], None)
                            .unwrap_err()
                    ),
                    WorkCallError::CallFailed
                ));
                Ok(noop())
            }))
            .unwrap_err();
        assert!(matches!(call_error(&error), WorkCallError::CallFailed));
        assert_eq!(store.read(metadata::high_water).unwrap(), 0);
    }
}

use org_work::{Caller, WorkCall as Command, WorkService};
use serde_json::{json, Value};

struct Oracle {
    db: Arc<Store>,
    mem: Arc<MemStore>,
    org: Arc<Mutex<MemOrg>>,
    native: WorkService<Arc<OfficeWorkStore>>,
    reference: WorkService<Arc<MemStore>>,
    next: u128,
    _root: tempfile::TempDir,
}

impl OrgSource for Mutex<MemOrg> {
    fn snapshot(&self) -> Result<Box<dyn OrgView + '_>, StoreError> {
        Ok(Box::new(self.lock().unwrap().clone()))
    }
}

fn normalize_answer(answer: &mut org_proto::work::WorkAccepted) {
    if let Some(pos) = &mut answer.position {
        pos.incarnation = 1;
    }
}

fn normalize_output(mut output: TxnOutput) -> TxnOutput {
    match &mut output {
        TxnOutput::Committed { accepted, changed } => {
            normalize_answer(accepted);
            for part in changed {
                part.position.incarnation = 1;
            }
        }
        TxnOutput::Replayed(Ok(answer)) => normalize_answer(answer),
        _ => {}
    }
    output
}

impl Oracle {
    fn new() -> Self {
        let (root, store) = fixture();
        super::super::tests::initialize_origin(&store);
        let db = Arc::new(store);
        let mut org = MemOrg::new("owner1")
            .with_project("project1", "one", &[])
            .with_project("project2", "two", &[])
            .with_area("old-area", "project1", None, None);
        org.position = Position { epoch: 3, seq: 11 };
        let mem = Arc::new(MemStore::new(org.clone()));
        let org = Arc::new(Mutex::new(org));
        let native =
            Arc::new(OfficeWorkStore::new(db.clone(), org.clone(), Arc::new(|| T0)).unwrap());
        let clock = native.clock();
        Self {
            _root: root,
            db,
            org,
            mem: mem.clone(),
            native: WorkService::new(native, clock, "OFFICE1"),
            reference: WorkService::new(mem, TestClock::at(T0), "OFFICE1"),
            next: 400_000,
        }
    }
    fn id(&mut self) -> String {
        self.next += 1;
        org_work::ulid_from_parts(T0, self.next)
    }
    fn version(&self, id: &str) -> u64 {
        self.mem.snapshot().items[id].version
    }
    fn run(&mut self, method: &str, mut params: Value) -> (Command, TxnOutput) {
        params["change_id"] = json!(self.id());
        let command = Command::parse(&org_proto::Request {
            id: 1,
            method: method.into(),
            principal: None,
            params,
        })
        .unwrap()
        .unwrap();
        let out = self.command(Caller::app("owner1"), command.clone());
        (command, out)
    }
    fn command(&self, caller: Caller, command: Command) -> TxnOutput {
        let expected = self
            .reference
            .call(caller.clone(), command.clone())
            .unwrap();
        let actual = self.native.call(caller, command).unwrap();
        assert_eq!(normalize_output(actual.clone()), normalize_output(expected));
        self.compare();
        actual
    }
    fn ok(&mut self, method: &str, params: Value) -> Command {
        let (call, out) = self.run(method, params);
        assert!(
            matches!(out, TxnOutput::Committed { .. }),
            "{method}: {out:?}"
        );
        call
    }
    fn task(&mut self, project: &str) -> String {
        let id = self.id();
        self.ok(
            "task.create",
            json!({"item_id":id,"project_id":project,"title":"task"}),
        );
        id
    }
    fn bug(&mut self) -> String {
        let id = self.id();
        self.ok("bug.report", json!({"item_id":id,"project_id":"project1","title":"bug","what_happened":"failure", "severity":"major","area_id":"old-area"}));
        id
    }
    fn compare(&self) {
        let expected = self.mem.snapshot();
        self.db.read(|db| {
            let tx = db.unchecked_transaction().map_err(OfficeError::storage)?;
            let ids: Vec<String> = tx.prepare("SELECT id FROM work_items ORDER BY id").unwrap().query_map([], |r|r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
            assert_eq!(ids, expected.items.keys().cloned().collect::<Vec<_>>());
            for id in &ids {
                assert_eq!(super::super::get(&tx, id)?, Some(expected.items[id].clone()));
                let events: Vec<_> = super::super::event_records(&tx, id, 0)?.into_iter().map(|(_,e)|e).collect();
                assert_eq!(events, expected.events[id]);
            }
            let results: Vec<String> = tx.prepare("SELECT change_id FROM work_change_results ORDER BY change_id").unwrap().query_map([], |r|r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
            assert_eq!(results, expected.results.keys().cloned().collect::<Vec<_>>());
            for id in results {
                let mut actual = metadata::change_result(&tx, &id)?.unwrap();
                let mut want = expected.results[&id].clone();
                if let Ok(a) = &mut actual.answer { normalize_answer(a); }
                if let Ok(a) = &mut want.answer { normalize_answer(a); }
                assert_eq!(actual, want);
            }
            let numbers: std::collections::BTreeMap<String,u64> = tx.prepare("SELECT scope,last FROM work_number_counters ORDER BY scope").unwrap().query_map([],|r| Ok((r.get(0)?, r.get::<_,i64>(1)? as u64))).unwrap().collect::<rusqlite::Result<_>>().unwrap();
            assert_eq!(numbers, expected.numbers);
            let feeds: Vec<(u64, Option<String>, String)> = tx.prepare("SELECT seq,change_id,targets FROM work_feed ORDER BY seq").unwrap().query_map([], |r|Ok((r.get::<_,i64>(0)? as u64,r.get(1)?,r.get(2)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap();
            assert_eq!(feeds.len(), expected.feed.len());
            for ((seq, change, targets), want) in feeds.into_iter().zip(&expected.feed) {
                assert_eq!(seq, want.position.seq);
                assert_eq!(change, want.change_id);
                // Feed targets are unique item ids; origin rows retain every event ref.
                let target_ids: Vec<String> = serde_json::from_str(&targets).unwrap();
                let mut seen = std::collections::BTreeSet::new();
                let want_ids: Vec<_> = want.refs.iter().filter_map(|r| seen.insert(r.target_id.clone()).then_some(r.target_id.clone())).collect();
                assert_eq!(target_ids, want_ids);
                let origin: Vec<(String,u64,u64,u64,u64,u64)> = tx.prepare("SELECT target_id,generation,version,org_epoch,org_seq,committed_at_ms FROM work_origin_log WHERE feed_seq=?1 ORDER BY origin_seq").unwrap().query_map([seq as i64], |r| Ok((r.get(0)?,r.get::<_,i64>(1)? as u64,r.get::<_,i64>(2)? as u64,r.get::<_,i64>(3)? as u64,r.get::<_,i64>(4)? as u64,r.get::<_,i64>(5)? as u64))).unwrap().collect::<rusqlite::Result<_>>().unwrap();
                assert_eq!(origin.len(), want.refs.len());
                for (actual, reference) in origin.iter().zip(&want.refs) {
                    assert_eq!(actual, &(reference.target_id.clone(), expected.items[&reference.target_id].generation, reference.version, want.org_position.epoch, want.org_position.seq, T0));
                }
            }
            Ok(())
        }).unwrap();
    }
}

#[test]
fn workservice_scripts_match_memory_for_tasks_bugs_checklists_replay_refusals_and_cascades() {
    let mut w = Oracle::new();
    let task = w.task("project1");
    let second = w.task("project2");
    let bug = w.bug();
    assert_eq!(w.mem.snapshot().items[&task].number, Some(1));
    assert_eq!(w.mem.snapshot().items[&second].number, Some(1));
    let cmd = w.ok(
        "task.update",
        json!({"item_id":task,"expected_version":w.version(&task),"title":"updated task"}),
    );
    assert!(matches!(
        w.command(Caller::app("owner1"), cmd),
        TxnOutput::Replayed(Ok(_))
    ));
    w.ok(
        "bug.update",
        json!({"item_id":bug,"expected_version":w.version(&bug),"title":"updated bug"}),
    );
    let step = w.id();
    w.ok(
        "checklist.add",
        json!({"item_id":task,"steps":[{"step_id":step,"text":"check","required":true}]}),
    );
    w.ok(
        "checklist.edit",
        json!({"step_id":step,"expected_version":1,"text":"edited"}),
    );
    w.ok("checklist.check", json!({"step_id":step,"done":true}));
    let made = w.id();
    let part = w.id();
    w.ok(
        "checklist.make_task",
        json!({"step_id":step,"task_id":made,"link_id":part}),
    );
    w.ok("checklist.unlink_task", json!({"step_id":step}));
    let link = w.id();
    w.ok("work.link", json!({"link_id":link,"from_item_id":task,"kind":"fixes","to":{"type":"item","item_id":bug}}));
    w.ok("work.unlink", json!({"link_id":link}));
    let removed = w.mem.snapshot().items[&task].checklist[0].version;
    w.ok(
        "checklist.remove",
        json!({"step_id":step,"expected_version":removed}),
    );
    let comment = w.id();
    w.ok(
        "work.comment",
        json!({"item_id":task,"comment_id":comment,"body":"kept in event"}),
    );
    let absent = w.id();
    let (stored, out) = w.run(
        "task.update",
        json!({"item_id":absent,"expected_version":1,"title":"absent"}),
    );
    assert!(matches!(out, TxnOutput::Refused(_)));
    assert!(w.mem.snapshot().results.contains_key(stored.change_id()));
    assert!(matches!(
        w.command(Caller::app("owner1"), stored),
        TxnOutput::Replayed(Err(_))
    ));
    let delete = Command::Delete(org_proto::work::WorkDeleteParams {
        change_id: w.id(),
        item_id: task.clone(),
        expected_version: w.version(&task),
    });
    assert!(matches!(
        w.command(Caller::agent("unknown"), delete.clone()),
        TxnOutput::Refused(_)
    ));
    assert!(!w.mem.snapshot().results.contains_key(delete.change_id()));
    let closed = w.bug();
    let deleted = w.bug();
    w.ok("bug.close", json!({"item_id":closed,"expected_version":w.version(&closed),"resolution":"wont_fix","reason":"no"}));
    w.ok(
        "work.delete",
        json!({"item_id":deleted,"expected_version":w.version(&deleted)}),
    );
    w.org
        .lock()
        .unwrap()
        .delete_area("old-area", "project1-general");
    w.mem
        .org_mut(|org| org.delete_area("old-area", "project1-general"));
    let expected = w.reference.reconcile_bugs().unwrap();
    let actual = w.native.reconcile_bugs().unwrap();
    assert_eq!(normalize_output(actual), normalize_output(expected));
    w.compare();
    assert_eq!(
        w.mem.snapshot().items[&bug].bug.area_id.as_deref(),
        Some("project1-general")
    );
    assert_eq!(
        w.mem.snapshot().items[&closed].bug.area_id.as_deref(),
        Some("old-area")
    );
    assert_eq!(
        w.mem.snapshot().items[&deleted].bug.area_id.as_deref(),
        Some("old-area")
    );
    w.ok(
        "work.delete",
        json!({"item_id":task,"expected_version":w.version(&task)}),
    );
    w.ok(
        "work.delete",
        json!({"item_id":bug,"expected_version":w.version(&bug)}),
    );
}

#[test]
fn writing_result_refusal_allows_record_then_finalized_result_but_blocks_later_mutations() {
    let (_root, store, native, _) = setup();
    super::super::tests::initialize_origin(&store);
    native
        .transact(Arc::new(|_, txn| {
            let (item, events) = task_fixture(1, false);
            assert_eq!(txn.allocate_number(&NumberScope::Requests)?, 1);
            txn.put_item(&item, &events)?;
            assert!(matches!(
                call_error(&txn.put_change_result("early", &saved()).unwrap_err()),
                WorkCallError::ResultBeforeRecord
            ));
            txn.record_change(
                Position { epoch: 9, seq: 12 },
                &refs(&events),
                Some("final"),
            )?;
            assert!(matches!(
                call_error(&txn.allocate_number(&NumberScope::Requests).unwrap_err()),
                WorkCallError::AlreadyRecorded
            ));
            txn.put_change_result("final", &saved())?;
            assert_eq!(txn.get(&item.id)?, Some(item));
            assert_eq!(txn.events_since(&events[0].target_id, 0)?, events);
            assert!(txn.change_result("final")?.is_some());
            Ok(noop())
        }))
        .unwrap();
    assert!(store
        .read(|db| metadata::change_result(db, "early"))
        .unwrap()
        .is_none());
    assert!(store
        .read(|db| metadata::change_result(db, "final"))
        .unwrap()
        .is_some());
}

fn database_state(store: &Store) -> Vec<Vec<Vec<rusqlite::types::Value>>> {
    store
        .read(|db| {
            let tx = db.unchecked_transaction().map_err(OfficeError::storage)?;
            let mut all = vec![];
            for table in [
                "work_items",
                "work_ids",
                "work_events",
                "work_feed",
                "work_origin_log",
                "work_origin_counter",
                "work_number_counters",
                "work_change_results",
                "work_clock",
                "meta",
                "sqlite_sequence",
            ] {
                let mut query = tx
                    .prepare(&format!("SELECT * FROM {table} ORDER BY 1,2"))
                    .unwrap();
                let cols = query.column_count();
                all.push(
                    query
                        .query_map([], |r| {
                            (0..cols)
                                .map(|i| r.get(i))
                                .collect::<rusqlite::Result<Vec<_>>>()
                        })
                        .unwrap()
                        .collect::<rusqlite::Result<Vec<_>>>()
                        .unwrap(),
                );
            }
            Ok(all)
        })
        .unwrap()
}

#[derive(Debug)]
struct Original(Arc<()>);
impl std::fmt::Display for Original {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("original")
    }
}
impl Error for Original {}

#[test]
fn callback_error_after_all_writes_returns_original_cause_and_rolls_every_table_back_after_reopen()
{
    let (root, store, native, wall) = setup();
    super::super::tests::initialize_origin(&store);
    let before = database_state(&store);
    wall.set(T0 + 100);
    let identity = Arc::new(());
    let marker = identity.clone();
    let error = native
        .transact(Arc::new(move |view, txn| {
            let (item, events) = task_fixture(1, true);
            assert_eq!(txn.allocate_number(&NumberScope::Requests)?, 1);
            txn.put_item(&item, &events)?;
            txn.record_change(
                view.applied_position()?,
                &refs(&events),
                Some("rolled-back"),
            )?;
            txn.put_change_result("rolled-back", &saved())?;
            Err(StoreError::new(Original(marker.clone())))
        }))
        .unwrap_err();
    assert!(Arc::ptr_eq(
        &error
            .source()
            .unwrap()
            .downcast_ref::<Original>()
            .unwrap()
            .0,
        &identity
    ));
    assert_eq!(database_state(&store), before);
    drop(native);
    drop(store);
    assert_eq!(database_state(&reopen(&root)), before);
}

#[test]
fn construction_snapshot_and_setup_errors_are_returned_without_calling_the_body() {
    struct Broken;
    impl OrgSource for Broken {
        fn snapshot(&self) -> Result<Box<dyn OrgView + '_>, StoreError> {
            Err(StoreError::new("snapshot failed"))
        }
    }
    let (_root, store, _, _) = setup();
    let native = OfficeWorkStore::new(store.clone(), Arc::new(Broken), Arc::new(|| T0)).unwrap();
    let error = native
        .transact(Arc::new(|_, _| {
            panic!("snapshot failure must precede body")
        }))
        .unwrap_err();
    assert_eq!(error.source().unwrap().to_string(), "snapshot failed");
    assert_eq!(store.read(metadata::high_water).unwrap(), 0);
    let native =
        OfficeWorkStore::new(store.clone(), Arc::new(MemOrg::default()), Arc::new(|| T0)).unwrap();
    store
        .write(|db| {
            db.execute_batch("DROP TABLE work_clock")
                .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .unwrap();
    let error = native
        .transact(Arc::new(|_, _| panic!("clock failure must precede body")))
        .unwrap_err();
    assert!(matches!(
        call_error(&error),
        WorkCallError::Store(OfficeError::Storage(_))
    ));
    let error = match OfficeWorkStore::new(store, Arc::new(MemOrg::default()), Arc::new(|| T0)) {
        Err(e) => e,
        Ok(_) => panic!("missing clock table accepted"),
    };
    assert!(matches!(
        call_error(&error),
        WorkCallError::Store(OfficeError::Storage(_))
    ));
}

#[test]
fn clock_does_not_hold_its_lock_while_calling_wall_and_does_not_resample_during_an_attempt() {
    let (_root, store, _, _) = setup();
    let target = Arc::new(Mutex::new(None::<std::sync::Weak<OfficeClock>>));
    let probe = target.clone();
    let samples = Arc::new(AtomicUsize::new(0));
    let count = samples.clone();
    let native = OfficeWorkStore::new(
        store,
        Arc::new(MemOrg::default()),
        Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
            if let Some(clock) = probe
                .lock()
                .unwrap()
                .as_ref()
                .and_then(|weak| weak.upgrade())
            {
                assert!(
                    clock.state.try_lock().is_ok(),
                    "clock mutex held while calling wall"
                );
            }
            T0
        }),
    )
    .unwrap();
    *target.lock().unwrap() = Some(Arc::downgrade(&native.clock));
    assert_eq!(native.clock().now_ms(), T0);
    let clock = native.clock();
    let observed = samples.clone();
    native
        .transact(Arc::new(move |_, _| {
            let before = observed.load(Ordering::SeqCst);
            assert_eq!(clock.now_ms(), T0);
            assert_eq!(clock.now_ms(), T0);
            assert_eq!(observed.load(Ordering::SeqCst), before);
            Ok(noop())
        }))
        .unwrap();
    assert_eq!(samples.load(Ordering::SeqCst), 2);
}

struct HeldSource {
    org: Mutex<MemOrg>,
    alive: Arc<AtomicUsize>,
}
struct HeldView {
    org: MemOrg,
    alive: Arc<AtomicUsize>,
}
impl Drop for HeldView {
    fn drop(&mut self) {
        assert_eq!(self.alive.fetch_sub(1, Ordering::SeqCst), 1);
    }
}
impl OrgSource for HeldSource {
    fn snapshot(&self) -> Result<Box<dyn OrgView + '_>, StoreError> {
        assert_eq!(self.alive.fetch_add(1, Ordering::SeqCst), 0);
        Ok(Box::new(HeldView {
            org: self.org.lock().unwrap().clone(),
            alive: self.alive.clone(),
        }))
    }
}
macro_rules! view_queries {
    ($(fn $name:ident($($arg:ident: $ty:ty),*) -> $out:ty;)*) => {
        $(fn $name(&self, $($arg: $ty),*) -> Result<$out, StoreError> { self.org.$name($($arg),*) })*
    };
}
impl OrgView for HeldView {
    view_queries! {
        fn applied_position() -> Position;
        fn member(id: &str) -> Option<org_proto::org::Member>;
        fn owner_id() -> String;
        fn project(id: &str) -> Option<org_proto::org::Project>;
        fn project_by_key(key: &str) -> Option<org_proto::org::Project>;
        fn project_teams(project_id: &str) -> Vec<String>;
        fn area(id: &str) -> Option<org_proto::org::Area>;
        fn general_area(project_id: &str) -> Option<String>;
        fn team_lead(team_id: &str) -> Option<String>;
    }
}

#[test]
fn org_snapshot_is_held_and_stable_through_success_error_and_panic() {
    let (_root, store, _, _) = setup();
    let source = Arc::new(HeldSource {
        org: Mutex::new(MemOrg::new("owner1")),
        alive: Arc::new(AtomicUsize::new(0)),
    });
    let native = OfficeWorkStore::new(store, source.clone(), Arc::new(|| T0)).unwrap();
    for outcome in ["ok", "error", "panic"] {
        let observed = source.clone();
        let result = native.transact(Arc::new(move |view, _| {
            assert_eq!(observed.alive.load(Ordering::SeqCst), 1);
            let before = view.applied_position()?;
            observed.org.lock().unwrap().position.seq += 1;
            assert_eq!(view.applied_position()?, before);
            match outcome {
                "error" => Err(StoreError::new("test")),
                "panic" => panic!("test snapshot drop"),
                _ => Ok(noop()),
            }
        }));
        assert_eq!(result.is_ok(), outcome == "ok");
        assert_eq!(source.alive.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn high_water_write_failure_rolls_back_a_finished_call_and_preserves_storage_cause() {
    let (root, store, native, wall) = setup();
    super::super::tests::initialize_origin(&store);
    native.transact(Arc::new(|_, _| Ok(noop()))).unwrap();
    store.write(|db| {
        db.execute_batch("CREATE TRIGGER refuse_clock BEFORE UPDATE ON work_clock BEGIN SELECT RAISE(ABORT, 'injected clock write'); END;").map_err(OfficeError::storage)?;
        Ok(((), None))
    }).unwrap();
    let before = database_state(&store);
    wall.set(T0 + 100);
    let reached = Arc::new(AtomicUsize::new(0));
    let marker = reached.clone();
    let error = native
        .transact(Arc::new(move |view, txn| {
            let (item, events) = task_fixture(1, false);
            txn.allocate_number(&NumberScope::Requests)?;
            txn.put_item(&item, &events)?;
            txn.record_change(
                view.applied_position()?,
                &refs(&events),
                Some("clock-write-failed"),
            )?;
            txn.put_change_result("clock-write-failed", &saved())?;
            marker.fetch_add(1, Ordering::SeqCst);
            Ok(noop())
        }))
        .unwrap_err();
    assert_eq!(reached.load(Ordering::SeqCst), 1);
    let WorkCallError::Store(OfficeError::Storage(cause)) = call_error(&error) else {
        panic!("expected typed storage failure")
    };
    assert!(cause.downcast_ref::<rusqlite::Error>().is_some());
    assert_eq!(database_state(&store), before);
    drop(native);
    drop(store);
    assert_eq!(database_state(&reopen(&root)), before);
}

#[test]
fn stopped_writer_and_disconnected_job_reply_keep_the_workcall_storage_wrapper() {
    let (_root, mut store) = fixture();
    drop(store.jobs.take());
    let native = OfficeWorkStore::new(
        Arc::new(store),
        Arc::new(MemOrg::default()),
        Arc::new(|| T0),
    )
    .unwrap();
    let error = native
        .transact(Arc::new(|_, _| panic!("stopped writer ran callback")))
        .unwrap_err();
    assert!(matches!(
        call_error(&error),
        WorkCallError::Store(OfficeError::Storage(_))
    ));
    let (job, answer) = native.transact_job(Arc::new(|_, _| panic!("unsent job ran callback")));
    drop(job);
    let error = answer.recv().unwrap_err();
    assert!(matches!(
        call_error(&error),
        WorkCallError::Store(OfficeError::Storage(_))
    ));
}

#[test]
fn request_fixture_create_update_delete_and_global_numbering_match_native_ports() {
    // Request commands belong to a later domain phase; exercise its existing stored shape.
    let w = Oracle::new();
    let (mut item, mut events) = task_fixture(90, false);
    item.kind = ItemKind::Request;
    item.project_id = None;
    item.number = Some(1);
    for version in 1..=3 {
        if version > 1 {
            item.version += 1;
            item.title = "updated request".into();
            item.deleted = version == 3;
            let mut event = events[0].clone();
            event.version = item.version;
            event.change = org_proto::work::WorkEventKind::FieldsChanged {
                fields: vec![org_proto::work::FieldChange {
                    field: "title".into(),
                    from: Some(json!("Persisted task")),
                    to: Some(json!("updated request")),
                }],
            };
            events = vec![event];
        }
        for backend in [w.native.store().as_ref() as &dyn WorkStore, w.mem.as_ref()] {
            let item = item.clone();
            let events = events.clone();
            backend
                .transact(Arc::new(move |view, txn| {
                    if version == 1 {
                        assert_eq!(txn.allocate_number(&NumberScope::Requests)?, 1);
                    }
                    txn.put_item(&item, &events)?;
                    txn.record_change(view.applied_position()?, &refs(&events), None)?;
                    Ok(noop())
                }))
                .unwrap();
        }
        w.compare();
    }
    assert!(w.mem.snapshot().items[&item.id].deleted);
    assert_eq!(w.mem.snapshot().items[&item.id].version, 3);
}

#[test]
#[cfg(debug_assertions)]
fn future_item_timestamp_trips_the_adapter_assertion_and_rolls_back() {
    let (_root, store, native, _) = setup();
    super::super::tests::initialize_origin(&store);
    let before = database_state(&store);
    let error = native
        .transact(Arc::new(|view, txn| {
            let (mut item, events) = task_fixture(1, false);
            item.updated_at_ms = T0 + 1;
            txn.put_item(&item, &events)?;
            txn.record_change(view.applied_position()?, &refs(&events), None)?;
            Ok(noop())
        }))
        .unwrap_err();
    assert!(matches!(
        call_error(&error),
        WorkCallError::Store(OfficeError::Storage(_))
    ));
    assert_eq!(database_state(&store), before);
}

#[test]
fn read_ports_keep_typed_sql_failures_instead_of_absence_or_an_empty_query() {
    for table in [
        "work_items",
        "work_ids",
        "work_events",
        "work_change_results",
    ] {
        let (_root, store, native, _) = setup();
        store
            .write(move |db| {
                db.execute_batch(&format!("DROP TABLE {table}"))
                    .map_err(OfficeError::storage)?;
                Ok(((), None))
            })
            .unwrap();
        let error = native
            .transact(Arc::new(move |_, txn| {
                let errors = match table {
                    "work_items" => vec![
                        txn.get("absent").unwrap_err(),
                        txn.linked_to("target", LinkKind::Fixes).unwrap_err(),
                        txn.open_bugs(None).unwrap_err(),
                    ],
                    "work_ids" => vec![
                        txn.id_in_use("absent").unwrap_err(),
                        txn.item_of_step("absent").unwrap_err(),
                        txn.item_of_link("absent").unwrap_err(),
                    ],
                    "work_events" => vec![txn.events_since("absent", 0).unwrap_err()],
                    _ => vec![txn.change_result("absent").unwrap_err()],
                };
                for error in errors {
                    let WorkCallError::Store(OfficeError::Storage(cause)) = call_error(&error)
                    else {
                        panic!("read lost SQL cause")
                    };
                    assert!(cause.downcast_ref::<rusqlite::Error>().is_some());
                }
                Err(StoreError::new("finish read failure"))
            }))
            .unwrap_err();
        assert_eq!(error.source().unwrap().to_string(), "finish read failure");
        assert_eq!(store.read(metadata::high_water).unwrap(), 0);
    }
}

#[test]
fn select_helper_preserves_prepare_row_decode_and_missing_selected_item_failures() {
    let (_root, store, _, _) = setup();
    for sql in ["SELECT id FROM missing", "SELECT 1", "SELECT 'absent'"] {
        let error = call::transact(&store, move |call| {
            NativeTxn { call, served: T0 }
                .select(sql, [])
                .map_err(WorkCallError::Port)
        })
        .unwrap_err();
        let error = port(error);
        if sql == "SELECT 'absent'" {
            assert!(matches!(
                call_error(&error),
                WorkCallError::Store(OfficeError::Invalid("selected work item is missing"))
            ));
        } else {
            let WorkCallError::Store(OfficeError::Storage(cause)) = call_error(&error) else {
                panic!("selected row lost SQL cause")
            };
            assert!(cause.downcast_ref::<rusqlite::Error>().is_some());
        }
    }
}

fn compare_unknown_row_queries(status: Value, link_kind: Value) {
    let (_root, store, native, _) = setup();
    super::super::tests::initialize_origin(&store);
    let memory = MemStore::new(MemOrg::default());
    let (mut item, events) = task_fixture(1, false);
    item.kind = ItemKind::Bug;
    item.status = ItemStatus::Unknown;
    item.links.push(StoredLink {
        id: org_work::ulid_from_parts(T0, 999),
        kind: LinkKind::Unknown,
        to: LinkTarget::Item {
            item_id: "target".into(),
        },
        created_by: "owner1".into(),
        created_at_ms: T0,
        removed: false,
    });
    for backend in [&native as &dyn WorkStore, &memory] {
        let item = item.clone();
        let events = events.clone();
        backend
            .transact(Arc::new(move |_, txn| {
                txn.put_item(&item, &events)?;
                txn.record_change(Position::default(), &refs(&events), None)?;
                Ok(noop())
            }))
            .unwrap();
    }
    store.write(move |db| {
        db.execute("UPDATE work_items SET data=json_set(data,'$.status',json(?1),'$.links[0].kind',json(?2))", params![status.to_string(),link_kind.to_string()]).map_err(OfficeError::storage)?;
        Ok(((), None))
    }).unwrap();
    let results = Arc::new(Mutex::new(Vec::new()));
    let captured = results.clone();
    memory
        .transact(Arc::new(move |_, txn| {
            *captured.lock().unwrap() = vec![
                txn.open_bugs(None)?,
                txn.linked_to("target", LinkKind::Unknown)?,
            ];
            Ok(noop())
        }))
        .unwrap();
    let results = results.lock().unwrap().clone();
    assert_eq!(results, vec![vec![item.clone()], vec![item.clone()]]);
    native
        .transact(Arc::new(move |_, txn| {
            assert_eq!(
                txn.get(&item.id)?,
                Some(item.clone()),
                "typed reader must recognize Unknown"
            );
            assert_eq!(
                txn.open_bugs(None)?,
                results[0],
                "SQL status filtering must match the typed reader"
            );
            assert_eq!(
                txn.linked_to("target", LinkKind::Unknown)?,
                results[1],
                "SQL kind filtering must match the typed reader"
            );
            Ok(noop())
        }))
        .unwrap();
}

#[test]
fn open_bug_sql_matches_typed_unknown_status_for_every_json_shape() {
    for status in [
        Value::Null,
        json!("future_status"),
        json!({}),
        json!([]),
        json!(17),
        json!(true),
    ] {
        compare_unknown_row_queries(status, json!("unknown"));
    }
}

#[test]
fn linked_sql_matches_typed_unknown_kind_for_every_json_shape() {
    for kind in [
        json!("future_link"),
        Value::Null,
        json!({}),
        json!([]),
        json!(17),
        json!(true),
    ] {
        compare_unknown_row_queries(json!("unknown"), kind);
    }
}

#[test]
fn sql_known_link_kind_constant_tracks_every_declared_serde_variant() {
    // Discover the declared wire inventory too, so adding a future macro variant makes
    // this fail until the SQL constant is updated; a hand-written test array would miss it.
    let source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../org-proto/src/work.rs"
    ));
    let body = source
        .split_once("LinkKind {")
        .unwrap()
        .1
        .split_once('}')
        .unwrap()
        .0;
    let declared: Vec<Value> = body
        .lines()
        .filter_map(|line| {
            line.split_once(" = ")
                .map(|(_, wire)| serde_json::from_str(wire.trim().trim_end_matches(',')).unwrap())
        })
        .collect();
    assert!(!declared.is_empty());
    let known: Vec<Value> = serde_json::from_str(LINK_KINDS).unwrap();
    assert_eq!(known, declared);
    assert!(!known.contains(&json!("unknown")));
    for wire in known {
        let kind: LinkKind = serde_json::from_value(wire.clone()).unwrap();
        assert_ne!(kind, LinkKind::Unknown);
        assert_eq!(serde_json::to_value(kind).unwrap(), wire);
    }
}
