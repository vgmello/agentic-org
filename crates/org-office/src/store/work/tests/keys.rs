//! Number scopes are persistence invariants, including tombstones and development-v6 admission.
use super::*;

const STALE_V6: Schema = Schema {
    admit: None,
    version: 6,
    create: |tx| {
        tx.execute_batch(include_str!("fixtures/office-v5.sql"))?;
        crate::store::owned_processes::create_table(tx)
    },
    upgrade: |_, _| unreachable!(),
};

fn index_names(db: &Connection) -> Vec<String> {
    db.prepare("SELECT name FROM sqlite_schema WHERE type='index' AND name IN ('idx_work_items_project_number','idx_work_items_request_number') ORDER BY name")
        .unwrap().query_map([], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap()
}

fn insert(db: &Connection, item: &StoredItem) -> rusqlite::Result<usize> {
    db.execute(
        "INSERT INTO work_items(id,generation,version,data) VALUES (?1,?2,?3,?4)",
        params![
            item.id,
            i64::try_from(item.generation).unwrap(),
            i64::try_from(item.version).unwrap(),
            serde_json::to_string(item).unwrap()
        ],
    )
}

fn key_item(seed: u128, with_step: bool) -> (StoredItem, Vec<WorkEvent>) {
    let (mut item, events) = task_fixture(seed, with_step);
    item.number = Some(1);
    (item, events)
}

#[test]
fn b111_fresh_storage_enforces_both_number_scopes_and_tombstones() {
    let (_root, store) = fixture();
    store
        .write(|db| {
            assert_eq!(
                index_names(db),
                [
                    "idx_work_items_project_number",
                    "idx_work_items_request_number"
                ]
            );
            let tx = db;
            let (first, _) = key_item(800, false);
            insert(tx, &first).unwrap();
            let (mut second, _) = key_item(801, false);
            assert!(insert(tx, &second).is_err(), "same project task number");
            second.project_id = Some("other-project".into());
            insert(tx, &second).unwrap();
            let (mut bug, _) = key_item(802, false);
            bug.kind = org_proto::work::ItemKind::Bug;
            insert(tx, &bug).unwrap();
            let (mut duplicate, _) = key_item(803, false);
            duplicate.kind = bug.kind;
            assert!(insert(tx, &duplicate).is_err(), "same project bug number");
            tx.execute(
                "UPDATE work_items SET data=json_set(data,'$.deleted',json('true')) WHERE id=?1",
                [&bug.id],
            )
            .unwrap();
            assert!(insert(tx, &duplicate).is_err(), "deleted bug keeps number");
            tx.execute(
                "UPDATE work_items SET data=json_set(data,'$.deleted',json('true')) WHERE id=?1",
                [&first.id],
            )
            .unwrap();
            duplicate.kind = first.kind;
            assert!(insert(tx, &duplicate).is_err(), "deleted task keeps number");
            duplicate.number = None;
            insert(tx, &duplicate).unwrap();
            let (mut request, _) = key_item(804, false);
            request.kind = org_proto::work::ItemKind::Request;
            request.project_id = None;
            insert(tx, &request).unwrap();
            let (mut duplicate_request, _) = key_item(805, false);
            duplicate_request.kind = request.kind;
            assert!(
                insert(tx, &duplicate_request).is_err(),
                "request numbers span projects"
            );
            tx.execute(
                "UPDATE work_items SET data=json_set(data,'$.deleted',json('true')) WHERE id=?1",
                [&request.id],
            )
            .unwrap();
            assert!(
                insert(tx, &duplicate_request).is_err(),
                "deleted request keeps global number"
            );
            duplicate_request.project_id = None;
            assert!(
                insert(tx, &duplicate_request).is_err(),
                "request NULL projects share scope"
            );
            duplicate_request.number = None;
            insert(tx, &duplicate_request).unwrap();
            Ok(((), None))
        })
        .unwrap();
}

#[test]
fn b111_development_v6_gains_indexes_and_second_open_is_noop() {
    let (root, old) = fixture_for_schema(&STALE_V6);
    assert!(old.read(|db| Ok(index_names(db))).unwrap().is_empty());
    drop(old);
    let store = reopen(&root);
    let before = store
        .read(|db| {
            assert_eq!(index_names(db).len(), 2);
            Ok(super::metadata::schema_rows(db))
        })
        .unwrap();
    drop(store);
    let store = reopen(&root);
    assert_eq!(
        store
            .read(|db| Ok(super::metadata::schema_rows(db)))
            .unwrap(),
        before
    );
}

#[test]
fn b111_duplicate_requests_refuse_v6_open_atomically() {
    let (root, old) = fixture_for_schema(&STALE_V6);
    old.write(|db| {
        for seed in [810, 811] {
            let (mut item, _) = key_item(seed, false);
            item.kind = org_proto::work::ItemKind::Request;
            item.project_id = Some(format!("project-{seed}"));
            insert(db, &item).map_err(OfficeError::storage)?;
        }
        Ok(((), None))
    })
    .unwrap();
    let before = old
        .read(|db| {
            Ok((
                super::metadata::schema_rows(db),
                super::metadata::old_rows(db),
            ))
        })
        .unwrap();
    drop(old);
    let error = Store::open(
        &root.path().join("office.db"),
        &Key::from_bytes([42; 32]),
        Opening::Existing,
        &OFFICE_SCHEMA,
    )
    .err()
    .expect("duplicate request numbers must refuse development-v6 admission");
    assert!(
        format!("{error:?}").contains("work"),
        "clear work key admission error: {error:?}"
    );
    let old = Store::open(
        &root.path().join("office.db"),
        &Key::from_bytes([42; 32]),
        Opening::Existing,
        &STALE_V6,
    )
    .unwrap();
    assert_eq!(
        old.read(|db| Ok((
            super::metadata::schema_rows(db),
            super::metadata::old_rows(db)
        )))
        .unwrap(),
        before
    );
}

const LEGACY_V5: Schema = Schema {
    admit: None,
    version: 5,
    create: |tx| tx.execute_batch(include_str!("fixtures/office-v5.sql")),
    upgrade: |_, _| unreachable!(),
};

#[test]
fn b111_upgrade_and_current_admission_reject_each_invalid_key_atomically() {
    for schema in [&LEGACY_V5, &STALE_V6] {
        for case in [
            "task-duplicate",
            "bug-duplicate",
            "request-duplicate",
            "task-no-project",
            "bug-no-project",
        ] {
            let (root, old) = fixture_for_schema(schema);
            old.write(move |db| {
                for seed in [820, 821] {
                    let (mut item, _) = key_item(seed, false);
                    item.kind = if case.starts_with("bug") {
                        org_proto::work::ItemKind::Bug
                    } else if case.starts_with("request") {
                        org_proto::work::ItemKind::Request
                    } else {
                        org_proto::work::ItemKind::Task
                    };
                    if case.ends_with("no-project") {
                        item.project_id = None;
                        item.number = Some(seed as u64);
                    }
                    if case.starts_with("request") {
                        item.project_id = Some(format!("project-{seed}"));
                    }
                    insert(db, &item).map_err(OfficeError::storage)?;
                }
                Ok(((), None))
            })
            .unwrap();
            let before = old
                .read(|db| {
                    Ok((
                        super::metadata::schema_rows(db),
                        super::metadata::old_rows(db),
                    ))
                })
                .unwrap();
            drop(old);
            let error = Store::open(
                &root.path().join("office.db"),
                &Key::from_bytes([42; 32]),
                Opening::Existing,
                &OFFICE_SCHEMA,
            )
            .err()
            .expect("invalid work key must fail admission");
            if case.ends_with("no-project") {
                assert!(matches!(
                    error,
                    OfficeError::Invalid(
                        "work_items contains a numbered task or bug without a project"
                    )
                ));
            } else {
                assert!(format!("{error:?}").contains("UNIQUE constraint failed"));
            }
            let old = Store::open(
                &root.path().join("office.db"),
                &Key::from_bytes([42; 32]),
                Opening::Existing,
                schema,
            )
            .unwrap();
            assert_eq!(
                old.read(|db| Ok((
                    super::metadata::schema_rows(db),
                    super::metadata::old_rows(db)
                )))
                .unwrap(),
                before,
                "v{} {case}",
                schema.version
            );
        }
    }
}

#[test]
fn b111_upgrade_admits_valid_shapes_without_changing_rows() {
    for schema in [&LEGACY_V5, &STALE_V6] {
        let (root, old) = fixture_for_schema(schema);
        old.write(|db| {
            for (seed, kind, number, project) in [
                (830, org_proto::work::ItemKind::Task, Some(1), Some("one")),
                (831, org_proto::work::ItemKind::Bug, Some(1), Some("one")),
                (832, org_proto::work::ItemKind::Task, Some(1), Some("two")),
                (833, org_proto::work::ItemKind::Task, None, None),
                (834, org_proto::work::ItemKind::Bug, None, None),
                (835, org_proto::work::ItemKind::Request, Some(1), None),
                (836, org_proto::work::ItemKind::Unknown, Some(1), None),
                (
                    837,
                    org_proto::work::ItemKind::Request,
                    Some(2),
                    Some("one"),
                ),
            ] {
                let (mut item, _) = key_item(seed, false);
                item.kind = kind;
                item.number = number;
                item.project_id = project.map(str::to_owned);
                insert(db, &item).map_err(OfficeError::storage)?;
            }
            Ok(((), None))
        })
        .unwrap();
        let before = old.read(|db| Ok(super::metadata::old_rows(db))).unwrap();
        drop(old);
        let store = reopen(&root);
        store
            .read(|db| {
                assert_eq!(index_names(db).len(), 2);
                let after = super::metadata::old_rows(db);
                for (name, rows) in &before {
                    assert_eq!(&after.iter().find(|(n, _)| n == name).unwrap().1, rows);
                }
                Ok(())
            })
            .unwrap();
    }
}

#[test]
fn b111_number_indexes_cover_update_replace_rollback_and_query_plans() {
    let (root, store) = fixture();
    initialize_origin(&store);
    let (first, events) = key_item(840, false);
    write(&store, first, events).unwrap();
    let (mut second, events) = key_item(841, false);
    second.number = Some(2);
    write(&store, second.clone(), events.clone()).unwrap();
    let before = super::snapshot(&store);
    let (duplicate, duplicate_events) = key_item(843, false);
    assert!(write(&store, duplicate, duplicate_events).is_err());
    assert_eq!(super::snapshot(&store), before);
    let (mut updated, event) = super::advance(&second, events.first().unwrap());
    updated.number = Some(1);
    assert!(write(&store, updated, vec![event]).is_err());
    assert_eq!(super::snapshot(&store), before);
    store.write(move |db| {
        let sql="UPDATE work_items SET data=json_set(data,'$.number',1) WHERE id=?1";
        assert!(db.execute(sql,[&second.id]).is_err());
        second.number=Some(1);
        assert!(db.execute("INSERT OR REPLACE INTO work_items(id,generation,version,data) VALUES (?1,?2,?3,?4)",
            params![second.id,i64::try_from(second.generation).unwrap(),i64::try_from(second.version).unwrap(),serde_json::to_string(&second).unwrap()]).is_err());
        assert_eq!(db.query_row("SELECT count(*) FROM work_items",[],|r|r.get::<_,i64>(0)).unwrap(),2);
        for (query,index) in [
            ("EXPLAIN QUERY PLAN SELECT id FROM work_items WHERE json_extract(data,'$.project_id')='project1' AND json_extract(data,'$.kind')='task' AND json_extract(data,'$.kind') IN ('task','bug') AND json_extract(data,'$.number')=1", "idx_work_items_project_number"),
            ("EXPLAIN QUERY PLAN SELECT id FROM work_items WHERE json_extract(data,'$.kind')='request' AND json_extract(data,'$.number')=1", "idx_work_items_request_number"),
        ] {
            let detail:String=db.query_row(query,[],|r|r.get(3)).unwrap();assert!(detail.contains(index),"{detail}");
        }
        Ok(((),None))
    }).unwrap();
    let before = store.read(|db| Ok(super::metadata::old_rows(db))).unwrap();
    let error = store.write(|db| {
        let (mut item, _) = key_item(842, false);
        item.number = Some(10);
        insert(db, &item).map_err(OfficeError::storage)?;
        Err::<((), Option<crate::store::Undo>), _>(OfficeError::Invalid("injected rollback"))
    });
    assert!(error.is_err());
    assert_eq!(
        store.read(|db| Ok(super::metadata::old_rows(db))).unwrap(),
        before
    );
    drop(store);
    let store = reopen(&root);
    assert_eq!(
        store.read(|db| Ok(super::metadata::old_rows(db))).unwrap(),
        before
    );
}

#[test]
fn b111_expression_indexes_refuse_malformed_json_without_writes() {
    let (_root, store) = fixture();
    initialize_origin(&store);
    let (item, events) = key_item(850, false);
    write(&store, item.clone(), events).unwrap();
    let before = super::snapshot(&store);
    let error = store
        .write(move |db| {
            db.execute("UPDATE work_items SET data='{' WHERE id=?1", [&item.id])
                .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .unwrap_err();
    assert!(format!("{error:?}").contains("malformed JSON"));
    assert_eq!(super::snapshot(&store), before);
}

#[test]
fn b111_native_parent_guard_preserves_link_read_storage_errors_without_writes() {
    use crate::store::work::assembly::OfficeWorkStore;
    use org_proto::work::{ItemStatus, TaskSetStatusParams};
    use std::error::Error;
    let (_root, db) = fixture();
    initialize_origin(&db);
    let db = Arc::new(db);
    let (parent, events) = key_item(860, false);
    write(&db, parent.clone(), events).unwrap();
    let (mut child, events) = key_item(861, false);
    child.number = Some(2);
    child.links.push(StoredLink {
        id: org_work::ulid_from_parts(T0, 999_860),
        kind: LinkKind::PartOf,
        to: LinkTarget::Item {
            item_id: parent.id.clone(),
        },
        created_by: "owner1".into(),
        created_at_ms: T0,
        removed: false,
    });
    write(&db, child.clone(), events).unwrap();
    // MemOrg implements the shared native-test OrgSource port; reuse that adapter.
    let native = OfficeWorkStore::new(
        db.clone(),
        Arc::new(MemOrg::new("owner1").with_project("project1", "one", &[])),
        Arc::new(|| T0),
    )
    .unwrap();
    let clock = native.clock();
    let service = WorkService::new(native, clock, "OFFICE1");
    let call = |seed| {
        WorkCall::TaskSetStatus(TaskSetStatusParams {
            change_id: org_work::ulid_from_parts(T0, seed),
            item_id: parent.id.clone(),
            expected_version: parent.version,
            status: ItemStatus::Done,
            resolution: None,
            blocked_reason: None,
            anyway_reason: Some("Must not bypass subtasks".into()),
            shortcut: None,
        })
    };
    let refusal = service.call(Caller::app("owner1"), call(999_861)).unwrap();
    assert!(
        matches!(refusal,TxnOutput::Refused(error) if error.code==org_proto::ErrorCode::InvalidRequest)
    );
    let child_id = child.id;
    db.write(move |db| {
        // Keep a live link and valid indexed JSON, but make the selected child's decode fail.
        db.execute(
            "UPDATE work_items SET data=json_remove(data,'$.title') WHERE id=?1",
            [&child_id],
        )
        .map_err(OfficeError::storage)?;
        Ok(((), None))
    })
    .unwrap();
    let before = db.read(|db| Ok(super::metadata::old_rows(db))).unwrap();
    let error = service
        .call(Caller::app("owner1"), call(999_862))
        .unwrap_err();
    let mut cause = error.source();
    let mut json_error = false;
    while let Some(current) = cause {
        json_error |= current.downcast_ref::<serde_json::Error>().is_some();
        cause = current.source();
    }
    assert!(
        json_error,
        "preserve the child's typed JSON failure: {error:?}"
    );
    assert_eq!(
        db.read(|db| Ok(super::metadata::old_rows(db))).unwrap(),
        before
    );
}

#[test]
fn b111_duplicate_admission_preserves_logical_state_in_wal_and_delete_journals() {
    for journal in ["WAL", "DELETE"] {
        let (root, old) = fixture_for_schema(&STALE_V6);
        old.write(|db| {
            for seed in [870, 871] {
                let (mut item, _) = key_item(seed, false);
                item.kind = org_proto::work::ItemKind::Request;
                item.project_id = Some(format!("project-{seed}"));
                insert(db, &item).map_err(OfficeError::storage)?;
            }
            Ok(((), None))
        })
        .unwrap();
        let before = old
            .read(|db| {
                Ok((
                    super::metadata::schema_rows(db),
                    super::metadata::old_rows(db),
                ))
            })
            .unwrap();
        drop(old);
        let path = root.path().join("office.db");
        let key = Key::from_bytes([42; 32]);
        // Reuse the store's initialized, keyed SQLCipher opener to construct the legacy
        // journal fixture after its writer closes. No transaction may change journal mode.
        let db = crate::store::connect(&path, &key, false, true).unwrap();
        let actual: String = db
            .pragma_update_and_check(None, "journal_mode", journal, |r| r.get(0))
            .unwrap();
        assert_eq!(actual.to_ascii_uppercase(), journal);
        drop(db);
        let original_bytes = std::fs::read(&path).unwrap();
        let error = Store::open(&path, &key, Opening::Existing, &OFFICE_SCHEMA)
            .err()
            .expect("duplicate numbers refuse admission in either journal mode");
        assert!(format!("{error:?}").contains("UNIQUE constraint failed"));
        let db = crate::store::connect(&path, &key, false, true).unwrap();
        assert_eq!(
            (
                super::metadata::schema_rows(&db),
                super::metadata::old_rows(&db)
            ),
            before
        );
        assert_eq!(
            db.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
                .unwrap(),
            6
        );
        assert!(
            index_names(&db).is_empty(),
            "first index must roll back when the second fails"
        );
        let actual: String = db
            .pragma_query_value(None, "journal_mode", |r| r.get(0))
            .unwrap();
        assert_eq!(actual, "wal");
        drop(db);
        if journal == "DELETE" {
            // Rowan01M7A79: the pre-existing WAL header switch is expected; transactional
            // atomicity means the schema, rows and user_version above, not byte identity.
            assert_ne!(std::fs::read(&path).unwrap(), original_bytes);
        }
    }
}
