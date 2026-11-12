use super as origin;
use super::{OriginEntry, OriginRef};
use crate::store::{messages, status, Opening, Schema, Store, Undo};
use crate::OfficeError;
use crate::{office::OFFICE_SCHEMA, Key};
use org_proto::Position;

fn original_incarnation() -> String {
    crate::ulid::from_ms(10, 1)
}

fn fixture() -> (tempfile::TempDir, Store) {
    fixture_for_schema(&OFFICE_SCHEMA)
}

fn fixture_for_schema(schema: &Schema) -> (tempfile::TempDir, Store) {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(
        &root.path().join("office.db"),
        &Key::from_bytes([34; 32]),
        Opening::New,
        schema,
    )
    .unwrap();
    store
        .write(|db| {
            db.execute("INSERT INTO meta VALUES ('office_id', 'OFFICE1')", [])
                .map_err(OfficeError::storage)?;
            db.execute(
                "INSERT INTO meta VALUES ('incarnation', ?1)",
                [original_incarnation()],
            )
            .map_err(OfficeError::storage)?;
            db.execute("INSERT INTO meta VALUES ('message_sequence', '41')", [])
                .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .unwrap();
    (root, store)
}

fn target(id: u8, version: u64) -> OriginRef {
    OriginRef {
        target_id: crate::ulid::from_ms(20, id),
        generation: 1,
        version,
    }
}

fn feed(db: &rusqlite::Connection, refs: &[OriginRef]) -> u64 {
    let refs: Vec<_> = refs
        .iter()
        .map(|item| org_proto::work::EventRef {
            target_id: item.target_id.clone(),
            version: item.version,
        })
        .collect();
    db.execute(
        "INSERT INTO work_feed(targets) VALUES (?1)",
        [serde_json::to_string(&refs).unwrap()],
    )
    .unwrap();
    db.last_insert_rowid() as u64
}

fn commit(store: &Store, refs: Vec<OriginRef>) -> std::ops::RangeInclusive<u64> {
    store
        .write(move |db| {
            let feed_seq = feed(db, &refs);
            let range = origin::append(db, Position { epoch: 2, seq: 9 }, &refs, feed_seq, 1000)?;
            Ok((range, None))
        })
        .unwrap()
}

fn state(store: &Store) -> (Vec<OriginEntry>, u64, u64) {
    store
        .read(|db| {
            Ok((
                origin::entries_after(db, &original_incarnation(), 0, 1000)?,
                origin::high_water(db, &original_incarnation())?,
                db.query_row("SELECT count(*) FROM work_feed", [], |row| {
                    row.get::<_, u32>(0).map(u64::from)
                })
                .map_err(OfficeError::storage)?,
            ))
        })
        .unwrap()
}

#[test]
fn origin_rollback_preserves_gap_free_sequence() {
    let (_root, store) = fixture();
    assert_eq!(commit(&store, vec![target(1, 0), target(1, 1)]), 1..=2);
    let before = state(&store);
    let error = store
        .write(|db| {
            let refs = [target(2, 0)];
            let feed_seq = feed(db, &refs);
            assert_eq!(
                origin::append(db, Position { epoch: 2, seq: 9 }, &refs, feed_seq, 1001)?,
                3..=3
            );
            Err::<((), Option<Undo>), _>(OfficeError::Invalid("injected rollback"))
        })
        .unwrap_err();
    assert!(matches!(error, OfficeError::Invalid("injected rollback")));
    assert_eq!(state(&store), before);
    assert_eq!(commit(&store, vec![target(2, 0), target(2, 1)]), 3..=4);
    let (rows, high, feeds) = state(&store);
    assert_eq!(
        (rows.len() as u64, rows.last().unwrap().seq, high, feeds),
        (4, 4, 4, 2)
    );
    assert_eq!(
        rows.iter().map(|row| row.seq).collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    assert_eq!(rows[0].target, target(1, 0));
    assert_eq!((rows[0].feed_seq, rows[1].feed_seq), (1, 1));
    assert_eq!((rows[2].feed_seq, rows[3].feed_seq), (2, 2));
    assert_eq!(rows[0].org_position, Position { epoch: 2, seq: 9 });
    assert_eq!(rows[0].committed_at_ms, 1000);
}

#[test]
fn origin_writer_replay_allocates_each_committed_range_once() {
    use crate::store::WriteJob;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc,
    };
    use std::time::Duration;

    let (_root, store) = fixture();
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
    // Queue this exact batch while the writer is held; no timing-based sleeps.
    for id in 1..=3 {
        let (attempts, replied) = (Arc::clone(&attempts), replied.clone());
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
                    let refs = [target(id, 0)];
                    let seq = feed(db, &refs);
                    let allocated =
                        origin::append(db, Position { epoch: 1, seq: 1 }, &refs, seq, 1000)?;
                    assert_eq!(allocated, if id == 1 { 1..=1 } else { 2..=2 });
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
    let (rows, high, feeds) = state(&store);
    assert_eq!((rows.len(), high, feeds), (2, 2, 2));
    assert_eq!((rows[0].seq, rows[1].seq), (1, 2));
    assert_eq!(
        (rows[0].target.clone(), rows[1].target.clone()),
        (target(1, 0), target(3, 0))
    );
}

#[test]
fn origin_rotation_keeps_old_rows_and_starts_zero() {
    let (_root, store) = fixture();
    commit(&store, vec![target(1, 0)]);
    let before = state(&store);
    store
        .write(|db| {
            db.execute(
                "UPDATE meta SET value = ?1 WHERE key = 'incarnation'",
                [crate::ulid::from_ms(11, 1)],
            )
            .map_err(OfficeError::storage)?;
            let origin = origin::local_origin(db)?;
            assert_eq!(origin::high_water(db, &origin.incarnation)?, 0);
            Ok(((), None))
        })
        .unwrap();
    assert_eq!(commit(&store, vec![target(2, 0)]), 1..=1);
    store
        .read(|db| {
            assert_eq!(
                origin::entries_after(db, &original_incarnation(), 0, 100)?,
                before.0
            );
            let newer = origin::entries_after(db, &crate::ulid::from_ms(11, 1), 0, 100)?;
            assert_eq!(newer.len(), 1);
            assert_eq!(newer[0].target, target(2, 0));
            assert_eq!(newer[0].seq, 1);
            Ok(())
        })
        .unwrap();
}

#[test]
fn origin_message_counter_is_independent() {
    let (_root, store) = fixture();
    commit(&store, vec![target(1, 0)]);
    let before = state(&store);
    store
        .write(|db| {
            assert_eq!(origin::incarnation(db)?, original_incarnation());
            assert_eq!(
                db.query_row(
                    "SELECT value FROM meta WHERE key = 'message_sequence'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
                "41"
            );
            db.execute(
                "UPDATE meta SET value = '999' WHERE key = 'message_sequence'",
                [],
            )
            .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .unwrap();
    assert_eq!(state(&store), before);
    assert_eq!(commit(&store, vec![target(2, 0)]), 2..=2);
}

#[test]
fn origin_queries_are_ordered_exclusive_and_bounded() {
    let (_root, store) = fixture();
    let refs = (0..1025)
        .map(|n| OriginRef {
            target_id: crate::ulid::from_ms(100 + n, 1),
            generation: 1,
            version: 0,
        })
        .collect();
    assert_eq!(commit(&store, refs), 1..=1025);
    store
        .read(|db| {
            db.pragma_update(None, "reverse_unordered_selects", true)
                .map_err(OfficeError::storage)?;
            let inc = original_incarnation();
            assert_eq!(origin::entries_after(db, &inc, 0, u32::MAX)?.len(), 1024);
            assert!(origin::entries_after(db, &inc, 0, 0)?.is_empty());
            assert_eq!(
                origin::entries_after(db, &inc, 1024, 10)?
                    .iter()
                    .map(|row| row.seq)
                    .collect::<Vec<_>>(),
                [1025]
            );
            assert_eq!(
                origin::entries_after(db, &inc, 1, 2)?
                    .iter()
                    .map(|row| row.seq)
                    .collect::<Vec<_>>(),
                [2, 3]
            );
            assert!(matches!(
                origin::entries_after(db, &inc, u64::MAX, 1),
                Err(OfficeError::Invalid("work origin integer is out of range"))
            ));
            Ok(())
        })
        .unwrap();
}

#[test]
fn origin_counter_failure_rolls_back_all_appended_rows() {
    for existing in [false, true] {
        let (_root, store) = fixture();
        if existing {
            commit(&store, vec![target(1, 0)]);
        }
        store.write(move |db| {
            let sql = if existing {
                "CREATE TRIGGER fail_counter BEFORE UPDATE ON work_origin_counter BEGIN SELECT RAISE(ABORT, 'counter update'); END;"
            } else {
                "CREATE TRIGGER fail_counter BEFORE INSERT ON work_origin_counter BEGIN SELECT RAISE(ABORT, 'counter insert'); END;"
            };
            db.execute_batch(sql).map_err(OfficeError::storage)?;
            Ok(((), None))
        }).unwrap();
        let before = state(&store);
        let error = store
            .write(|db| {
                let refs = [target(2, 0)];
                let seq = feed(db, &refs);
                Ok((
                    origin::append(db, Position { epoch: 1, seq: 1 }, &refs, seq, 1)?,
                    None,
                ))
            })
            .unwrap_err();
        assert!(matches!(error, OfficeError::Storage(_)));
        assert_eq!(state(&store), before);
    }
}

#[test]
fn origin_refusals_leave_feed_log_and_counter_unchanged() {
    let (_root, store) = fixture();
    commit(&store, vec![target(1, 0)]);
    let before = state(&store);
    for case in [
        "empty",
        "feed-zero",
        "duplicate",
        "duplicate-generation",
        "descending",
        "interleaved-descending",
        "id",
        "generation-zero",
        "generation-overflow",
        "version-overflow",
        "feed-overflow",
        "time-overflow",
        "epoch-overflow",
        "org-seq-overflow",
    ] {
        let error = store
            .write(move |db| {
                let mut refs = vec![target(2, 0)];
                let mut position = Position { epoch: 2, seq: 9 };
                let mut at_ms = 1000;
                let mut feed_seq = feed(db, &refs);
                match case {
                    "empty" => refs.clear(),
                    "feed-zero" => feed_seq = 0,
                    "duplicate" => refs.push(refs[0].clone()),
                    "duplicate-generation" => {
                        refs.push(refs[0].clone());
                        refs[1].generation = 2;
                    }
                    "descending" => {
                        refs = vec![target(2, 2), target(2, 1)];
                    }
                    "interleaved-descending" => {
                        refs = vec![target(2, 2), target(3, 0), target(2, 1)];
                    }
                    "id" => refs[0].target_id = "not-a-work-id".into(),
                    "generation-zero" => refs[0].generation = 0,
                    "generation-overflow" => refs[0].generation = u64::MAX,
                    "version-overflow" => refs[0].version = u64::MAX,
                    "feed-overflow" => feed_seq = u64::MAX,
                    "time-overflow" => at_ms = u64::MAX,
                    "epoch-overflow" => position.epoch = u64::MAX,
                    _ => position.seq = u64::MAX,
                }
                let range = origin::append(db, position, &refs, feed_seq, at_ms)?;
                Ok((range, None))
            })
            .unwrap_err();
        let expected = match case {
            "empty" | "feed-zero" => "work origin append requires versions and a feed row",
            "duplicate" | "duplicate-generation" | "descending" | "interleaved-descending" => {
                "work origin versions must advance within an append"
            }
            "id" | "generation-zero" => "invalid work origin reference",
            _ => "work origin integer is out of range",
        };
        assert!(
            matches!(error, OfficeError::Invalid(reason) if reason == expected),
            "{case}: {error}"
        );
        assert_eq!(state(&store), before, "{case}");
    }
}

#[test]
fn origin_append_requires_a_writer_transaction() {
    let (_root, store) = fixture();
    let before = state(&store);
    let error = store
        .read(|db| origin::append(db, Position { epoch: 1, seq: 1 }, &[target(1, 0)], 1, 1))
        .unwrap_err();
    assert!(matches!(
        error,
        OfficeError::Invalid("work origin append requires a writer transaction")
    ));
    assert_eq!(state(&store), before);
}

#[test]
fn origin_append_failure_rolls_back_after_part_of_the_reference_list() {
    let (_root, store) = fixture();
    commit(&store, vec![target(1, 0)]);
    // The second insert fails after the first was accepted, inside the writer savepoint.
    store
        .write(|db| {
            db.execute_batch(
                "CREATE TRIGGER fail_second_origin BEFORE INSERT ON work_origin_log
                          WHEN NEW.origin_seq = 3 BEGIN SELECT RAISE(ABORT, 'injected'); END;",
            )
            .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .unwrap();
    let before = state(&store);
    let error = store
        .write(|db| {
            let refs = [target(2, 0), target(2, 1)];
            let seq = feed(db, &refs);
            Ok((
                origin::append(db, Position { epoch: 1, seq: 1 }, &refs, seq, 1000)?,
                None,
            ))
        })
        .unwrap_err();
    assert!(matches!(error, OfficeError::Storage(_)));
    assert_eq!(state(&store), before);
    store
        .write(|db| {
            db.execute_batch("DROP TRIGGER fail_second_origin")
                .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .unwrap();
    assert_eq!(commit(&store, vec![target(2, 0), target(2, 1)]), 2..=3);
}

#[test]
fn origin_sequence_exhaustion_refuses_without_consuming_feed_or_rows() {
    let (_root, store) = fixture();
    store
        .write(|db| {
            db.execute(
                "INSERT INTO work_origin_counter VALUES (?1, ?2)",
                rusqlite::params![original_incarnation(), i64::MAX],
            )
            .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .unwrap();
    let before = state(&store);
    let error = store
        .write(|db| {
            let refs = [target(1, 0)];
            let seq = feed(db, &refs);
            Ok((
                origin::append(db, Position { epoch: 0, seq: 0 }, &refs, seq, 0)?,
                None,
            ))
        })
        .unwrap_err();
    assert!(matches!(
        error,
        OfficeError::Invalid("work origin sequence is exhausted")
    ));
    assert_eq!(state(&store), before);
}

#[test]
fn origin_rejects_malformed_identity_on_write_and_load() {
    for key in ["office_id", "incarnation"] {
        let (_root, store) = fixture();
        commit(&store, vec![target(1, 0)]);
        store
            .write(move |db| {
                db.execute(
                    "UPDATE meta SET value = 'not-an-origin' WHERE key = ?1",
                    [key],
                )
                .map_err(OfficeError::storage)?;
                Ok(((), None))
            })
            .unwrap();
        let before = state(&store);
        let error = store
            .write(|db| {
                let refs = [target(2, 0)];
                let seq = feed(db, &refs);
                Ok((
                    origin::append(db, Position { epoch: 0, seq: 0 }, &refs, seq, 0)?,
                    None,
                ))
            })
            .unwrap_err();
        assert!(matches!(
            error,
            OfficeError::Invalid("invalid work origin identity")
        ));
        assert_eq!(state(&store), before);
    }
    for column in ["target_id", "origin_office_id"] {
        let (_root, store) = fixture();
        commit(&store, vec![target(1, 0)]);
        store
            .write(move |db| {
                let sql = match column {
                    "target_id" => "UPDATE work_origin_log SET target_id = 'bad'",
                    _ => "UPDATE work_origin_log SET origin_office_id = 'not-an-origin'",
                };
                db.execute(sql, []).map_err(OfficeError::storage)?;
                Ok(((), None))
            })
            .unwrap();
        let error = store
            .read(|db| origin::entries_after(db, &original_incarnation(), 0, 10))
            .unwrap_err();
        assert!(matches!(error, OfficeError::Invalid(_)));
    }
}

#[test]
fn origin_sql_integer_conversion_accepts_boundaries_and_refuses_negatives() {
    let (_root, store) = fixture();
    store
        .read(|db| {
            for (sql, expected) in [
                ("SELECT 0", 0),
                ("SELECT 9223372036854775807", i64::MAX as u64),
            ] {
                assert_eq!(
                    db.query_row(sql, [], |row| origin::read_integer(row, 0))
                        .unwrap(),
                    expected
                );
            }
            assert!(matches!(
                db.query_row("SELECT -1", [], |row| origin::read_integer(row, 0)),
                Err(rusqlite::Error::IntegralValueOutOfRange(0, -1))
            ));
            Ok(())
        })
        .unwrap();
}

const V3: Schema = Schema {
    version: 3,
    create: |tx| {
        tx.execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;")?;
        messages::create_tables(tx)?;
        status::create_table(tx)
    },
    upgrade: |_, _| panic!("fixture starts as version 3"),
};

#[test]
fn origin_upgrade_failure_preserves_the_prior_schema() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("office.db");
    let key = Key::from_bytes([35; 32]);
    let old = Store::open(&path, &key, Opening::New, &V3).unwrap();
    old.write(|db| {
        db.execute_batch("CREATE TABLE work_origin_log (sentinel TEXT); INSERT INTO work_origin_log VALUES ('keep');
                          INSERT INTO meta VALUES ('message_sequence', '41');").map_err(OfficeError::storage)?;
        Ok(((), None))
    }).unwrap();
    drop(old);
    assert!(matches!(
        Store::open(&path, &key, Opening::Existing, &OFFICE_SCHEMA),
        Err(OfficeError::Storage(_))
    ));
    let old = Store::open(&path, &key, Opening::Existing, &V3).unwrap();
    old.read(|db| {
        assert_eq!(db.pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0)).unwrap(), 3);
        assert_eq!(db.query_row("SELECT sentinel FROM work_origin_log", [], |row| row.get::<_, String>(0)).unwrap(), "keep");
        assert_eq!(db.query_row("SELECT count(*) FROM sqlite_master WHERE name IN ('work_feed', 'work_origin_counter')", [], |row| row.get::<_, u32>(0)).unwrap(), 0);
        assert_eq!(db.query_row("SELECT value FROM meta WHERE key = 'message_sequence'", [], |row| row.get::<_, String>(0)).unwrap(), "41");
        Ok(())
    }).unwrap();
}

#[test]
fn origin_upgrade_preserves_message_counter_and_shared_incarnation() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("office.db");
    let key = Key::from_bytes([32; 32]);
    let old = Store::open(&path, &key, Opening::New, &V3).unwrap();
    let incarnation = old
        .write(|db| {
            db.execute("INSERT INTO meta VALUES ('message_sequence', '41')", [])
                .map_err(crate::OfficeError::storage)?;
            Ok((origin::incarnation(db)?, None))
        })
        .unwrap();
    drop(old);
    for _ in 0..2 {
        let store = Store::open(&path, &key, Opening::Existing, &OFFICE_SCHEMA).unwrap();
        let expected = incarnation.clone();
        store
            .write(move |db| {
                assert_eq!(origin::incarnation(db)?, expected);
                assert_eq!(
                    db.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
                        .unwrap(),
                    6
                );
                assert_eq!(
                    db.query_row(
                        "SELECT value FROM meta WHERE key = 'message_sequence'",
                        [],
                        |r| r.get::<_, String>(0)
                    )
                    .unwrap(),
                    "41"
                );
                assert_eq!(
                    db.query_row("SELECT count(*) FROM work_origin_counter", [], |r| r
                        .get::<_, u32>(0))
                        .unwrap(),
                    0
                );
                assert_eq!(
                    db.query_row("SELECT count(*) FROM work_origin_log", [], |r| r
                        .get::<_, u32>(0))
                        .unwrap(),
                    0
                );
                assert_eq!(
                    db.query_row("SELECT count(*) FROM msg_headers", [], |r| r
                        .get::<_, u32>(0))
                        .unwrap(),
                    0
                );
                Ok(((), None))
            })
            .unwrap();
    }
}

#[test]
fn origin_schema_refuses_invalid_sequences_and_missing_feed_rows() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(
        &root.path().join("office.db"),
        &Key::from_bytes([33; 32]),
        Opening::New,
        &OFFICE_SCHEMA,
    )
    .unwrap();
    store
        .write(|db| {
            db.execute("INSERT INTO work_feed (seq, targets) VALUES (1, '[]')", [])
                .map_err(crate::OfficeError::storage)?;
            Ok(((), None))
        })
        .unwrap();
    for (column, value) in [
        ("origin_seq", 0),
        ("generation", 0),
        ("version", -1),
        ("org_epoch", -1),
        ("org_seq", -1),
        ("committed_at_ms", -1),
        ("feed_seq", 2),
    ] {
        let error = store.write(move |db| {
            let fields = ["origin_seq", "feed_seq", "generation", "version", "org_epoch", "org_seq", "committed_at_ms"];
            let mut values = [1, 1, 1, 0, 0, 0, 0];
            values[fields.iter().position(|field| *field == column).unwrap()] = value;
            db.execute("INSERT INTO work_origin_log VALUES ('OFFICE1', 'INCARNATION', ?1, ?2, 'TARGET', ?3, ?4, ?5, ?6, ?7)", rusqlite::params_from_iter(values)).map_err(crate::OfficeError::storage)?;
            Ok(((), None))
        }).unwrap_err();
        let crate::OfficeError::Storage(source) = error else {
            panic!("expected SQL constraint for {column}")
        };
        let sql = source.downcast_ref::<rusqlite::Error>().unwrap();
        assert_eq!(
            sql.sqlite_error_code(),
            Some(rusqlite::ErrorCode::ConstraintViolation),
            "{column}"
        );
        assert_eq!(
            store
                .read(|db| db
                    .query_row("SELECT count(*) FROM work_origin_log", [], |row| row
                        .get::<_, u32>(0))
                    .map_err(crate::OfficeError::storage))
                .unwrap(),
            0
        );
    }
}

#[test]
fn origin_ledger_survives_reopen() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("office.db");
    let key = Key::from_bytes([31; 32]);
    for opening in [Opening::New, Opening::Existing] {
        let store = Store::open(&path, &key, opening, &OFFICE_SCHEMA).unwrap();
        let count = store
            .read(|db| {
                db.query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type = 'table'
                 AND name IN ('work_feed', 'work_origin_counter', 'work_origin_log')",
                    [],
                    |row| row.get::<_, u32>(0),
                )
                .map_err(crate::OfficeError::storage)
            })
            .unwrap();
        assert_eq!(count, 3, "all three origin ledger tables must persist");
    }
    let store = Store::open(&path, &key, Opening::Existing, &OFFICE_SCHEMA).unwrap();
    store
        .write(|db| {
            db.execute("INSERT INTO meta VALUES ('office_id', 'OFFICE1')", [])
                .map_err(OfficeError::storage)?;
            db.execute(
                "INSERT INTO meta VALUES ('incarnation', ?1)",
                [original_incarnation()],
            )
            .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .unwrap();
    assert_eq!(commit(&store, vec![target(1, 0), target(1, 1)]), 1..=2);
    let before = state(&store);
    drop(store);
    let reopened = Store::open(&path, &key, Opening::Existing, &OFFICE_SCHEMA).unwrap();
    assert_eq!(state(&reopened), before);
    assert_eq!(commit(&reopened, vec![target(2, 0)]), 3..=3);
}

#[test]
fn origin_counter_schema_accepts_zero_and_refuses_negative_values() {
    let (_root, store) = fixture();
    store
        .write(|db| {
            db.execute(
                "INSERT INTO work_origin_counter VALUES (?1, 0)",
                [original_incarnation()],
            )
            .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .unwrap();
    let before = state(&store);
    let error = store
        .write(|db| {
            db.execute("UPDATE work_origin_counter SET last_seq = -1", [])
                .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .unwrap_err();
    let OfficeError::Storage(source) = error else {
        panic!("expected SQL constraint")
    };
    assert_eq!(
        source
            .downcast_ref::<rusqlite::Error>()
            .unwrap()
            .sqlite_error_code(),
        Some(rusqlite::ErrorCode::ConstraintViolation)
    );
    assert_eq!(state(&store), before);
    let error = store
        .write(|db| {
            db.execute(
                "INSERT INTO work_origin_counter VALUES (?1, 1)",
                [original_incarnation()],
            )
            .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .unwrap_err();
    assert!(matches!(error, OfficeError::Storage(_)));
    assert_eq!(state(&store), before);
}

#[test]
fn origin_duplicate_across_appends_rolls_back_feed_prefix_and_counter_after_reopen() {
    for source in ["new", "upgrade", "rotated"] {
        let (root, mut store) = fixture_for_schema(if source == "upgrade" {
            &V3
        } else {
            &OFFICE_SCHEMA
        });
        let path = root.path().join("office.db");
        let key = Key::from_bytes([34; 32]);
        if source == "upgrade" {
            drop(store);
            store = Store::open(&path, &key, Opening::Existing, &OFFICE_SCHEMA).unwrap();
        }
        assert_eq!(commit(&store, vec![target(1, 0)]), 1..=1);
        let active_incarnation = if source == "rotated" {
            let next = crate::ulid::from_ms(11, 1);
            let saved = next.clone();
            store
                .write(move |db| {
                    db.execute(
                        "UPDATE meta SET value = ?1 WHERE key = 'incarnation'",
                        [&saved],
                    )
                    .map_err(OfficeError::storage)?;
                    Ok(((), None))
                })
                .unwrap();
            next
        } else {
            original_incarnation()
        };
        let before = state(&store);
        let active_before = store
            .read({
                let active = active_incarnation.clone();
                move |db| {
                    Ok((
                        origin::entries_after(db, &active, 0, 100)?,
                        origin::high_water(db, &active)?,
                    ))
                }
            })
            .unwrap();
        // This is a different writer request, and its first insert is new. The
        // duplicate is encountered only after both the feed and prefix were written.
        let error = store
            .write(|db| {
                let refs = [target(2, 0), target(1, 0)];
                let seq = feed(db, &refs);
                let range = origin::append(db, Position { epoch: 2, seq: 10 }, &refs, seq, 1001)?;
                Ok((range, None))
            })
            .expect_err("a committed work version cannot acquire a second origin row");
        let OfficeError::Storage(cause) = error else {
            panic!("expected SQL constraint for {source}")
        };
        assert!(
            matches!(cause.downcast_ref::<rusqlite::Error>(), Some(rusqlite::Error::SqliteFailure(code, _)) if code.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE),
            "{source}: {cause}"
        );
        assert_eq!(
            state(&store),
            before,
            "{source}: rollback must retain prior rows and feed"
        );
        drop(store);
        let store = Store::open(&path, &key, Opening::Existing, &OFFICE_SCHEMA).unwrap();
        assert_eq!(
            state(&store),
            before,
            "{source}: committed state after reopen"
        );
        let active = active_incarnation.clone();
        store
            .read(move |db| {
                assert_eq!(
                    (
                        origin::entries_after(db, &active, 0, 100)?,
                        origin::high_water(db, &active)?
                    ),
                    active_before
                );
                assert_eq!(
                    db.query_row(
                        "SELECT value FROM meta WHERE key = 'message_sequence'",
                        [],
                        |r| r.get::<_, String>(0)
                    )
                    .unwrap(),
                    "41"
                );
                assert_eq!(
                    db.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
                        .unwrap(),
                    6
                );
                Ok(())
            })
            .unwrap();
        let next = if source == "rotated" { 1 } else { 2 };
        assert_eq!(commit(&store, vec![target(2, 0)]), next..=next);
        store
            .read(move |db| {
                let entries = origin::entries_after(db, &active_incarnation, next - 1, 100)?;
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].target, target(2, 0));
                assert_eq!(
                    entries[0].feed_seq, 2,
                    "failed append must not consume the feed allocator"
                );
                Ok(())
            })
            .unwrap();
    }
}

#[test]
fn origin_unique_triple_allows_distinct_targets_generations_and_versions() {
    let (root, store) = fixture();
    let mut new_generation = target(1, 0);
    new_generation.generation = 2;
    let refs = [target(1, 0), target(1, 1), new_generation, target(2, 0)];
    for (index, reference) in refs.iter().enumerate() {
        let next = index as u64 + 1;
        assert_eq!(commit(&store, vec![reference.clone()]), next..=next);
    }
    let before = state(&store);
    assert_eq!((before.0.len(), before.1, before.2), (4, 4, 4));
    assert_eq!(
        before
            .0
            .iter()
            .map(|r| r.target.clone())
            .collect::<Vec<_>>(),
        refs
    );
    drop(store);
    let reopened = Store::open(
        &root.path().join("office.db"),
        &Key::from_bytes([34; 32]),
        Opening::Existing,
        &OFFICE_SCHEMA,
    )
    .unwrap();
    assert_eq!(state(&reopened), before);
}
