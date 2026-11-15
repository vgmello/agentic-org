use super::*;

const V4: Schema = Schema {
    admit: None,
    version: 4,
    create: |tx| tx.execute_batch(include_str!("fixtures/office-v4.sql")),
    upgrade: |_, _| unreachable!("fixture creates a released v4 file"),
};

#[test]
fn work_v5_schema_has_the_three_storage_tables_and_expiry_index() {
    let (_root, store) = fixture();
    store.read(|db| {
        let tables: Vec<String> = db.prepare("SELECT name FROM sqlite_schema WHERE name IN ('work_change_results','work_number_counters','work_clock') ORDER BY name").unwrap().query_map([], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
        assert_eq!(tables, ["work_change_results", "work_clock", "work_number_counters"]);
        assert_eq!(db.pragma_query_value::<i64,_>(None,"user_version",|r|r.get(0)).unwrap(),6);
        assert_eq!(db.query_row("SELECT count(*) FROM sqlite_schema WHERE name='idx_work_change_results_expiry'",[],|r|r.get::<_,i64>(0)).unwrap(),1);
        assert_eq!(db.query_row("SELECT name FROM pragma_index_info('idx_work_change_results_expiry')",[],|r|r.get::<_,String>(0)).unwrap(),"expires_at_ms");
        let flags:Vec<(String,i64,i64)>=db.prepare("SELECT name,wr,strict FROM pragma_table_list WHERE schema='main' AND name IN ('work_change_results','work_number_counters','work_clock') ORDER BY name").unwrap().query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap();
        assert_eq!(flags,[("work_change_results".into(),1,1),("work_clock".into(),0,1),("work_number_counters".into(),1,1)]);
        assert_eq!(db.query_row("SELECT type FROM pragma_table_info('work_clock') WHERE name='id'",[],|r|r.get::<_,String>(0)).unwrap(),"INTEGER");
        Ok(())
    }).unwrap();
}

#[test]
fn work_v4_fixture_is_independent_and_retains_native_work_tables() {
    let (_root, store) = fixture_for_schema(&V4);
    store.read(|db| {
        assert_eq!(db.pragma_query_value::<i64,_>(None,"user_version",|r|r.get(0)).unwrap(),4);
        assert_eq!(db.query_row("SELECT count(*) FROM sqlite_schema WHERE name IN ('work_items','work_events','work_ids','work_feed','work_origin_log','work_origin_counter')",[],|r|r.get::<_,i64>(0)).unwrap(),6);
        Ok(())
    }).unwrap();
}

use super::super::metadata::{
    allocate_number, change_result, high_water, put_change_result, raise_high_water,
};
use org_proto::work::{WorkAccepted, WorkPosition, WorkStatus};
use org_proto::{ApiError, ErrorCode};
use org_work::{NumberScope, StoredResult};

fn accepted() -> StoredResult {
    StoredResult {
        params_hash: [7; 32],
        answer: Ok(WorkAccepted {
            status: WorkStatus::Committed,
            position: Some(WorkPosition {
                incarnation: 7,
                seq: 11,
            }),
            items: vec![],
            features: vec![],
            docs: vec![],
        }),
        expires_at_ms: 0,
    }
}
fn meta_write<T: Send + 'static>(
    store: &Store,
    f: impl Fn(&Connection) -> Result<T, OfficeError> + Send + 'static,
) -> Result<T, OfficeError> {
    store.write(move |db| f(db).map(|v| (v, None)))
}
fn sql_failure(error: OfficeError) {
    let OfficeError::Storage(cause) = error else {
        panic!("expected native SQL failure")
    };
    assert!(
        cause.downcast_ref::<rusqlite::Error>().is_some(),
        "a writer panic is not SQL failure"
    );
}
fn meta_rows(db: &Connection) -> Vec<Vec<Vec<Value>>> {
    [
        "SELECT * FROM work_change_results ORDER BY change_id",
        "SELECT * FROM work_number_counters ORDER BY scope",
        "SELECT * FROM work_clock ORDER BY id",
    ]
    .iter()
    .map(|sql| {
        let mut q = db.prepare(sql).unwrap();
        let n = q.column_count();
        q.query_map([], |r| {
            (0..n)
                .map(|i| r.get(i))
                .collect::<rusqlite::Result<Vec<Value>>>()
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
    })
    .collect()
}
fn saved_meta(store: &Store) -> Vec<Vec<Vec<Value>>> {
    store.read(|db| Ok(meta_rows(db))).unwrap()
}

#[test]
fn work_v5_results_pin_both_answer_bytes_expired_replay_and_null_principals() {
    let (root, store) = fixture();
    let ok = accepted();
    let err = StoredResult {
        params_hash: [9; 32],
        answer: Err(ApiError::new(ErrorCode::InvalidRequest, "refused")),
        expires_at_ms: i64::MAX as u64,
    };
    let expected = [ok.clone(), err.clone()];
    meta_write(&store, move |db| {
        assert!(change_result(db, "absent")?.is_none());
        put_change_result(db, "ok", &ok)?;
        put_change_result(db, "err", &err)?;
        Ok(())
    })
    .unwrap();
    store.read(|db| {
        for (id,value) in ["ok","err"].into_iter().zip(expected.clone()) { assert_eq!(change_result(db,id)?,Some(value)); }
        let answer=|id|db.query_row("SELECT answer FROM work_change_results WHERE change_id=?1",[id],|r|r.get::<_,String>(0)).unwrap();
        assert_eq!(answer("ok"),r#"{"ok":{"status":"committed","position":{"incarnation":7,"seq":11},"items":[]}}"#);
        assert_eq!(answer("err"),r#"{"err":{"code":"invalid_request","message":"refused"}}"#);
        let nulls:i64=db.query_row("SELECT count(*) FROM work_change_results WHERE principal_account IS NULL AND principal_member IS NULL AND actor_kind IS NULL",[],|r|r.get(0)).unwrap(); assert_eq!(nulls,2);
        Ok(())
    }).unwrap();
    let before = saved_meta(&store);
    sql_failure(
        meta_write(&store, |db| {
            let mut different = accepted();
            different.params_hash = [8; 32];
            different.expires_at_ms = 1;
            put_change_result(db, "ok", &different)
        })
        .unwrap_err(),
    );
    assert_eq!(saved_meta(&store), before);
    drop(store);
    let store = reopen(&root);
    assert_eq!(saved_meta(&store), before);
    store
        .read(|db| {
            assert_eq!(change_result(db, "ok")?, Some(expected[0].clone()));
            Ok(())
        })
        .unwrap();
}

#[test]
fn work_v5_result_corruption_is_fixed_table_error_and_unknown_inner_values_are_tolerated() {
    for answer in [
        "private payload",
        r#"{"Ok":{}}"#,
        r#"{"ok":null}"#,
        r#"{"ok":{"status":"committed"},"err":{"code":"invalid_request","message":"private"}}"#,
        r#"{}"#,
    ] {
        let (_root, store) = fixture();
        let answer = answer.to_owned();
        meta_write(&store, move |db| {
            put_change_result(db, "bad", &accepted())?;
            db.execute("UPDATE work_change_results SET answer=?1", [&answer])
                .map_err(OfficeError::storage)?;
            Ok(())
        })
        .unwrap();
        let error = store.read(|db| change_result(db, "bad")).unwrap_err();
        assert!(matches!(
            error,
            OfficeError::Invalid("work_change_results contains an invalid answer")
        ));
        assert!(!error.to_string().contains("private"));
    }
    let (_root, store) = fixture();
    meta_write(&store, |db| {
        put_change_result(db, "future", &accepted())?;
        db.execute(
            "UPDATE work_change_results SET answer=?1",
            [r#"{"ok":{"status":"future","items":[]}}"#],
        )
        .map_err(OfficeError::storage)?;
        Ok(())
    })
    .unwrap();
    store
        .read(|db| {
            assert_eq!(
                change_result(db, "future")?.unwrap().answer.unwrap().status,
                WorkStatus::Unknown
            );
            Ok(())
        })
        .unwrap();
    meta_write(&store, |db| {
        db.execute(
            "UPDATE work_change_results SET answer=?1",
            [r#"{"err":{"code":"future","message":"refused"}}"#],
        )
        .map_err(OfficeError::storage)?;
        Ok(())
    })
    .unwrap();
    store
        .read(|db| {
            assert_eq!(
                change_result(db, "future")?
                    .unwrap()
                    .answer
                    .unwrap_err()
                    .code,
                ErrorCode::Unknown
            );
            Ok(())
        })
        .unwrap();
    for (column, value, message) in [
        (
            "params_hash",
            "zeroblob(31)",
            "work_change_results contains an invalid hash",
        ),
        (
            "params_hash",
            "zeroblob(33)",
            "work_change_results contains an invalid hash",
        ),
        (
            "expires_at_ms",
            "-1",
            "work_change_results contains an invalid expiry",
        ),
    ] {
        let (_root, store) = fixture();
        let sql = format!("UPDATE work_change_results SET {column}={value}");
        meta_write(&store, move |db| {
            put_change_result(db, "bad", &accepted())?;
            db.pragma_update(None, "ignore_check_constraints", true)
                .map_err(OfficeError::storage)?;
            db.execute(&sql, []).map_err(OfficeError::storage)?;
            db.pragma_update(None, "ignore_check_constraints", false)
                .map_err(OfficeError::storage)?;
            Ok(())
        })
        .unwrap();
        assert!(
            matches!(store.read(|db|change_result(db,"bad")),Err(OfficeError::Invalid(m)) if m==message)
        );
    }
}

#[test]
fn work_v5_numbers_use_existing_scope_keys_and_never_leave_rollback_gaps() {
    let (root, store) = fixture();
    let scopes = [
        NumberScope::Tasks {
            project_id: "p".into(),
        },
        NumberScope::Bugs {
            project_id: "p".into(),
        },
        NumberScope::Requests,
        NumberScope::Tasks {
            project_id: "q".into(),
        },
    ];
    meta_write(&store, move |db| {
        for scope in &scopes {
            for n in 1..=3 {
                assert_eq!(allocate_number(db, scope)?, n);
            }
        }
        let keys: Vec<String> = db
            .prepare("SELECT scope FROM work_number_counters ORDER BY scope")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(keys, ["bug:p", "request", "task:p", "task:q"]);
        Ok(())
    })
    .unwrap();
    let before = saved_meta(&store);
    assert!(matches!(
        meta_write::<()>(&store, |db| {
            allocate_number(db, &NumberScope::Requests)?;
            put_change_result(db, "rolled-back", &accepted())?;
            raise_high_water(db, 99)?;
            Err(OfficeError::Invalid("metadata rollback"))
        }),
        Err(OfficeError::Invalid("metadata rollback"))
    ));
    assert_eq!(saved_meta(&store), before);
    drop(store);
    let store = reopen(&root);
    assert_eq!(saved_meta(&store), before);
    assert_eq!(
        meta_write(&store, |db| allocate_number(db, &NumberScope::Requests)).unwrap(),
        4
    );
    for project in ["", ":", "a:b"] {
        for bug in [false, true] {
            let project = project.to_owned();
            let before = saved_meta(&store);
            assert!(matches!(
                meta_write(&store, move |db| allocate_number(
                    db,
                    &if bug {
                        NumberScope::Bugs {
                            project_id: project.clone(),
                        }
                    } else {
                        NumberScope::Tasks {
                            project_id: project.clone(),
                        }
                    }
                )),
                Err(OfficeError::Invalid("invalid work number scope"))
            ));
            assert_eq!(saved_meta(&store), before);
        }
    }
    meta_write(&store, |db| {
        db.execute(
            "UPDATE work_number_counters SET last=?1 WHERE scope='request'",
            [i64::MAX - 1],
        )
        .map_err(OfficeError::storage)?;
        assert_eq!(
            allocate_number(db, &NumberScope::Requests)?,
            i64::MAX as u64
        );
        Ok(())
    })
    .unwrap();
    let before = saved_meta(&store);
    sql_failure(meta_write(&store, |db| allocate_number(db, &NumberScope::Requests)).unwrap_err());
    assert_eq!(saved_meta(&store), before);
    let error = meta_write(&store, |db| {
        db.pragma_update(None, "ignore_check_constraints", true)
            .map_err(OfficeError::storage)?;
        db.execute(
            "UPDATE work_number_counters SET last=-2 WHERE scope='request'",
            [],
        )
        .map_err(OfficeError::storage)?;
        let result = allocate_number(db, &NumberScope::Requests);
        db.pragma_update(None, "ignore_check_constraints", false)
            .map_err(OfficeError::storage)?;
        result
    })
    .unwrap_err();
    assert!(matches!(
        error,
        OfficeError::Invalid("work_number_counters contains an invalid number")
    ));
    assert_eq!(saved_meta(&store), before);
}

#[test]
fn work_v5_clock_is_monotone_bounded_and_durable_without_a_default_row() {
    let (root, store) = fixture();
    assert_eq!(store.read(high_water).unwrap(), 0);
    meta_write(&store, |db| {
        for (ms, expected) in [(0, 0), (10, 10)] {
            raise_high_water(db, ms)?;
            assert_eq!(high_water(db)?, expected);
        }
        Ok(())
    })
    .unwrap();
    assert!(matches!(
        meta_write::<()>(&store, |db| {
            raise_high_water(db, 11)?;
            Err(OfficeError::Invalid("clock rollback"))
        }),
        Err(OfficeError::Invalid("clock rollback"))
    ));
    assert_eq!(store.read(high_water).unwrap(), 10);
    meta_write(&store, |db| {
        for (ms, expected) in [
            (9, 10),
            (10, 10),
            (11, 11),
            (i64::MAX as u64, i64::MAX as u64),
        ] {
            raise_high_water(db, ms)?;
            assert_eq!(high_water(db)?, expected);
        }
        Ok(())
    })
    .unwrap();
    let before = saved_meta(&store);
    for ms in [i64::MAX as u64 + 1, u64::MAX] {
        assert!(matches!(
            meta_write(&store, move |db| raise_high_water(db, ms)),
            Err(OfficeError::Invalid("work integer is out of range"))
        ));
        let mut value = accepted();
        value.expires_at_ms = ms;
        assert!(matches!(
            meta_write(&store, move |db| put_change_result(db, "too-late", &value)),
            Err(OfficeError::Invalid("work integer is out of range"))
        ));
        assert_eq!(saved_meta(&store), before);
    }
    drop(store);
    let store = reopen(&root);
    assert_eq!(saved_meta(&store), before);
    assert_eq!(store.read(high_water).unwrap(), i64::MAX as u64);
    meta_write(&store, |db| {
        db.pragma_update(None, "ignore_check_constraints", true)
            .map_err(OfficeError::storage)?;
        db.execute("UPDATE work_clock SET high_water_ms=-1", [])
            .map_err(OfficeError::storage)?;
        db.pragma_update(None, "ignore_check_constraints", false)
            .map_err(OfficeError::storage)?;
        Ok(())
    })
    .unwrap();
    assert!(matches!(
        store.read(high_water),
        Err(OfficeError::Invalid(
            "work_clock contains an invalid high water"
        ))
    ));
}

#[test]
fn work_v5_sql_guards_cover_insert_update_replace_delete_and_reopen() {
    let (root, store) = fixture();
    let tables = [
        (
            "work_change_results",
            vec![
                "change_id",
                "params_hash",
                "answer",
                "expires_at_ms",
                "actor_kind",
            ],
            vec![
                "'base'".to_owned(),
                "zeroblob(32)".into(),
                "'{}'".into(),
                "0".into(),
                "NULL".into(),
            ],
            vec![
                (0, "''".into()),
                (0, format!("'{}'", "x".repeat(129))),
                (0, "NULL".into()),
                (1, "zeroblob(31)".into()),
                (1, "zeroblob(33)".into()),
                (1, format!("'{}'", "x".repeat(32))),
                (1, "NULL".into()),
                (1, "'text'".into()),
                (2, "NULL".into()),
                (3, "-1".into()),
                (3, "9223372036854775808".into()),
                (3, "NULL".into()),
                (3, "'text'".into()),
                (4, "''".into()),
            ],
        ),
        (
            "work_number_counters",
            vec!["scope", "last"],
            vec!["'base'".into(), "0".into()],
            vec![
                (0, "NULL".into()),
                (1, "-1".into()),
                (1, "9223372036854775808".into()),
                (1, "NULL".into()),
                (1, "'text'".into()),
            ],
        ),
        (
            "work_clock",
            vec!["id", "high_water_ms"],
            vec!["1".into(), "0".into()],
            vec![
                (0, "NULL".into()),
                (0, "0".into()),
                (0, "2".into()),
                (1, "-1".into()),
                (1, "9223372036854775808".into()),
                (1, "NULL".into()),
                (1, "'text'".into()),
            ],
        ),
    ];
    for (table, columns, values, invalid) in tables {
        let insert = format!(
            "INSERT INTO {table}({}) VALUES ({})",
            columns.join(","),
            values.join(",")
        );
        let seed = insert.clone();
        meta_write(&store, move |db| {
            db.execute(&seed, []).map_err(OfficeError::storage)?;
            Ok(())
        })
        .unwrap();
        let before = saved_meta(&store);
        for (col, bad) in invalid {
            for op in ["INSERT", "UPDATE", "INSERT OR REPLACE"] {
                let mut args = values.clone();
                args[col] = bad.clone();
                let sql = if op == "UPDATE" {
                    format!("UPDATE {table} SET {}={bad}", columns[col])
                } else {
                    format!(
                        "{op} INTO {table}({}) VALUES ({})",
                        columns.join(","),
                        args.join(",")
                    )
                };
                let label = sql.clone();
                let result = meta_write(&store, move |db| Ok(db.execute(&sql, []))).unwrap();
                assert!(
                    matches!(result, Err(rusqlite::Error::SqliteFailure(..))),
                    "{label}"
                );
                assert_eq!(saved_meta(&store), before, "{label}");
            }
        }
        let delete = format!("DELETE FROM {table}");
        meta_write(&store, move |db| {
            assert_eq!(db.execute(&delete, []).map_err(OfficeError::storage)?, 1);
            db.execute(&insert, []).map_err(OfficeError::storage)?;
            Ok(())
        })
        .unwrap();
        assert_eq!(saved_meta(&store), before);
    }
    meta_write(&store, |db| {
        for id in ["x".to_owned(), "x".repeat(128), "é".repeat(128)] {
            put_change_result(db, &id, &accepted())?;
        }
        db.execute(
            "UPDATE work_change_results SET actor_kind='k' WHERE change_id='base'",
            [],
        )
        .map_err(OfficeError::storage)?;
        db.execute("UPDATE work_change_results SET principal_account='account',principal_member='member' WHERE change_id='base'",[]).map_err(OfficeError::storage)?;
        db.execute(
            "UPDATE work_change_results SET actor_kind='future_kind' WHERE change_id='base'",
            [],
        )
        .map_err(OfficeError::storage)?;
        Ok(())
    })
    .unwrap();
    let before = saved_meta(&store);
    drop(store);
    let store = reopen(&root);
    assert_eq!(saved_meta(&store), before);
}

const V5: Schema = Schema {
    admit: None,
    version: 5,
    create: |tx| tx.execute_batch(include_str!("fixtures/office-v5.sql")),
    upgrade: |_, _| unreachable!(),
};

const V1: Schema = Schema {
    admit: None,
    version: 1,
    create: |tx| tx.execute_batch(include_str!("fixtures/office-v1.sql")),
    upgrade: |_, _| unreachable!(),
};
const V2: Schema = Schema {
    admit: None,
    version: 2,
    create: |tx| tx.execute_batch(include_str!("fixtures/office-v2.sql")),
    upgrade: |_, _| unreachable!(),
};
const LEGACY_V3: Schema = Schema {
    admit: None,
    version: 3,
    create: |tx| tx.execute_batch(include_str!("fixtures/office-v3.sql")),
    upgrade: |_, _| unreachable!(),
};

pub(super) fn schema_rows(db: &Connection) -> Vec<(String, String, String, Option<String>)> {
    db.prepare("SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}
pub(super) fn old_rows(db: &Connection) -> Vec<(String, Vec<Vec<Value>>)> {
    let names: Vec<String> = db
        .prepare("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    names
        .into_iter()
        .map(|name| {
            let sql = format!("SELECT * FROM \"{name}\"");
            let n = db.prepare(&sql).unwrap().column_count();
            let sql = format!(
                "{sql} ORDER BY {}",
                (1..=n).map(|n| n.to_string()).collect::<Vec<_>>().join(",")
            );
            let rows = db
                .prepare(&sql)
                .unwrap()
                .query_map([], |r| {
                    (0..n)
                        .map(|i| r.get(i))
                        .collect::<rusqlite::Result<Vec<Value>>>()
                })
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            (name, rows)
        })
        .collect()
}
fn seed_legacy(store: &Store, version: i64) {
    meta_write(store,move |db| {
        db.execute_batch("INSERT INTO meta VALUES ('fixture','retained');").map_err(OfficeError::storage)?;
        if version<4 { db.execute("INSERT INTO meta VALUES ('message_sequence','41')",[]).map_err(OfficeError::storage)?; }
        if version>=2 {db.execute_batch("INSERT INTO msg_headers(id,thread_id,origin_office_id,origin_incarnation,origin_seq,local_seq,org_epoch,org_seq,from_member_id,from_display_name,to_member_id,to_display_name,kind,created_at,body_bytes,content_hash) VALUES ('message','thread','OFFICE1','incarnation',1,41,0,9,'owner','Owner','agent','Agent','msg','2026-11-11T00:00:00Z',5,zeroblob(32));
             INSERT INTO msg_bodies VALUES ('message',0,X'68656c6c6f',X'7b7d');
             INSERT INTO msg_threads VALUES ('thread','subject','2026-11-11T00:00:00Z','message','2026-11-11T00:00:00Z',1,'[\"owner\",\"agent\"]');
             INSERT INTO msg_thread_participants VALUES ('agent','thread');
             INSERT INTO msg_state(id,state,updated_at,owner_office_id,origin_incarnation,origin_seq,org_epoch,org_seq) VALUES ('message','new','2026-11-11T00:00:00Z','OFFICE1','incarnation',1,0,9);
             INSERT INTO msg_unread(to_member_id,local_seq,id,from_member_id,kind,created_at) VALUES ('agent',41,'message','owner','msg','2026-11-11T00:00:00Z');").map_err(OfficeError::storage)?;}
        if version>=3 {db.execute("INSERT INTO member_status VALUES ('agent','working',NULL,'2026-11-11T00:00:00Z','2026-11-12T00:00:00Z')",[]).map_err(OfficeError::storage)?;}
        Ok(())
    }).unwrap();
    if version == 4 {
        initialize_origin(store);
        let mut h = history();
        for (item, _) in &mut h.states {
            item.number = None;
        }
        super::super::call::transact(store, move |call| {
            let mut refs = Vec::new();
            for (item, events) in &h.states {
                call.put_item(item, events)?;
                refs.extend(events.iter().map(|e| org_proto::work::EventRef {
                    target_id: e.target_id.clone(),
                    version: e.version,
                }));
            }
            call.record_change(
                org_proto::Position { epoch: 2, seq: 9 },
                &refs,
                Some("legacy"),
                T0,
            )?;
            Ok(())
        })
        .unwrap();
    }
}
fn open_old(root: &tempfile::TempDir, schema: &Schema) -> Store {
    Store::open(
        &root.path().join("office.db"),
        &Key::from_bytes([42; 32]),
        Opening::Existing,
        schema,
    )
    .unwrap()
}

#[test]
fn work_v6_all_released_versions_upgrade_preserve_every_row_and_match_fresh_schema() {
    let (_fresh_root, fresh) = fixture();
    let expected = fresh.read(|db| Ok(schema_rows(db))).unwrap();
    for schema in [&V1, &V2, &LEGACY_V3, &V4, &V5] {
        let (root, old) = fixture_for_schema(schema);
        seed_legacy(&old, schema.version);
        let before = old.read(|db| Ok(old_rows(db))).unwrap();
        drop(old);
        let store = reopen(&root);
        store
            .read(|db| {
                let after = old_rows(db);
                for (name, rows) in &before {
                    assert_eq!(
                        &after.iter().find(|(n, _)| n == name).unwrap().1,
                        rows,
                        "v{} {name}",
                        schema.version
                    );
                }
                assert_eq!(schema_rows(db), expected, "v{} schema", schema.version);
                assert_eq!(
                    db.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
                        .unwrap(),
                    6
                );
                Ok(())
            })
            .unwrap();
        let persisted = store.read(|db| Ok(old_rows(db))).unwrap();
        drop(store);
        let store = reopen(&root);
        assert_eq!(store.read(|db| Ok(old_rows(db))).unwrap(), persisted);
        drop(store);
        let error = Store::open(
            &root.path().join("office.db"),
            &Key::from_bytes([42; 32]),
            Opening::Existing,
            &V5,
        )
        .err()
        .expect("max-v5 binary refuses v6");
        let OfficeError::Storage(cause) = error else {
            panic!("expected older-binary refusal")
        };
        assert!(cause
            .to_string()
            .contains("newer than this server supports"));
    }
}

#[test]
fn work_v5_second_create_failure_rolls_back_all_v4_state_then_retries_cleanly() {
    let (root, old) = fixture_for_schema(&V4);
    seed_legacy(&old, 4);
    meta_write(&old,|db|{db.execute_batch("CREATE TABLE work_number_counters(sentinel TEXT);INSERT INTO work_number_counters VALUES ('keep');").map_err(OfficeError::storage)?;Ok(())}).unwrap();
    let rows = old.read(|db| Ok(old_rows(db))).unwrap();
    let schema = old.read(|db| Ok(schema_rows(db))).unwrap();
    drop(old);
    let error = Store::open(
        &root.path().join("office.db"),
        &Key::from_bytes([42; 32]),
        Opening::Existing,
        &OFFICE_SCHEMA,
    )
    .err()
    .expect("injected second CREATE collision");
    sql_failure(error);
    let old = open_old(&root, &V4);
    assert_eq!(old.read(|db| Ok(old_rows(db))).unwrap(), rows);
    assert_eq!(old.read(|db| Ok(schema_rows(db))).unwrap(), schema);
    old.read(|db| {
        assert_eq!(
            db.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
                .unwrap(),
            4
        );
        Ok(())
    })
    .unwrap();
    meta_write(&old, |db| {
        db.execute("DROP TABLE work_number_counters", [])
            .map_err(OfficeError::storage)?;
        Ok(())
    })
    .unwrap();
    drop(old);
    let store = reopen(&root);
    assert!(store
        .read(|db| Ok(schema_rows(db)))
        .unwrap()
        .iter()
        .any(|(_, name, _, _)| name == "work_clock"));
    store
        .read(|db| {
            assert_eq!(
                db.query_row("SELECT count(*) FROM work_origin_log", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                7
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn work_v5_numbered_v4_is_refused_before_ddl_and_preserves_version_and_rows() {
    for value in ["0", "1", "'unexpected'", "malformed"] {
        let (root, old) = fixture_for_schema(&V4);
        seed_legacy(&old, 4);
        let sql = if value == "malformed" {
            "PRAGMA ignore_check_constraints=ON; UPDATE work_items SET data='private invalid JSON'; PRAGMA ignore_check_constraints=OFF;".to_owned()
        } else {
            format!("UPDATE work_items SET data=json_set(data,'$.number',{value})")
        };
        meta_write(&old, move |db| {
            db.execute_batch(&sql).map_err(OfficeError::storage)?;
            Ok(())
        })
        .unwrap();
        let rows = old.read(|db| Ok(old_rows(db))).unwrap();
        let schema = old.read(|db| Ok(schema_rows(db))).unwrap();
        drop(old);
        sql_failure(
            Store::open(
                &root.path().join("office.db"),
                &Key::from_bytes([42; 32]),
                Opening::Existing,
                &OFFICE_SCHEMA,
            )
            .err()
            .expect("refuse numbered v4"),
        );
        let old = open_old(&root, &V4);
        assert_eq!(old.read(|db| Ok(old_rows(db))).unwrap(), rows);
        assert_eq!(old.read(|db| Ok(schema_rows(db))).unwrap(), schema);
    }
}

#[test]
fn work_v5_storage_failures_keep_native_causes_and_readers_do_not_hide_sql_errors() {
    for table in ["work_change_results", "work_number_counters", "work_clock"] {
        let (_root, store) = fixture();
        let sql=format!("CREATE TRIGGER fail_metadata BEFORE INSERT ON {table} BEGIN SELECT RAISE(ABORT,'private payload'); END;");
        meta_write(&store, move |db| {
            db.execute_batch(&sql).map_err(OfficeError::storage)?;
            Ok(())
        })
        .unwrap();
        let before = saved_meta(&store);
        let error = meta_write(&store, move |db| match table {
            "work_change_results" => put_change_result(db, "blocked", &accepted()),
            "work_number_counters" => allocate_number(db, &NumberScope::Requests).map(|_| ()),
            _ => raise_high_water(db, 1),
        })
        .unwrap_err();
        assert!(!error.to_string().contains("private payload"));
        sql_failure(error);
        assert_eq!(saved_meta(&store), before);
    }
    for table in ["work_change_results", "work_clock"] {
        let (_root, store) = fixture();
        let sql = format!("DROP TABLE {table}");
        meta_write(&store, move |db| {
            db.execute(&sql, []).map_err(OfficeError::storage)?;
            Ok(())
        })
        .unwrap();
        sql_failure(
            store
                .read(move |db| {
                    if table == "work_change_results" {
                        change_result(db, "missing").map(|_| ())
                    } else {
                        high_water(db).map(|_| ())
                    }
                })
                .unwrap_err(),
        );
    }
}
