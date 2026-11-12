use super::super::call::{
    attempt, finish, run, transact, transact_with_abort, WorkCall as NativeCall, WorkCallError,
    ROLLBACK,
};
use super::*;
use std::sync::Mutex;

fn tag(error: &WorkCallError) -> &'static str {
    match error {
        WorkCallError::Unrecorded => "Unrecorded",
        WorkCallError::AlreadyRecorded => "AlreadyRecorded",
        WorkCallError::NothingToRecord => "NothingToRecord",
        WorkCallError::InvalidRefs => "InvalidRefs",
        WorkCallError::RefsMismatch => "RefsMismatch",
        WorkCallError::DuplicatePut => "DuplicatePut",
        WorkCallError::CallFailed => "CallFailed",
        WorkCallError::ResultBeforeRecord => "ResultBeforeRecord",
        WorkCallError::Port(_) => "Port",
        WorkCallError::Store(_) => "Store",
    }
}
fn error<T>(result: Result<T, WorkCallError>, expected: &str) -> WorkCallError {
    match result {
        Err(e) => {
            assert_eq!(tag(&e), expected);
            e
        }
        Ok(_) => panic!("expected {expected}"),
    }
}
fn refs(events: &[WorkEvent]) -> Vec<org_proto::work::EventRef> {
    events
        .iter()
        .map(|e| org_proto::work::EventRef {
            target_id: e.target_id.clone(),
            version: e.version,
        })
        .collect()
}
fn position() -> org_proto::Position {
    org_proto::Position { epoch: 7, seq: 19 }
}
fn invoke<T: Send + 'static>(
    store: &Store,
    f: impl Fn(&mut NativeCall<'_>) -> Result<T, WorkCallError> + Send + 'static,
) -> Result<T, WorkCallError> {
    transact(store, f)
}
fn raw<T: Send + 'static>(
    store: &Store,
    f: impl Fn(&Connection, &mut NativeCall<'_>) -> Result<T, WorkCallError> + Send + 'static,
) -> Result<T, OfficeError> {
    store.write(move |db| {
        run(db, |call| f(db, call))
            .map(|value| (value, None))
            .map_err(OfficeError::storage)
    })
}
fn writer_snapshot(db: &Connection) -> Vec<Vec<Vec<Value>>> {
    [
        "SELECT * FROM work_items ORDER BY id",
        "SELECT * FROM work_ids ORDER BY id",
        "SELECT * FROM work_events ORDER BY target_id,version",
        "SELECT * FROM work_feed ORDER BY seq",
        "SELECT * FROM work_origin_log ORDER BY origin_office_id,origin_incarnation,origin_seq",
        "SELECT * FROM work_origin_counter ORDER BY origin_incarnation",
        "SELECT * FROM sqlite_sequence ORDER BY name",
        "SELECT * FROM meta ORDER BY key",
    ]
    .iter()
    .map(|sql| {
        let mut q = db.prepare(sql).unwrap();
        let cols = q.column_count();
        q.query_map([], |r| {
            (0..cols)
                .map(|n| r.get(n))
                .collect::<rusqlite::Result<Vec<Value>>>()
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
    })
    .collect()
}
fn batch(seed: u128, count: usize) -> (StoredItem, Vec<WorkEvent>) {
    let (mut item, mut events) = task_fixture(seed, false);
    while events.len() < count {
        let old = item.title.clone();
        item.version += 1;
        item.title = format!("Version {}", item.version);
        let mut e = events[0].clone();
        e.version = item.version;
        e.change = WorkEventKind::FieldsChanged {
            fields: vec![FieldChange {
                field: "title".into(),
                from: Some(old.into()),
                to: Some(item.title.clone().into()),
            }],
        };
        events.push(e);
    }
    (item, events)
}
fn set_incarnation(store: &Store, value: &'static str) {
    store
        .write(move |db| {
            db.execute("UPDATE meta SET value=?1 WHERE key='incarnation'", [value])
                .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .unwrap();
}

#[test]
fn work_call_ok_put_without_record_change_rolls_back_after_reopen() {
    let (root, store) = fixture();
    initialize_origin(&store);
    let before = snapshot(&store);
    let (item, events) = task_fixture(1, true);
    error(
        invoke(&store, move |call| {
            call.put_item(&item, &events)?;
            Ok(())
        }),
        "Unrecorded",
    );
    assert_eq!(snapshot(&store), before);
    drop(store);
    let store = reopen(&root);
    assert_eq!(snapshot(&store), before);
}

#[test]
fn work_call_history_generations_feed_metadata_and_origin_order_survive_reopen() {
    let (root, store) = fixture();
    initialize_origin(&store);
    set_incarnation(&store, "01ARZ3NDEKTSV4RRFFQ69G5FAV");
    let mut h = history();
    let source = h.source.clone();
    let second = h.states[1].0.id.clone();
    // First-put order differs from ID sort order and from the caller's reversed refs.
    assert!(second > source);
    h.states.swap(0, 1);
    for (item, _) in &mut h.states {
        item.generation = if item.id == source { 3 } else { 7 };
    }
    let expected = h.states.last().unwrap().0.clone();
    let ordered: Vec<_> = h.states.iter().flat_map(|(_, es)| refs(es)).collect();
    let mut supplied = ordered.clone();
    supplied.reverse();
    let change = org_work::ulid_from_parts(T0, 900);
    let saved_change = change.clone();
    let output = invoke(&store, move |call| {
        for (item, events) in &h.states {
            call.put_item(item, events)?;
        }
        call.record_change(position(), &supplied, Some(&change), T0 + 17)
    })
    .unwrap();
    assert_eq!(output.seq, 1);
    assert_eq!(output.incarnation, 6371782407648578706);
    assert_eq!(read(&store, &expected.id), Some(expected.clone()));
    assert!(expected.deleted && expected.checklist[0].removed && expected.links[0].removed);
    store
        .read(|db| {
            let (targets, change): (String, String) = db
                .query_row(
                    "SELECT targets,change_id FROM work_feed WHERE seq=1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!(
                serde_json::from_str::<Vec<String>>(&targets).unwrap(),
                [second.clone(), source.clone()]
            );
            assert_eq!(change, saved_change);
            let origin = origin::local_origin(db)?;
            let rows = origin::entries_after(db, &origin.incarnation, 0, 1024)?;
            assert_eq!(
                rows.iter()
                    .map(|r| org_proto::work::EventRef {
                        target_id: r.target.target_id.clone(),
                        version: r.target.version
                    })
                    .collect::<Vec<_>>(),
                ordered
            );
            assert!(rows.len() > 2);
            for (n, row) in rows.iter().enumerate() {
                assert_eq!(row.seq, n as u64 + 1);
                assert_eq!(row.feed_seq, 1);
                assert_eq!(row.org_position, position());
                assert_eq!(row.committed_at_ms, T0 + 17);
                assert_eq!(
                    row.target.generation,
                    if row.target.target_id == source { 3 } else { 7 }
                );
            }
            assert_eq!(
                origin::high_water(db, &origin.incarnation)?,
                rows.len() as u64
            );
            Ok(())
        })
        .unwrap();
    let before = snapshot(&store);
    drop(store);
    let store = reopen(&root);
    assert_eq!(snapshot(&store), before);
    set_incarnation(&store, "01ARZ3NDEKTSV4RRFFQ69G5FAW");
    let (item, events) = task_fixture(3, false);
    let next_id = item.id.clone();
    let output = invoke(&store, move |call| {
        call.put_item(&item, &events)?;
        call.record_change(position(), &refs(&events), None, T0 + 18)
    })
    .unwrap();
    assert_eq!(output.seq, 2);
    assert_eq!(output.incarnation, 8032518857659272812);
    store
        .read(|db| {
            assert!(db
                .query_row(
                    "SELECT change_id IS NULL FROM work_feed WHERE seq=2",
                    [],
                    |r| r.get::<_, bool>(0)
                )
                .unwrap());
            let rows: Vec<(String, Option<String>)> = db
                .prepare("SELECT targets,change_id FROM work_feed ORDER BY seq")
                .unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            assert_eq!(rows.len(), 2);
            assert_eq!(
                serde_json::from_str::<Vec<String>>(&rows[0].0).unwrap(),
                [second.clone(), source.clone()]
            );
            assert_eq!(rows[0].1.as_deref(), Some(saved_change.as_str()));
            assert_eq!(
                serde_json::from_str::<Vec<String>>(&rows[1].0).unwrap(),
                [next_id]
            );
            assert_eq!(rows[1].1, None);
            Ok(())
        })
        .unwrap();
}

#[test]
fn work_call_allocation_is_lazy_and_refused_first_put_creates_no_feed() {
    let (_root, store) = fixture();
    let pristine = snapshot(&store);
    invoke(&store, |_| Ok(())).unwrap();
    assert_eq!(snapshot(&store), pristine);
    initialize_origin(&store);
    let before = snapshot(&store);
    invoke(&store, |_| Ok(())).unwrap();
    assert_eq!(snapshot(&store), before);
    let (item, events) = task_fixture(1, false);
    let duplicate = vec![events[0].clone(), events[0].clone()];
    invoke(&store, move |call| {
        error(call.put_item(&item, &duplicate), "DuplicatePut");
        Ok(())
    })
    .unwrap();
    assert_eq!(snapshot(&store), before);
    let (item, events) = task_fixture(1, false);
    raw(&store, move |db, call| {
        call.put_item(&item, &events)?;
        let (targets, change): (String, Option<String>) = db
            .query_row("SELECT targets,change_id FROM work_feed", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(targets, "[]");
        assert_eq!(change, None);
        call.record_change(position(), &refs(&events), None, T0)
    })
    .unwrap();
}

#[test]
fn work_call_refusals_preserve_frozen_order_and_allow_corrected_retry() {
    for state in ["open", "writing", "finalized"] {
        for case in [
            "empty",
            "duplicate",
            "duplicate-extra",
            "missing",
            "extra",
            "wrong-target",
            "wrong-version",
            "valid",
        ] {
            let (_root, store) = fixture();
            initialize_origin(&store);
            let (item, events) = task_fixture(1, false);
            let mut supplied = refs(&events);
            let expected = match case {
                "empty" => {
                    supplied.clear();
                    Some("InvalidRefs")
                }
                "duplicate" => {
                    supplied.push(supplied[0].clone());
                    Some("InvalidRefs")
                }
                "duplicate-extra" => {
                    supplied[0].target_id = org_work::ulid_from_parts(T0, 999);
                    supplied.push(supplied[0].clone());
                    Some("InvalidRefs")
                }
                "missing" => Some("RefsMismatch"),
                "extra" => {
                    supplied.push(org_proto::work::EventRef {
                        target_id: org_work::ulid_from_parts(T0, 999),
                        version: 1,
                    });
                    Some("RefsMismatch")
                }
                "wrong-target" => {
                    supplied[0].target_id = org_work::ulid_from_parts(T0, 999);
                    Some("RefsMismatch")
                }
                "wrong-version" => {
                    supplied[0].version += 1;
                    Some("RefsMismatch")
                }
                _ => None,
            };
            raw(&store, move |db, call| {
                let mut complete = refs(&events);
                if state != "open" {
                    call.put_item(&item, &events)?;
                    if case == "missing" {
                        let (other, es) = task_fixture(2, false);
                        call.put_item(&other, &es)?;
                        complete.extend(refs(&es));
                    }
                }
                if state == "finalized" {
                    call.record_change(position(), &complete, None, T0)?;
                }
                let before = writer_snapshot(db);
                let result = call.record_change(position(), &supplied, None, T0);
                let expected = match state {
                    "open" => Some("NothingToRecord"),
                    "finalized" => Some("AlreadyRecorded"),
                    _ => expected,
                };
                if let Some(expected) = expected {
                    error(result, expected);
                    assert_eq!(writer_snapshot(db), before);
                    if state == "writing" {
                        call.record_change(position(), &complete, None, T0)?;
                    }
                } else {
                    assert_eq!(result.unwrap().seq, 1);
                }
                Ok(())
            })
            .unwrap();
        }
    }
}

#[test]
fn work_call_overlap_in_any_event_is_refused_before_inner_put_and_finalized_is_terminal() {
    for tail in [false, true] {
        let (_root, store) = fixture();
        initialize_origin(&store);
        let (first, events) = batch(1, 2);
        let (next, more) = batch(1, 3);
        let (other, other_events) = task_fixture(2, false);
        raw(&store, move |db, call| {
            call.put_item(&first, &events)?;
            let overlap = if tail {
                vec![more[2].clone(), events[1].clone()]
            } else {
                vec![events[1].clone()]
            };
            let before = writer_snapshot(db);
            error(call.put_item(&next, &overlap), "DuplicatePut");
            assert_eq!(writer_snapshot(db), before);
            call.put_item(&next, &more[2..])?;
            call.put_item(&other, &other_events)?;
            let mut all = refs(&more);
            all.extend(refs(&other_events));
            call.record_change(position(), &all, None, T0)?;
            let before = writer_snapshot(db);
            error(call.put_item(&next, &overlap), "AlreadyRecorded");
            error(
                call.record_change(position(), &[], None, T0),
                "AlreadyRecorded",
            );
            assert_eq!(writer_snapshot(db), before);
            Ok(())
        })
        .unwrap();
    }
}

#[test]
fn work_call_5000_events_commit_and_rollback_are_gap_free_after_reopen() {
    for rollback in [false, true] {
        let (root, store) = fixture();
        initialize_origin(&store);
        let before = snapshot(&store);
        let mut items: Vec<_> = (1..=5).map(|n| batch(n, 1000)).collect();
        for (n, (i, _)) in items.iter_mut().enumerate() {
            i.generation = n as u64 + 1;
        }
        let result = invoke(&store, move |call| {
            let mut all = Vec::new();
            for (i, es) in &items {
                call.put_item(i, es)?;
                all.extend(refs(es));
            }
            all.reverse();
            assert_eq!(all.len(), 5000);
            let output = call.record_change(position(), &all, None, T0)?;
            if rollback {
                return Err(WorkCallError::RefsMismatch);
            }
            Ok(output)
        });
        if rollback {
            error(result, "RefsMismatch");
            assert_eq!(snapshot(&store), before);
        } else {
            assert_eq!(result.unwrap().seq, 1);
            store
                .read(|db| {
                    assert_eq!(
                        db.query_row("SELECT count(*) FROM work_events", [], |r| r
                            .get::<_, i64>(0))
                            .unwrap(),
                        5000
                    );
                    let (count, min, max): (i64, i64, i64) = db
                        .query_row(
                            "SELECT count(*),min(origin_seq),max(origin_seq) FROM work_origin_log",
                            [],
                            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                        )
                        .unwrap();
                    // Unique origin keys plus these bounds prove no gaps.
                    assert_eq!((count, min, max), (5000, 1, 5000));
                    let origin = origin::local_origin(db)?;
                    assert_eq!(origin::high_water(db, &origin.incarnation)?, 5000);
                    Ok(())
                })
                .unwrap();
        }
        let committed = snapshot(&store);
        drop(store);
        let store = reopen(&root);
        assert_eq!(snapshot(&store), committed);
        let (item, events) = task_fixture(9, false);
        let output = invoke(&store, move |call| {
            call.put_item(&item, &events)?;
            call.record_change(position(), &refs(&events), None, T0)
        })
        .unwrap();
        assert_eq!(output.seq, if rollback { 1 } else { 2 });
    }
}

#[test]
fn work_call_every_storage_failure_is_terminal_and_preserves_the_typed_cause() {
    for stage in [
        "allocate",
        "inner-check",
        "shape-check",
        "inner-write",
        "append",
        "feed",
        "ignored-feed",
        "position",
    ] {
        let (root, store) = fixture();
        initialize_origin(&store);
        let (held, held_events) = task_fixture(9, false);
        let held_id = held.id.clone();
        if stage == "inner-check" {
            invoke(&store, move |call| {
                call.put_item(&held, &held_events)?;
                call.record_change(position(), &refs(&held_events), None, T0)
            })
            .unwrap();
        }
        let sql=match stage {
            "allocate"=>"CREATE TRIGGER fail_call BEFORE INSERT ON work_feed BEGIN SELECT RAISE(IGNORE); END;",
            "inner-write"=>"CREATE TRIGGER fail_call BEFORE INSERT ON work_ids WHEN NEW.kind='step' BEGIN SELECT RAISE(ABORT,'private payload'); END;",
            "append"=>"CREATE TRIGGER fail_call BEFORE INSERT ON work_origin_log WHEN NEW.origin_seq=2 BEGIN SELECT RAISE(ABORT,'private payload'); END;",
            "feed"=>"CREATE TRIGGER fail_call BEFORE UPDATE ON work_feed BEGIN SELECT RAISE(ABORT,'private payload'); END;",
            "ignored-feed"=>"CREATE TRIGGER fail_call BEFORE UPDATE ON work_feed BEGIN SELECT RAISE(IGNORE); END;",
            "position"=>"CREATE TRIGGER fail_call AFTER UPDATE ON work_feed BEGIN UPDATE meta SET value='bad-incarnation' WHERE key='incarnation'; END;",
            _=>"SELECT 1;",
        };
        store
            .write(move |db| {
                db.execute_batch(sql).map_err(OfficeError::storage)?;
                Ok(((), None))
            })
            .unwrap();
        let before = snapshot(&store);
        let (mut item, events) = batch(1, 2);
        let (with_step, _) = task_fixture(1, true);
        item.checklist = with_step.checklist;
        if stage == "inner-check" {
            item.checklist[0].id = held_id;
        }
        if stage == "shape-check" {
            item.generation = 0;
        }
        let fail = invoke(&store, move |call| {
            let result = call.put_item(&item, &events);
            let failure = match result {
                Err(e) => e,
                Ok(()) => error(
                    call.record_change(position(), &refs(&events), None, T0),
                    "Store",
                ),
            };
            assert_eq!(tag(&failure), "Store");
            let WorkCallError::Store(cause) = &failure else {
                unreachable!()
            };
            if matches!(stage, "allocate" | "inner-write" | "append" | "feed") {
                assert!(
                    matches!(cause, OfficeError::Storage(_)),
                    "original SQL cause must survive"
                );
            } else {
                assert!(
                    matches!(cause, OfficeError::Invalid(_)),
                    "original native refusal must survive"
                );
            }
            assert!(!format!("{failure:?} {failure}").contains("private payload"));
            error(call.put_item(&item, &events), "CallFailed");
            error(call.record_change(position(), &[], None, T0), "CallFailed");
            Ok(())
        });
        error(fail, "CallFailed");
        assert_eq!(snapshot(&store), before);
        drop(store);
        let store = reopen(&root);
        assert_eq!(snapshot(&store), before);
        if stage != "inner-check" && stage != "shape-check" {
            store
                .write(|db| {
                    db.execute_batch("DROP TRIGGER fail_call")
                        .map_err(OfficeError::storage)?;
                    Ok(((), None))
                })
                .unwrap();
        }
        let (valid, events) = task_fixture(1, true);
        assert_eq!(
            invoke(&store, move |call| {
                call.put_item(&valid, &events)?;
                call.record_change(position(), &refs(&events), None, T0)
            })
            .unwrap()
            .seq,
            if stage == "inner-check" { 2 } else { 1 }
        );
    }
    let (_root, store) = fixture();
    let e = error(
        invoke::<()>(&store, |_| {
            Err(WorkCallError::Store(OfficeError::Invalid(
                "original typed cause",
            )))
        }),
        "Store",
    );
    assert!(matches!(
        e,
        WorkCallError::Store(OfficeError::Invalid("original typed cause"))
    ));
}

#[test]
fn work_call_store_error_reaches_batch_abort_predicate_unchanged() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static CALLED: AtomicBool = AtomicBool::new(false);
    CALLED.store(false, Ordering::SeqCst);
    let (_root, store) = fixture();
    let result = transact_with_abort::<()>(
        &store,
        |_| {
            Err(WorkCallError::Store(OfficeError::Invalid(
                "batch fatal work error",
            )))
        },
        |e| {
            CALLED.store(true, Ordering::SeqCst);
            assert!(matches!(e, OfficeError::Invalid("batch fatal work error")));
            true
        },
    );
    assert!(matches!(
        error(result, "Store"),
        WorkCallError::Store(OfficeError::Invalid("batch fatal work error"))
    ));
    assert!(CALLED.load(Ordering::SeqCst));
}

#[test]
fn work_call_error_slot_replay_and_pre_attempt_failure_never_mask_causes() {
    use crate::store::WriteJob;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };
    use std::time::Duration;
    for mode in ["clean", "begin-failure", "store-sentinel", "other-invalid"] {
        let (root, store) = fixture();
        initialize_origin(&store);
        let before = snapshot(&store);
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
                reply: Box::new(|r| r.unwrap()),
            })
            .unwrap();
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        let slot = Arc::new(Mutex::new(None));
        let attempt_slot = Arc::clone(&slot);
        let attempts = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&attempts);
        let (item, events) = task_fixture(1, true);
        let (replied, reply) = mpsc::channel();
        queue
            .send(WriteJob {
                attempt: Box::new(move |db| {
                    if mode == "other-invalid" && counter.load(Ordering::SeqCst) > 0 {
                        return Err(OfficeError::Invalid("other"));
                    }
                    attempt(db, &attempt_slot, |call| {
                        let n = counter.fetch_add(1, Ordering::SeqCst);
                        if n > 0 && mode == "store-sentinel" {
                            return Err(WorkCallError::Store(OfficeError::Invalid(ROLLBACK)));
                        }
                        call.put_item(&item, &events)?;
                        if n > 0 {
                            call.record_change(position(), &refs(&events), None, T0)?;
                        }
                        Ok(())
                    })
                    .map(|_| None)
                }),
                abort_batch: |_| false,
                reply: Box::new(move |r| {
                    replied.send(finish(r, &slot)).unwrap();
                }),
            })
            .unwrap();
        queue
            .send(WriteJob {
                attempt: Box::new(move |db| {
                    if mode == "begin-failure" {
                        db.pragma_update(None, "query_only", true)
                            .map_err(OfficeError::storage)?;
                    }
                    db.execute_batch("ROLLBACK").map_err(OfficeError::storage)?;
                    Err(OfficeError::Invalid("injected batch abort"))
                }),
                abort_batch: |_| false,
                reply: Box::new(|r| {
                    assert!(matches!(
                        r,
                        Err(OfficeError::Invalid("injected batch abort"))
                    ))
                }),
            })
            .unwrap();
        release.send(()).unwrap();
        let result = reply.recv_timeout(Duration::from_secs(10)).unwrap();
        if mode == "clean" {
            result.unwrap();
            assert_eq!(attempts.load(Ordering::SeqCst), 2);
        } else {
            let e = error(result, "Store");
            if mode == "store-sentinel" {
                assert!(matches!(
                    e,
                    WorkCallError::Store(OfficeError::Invalid(ROLLBACK))
                ));
                assert_eq!(attempts.load(Ordering::SeqCst), 2);
            } else {
                if mode == "other-invalid" {
                    assert!(matches!(
                        e,
                        WorkCallError::Store(OfficeError::Invalid("other"))
                    ));
                } else {
                    assert!(matches!(e, WorkCallError::Store(OfficeError::Storage(_))));
                }
                assert_eq!(attempts.load(Ordering::SeqCst), 1);
            }
            assert_eq!(snapshot(&store), before);
        }
        let committed = snapshot(&store);
        drop(store);
        let store = reopen(&root);
        assert_eq!(snapshot(&store), committed);
        let (item, events) = task_fixture(3, false);
        assert_eq!(
            invoke(&store, move |call| {
                call.put_item(&item, &events)?;
                call.record_change(position(), &refs(&events), None, T0)
            })
            .unwrap()
            .seq,
            if mode == "clean" { 2 } else { 1 }
        );
    }
}

#[test]
fn work_call_logical_errors_cross_the_real_writer_as_the_original_variant() {
    for case in [
        "AlreadyRecorded",
        "NothingToRecord",
        "InvalidRefs",
        "RefsMismatch",
        "DuplicatePut",
    ] {
        let (root, store) = fixture();
        initialize_origin(&store);
        let before = snapshot(&store);
        let (item, events) = task_fixture(1, false);
        let result = invoke(&store, move |call| {
            if case == "NothingToRecord" {
                return call.record_change(position(), &[], None, T0).map(|_| ());
            }
            if case == "DuplicatePut" {
                return call.put_item(&item, &[events[0].clone(), events[0].clone()]);
            }
            call.put_item(&item, &events)?;
            if case == "AlreadyRecorded" {
                call.record_change(position(), &refs(&events), None, T0)?;
            }
            let supplied = if case == "RefsMismatch" {
                vec![org_proto::work::EventRef {
                    target_id: org_work::ulid_from_parts(T0, 999),
                    version: 1,
                }]
            } else {
                vec![]
            };
            call.record_change(position(), &supplied, None, T0)
                .map(|_| ())
        });
        let e = error(result, case);
        assert!(std::error::Error::source(&e).is_none());
        assert_eq!(snapshot(&store), before);
        drop(store);
        let store = reopen(&root);
        assert_eq!(snapshot(&store), before);
    }
    let (_root, store) = fixture();
    let e = error(
        invoke::<()>(&store, |_| {
            Err(WorkCallError::Store(OfficeError::Invalid(
                "private source marker",
            )))
        }),
        "Store",
    );
    assert!(std::error::Error::source(&e).is_some());
    assert!(!format!("{e:?} {e}").contains("private source marker"));
}

#[test]
fn work_call_append_precedes_feed_finalization_when_both_would_fail() {
    let (root, store) = fixture();
    initialize_origin(&store);
    store.write(|db| {
        db.execute_batch("CREATE TRIGGER fail_append BEFORE INSERT ON work_origin_log WHEN NEW.origin_seq=2 BEGIN SELECT RAISE(ABORT,'private origin failure'); END;
            CREATE TRIGGER fail_feed BEFORE UPDATE ON work_feed BEGIN SELECT RAISE(IGNORE); END;")
            .map_err(OfficeError::storage)?;Ok(((),None))
    }).unwrap();
    let before = snapshot(&store);
    let (item, events) = batch(1, 2);
    let result = invoke(&store, move |call| {
        call.put_item(&item, &events)?;
        call.record_change(position(), &refs(&events), None, T0)
            .map(|_| ())
    });
    let cause = error(result, "Store");
    let WorkCallError::Store(OfficeError::Storage(cause)) = cause else {
        panic!("expected original append SQL error before feed finalization")
    };
    assert!(matches!(cause.downcast_ref::<rusqlite::Error>(),
        Some(rusqlite::Error::SqliteFailure(code, _))
            if code.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER));
    assert_eq!(snapshot(&store), before);
    drop(store);
    let store = reopen(&root);
    assert_eq!(snapshot(&store), before);
}

#[test]
fn work_call_outside_writer_is_refused_before_callback_or_feed_allocation() {
    let (root, store) = fixture();
    initialize_origin(&store);
    let before = snapshot(&store);
    let db = crate::store::connect(
        &root.path().join("office.db"),
        &Key::from_bytes([42; 32]),
        false,
        true,
    )
    .unwrap();
    let (item, events) = task_fixture(1, false);
    let mut called = false;
    let result = run(&db, |call| {
        called = true;
        call.put_item(&item, &events)?;
        Ok(())
    });
    assert!(!called);
    assert!(matches!(
        error(result, "Store"),
        WorkCallError::Store(OfficeError::Invalid(
            "work call requires a writer transaction"
        ))
    ));
    assert_eq!(snapshot(&store), before);
    drop(db);
    drop(store);
    let store = reopen(&root);
    assert_eq!(snapshot(&store), before);
}

#[test]
fn poisoned_error_slot_is_recovered_and_cleared_before_the_next_attempt() {
    let (_root, store) = fixture();
    let slot = Arc::new(Mutex::new(Some(WorkCallError::CallFailed)));
    let poison = slot.clone();
    assert!(std::panic::catch_unwind(move || {
        let _held = poison.lock().unwrap();
        panic!("test slot poison");
    })
    .is_err());
    assert!(matches!(
        finish::<()>(Err(OfficeError::Invalid(ROLLBACK)), &slot),
        Err(WorkCallError::CallFailed)
    ));
    let writer_slot = slot.clone();
    store
        .write(move |db| attempt(db, &writer_slot, |_| Ok(())).map(|()| ((), None)))
        .unwrap();
    assert!(slot
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_none());
    let writer_slot = slot.clone();
    let result = store.write(move |db| {
        attempt(db, &writer_slot, |_| {
            Err::<(), _>(WorkCallError::AlreadyRecorded)
        })
        .map(|()| ((), None))
    });
    assert!(matches!(
        finish(result, &slot),
        Err(WorkCallError::AlreadyRecorded)
    ));
}
