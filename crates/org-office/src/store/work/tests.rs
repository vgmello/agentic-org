use super::{event_records, get, owner_of, put_item};
use crate::office::OFFICE_SCHEMA;
use crate::store::origin;
use crate::store::{Opening, Schema, Store};
use crate::Key;

mod call;
mod keys;
mod metadata;
use crate::OfficeError;
use org_proto::work::{CommentKind, FieldChange, LinkKind, LinkTarget, WorkEventKind};
use org_proto::work::{NewStep, TaskCreateParams, WorkEvent};
use org_work::testkit::{MemOrg, MemStore, TestClock};
use org_work::{Caller, StoredItem, TxnOutput, WorkCall, WorkService};
use org_work::{StoredComment, StoredLink};
use rusqlite::{params, types::Value, Connection};
use std::sync::Arc;

pub(super) fn fixture() -> (tempfile::TempDir, Store) {
    fixture_for_schema(&OFFICE_SCHEMA)
}

fn fixture_for_schema(schema: &Schema) -> (tempfile::TempDir, Store) {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(
        &root.path().join("office.db"),
        &Key::from_bytes([42; 32]),
        Opening::New,
        schema,
    )
    .unwrap();
    (root, store)
}

#[test]
fn native_work_schema_supports_item_event_and_id_storage() {
    let (_root, store) = fixture();
    store.read(|db| {
        let tables: Vec<String> = db.prepare("SELECT name FROM sqlite_master WHERE name IN ('work_items','work_events','work_ids') ORDER BY name").unwrap()
            .query_map([], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
        assert_eq!(tables, ["work_events", "work_ids", "work_items"]);
        Ok(())
    }).unwrap();
    let (item, events) = task_fixture(1, false);
    assert_eq!(item.version, 1);
    assert_eq!(events.len(), 1);
}

pub(super) const T0: u64 = 1_793_000_000_000;

fn core_task(seed: u128, with_step: bool) -> (Arc<MemStore>, WorkService<Arc<MemStore>>, String) {
    let id = org_work::ulid_from_parts(T0, seed * 10 + 1);
    let store = Arc::new(MemStore::new(MemOrg::new("owner1").with_project(
        "project1",
        "tst",
        &[],
    )));
    let service = WorkService::new(Arc::clone(&store), TestClock::at(T0), "OFFICE1");
    let output = service
        .call(
            Caller::app("owner1"),
            WorkCall::TaskCreate(TaskCreateParams {
                change_id: org_work::ulid_from_parts(T0, seed * 10),
                item_id: id.clone(),
                project_id: "project1".into(),
                title: "Persisted task".into(),
                description: None,
                definition_of_done: None,
                assignee_id: None,
                team_id: None,
                priority: None,
                feature_id: None,
                fixes: vec![],
                checklist: if with_step {
                    vec![NewStep {
                        step_id: org_work::ulid_from_parts(T0, seed * 10 + 2),
                        text: "Retained step".into(),
                        required: true,
                    }]
                } else {
                    vec![]
                },
            }),
        )
        .unwrap();
    assert!(matches!(output, TxnOutput::Committed { .. }));
    (store, service, id)
}

pub(super) fn task_fixture(seed: u128, with_step: bool) -> (StoredItem, Vec<WorkEvent>) {
    let (store, _, id) = core_task(seed, with_step);
    let mut saved = store.snapshot();
    let mut item = saved.items.remove(&id).unwrap();
    // Each source MemStore starts at T-1; native multi-item fixtures need distinct keys.
    // Reserve 1 and 2 for history(), which creates both tasks in one source store.
    item.number = Some(u64::try_from(seed).unwrap().checked_mul(10).unwrap());
    let events = saved.events.remove(&id).unwrap();
    assert!(!events.is_empty());
    assert_eq!(events[0].version, 1);
    assert_eq!(events.last().unwrap().version, item.version);
    (item, events)
}

pub(super) fn reopen(root: &tempfile::TempDir) -> Store {
    Store::open(
        &root.path().join("office.db"),
        &Key::from_bytes([42; 32]),
        Opening::Existing,
        &OFFICE_SCHEMA,
    )
    .unwrap()
}

pub(super) fn initialize_origin(store: &Store) {
    store
        .write(|db| {
            db.execute("INSERT INTO meta VALUES ('office_id','OFFICE1')", [])
                .map_err(OfficeError::storage)?;
            db.execute(
                "INSERT INTO meta VALUES ('incarnation',?1)",
                [org_work::ulid_from_parts(T0, 1)],
            )
            .map_err(OfficeError::storage)?;
            db.execute("INSERT INTO meta VALUES ('message_sequence','41')", [])
                .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .unwrap();
}

fn feed(db: &Connection, events: &[WorkEvent]) -> Result<u64, OfficeError> {
    let refs: Vec<_> = events
        .iter()
        .map(|e| org_proto::work::EventRef {
            target_id: e.target_id.clone(),
            version: e.version,
        })
        .collect();
    db.execute(
        "INSERT INTO work_feed(targets) VALUES (?1)",
        [serde_json::to_string(&refs).unwrap()],
    )
    .map_err(OfficeError::storage)?;
    Ok(db.last_insert_rowid() as u64)
}

fn write(store: &Store, item: StoredItem, events: Vec<WorkEvent>) -> Result<u64, OfficeError> {
    store.write(move |db| {
        let seq = feed(db, &events)?;
        put_item(db, &item, &events, seq)?;
        let refs: Vec<_> = events
            .iter()
            .map(|e| origin::OriginRef {
                target_id: e.target_id.clone(),
                generation: item.generation,
                version: e.version,
            })
            .collect();
        origin::append(db, org_proto::Position { epoch: 0, seq: 0 }, &refs, seq, T0)?;
        Ok((seq, None))
    })
}

fn read(store: &Store, id: &str) -> Option<StoredItem> {
    store
        .read(|db| {
            db.pragma_update(None, "reverse_unordered_selects", true)
                .map_err(OfficeError::storage)?;
            let tx = db.unchecked_transaction().map_err(OfficeError::storage)?;
            get(&tx, id)
        })
        .unwrap()
}

fn snapshot(store: &Store) -> Vec<Vec<Vec<Value>>> {
    store.read(|db| {
        let tx = db.unchecked_transaction().map_err(OfficeError::storage)?;
        let mut result = vec![];
        for sql in [
            "SELECT * FROM work_items ORDER BY id", "SELECT * FROM work_ids ORDER BY id",
            "SELECT * FROM work_events ORDER BY target_id,version", "SELECT * FROM work_feed ORDER BY seq",
            "SELECT * FROM work_origin_log ORDER BY origin_office_id,origin_incarnation,origin_seq",
            "SELECT * FROM work_origin_counter ORDER BY origin_incarnation",
            "SELECT * FROM sqlite_sequence ORDER BY name", "SELECT * FROM meta ORDER BY key",
        ] {
            let mut query = tx.prepare(sql).map_err(OfficeError::storage)?;
            let cols = query.column_count();
            let rows = query.query_map([], |row| (0..cols).map(|n|row.get(n)).collect::<rusqlite::Result<Vec<Value>>>())
                .map_err(OfficeError::storage)?.collect::<rusqlite::Result<Vec<_>>>().map_err(OfficeError::storage)?;
            result.push(rows);
        }
        Ok(result)
    }).unwrap()
}

fn advance(item: &StoredItem, previous: &WorkEvent) -> (StoredItem, WorkEvent) {
    let mut item = item.clone();
    let old = item.title.clone();
    item.title = "Updated task".into();
    item.version += 1;
    let mut event = previous.clone();
    event.version = item.version;
    event.change = WorkEventKind::FieldsChanged {
        fields: vec![FieldChange {
            field: "title".into(),
            from: Some(old.into()),
            to: Some(item.title.clone().into()),
        }],
    };
    (item, event)
}

struct History {
    states: Vec<(StoredItem, Vec<WorkEvent>)>,
    source: String,
}

fn history() -> History {
    use org_proto::work::{
        ChecklistRemoveParams, WorkCommentParams, WorkDeleteParams, WorkLinkParams,
        WorkUnlinkParams,
    };
    let (store, service, source) = core_task(1, true);
    let mut states = vec![];
    let capture = |id: &str, states: &mut Vec<(StoredItem, Vec<WorkEvent>)>| {
        let data = store.snapshot();
        let item = data.items[id].clone();
        let previous = states
            .iter()
            .rev()
            .find(|(i, _)| i.id == id)
            .map_or(0, |(i, _)| i.version);
        let events = data.events[id]
            .iter()
            .filter(|e| e.version > previous)
            .cloned()
            .collect::<Vec<_>>();
        assert!(!events.is_empty());
        states.push((item, events));
    };
    capture(&source, &mut states);
    let target = org_work::ulid_from_parts(T0, 19);
    let params: TaskCreateParams = serde_json::from_value(serde_json::json!({
        "change_id": org_work::ulid_from_parts(T0,18), "item_id":target,
        "project_id":"project1", "title":"Related task"
    }))
    .unwrap();
    let calls = [
        WorkCall::TaskCreate(params),
        WorkCall::Comment(WorkCommentParams {
            change_id: org_work::ulid_from_parts(T0, 13),
            item_id: source.clone(),
            comment_id: org_work::ulid_from_parts(T0, 14),
            body: "Immutable comment".into(),
            comment_kind: None,
            asks: None,
            answers: None,
            watch: false,
        }),
        WorkCall::Link(WorkLinkParams {
            change_id: org_work::ulid_from_parts(T0, 15),
            link_id: org_work::ulid_from_parts(T0, 16),
            from_item_id: source.clone(),
            kind: LinkKind::RelatesTo,
            to: LinkTarget::Item {
                item_id: target.clone(),
            },
        }),
    ];
    for (n, call) in calls.into_iter().enumerate() {
        assert!(matches!(
            service.call(Caller::app("owner1"), call).unwrap(),
            TxnOutput::Committed { .. }
        ));
        capture(if n == 0 { &target } else { &source }, &mut states);
    }
    let current = store.snapshot().items[&source].clone();
    let step = &current.checklist[0];
    let remove = WorkCall::ChecklistRemove(ChecklistRemoveParams {
        change_id: org_work::ulid_from_parts(T0, 30),
        step_id: step.id.clone(),
        expected_version: step.version,
    });
    let unlink = WorkCall::Unlink(WorkUnlinkParams {
        change_id: org_work::ulid_from_parts(T0, 31),
        link_id: current.links[0].id.clone(),
    });
    for call in [remove, unlink] {
        assert!(matches!(
            service.call(Caller::app("owner1"), call).unwrap(),
            TxnOutput::Committed { .. }
        ));
        capture(&source, &mut states);
    }
    let delete = WorkCall::Delete(WorkDeleteParams {
        change_id: org_work::ulid_from_parts(T0, 32),
        item_id: source.clone(),
        expected_version: store.snapshot().items[&source].version,
    });
    assert!(matches!(
        service.call(Caller::app("owner1"), delete).unwrap(),
        TxnOutput::Committed { .. }
    ));
    capture(&source, &mut states);
    History { states, source }
}

#[test]
fn native_work_item_history_feed_and_id_claims_survive_reopen() {
    let (root, store) = fixture();
    initialize_origin(&store);
    let history = history();
    let mut expected = vec![];
    for (n, (item, events)) in history.states.iter().enumerate() {
        let seq = n as u64 + 1;
        assert_eq!(write(&store, item.clone(), events.clone()).unwrap(), seq);
        assert_eq!(read(&store, &item.id), Some(item.clone()));
        if item.id == history.source {
            expected.extend(events.iter().cloned().map(|e| (seq, e)));
        }
    }
    let final_item = history.states.last().unwrap().0.clone();
    assert!(final_item.deleted);
    assert!(final_item.checklist[0].removed);
    assert!(final_item.links[0].removed);
    assert_eq!(final_item.comments.len(), 1);
    store
        .read(|db| {
            db.pragma_update(None, "reverse_unordered_selects", true)
                .map_err(OfficeError::storage)?;
            for (id, kind) in [
                (&final_item.id, "item"),
                (&final_item.checklist[0].id, "step"),
                (&final_item.links[0].id, "link"),
                (&final_item.comments[0].id, "comment"),
            ] {
                assert_eq!(
                    owner_of(db, id)?,
                    Some((final_item.id.clone(), kind.into()))
                );
            }
            assert_eq!(owner_of(db, "missing")?, None);
            assert!(!expected.is_empty());
            assert_eq!(event_records(db, &history.source, 0)?, expected);
            let suffix = expected
                .iter()
                .filter(|(_, e)| e.version > 1)
                .cloned()
                .collect::<Vec<_>>();
            assert!(!suffix.is_empty());
            assert_eq!(event_records(db, &history.source, 1)?, suffix);
            assert!(event_records(db, &history.source, final_item.version)?.is_empty());
            Ok(())
        })
        .unwrap();
    let before = snapshot(&store);
    drop(store);
    let store = reopen(&root);
    assert_eq!(snapshot(&store), before);
    assert_eq!(read(&store, &final_item.id), Some(final_item));
}

#[test]
fn native_work_requires_transactions_and_existing_positive_feed() {
    let (_root, store) = fixture();
    initialize_origin(&store);
    let (item, events) = task_fixture(1, false);
    let expected = snapshot(&store);
    let copy = item.clone();
    let copied = events.clone();
    let error = store
        .read(move |db| put_item(db, &copy, &copied, 1))
        .unwrap_err();
    assert!(matches!(
        error,
        OfficeError::Invalid("work item write requires a writer transaction")
    ));
    let error = store.read(|db| get(db, &item.id)).unwrap_err();
    assert!(matches!(
        error,
        OfficeError::Invalid("work item read requires a transaction")
    ));
    for seq in [0, u64::MAX, 999] {
        let copy = item.clone();
        let copied = events.clone();
        let error = store
            .write(move |db| {
                put_item(db, &copy, &copied, seq)?;
                Ok(((), None))
            })
            .unwrap_err();
        match seq {
            0 => assert!(matches!(
                error,
                OfficeError::Invalid("work item write requires a feed row")
            )),
            u64::MAX => assert!(matches!(
                error,
                OfficeError::Invalid("work integer is out of range")
            )),
            _ => assert!(matches!(error, OfficeError::Storage(_))),
        }
        assert_eq!(snapshot(&store), expected);
    }
    assert_eq!(read(&store, &item.id), None);
    assert_eq!(write(&store, item, events).unwrap(), 1);
}

#[test]
fn native_work_events_are_immutable_even_through_raw_sql() {
    let (root, store) = fixture();
    initialize_origin(&store);
    let (item, events) = task_fixture(1, false);
    write(&store, item.clone(), events).unwrap();
    let before = snapshot(&store);
    for sql in [
        "UPDATE work_events SET feed_seq = feed_seq WHERE version = 1",
        "DELETE FROM work_events WHERE version = 1",
    ] {
        let error = store
            .write(move |db| {
                assert_eq!(
                    db.query_row("SELECT count(*) FROM work_events", [], |r| r
                        .get::<_, i64>(0))
                        .unwrap(),
                    1
                );
                db.execute(sql, []).map_err(OfficeError::storage)?;
                Ok(((), None))
            })
            .unwrap_err();
        let OfficeError::Storage(cause) = error else {
            panic!("expected immutable SQL constraint")
        };
        assert!(
            matches!(cause.downcast_ref::<rusqlite::Error>(),Some(rusqlite::Error::SqliteFailure(code,_)) if code.extended_code==rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER)
        );
        assert_eq!(snapshot(&store), before);
    }
    drop(store);
    let store = reopen(&root);
    assert_eq!(snapshot(&store), before);
    assert_eq!(read(&store, &item.id), Some(item));
}

#[test]
fn native_work_replace_cannot_change_committed_events_or_feed() {
    for recursive in [false, true] {
        let (root, store) = fixture();
        initialize_origin(&store);
        let (item, events) = task_fixture(1, false);
        let event = events[0].clone();
        write(&store, item.clone(), events).unwrap();
        let (other, other_events) = task_fixture(2, false);
        assert_eq!(write(&store, other.clone(), other_events).unwrap(), 2);
        store
            .write(move |db| {
                // Exercise SQLite's default as well as its recursive-delete trigger mode.
                db.pragma_update(None, "recursive_triggers", recursive)
                    .map_err(OfficeError::storage)?;
                Ok(((), None))
            })
            .unwrap();
        for mode in ["payload", "feed", "both", "identical"] {
            let before = snapshot(&store);
            let mut replacement = event.clone();
            if mode == "payload" || mode == "both" {
                replacement.at = "2026-11-12T01:02:03.004Z".into();
                assert_ne!(replacement, event);
            }
            let seq = if mode == "feed" || mode == "both" {
                2
            } else {
                1
            };
            let error = store.write(move |db| {
                db.execute("INSERT OR REPLACE INTO work_events(target_id,version,feed_seq,data) VALUES (?1,?2,?3,?4)",
                    params![replacement.target_id,replacement.version as i64,seq,serde_json::to_string(&replacement).unwrap()])
                    .map_err(OfficeError::storage)?;
                Ok(((), None))
            }).unwrap_err();
            let OfficeError::Storage(cause) = error else {
                panic!("expected immutable SQL constraint")
            };
            assert!(
                matches!(cause.downcast_ref::<rusqlite::Error>(),Some(rusqlite::Error::SqliteFailure(code,_)) if code.extended_code==rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER)
            );
            assert_eq!(
                snapshot(&store),
                before,
                "recursive={recursive}, mode={mode}"
            );
        }
        let before = snapshot(&store);
        drop(store);
        let store = reopen(&root);
        assert_eq!(snapshot(&store), before);
        assert_eq!(read(&store, &item.id), Some(item.clone()));
        assert_eq!(read(&store, &other.id), Some(other));
        let (next, event) = advance(&item, &event);
        assert_eq!(write(&store, next, vec![event]).unwrap(), 3);
    }
}

fn seed_history(store: &Store) -> (StoredItem, WorkEvent) {
    let h = history();
    for (item, events) in &h.states[..4] {
        write(store, item.clone(), events.clone()).unwrap();
    }
    let (item, events) = h.states[3].clone();
    (item, events.last().unwrap().clone())
}

#[test]
fn native_work_refusals_preserve_item_ids_history_feed_and_origin_after_reopen() {
    for case in [
        "empty",
        "stale",
        "last-version",
        "first-gap",
        "inner-gap",
        "target",
        "zero-event",
        "generation-zero",
        "generation-overflow",
        "version-zero",
        "version-overflow",
        "item-id",
        "step-id",
        "duplicate-step",
        "cross-kind",
        "timestamp",
        "epoch",
        "org-seq",
        "forget-step",
        "forget-link",
        "forget-comment",
        "change-kind",
    ] {
        let (root, store) = fixture();
        initialize_origin(&store);
        let (base, previous) = seed_history(&store);
        let (mut item, next) = advance(&base, &previous);
        let mut events = vec![next];
        let expected = match case {
            "empty" => {
                events.clear();
                "work versions must be contiguous"
            }
            "stale" => {
                item.version = base.version;
                events[0].version = base.version;
                "work versions must be contiguous"
            }
            "last-version" => {
                item.version += 1;
                "work versions must be contiguous"
            }
            "first-gap" => {
                events[0].version += 1;
                "work versions must be contiguous"
            }
            "inner-gap" => {
                let mut e = events[0].clone();
                e.version += 2;
                events.push(e);
                item.version += 1;
                "work versions must be contiguous"
            }
            "target" => {
                events[0].target_id = org_work::ulid_from_parts(T0, 999);
                "work versions must be contiguous"
            }
            "zero-event" => {
                events[0].version = 0;
                "work versions must be contiguous"
            }
            "generation-zero" => {
                item.generation = 0;
                "invalid work item version or generation"
            }
            "generation-overflow" => {
                item.generation = u64::MAX;
                "work integer is out of range"
            }
            "version-zero" => {
                item.version = 0;
                "invalid work item version or generation"
            }
            "version-overflow" => {
                item.version = u64::MAX;
                "work integer is out of range"
            }
            "item-id" => {
                item.id = "invalid".into();
                events[0].target_id = item.id.clone();
                "invalid or duplicate work id"
            }
            "step-id" => {
                item.checklist[0].id = "invalid".into();
                "invalid or duplicate work id"
            }
            "duplicate-step" => {
                item.checklist.push(item.checklist[0].clone());
                "invalid or duplicate work id"
            }
            "cross-kind" => {
                item.comments[0].id = item.checklist[0].id.clone();
                "invalid or duplicate work id"
            }
            "timestamp" => {
                events[0].at = "not-a-time".into();
                "invalid work event timestamp"
            }
            "epoch" => {
                events[0].org_position.epoch = u64::MAX;
                "work integer is out of range"
            }
            "org-seq" => {
                events[0].org_position.seq = u64::MAX;
                "work integer is out of range"
            }
            "forget-step" => {
                item.checklist.clear();
                "work ids cannot be forgotten"
            }
            "forget-link" => {
                item.links.clear();
                "work ids cannot be forgotten"
            }
            "forget-comment" => {
                item.comments.clear();
                "work ids cannot be forgotten"
            }
            "change-kind" => {
                let step = item.checklist.remove(0);
                item.comments.push(StoredComment {
                    id: step.id,
                    kind: CommentKind::Note,
                    asks: None,
                    answered: false,
                });
                "work ids cannot be forgotten"
            }
            _ => unreachable!(),
        };
        let before = snapshot(&store);
        let error = write(&store, item, events).unwrap_err();
        assert!(
            matches!(error,OfficeError::Invalid(reason) if reason==expected),
            "{case}: {error}"
        );
        assert_eq!(snapshot(&store), before, "{case}: writes on refusal");
        drop(store);
        let store = reopen(&root);
        assert_eq!(snapshot(&store), before, "{case}: state after reopen");
        assert_eq!(read(&store, &base.id), Some(base.clone()));
        let (valid, event) = advance(&base, &previous);
        assert_eq!(
            write(&store, valid, vec![event]).unwrap(),
            5,
            "{case}: feed allocator consumed"
        );
    }
}

#[test]
fn native_work_global_id_namespace_refuses_cross_item_and_cross_kind_reuse() {
    for kind in ["item", "step-item", "step-step", "link", "comment"] {
        let (root, store) = fixture();
        initialize_origin(&store);
        let (old, _) = seed_history(&store);
        let (mut candidate, mut events) = task_fixture(2, true);
        match kind {
            "item" => {
                candidate.id = old.checklist[0].id.clone();
                for event in &mut events {
                    event.target_id = candidate.id.clone();
                }
            }
            "step-item" => candidate.checklist[0].id = old.id.clone(),
            "step-step" => candidate.checklist[0].id = old.checklist[0].id.clone(),
            "link" => candidate.links.push(StoredLink {
                id: old.links[0].id.clone(),
                ..old.links[0].clone()
            }),
            "comment" => candidate.comments.push(old.comments[0].clone()),
            _ => unreachable!(),
        }
        let before = snapshot(&store);
        let error = write(&store, candidate, events).unwrap_err();
        assert!(
            matches!(error, OfficeError::Invalid("work id is already claimed")),
            "{kind}: {error}"
        );
        assert_eq!(snapshot(&store), before);
        drop(store);
        let store = reopen(&root);
        assert_eq!(snapshot(&store), before);
        let (candidate, events) = task_fixture(2, true);
        assert_eq!(write(&store, candidate, events).unwrap(), 5);
    }
}

#[test]
fn native_work_append_failure_after_first_event_rolls_back_every_table() {
    let (root, store) = fixture();
    initialize_origin(&store);
    let h = history();
    let item = h.states[3].0.clone();
    let events = h.states[..4]
        .iter()
        .filter(|(i, _)| i.id == item.id)
        .flat_map(|(_, es)| es.clone())
        .collect::<Vec<_>>();
    assert!(events.len() > 1);
    store.write(|db|{db.execute_batch("CREATE TRIGGER fail_second_work_event BEFORE INSERT ON work_events WHEN NEW.version = 2 BEGIN SELECT RAISE(ABORT,'injected'); END;").map_err(OfficeError::storage)?;Ok(((),None))}).unwrap();
    let before = snapshot(&store);
    assert!(matches!(
        write(&store, item.clone(), events.clone()),
        Err(OfficeError::Storage(_))
    ));
    assert_eq!(snapshot(&store), before);
    drop(store);
    let store = reopen(&root);
    assert_eq!(snapshot(&store), before);
    store
        .write(|db| {
            db.execute_batch("DROP TRIGGER fail_second_work_event")
                .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .unwrap();
    assert_eq!(write(&store, item.clone(), events).unwrap(), 1);
    assert_eq!(read(&store, &item.id), Some(item));
}

#[test]
fn native_work_read_refuses_corrupt_item_and_id_index_without_repairing_it() {
    for case in [
        "json",
        "key",
        "generation",
        "version",
        "last-event",
        "zero-generation",
        "zero-version",
        "missing-event",
        "missing-id",
        "extra-id",
        "claim-kind",
        "claim-owner",
    ] {
        let (root, store) = fixture();
        initialize_origin(&store);
        let (item, previous) = seed_history(&store);
        let copy = item.clone();
        store
            .write(move |db| {
                db.execute_batch("PRAGMA ignore_check_constraints=ON;")
                    .map_err(OfficeError::storage)?;
                let mut data = serde_json::to_value(&copy).unwrap();
                match case {
                    "json" => {
                        // Expression indexes now reject malformed JSON on write. A valid JSON
                        // value with the wrong item shape still exercises the reader's decoder.
                        db.execute("UPDATE work_items SET data='[]' WHERE id=?1", [&copy.id])
                            .map_err(OfficeError::storage)?;
                    }
                    "key" => {
                        data["id"] = org_work::ulid_from_parts(T0, 999).into();
                    }
                    "generation" => data["generation"] = 2.into(),
                    "version" => data["version"] = 1.into(),
                    "zero-generation" => {
                        data["generation"] = 0.into();
                        db.execute("UPDATE work_items SET generation=0 WHERE id=?1", [&copy.id])
                            .map_err(OfficeError::storage)?;
                    }
                    "zero-version" => {
                        data["version"] = 0.into();
                        db.execute("UPDATE work_items SET version=0 WHERE id=?1", [&copy.id])
                            .map_err(OfficeError::storage)?;
                    }
                    "last-event" => {
                        data["version"] = (copy.version + 1).into();
                        db.execute(
                            "UPDATE work_items SET version=version+1 WHERE id=?1",
                            [&copy.id],
                        )
                        .map_err(OfficeError::storage)?;
                    }
                    "missing-event" => {
                        db.execute_batch("DROP TRIGGER work_events_no_delete")
                            .map_err(OfficeError::storage)?;
                        db.execute("DELETE FROM work_events WHERE target_id=?1", [&copy.id])
                            .map_err(OfficeError::storage)?;
                    }
                    "missing-id" => {
                        db.execute("DELETE FROM work_ids WHERE id=?1", [&copy.checklist[0].id])
                            .map_err(OfficeError::storage)?;
                    }
                    "extra-id" => {
                        db.execute(
                            "INSERT INTO work_ids VALUES (?1,?2,'step')",
                            params![org_work::ulid_from_parts(T0, 999), copy.id],
                        )
                        .map_err(OfficeError::storage)?;
                    }
                    "claim-kind" => {
                        db.execute(
                            "UPDATE work_ids SET kind='link' WHERE id=?1",
                            [&copy.checklist[0].id],
                        )
                        .map_err(OfficeError::storage)?;
                    }
                    "claim-owner" => {
                        db.execute(
                            "UPDATE work_ids SET item_id=?1 WHERE id=?2",
                            params![org_work::ulid_from_parts(T0, 19), copy.checklist[0].id],
                        )
                        .map_err(OfficeError::storage)?;
                    }
                    _ => unreachable!(),
                }
                if matches!(
                    case,
                    "key"
                        | "generation"
                        | "version"
                        | "last-event"
                        | "zero-generation"
                        | "zero-version"
                ) {
                    db.execute(
                        "UPDATE work_items SET data=?1 WHERE id=?2",
                        params![serde_json::to_string(&data).unwrap(), copy.id],
                    )
                    .map_err(OfficeError::storage)?;
                }
                Ok(((), None))
            })
            .unwrap();
        let before = snapshot(&store);
        let read_error = store
            .read(|db| {
                let tx = db.unchecked_transaction().map_err(OfficeError::storage)?;
                get(&tx, &item.id)
            })
            .unwrap_err();
        let expected = match case {
            "json" => None,
            "zero-generation" | "zero-version" => Some("invalid work item version or generation"),
            "missing-id" | "extra-id" | "claim-kind" | "claim-owner" => {
                Some("stored work id claims are inconsistent")
            }
            _ => Some("stored work item is inconsistent"),
        };
        match expected {
            Some(reason) => assert!(
                matches!(read_error,OfficeError::Invalid(r) if r==reason),
                "{case}: {read_error}"
            ),
            None => assert!(matches!(read_error, OfficeError::Storage(_))),
        }
        let (next, event) = advance(&item, &previous);
        let write_error = write(&store, next, vec![event]).unwrap_err();
        match expected {
            Some(reason) => assert!(
                matches!(write_error,OfficeError::Invalid(r) if r==reason),
                "{case}: {write_error}"
            ),
            None => assert!(matches!(write_error, OfficeError::Storage(_))),
        }
        assert_eq!(snapshot(&store), before);
        drop(store);
        let store = reopen(&root);
        assert!(
            store
                .read(|db| {
                    let tx = db.unchecked_transaction().map_err(OfficeError::storage)?;
                    get(&tx, &item.id)
                })
                .is_err(),
            "{case}"
        );
        assert_eq!(snapshot(&store), before);
    }
}

#[test]
fn native_work_event_reads_validate_payload_keys_time_and_feed() {
    for case in [
        "json",
        "target",
        "version",
        "zero-version",
        "timestamp",
        "epoch",
        "org-seq",
        "zero-feed",
        "negative-feed",
    ] {
        let (root, store) = fixture();
        initialize_origin(&store);
        let (item, events) = task_fixture(1, false);
        write(&store, item.clone(), events.clone()).unwrap();
        let copied = item.clone();
        let original = events[0].clone();
        store
            .write(move |db| {
                db.execute_batch(
                    "PRAGMA ignore_check_constraints=ON; DROP TRIGGER work_events_no_update;",
                )
                .map_err(OfficeError::storage)?;
                let mut event = original.clone();
                match case {
                    "json" => {
                        db.execute("UPDATE work_events SET data='{'", [])
                            .map_err(OfficeError::storage)?;
                    }
                    "target" => event.target_id = org_work::ulid_from_parts(T0, 999),
                    "version" => event.version += 1,
                    "zero-version" => event.version = 0,
                    "timestamp" => event.at = "bad".into(),
                    "epoch" => event.org_position.epoch = u64::MAX,
                    "org-seq" => event.org_position.seq = u64::MAX,
                    "zero-feed" | "negative-feed" => {
                        let seq = if case == "zero-feed" { 0 } else { -1 };
                        db.execute("INSERT INTO work_feed(seq,targets) VALUES (?1,'[]')", [seq])
                            .map_err(OfficeError::storage)?;
                        db.execute("UPDATE work_events SET feed_seq=?1", [seq])
                            .map_err(OfficeError::storage)?;
                    }
                    _ => unreachable!(),
                }
                if !matches!(case, "json" | "zero-feed" | "negative-feed") {
                    db.execute(
                        "UPDATE work_events SET data=?1 WHERE target_id=?2",
                        params![serde_json::to_string(&event).unwrap(), copied.id],
                    )
                    .map_err(OfficeError::storage)?;
                }
                Ok(((), None))
            })
            .unwrap();
        let before = snapshot(&store);
        let error = store.read(|db| event_records(db, &item.id, 0)).unwrap_err();
        let expected = match case {
            "json" => None,
            "timestamp" => Some("invalid work event timestamp"),
            "epoch" | "org-seq" => Some("work integer is out of range"),
            "zero-version" => Some("invalid work event identity"),
            _ => Some("stored work event is inconsistent"),
        };
        match expected {
            Some(reason) => assert!(
                matches!(error,OfficeError::Invalid(r) if r==reason),
                "{case}: {error}"
            ),
            None => assert!(matches!(error, OfficeError::Storage(_))),
        }
        assert_eq!(snapshot(&store), before);
        drop(store);
        let store = reopen(&root);
        assert!(store.read(|db| event_records(db, &item.id, 0)).is_err());
        assert_eq!(snapshot(&store), before);
    }
}

#[test]
fn native_work_shared_shape_and_integer_validators_cover_boundaries() {
    use super::{integer, last_version, validate_event, validate_item};
    let max = i64::MAX as u64;
    assert_eq!(integer(0).unwrap(), 0);
    assert_eq!(integer(max).unwrap(), i64::MAX);
    for n in [max + 1, u64::MAX] {
        assert!(matches!(
            integer(n),
            Err(OfficeError::Invalid("work integer is out of range"))
        ));
    }
    assert_eq!(last_version(0, 1).unwrap(), 1);
    assert_eq!(last_version(max - 1, 1).unwrap(), max);
    for (previous, count) in [(max, 1), (u64::MAX, 1), (0, usize::MAX)] {
        assert!(matches!(
            last_version(previous, count),
            Err(OfficeError::Invalid("work version is exhausted"))
        ));
    }
    let h = history();
    let (item, events) = &h.states[3];
    assert!(validate_item(item).is_ok());
    for case in [
        "gen-zero",
        "version-zero",
        "gen-overflow",
        "version-overflow",
        "id",
        "step",
        "link",
        "comment",
        "duplicate",
    ] {
        let mut changed = item.clone();
        let expected = match case {
            "gen-zero" => {
                changed.generation = 0;
                "invalid work item version or generation"
            }
            "version-zero" => {
                changed.version = 0;
                "invalid work item version or generation"
            }
            "gen-overflow" => {
                changed.generation = u64::MAX;
                "work integer is out of range"
            }
            "version-overflow" => {
                changed.version = u64::MAX;
                "work integer is out of range"
            }
            "id" => {
                changed.id = "bad".into();
                "invalid or duplicate work id"
            }
            "step" => {
                changed.checklist[0].id = "bad".into();
                "invalid or duplicate work id"
            }
            "link" => {
                changed.links[0].id = "bad".into();
                "invalid or duplicate work id"
            }
            "comment" => {
                changed.comments[0].id = "bad".into();
                "invalid or duplicate work id"
            }
            "duplicate" => {
                changed.checklist.push(changed.checklist[0].clone());
                "invalid or duplicate work id"
            }
            _ => unreachable!(),
        };
        assert!(
            matches!(validate_item(&changed),Err(OfficeError::Invalid(r)) if r==expected),
            "{case}"
        );
    }
    let mut largest = item.clone();
    largest.generation = max;
    largest.version = max;
    assert!(validate_item(&largest).is_ok());
    let event = &events[0];
    assert!(validate_event(event).is_ok());
    for case in ["target", "zero", "version", "epoch", "seq", "at"] {
        let mut changed = event.clone();
        let expected = match case {
            "target" => {
                changed.target_id = "bad".into();
                "invalid work event identity"
            }
            "zero" => {
                changed.version = 0;
                "invalid work event identity"
            }
            "version" => {
                changed.version = u64::MAX;
                "work integer is out of range"
            }
            "epoch" => {
                changed.org_position.epoch = u64::MAX;
                "work integer is out of range"
            }
            "seq" => {
                changed.org_position.seq = u64::MAX;
                "work integer is out of range"
            }
            "at" => {
                changed.at = "bad".into();
                "invalid work event timestamp"
            }
            _ => unreachable!(),
        };
        assert!(
            matches!(validate_event(&changed),Err(OfficeError::Invalid(r)) if r==expected),
            "{case}"
        );
    }
    let mut largest = event.clone();
    largest.version = max;
    largest.org_position.epoch = max;
    largest.org_position.seq = max;
    largest.at = "9999-12-31T23:59:59.999Z".into();
    assert!(validate_event(&largest).is_ok());
}

#[test]
fn native_work_version_exhaustion_refuses_before_allocating_rows() {
    let (root, store) = fixture();
    initialize_origin(&store);
    let (mut item, mut events) = task_fixture(1, false);
    write(&store, item.clone(), events.clone()).unwrap();
    item.version = i64::MAX as u64;
    events[0].version = item.version;
    let copy = item.clone();
    let event = events[0].clone();
    // Seed the numeric boundary without allocating i64::MAX historical events.
    store
        .write(move |db| {
            db.execute_batch("DROP TRIGGER work_events_no_update")
                .map_err(OfficeError::storage)?;
            db.execute(
                "UPDATE work_items SET version=?1,data=?2 WHERE id=?3",
                params![i64::MAX, serde_json::to_string(&copy).unwrap(), copy.id],
            )
            .map_err(OfficeError::storage)?;
            db.execute(
                "UPDATE work_events SET version=?1,data=?2",
                params![i64::MAX, serde_json::to_string(&event).unwrap()],
            )
            .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .unwrap();
    assert_eq!(read(&store, &item.id), Some(item.clone()));
    let before = snapshot(&store);
    assert!(matches!(
        write(&store, item, events),
        Err(OfficeError::Invalid("work version is exhausted"))
    ));
    assert_eq!(snapshot(&store), before);
    drop(store);
    let store = reopen(&root);
    assert_eq!(snapshot(&store), before);
}

const V3: Schema = Schema {
    admit: None,
    version: 3,
    create: |tx| {
        tx.execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;")?;
        crate::store::messages::create_tables(tx)?;
        crate::store::status::create_table(tx)
    },
    upgrade: |_, _| panic!("fixture starts at v3"),
};

#[test]
fn native_work_v3_upgrade_and_reopen_preserve_existing_message_metadata() {
    let (root, old) = fixture_for_schema(&V3);
    initialize_origin(&old);
    drop(old);
    let store = reopen(&root);
    let (item, events) = task_fixture(1, true);
    write(&store, item.clone(), events).unwrap();
    assert_eq!(read(&store, &item.id), Some(item.clone()));
    let before = snapshot(&store);
    drop(store);
    let store = reopen(&root);
    assert_eq!(snapshot(&store), before);
    store
        .read(|db| {
            assert_eq!(
                db.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
                    .unwrap(),
                6
            );
            assert_eq!(
                db.query_row(
                    "SELECT value FROM meta WHERE key='message_sequence'",
                    [],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
                "41"
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn native_work_failed_upgrade_rolls_back_new_tables_and_version() {
    let (root, old) = fixture_for_schema(&V3);
    initialize_origin(&old);
    old.write(|db| {
        db.execute_batch(
            "CREATE TABLE work_events (sentinel TEXT); INSERT INTO work_events VALUES ('keep');",
        )
        .map_err(OfficeError::storage)?;
        Ok(((), None))
    })
    .unwrap();
    drop(old);
    assert!(matches!(
        Store::open(
            &root.path().join("office.db"),
            &Key::from_bytes([42; 32]),
            Opening::Existing,
            &OFFICE_SCHEMA
        ),
        Err(OfficeError::Storage(_))
    ));
    let old = Store::open(
        &root.path().join("office.db"),
        &Key::from_bytes([42; 32]),
        Opening::Existing,
        &V3,
    )
    .unwrap();
    old.read(|db|{
        assert_eq!(db.pragma_query_value::<i64,_>(None,"user_version",|r|r.get(0)).unwrap(),3);
        assert_eq!(db.query_row("SELECT sentinel FROM work_events",[],|r|r.get::<_,String>(0)).unwrap(),"keep");
        assert_eq!(db.query_row("SELECT count(*) FROM sqlite_master WHERE name IN ('work_items','work_ids','work_feed','work_origin_log','work_origin_counter')",[],|r|r.get::<_,i64>(0)).unwrap(),0);
        assert_eq!(db.query_row("SELECT value FROM meta WHERE key='message_sequence'",[],|r|r.get::<_,String>(0)).unwrap(),"41");
        Ok(())
    }).unwrap();
}

#[test]
fn native_work_schema_checks_keys_bounds_and_references_independently() {
    for case in [
        "item-generation",
        "item-version",
        "item-json",
        "item-key",
        "item-gen-key",
        "item-ver-key",
        "id-kind",
        "id-owner",
        "event-target-owner",
        "event-feed-owner",
        "event-zero-feed",
        "event-version",
        "event-json",
        "event-target-key",
        "event-ver-key",
        "event-duplicate",
    ] {
        let (root, store) = fixture();
        initialize_origin(&store);
        let (item, events) = task_fixture(1, false);
        write(&store, item.clone(), events.clone()).unwrap();
        let before = snapshot(&store);
        let copy = item.clone();
        let original = events[0].clone();
        let error = store
            .write(move |db| {
                if case.starts_with("item-") {
                    let mut data = serde_json::to_value(&copy).unwrap();
                    let (generation, version) = match case {
                        "item-generation" => {
                            data["generation"] = 0.into();
                            (0, copy.version as i64)
                        }
                        "item-version" => {
                            data["version"] = 0.into();
                            (copy.generation as i64, 0)
                        }
                        "item-key" => {
                            data["id"] = org_work::ulid_from_parts(T0, 999).into();
                            (copy.generation as i64, copy.version as i64)
                        }
                        "item-gen-key" => {
                            data["generation"] = 2.into();
                            (copy.generation as i64, copy.version as i64)
                        }
                        "item-ver-key" => {
                            data["version"] = 2.into();
                            (copy.generation as i64, copy.version as i64)
                        }
                        _ => (copy.generation as i64, copy.version as i64),
                    };
                    let text = if case == "item-json" {
                        "{".into()
                    } else {
                        serde_json::to_string(&data).unwrap()
                    };
                    db.execute(
                        "UPDATE work_items SET generation=?1,version=?2,data=?3 WHERE id=?4",
                        params![generation, version, text, copy.id],
                    )
                    .map_err(OfficeError::storage)?;
                } else if case == "id-kind" {
                    db.execute("UPDATE work_ids SET kind='invalid' WHERE id=?1", [&copy.id])
                        .map_err(OfficeError::storage)?;
                } else if case == "id-owner" {
                    db.execute(
                        "INSERT INTO work_ids VALUES (?1,?2,'step')",
                        params![
                            org_work::ulid_from_parts(T0, 999),
                            org_work::ulid_from_parts(T0, 998)
                        ],
                    )
                    .map_err(OfficeError::storage)?;
                } else {
                    let mut event = original.clone();
                    event.version = 2;
                    let mut target = copy.id.clone();
                    let mut version = 2i64;
                    let mut seq = 1i64;
                    match case {
                        "event-target-owner" => {
                            target = org_work::ulid_from_parts(T0, 999);
                            event.target_id = target.clone();
                        }
                        "event-feed-owner" => seq = 999,
                        "event-zero-feed" => {
                            db.execute("INSERT INTO work_feed(seq,targets) VALUES (0,'[]')", [])
                                .map_err(OfficeError::storage)?;
                            seq = 0;
                        }
                        "event-version" => {
                            version = 0;
                            event.version = 0;
                        }
                        "event-target-key" => event.target_id = org_work::ulid_from_parts(T0, 999),
                        "event-ver-key" => event.version = 3,
                        "event-duplicate" => {
                            // Prove the primary key independently of the REPLACE guard.
                            db.execute_batch("DROP TRIGGER work_events_no_replace")
                                .map_err(OfficeError::storage)?;
                            version = 1;
                            event.version = 1;
                            seq = feed(db, std::slice::from_ref(&event))? as i64;
                        }
                        _ => {}
                    }
                    let text = if case == "event-json" {
                        "{".into()
                    } else {
                        serde_json::to_string(&event).unwrap()
                    };
                    db.execute(
                        "INSERT INTO work_events VALUES (?1,?2,?3,?4)",
                        params![target, version, seq, text],
                    )
                    .map_err(OfficeError::storage)?;
                }
                Ok(((), None))
            })
            .unwrap_err();
        assert!(
            matches!(error, OfficeError::Storage(_)),
            "{case}: expected SQL schema refusal, got {error}"
        );
        assert_eq!(snapshot(&store), before, "{case}");
        drop(store);
        let store = reopen(&root);
        assert_eq!(snapshot(&store), before, "{case}: reopen");
    }
}

#[test]
fn native_work_id_claim_operation_checks_owner_and_role() {
    use super::claim_id;
    let (root, store) = fixture();
    initialize_origin(&store);
    let (item, _) = seed_history(&store);
    let before = snapshot(&store);
    let copy = item.clone();
    store
        .write(move |db| {
            for (id, kind) in [
                (&copy.id, "item"),
                (&copy.checklist[0].id, "step"),
                (&copy.links[0].id, "link"),
                (&copy.comments[0].id, "comment"),
            ] {
                claim_id(db, id, &copy.id, kind)?;
            }
            Ok(((), None))
        })
        .unwrap();
    assert_eq!(snapshot(&store), before);
    for (owner, kind) in [
        (item.id.clone(), "step"),
        (org_work::ulid_from_parts(T0, 19), "item"),
        (org_work::ulid_from_parts(T0, 19), "comment"),
    ] {
        let id = item.id.clone();
        let error = store
            .write(move |db| {
                claim_id(db, &id, &owner, kind)?;
                Ok(((), None))
            })
            .unwrap_err();
        assert!(matches!(
            error,
            OfficeError::Invalid("work id is already claimed")
        ));
        assert_eq!(snapshot(&store), before);
    }
    drop(store);
    let store = reopen(&root);
    assert_eq!(snapshot(&store), before);
}

#[test]
fn native_work_write_errors_at_item_and_id_stages_roll_back() {
    for stage in ["new-item", "update-item", "new-id", "retained-id"] {
        let (root, store) = fixture();
        initialize_origin(&store);
        let (base, original) = task_fixture(1, true);
        let (item, events) = if matches!(stage, "update-item" | "retained-id") {
            write(&store, base.clone(), original.clone()).unwrap();
            let (item, event) = advance(&base, original.last().unwrap());
            (item, vec![event])
        } else {
            (base, original)
        };
        let sql=match stage {
            "new-item"=>"CREATE TRIGGER fail_work_stage BEFORE INSERT ON work_items BEGIN SELECT RAISE(ABORT,'injected'); END;",
            "update-item"=>"CREATE TRIGGER fail_work_stage BEFORE UPDATE ON work_items BEGIN SELECT RAISE(ABORT,'injected'); END;",
            "new-id"=>"CREATE TRIGGER fail_work_stage BEFORE INSERT ON work_ids WHEN NEW.kind='step' BEGIN SELECT RAISE(ABORT,'injected'); END;",
            _=>"CREATE TRIGGER fail_work_stage BEFORE UPDATE ON work_ids BEGIN SELECT RAISE(ABORT,'injected'); END;",
        };
        store
            .write(move |db| {
                db.execute_batch(sql).map_err(OfficeError::storage)?;
                Ok(((), None))
            })
            .unwrap();
        let before = snapshot(&store);
        assert!(
            matches!(
                write(&store, item.clone(), events.clone()),
                Err(OfficeError::Storage(_))
            ),
            "{stage}"
        );
        assert_eq!(snapshot(&store), before);
        drop(store);
        let store = reopen(&root);
        assert_eq!(snapshot(&store), before);
        store
            .write(|db| {
                db.execute_batch("DROP TRIGGER fail_work_stage")
                    .map_err(OfficeError::storage)?;
                Ok(((), None))
            })
            .unwrap();
        let seq = if stage == "new-item" || stage == "new-id" {
            1
        } else {
            2
        };
        assert_eq!(write(&store, item, events).unwrap(), seq);
    }
}

#[test]
fn native_work_query_failures_are_errors_not_absent_or_empty_results() {
    for missing in ["work_events", "work_ids"] {
        let (_root, store) = fixture();
        initialize_origin(&store);
        let (item, events) = task_fixture(1, false);
        write(&store, item.clone(), events).unwrap();
        store
            .write(move |db| {
                let sql = if missing == "work_events" {
                    "DROP TRIGGER work_events_no_delete; DROP TABLE work_events;"
                } else {
                    "DROP TABLE work_ids;"
                };
                db.execute_batch(sql).map_err(OfficeError::storage)?;
                Ok(((), None))
            })
            .unwrap();
        let error = store
            .read(|db| {
                let tx = db.unchecked_transaction().map_err(OfficeError::storage)?;
                get(&tx, &item.id)
            })
            .unwrap_err();
        assert!(matches!(error, OfficeError::Storage(_)));
        if missing == "work_events" {
            assert!(matches!(
                store.read(|db| event_records(db, &item.id, 0)),
                Err(OfficeError::Storage(_))
            ));
        } else {
            assert!(matches!(
                store.read(|db| owner_of(db, &item.id)),
                Err(OfficeError::Storage(_))
            ));
        }
        store
            .read(move |db| {
                assert_eq!(
                    db.query_row("SELECT count(*) FROM work_items", [], |r| r
                        .get::<_, i64>(0))
                        .unwrap(),
                    1
                );
                assert_eq!(
                    db.query_row("SELECT count(*) FROM work_feed", [], |r| r.get::<_, i64>(0))
                        .unwrap(),
                    1
                );
                assert_eq!(
                    db.query_row(
                        "SELECT count(*) FROM sqlite_master WHERE name=?1",
                        [missing],
                        |r| r.get::<_, i64>(0)
                    )
                    .unwrap(),
                    0
                );
                Ok(())
            })
            .unwrap();
    }
}

#[test]
fn native_work_empty_put_is_refused_without_origin_append_as_a_backstop() {
    let (root, store) = fixture();
    initialize_origin(&store);
    let (item, events) = task_fixture(1, true);
    write(&store, item.clone(), events).unwrap();
    let before = snapshot(&store);
    let copy = item.clone();
    let error = store
        .write(move |db| {
            let seq = feed(db, &[])?;
            put_item(db, &copy, &[], seq)?;
            Ok(((), None))
        })
        .unwrap_err();
    assert!(matches!(
        error,
        OfficeError::Invalid("work versions must be contiguous")
    ));
    assert_eq!(snapshot(&store), before);
    drop(store);
    let store = reopen(&root);
    assert_eq!(snapshot(&store), before);
}

#[test]
fn native_work_event_cursor_accepts_signed_max_and_refuses_overflow() {
    let (_root, store) = fixture();
    initialize_origin(&store);
    let (item, events) = task_fixture(1, false);
    write(&store, item.clone(), events).unwrap();
    let before = snapshot(&store);
    store
        .read(|db| {
            assert!(event_records(db, &item.id, i64::MAX as u64)?.is_empty());
            Ok(())
        })
        .unwrap();
    for after in [i64::MAX as u64 + 1, u64::MAX] {
        let error = store
            .read(|db| event_records(db, &item.id, after))
            .unwrap_err();
        assert!(matches!(
            error,
            OfficeError::Invalid("work integer is out of range")
        ));
        assert_eq!(snapshot(&store), before);
    }
}

#[test]
fn native_work_generation_round_trips_through_the_signed_boundary() {
    for generation in [2, i64::MAX as u64] {
        let (root, store) = fixture();
        initialize_origin(&store);
        let (mut item, events) = task_fixture(1, true);
        item.generation = generation;
        write(&store, item.clone(), events).unwrap();
        assert_eq!(read(&store, &item.id), Some(item.clone()));
        let before = snapshot(&store);
        drop(store);
        let store = reopen(&root);
        assert_eq!(read(&store, &item.id), Some(item.clone()));
        assert_eq!(snapshot(&store), before);
        store
            .read(|db| {
                assert_eq!(
                    db.query_row(
                        "SELECT generation FROM work_items WHERE id=?1",
                        [&item.id],
                        |r| r.get::<_, i64>(0)
                    )
                    .unwrap(),
                    generation as i64
                );
                assert_eq!(
                    db.query_row(
                        "SELECT generation FROM work_origin_log WHERE target_id=?1",
                        [&item.id],
                        |r| r.get::<_, i64>(0)
                    )
                    .unwrap(),
                    generation as i64
                );
                Ok(())
            })
            .unwrap();
    }
}

#[test]
fn native_work_writer_replay_commits_items_ids_events_and_origin_once() {
    use crate::store::WriteJob;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };
    use std::time::Duration;

    let (root, store) = fixture();
    initialize_origin(&store);
    let queue = store.jobs.as_ref().unwrap();
    let (started, ready) = mpsc::channel();
    let (release, held) = mpsc::channel();
    queue
        .send(WriteJob {
            attempt: Box::new(move |_| {
                started.send(()).map_err(OfficeError::storage)?;
                held.recv_timeout(Duration::from_secs(5))
                    .map_err(OfficeError::storage)?;
                Ok(None)
            }),
            abort_batch: |_| false,
            reply: Box::new(|result| result.unwrap()),
        })
        .unwrap();
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    let attempts = Arc::new(AtomicUsize::new(0));
    let (replied, replies) = mpsc::channel();
    let expected = [task_fixture(1, true), task_fixture(3, true)];
    // The writer is held until all three jobs are queued, so replay is deterministic.
    for id in 1..=3 {
        let (attempts, replied) = (Arc::clone(&attempts), replied.clone());
        let (item, events) = task_fixture(id, true);
        queue
            .send(WriteJob {
                attempt: Box::new(move |db| {
                    if id == 2 {
                        db.execute_batch("ROLLBACK").map_err(OfficeError::storage)?;
                        return Err(OfficeError::Invalid("injected batch abort"));
                    }
                    if id == 1 {
                        attempts.fetch_add(1, Ordering::SeqCst);
                    }
                    let seq = feed(db, &events)?;
                    put_item(db, &item, &events, seq)?;
                    let refs: Vec<_> = events
                        .iter()
                        .map(|e| origin::OriginRef {
                            target_id: e.target_id.clone(),
                            generation: item.generation,
                            version: e.version,
                        })
                        .collect();
                    origin::append(db, org_proto::Position { epoch: 0, seq: 0 }, &refs, seq, T0)?;
                    assert_eq!(seq, if id == 1 { 1 } else { 2 });
                    Ok(None)
                }),
                abort_batch: |_| false,
                reply: Box::new(move |result| {
                    replied.send((id, result)).unwrap();
                }),
            })
            .unwrap();
    }
    release.send(()).unwrap();
    let mut outcomes: Vec<_> = (0..3)
        .map(|_| replies.recv_timeout(Duration::from_secs(5)).unwrap())
        .collect();
    outcomes.sort_by_key(|(id, _)| *id);
    assert!(outcomes[0].1.is_ok());
    assert!(matches!(
        outcomes[1].1,
        Err(OfficeError::Invalid("injected batch abort"))
    ));
    assert!(outcomes[2].1.is_ok());
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    for (n, (item, events)) in expected.iter().enumerate() {
        assert_eq!(read(&store, &item.id), Some(item.clone()));
        store
            .read(|db| {
                assert_eq!(
                    owner_of(db, &item.checklist[0].id)?,
                    Some((item.id.clone(), "step".into()))
                );
                assert_eq!(
                    event_records(db, &item.id, 0)?,
                    events
                        .iter()
                        .cloned()
                        .map(|e| (n as u64 + 1, e))
                        .collect::<Vec<_>>()
                );
                Ok(())
            })
            .unwrap();
    }
    assert_eq!(read(&store, &task_fixture(2, true).0.id), None);
    store
        .read(|db| {
            for (table, count) in [
                ("work_items", 2),
                ("work_ids", 4),
                ("work_events", 2),
                ("work_feed", 2),
                ("work_origin_log", 2),
            ] {
                let sql = format!("SELECT count(*) FROM {table}");
                assert_eq!(
                    db.query_row(&sql, [], |r| r.get::<_, i64>(0)).unwrap(),
                    count
                );
            }
            Ok(())
        })
        .unwrap();
    let before = snapshot(&store);
    drop(store);
    let store = reopen(&root);
    assert_eq!(snapshot(&store), before);
    for (item, _) in expected {
        assert_eq!(read(&store, &item.id), Some(item));
    }
}
