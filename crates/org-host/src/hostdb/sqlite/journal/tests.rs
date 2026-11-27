use super::*;
use crate::hostdb::sqlite::tests::admin_fixture::PHC;
use crate::hostdb::StoreFault;
use serde_json::json;

thread_local! {
    static BEGIN_COUNT: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static BEGIN_HOOK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}
pub(super) fn after_begin() {
    BEGIN_COUNT.with(|count| count.set(count.get() + 1));
    let hook = BEGIN_HOOK.with(|cell| cell.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

const NOW: i64 = 1_800_000_000_000;
fn id(at: i64, sequence: u64) -> String {
    let ms = u64::try_from(at).unwrap();
    let rand_a = ((sequence >> 62) & 0x0fff) as u16;
    let rand_b = sequence & ((1u64 << 62) - 1);
    let hi = (ms << 16) | (0x7 << 12) | u64::from(rand_a);
    let lo = (0b10u64 << 62) | rand_b;
    let hex = format!("{hi:016x}{lo:016x}");
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}
fn attempt(id: &str) -> Attempt<'_> {
    Attempt {
        id,
        kind: "office.restart",
        fingerprint: [7; 32],
        account: "creator",
    }
}
fn completed() -> Effect {
    Effect {
        stored: Stored::Completed {
            result: json!({"done":true}),
        },
        verifier: None,
    }
}
fn rejected(code: org_proto::ErrorCode) -> Effect {
    Effect {
        stored: Stored::Rejected {
            refusal: org_proto::ApiError::new(code, "refused"),
        },
        verifier: None,
    }
}
fn create_effect(account: &str) -> Effect {
    Effect {
        stored: Stored::Completed {
            result: json!({"account":account}),
        },
        verifier: Some((account.into(), Verifier::new(PHC.into()).unwrap())),
    }
}
fn scalar(db: &HostDb, sql: &str) -> i64 {
    db.connection
        .lock()
        .unwrap()
        .query_row(sql, [], |r| r.get(0))
        .unwrap()
}
fn assert_replay(db: &HostDb, op: &Attempt<'_>, at: i64, expected: Stored) {
    match db.lookup(op, at).unwrap() {
        Lookup::Replay(stored) => assert_eq!(stored, expected),
        _ => panic!("expected replay"),
    }
}
#[test]
fn first_insert_effect_and_exact_replay_survive_reopen() {
    let (home, secrets, db) = super::super::tests::fixture();
    let ident = id(NOW, 1);
    let op = attempt(&ident);
    assert!(matches!(db.lookup(&op, NOW).unwrap(), Lookup::Vacant));
    let written = db
        .write(&op, NOW, |tx| {
            tx.execute(
                "INSERT INTO org_owners VALUES ('org','creator','member')",
                [],
            )?;
            Ok(completed())
        })
        .unwrap();
    assert!(matches!(
        written,
        Write::Committed(Stored::Completed { .. })
    ));
    assert_eq!(scalar(&db, "SELECT count(*) FROM org_owners"), 1);
    let expected = completed().stored;
    assert_replay(&db, &op, NOW, expected.clone());
    assert!(matches!(
        db.write(&op, NOW, |_| panic!("must not repeat effect"))
            .unwrap(),
        Write::Existing(Lookup::Replay(_))
    ));
    drop(db);
    let db = HostDb::open(home.path(), &secrets).unwrap();
    assert_replay(&db, &op, NOW, expected);
}
#[test]
fn kind_account_and_fingerprint_reuse_never_run_the_effect() {
    let (_home, _secrets, db) = super::super::tests::fixture();
    let ident = id(NOW, 2);
    let op = attempt(&ident);
    db.write(&op, NOW, |_| Ok(completed())).unwrap();
    for altered in [
        Attempt {
            kind: "account.create",
            ..attempt(&ident)
        },
        Attempt {
            account: "another",
            ..attempt(&ident)
        },
        Attempt {
            fingerprint: [8; 32],
            ..attempt(&ident)
        },
    ] {
        assert!(matches!(
            db.lookup(&altered, NOW),
            Err(Error::OperationReused)
        ));
        assert!(matches!(
            db.write(&altered, NOW, |_| panic!("reused")),
            Err(Error::OperationReused)
        ));
    }
}
#[test]
fn a_row_appearing_after_the_unlocked_lookup_takes_replay_path() {
    let (_home, _secrets, db) = super::super::tests::fixture();
    let ident = id(NOW, 3);
    let op = attempt(&ident);
    assert!(matches!(db.lookup(&op, NOW).unwrap(), Lookup::Vacant));
    // A second attempt wins while the first caller is outside the lock (e.g. hashing).
    db.write(&op, NOW, |_| Ok(completed())).unwrap();
    assert!(matches!(
        db.write(&op, NOW, |_| panic!("second effect")).unwrap(),
        Write::Existing(Lookup::Replay(_))
    ));
    assert_eq!(scalar(&db, "SELECT count(*) FROM op_journal"), 1);
}
#[test]
fn concurrent_attempts_commit_exactly_one_sql_effect() {
    let (_home, _secrets, db) = super::super::tests::fixture();
    let barrier = std::sync::Barrier::new(2);
    let ident = id(NOW, 4);
    let count = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..2)
            .map(|_| {
                scope.spawn(|| {
                    let op = attempt(&ident);
                    assert!(matches!(db.lookup(&op, NOW).unwrap(), Lookup::Vacant));
                    barrier.wait();
                    db.write(&op, NOW, |tx| {
                        count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        tx.execute(
                            "INSERT INTO org_owners VALUES ('org','creator','member')",
                            [],
                        )?;
                        Ok(completed())
                    })
                    .unwrap()
                })
            })
            .collect();
        for worker in workers {
            worker.join().expect("worker");
        }
    });
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(scalar(&db, "SELECT count(*) FROM org_owners"), 1);
}

#[test]
fn the_effect_begins_with_an_immediate_write_reservation() {
    let (home, secrets, db) = super::super::tests::fixture();
    let other = HostDb::open(home.path(), &secrets).unwrap();
    other
        .connection
        .lock()
        .unwrap()
        .busy_timeout(std::time::Duration::ZERO)
        .unwrap();
    let ident = id(NOW, 1);
    let observed = std::rc::Rc::new(std::cell::Cell::new(false));
    let called = observed.clone();
    BEGIN_HOOK.with(|cell| *cell.borrow_mut() = Some(Box::new(move || {
        called.set(true);
        let c = other.connection.lock().unwrap();
        let result = c.execute_batch("BEGIN IMMEDIATE");
        // Release a wrongly acquired reservation before any assertion can unwind.
        if result.is_ok() { c.execute_batch("ROLLBACK").unwrap(); }
        assert!(matches!(result,Err(e) if e.sqlite_error_code()==Some(rusqlite::ErrorCode::DatabaseBusy)));
    })));
    db.write(&attempt(&ident), NOW, |_| Ok(completed()))
        .unwrap();
    assert!(
        observed.get(),
        "probe must run before the first journal read"
    );
}
#[test]
fn unknown_id_format_age_and_skew_boundaries_leave_no_row() {
    let (_home, _secrets, db) = super::super::tests::fixture();
    for (offset, expected) in [
        (-29 * DAY, None),
        (-29 * DAY - 1, Some(Error::Expired)),
        (300_000, None),
        (300_001, Some(Error::ClockSkew)),
    ] {
        let ident = id(NOW + offset, 1);
        let op = attempt(&ident);
        match expected {
            None => assert!(matches!(db.lookup(&op, NOW).unwrap(), Lookup::Vacant)),
            Some(expected) => assert!(matches!(db.lookup(&op,NOW),Err(e) if e==expected)),
        }
    }
    for bad in [
        "",
        "01M72N8MG00000000000000006",
        "0190A5D2-7C3E-7A01-8B2C-3D4E5F6071E0",
        "0190a5d2-7c3e-4a01-8b2c-3d4e5f6071e0",
        "0190a5d2-7c3e-7a01-cb2c-3d4e5f6071e0",
    ] {
        assert!(matches!(
            db.lookup(&attempt(bad), NOW),
            Err(Error::InvalidId)
        ));
    }
    assert_eq!(scalar(&db, "SELECT count(*) FROM op_journal"), 0);
}
#[test]
fn known_rows_bypass_age_until_expiry_then_expiry_precedes_reuse() {
    let (_home, _secrets, db) = super::super::tests::fixture();
    let ident = id(NOW, 1);
    let op = attempt(&ident);
    db.write(&op, NOW, |_| Ok(completed())).unwrap();
    assert_replay(&db, &op, NOW + 29 * DAY + 1, completed().stored);
    assert_replay(&db, &op, NOW + 30 * DAY - 1, completed().stored);
    let changed = Attempt {
        account: "other",
        ..attempt(&ident)
    };
    for at in [NOW + 30 * DAY, NOW + 30 * DAY + 1] {
        assert!(matches!(db.lookup(&changed, at), Err(Error::Expired)));
    }
    assert_eq!(db.prune(NOW + 30 * DAY).unwrap(), 1);
    assert!(matches!(
        db.lookup(&changed, NOW + 30 * DAY),
        Err(Error::Expired)
    ));
}
#[test]
fn monotonic_clock_survives_reopen_and_rollback() {
    let (home, secrets, db) = super::super::tests::fixture();
    db.prune(NOW).unwrap();
    drop(db);
    let db = HostDb::open(home.path(), &secrets).unwrap();
    let old = id(NOW - 29 * DAY - 1, 1);
    assert!(matches!(
        db.lookup(&attempt(&old), NOW - 10 * DAY),
        Err(Error::Expired)
    ));
    let ident = id(NOW, 2);
    let op = attempt(&ident);
    assert!(matches!(
        db.write(&op, NOW + 1, |tx| {
            tx.execute(
                "INSERT INTO org_owners VALUES ('org','creator','member')",
                [],
            )?;
            Err(Error::Unsafe)
        }),
        Err(Error::Unsafe)
    ));
    assert_eq!(
        scalar(
            &db,
            "SELECT CAST(value AS INTEGER) FROM meta WHERE key='clock_high_ms'"
        ),
        NOW
    );
    assert_eq!(scalar(&db, "SELECT count(*) FROM org_owners"), 0);
    assert_eq!(scalar(&db, "SELECT count(*) FROM op_journal"), 0);
    for bad in ["-1", "bad", "9223372036854775808"] {
        db.connection
            .lock()
            .unwrap()
            .execute("UPDATE meta SET value=?1 WHERE key='clock_high_ms'", [bad])
            .unwrap();
        assert!(matches!(db.lookup(&op, NOW), Err(Error::Unsafe)));
    }
}
#[test]
fn verifier_is_atomic_and_successful_create_never_blindly_replays() {
    let (home, secrets, db) = super::super::tests::fixture();
    let ident = id(NOW, 1);
    let op = Attempt {
        kind: "account.create",
        ..attempt(&ident)
    };
    assert!(matches!(
        db.write(&op, NOW, |_| Ok(completed())),
        Err(Error::Unsafe)
    ));
    assert_eq!(scalar(&db, "SELECT count(*) FROM op_journal"), 0);
    db.write(&op, NOW, |_| Ok(create_effect("created")))
        .unwrap();
    drop(db);
    let db = HostDb::open(home.path(), &secrets).unwrap();
    match db.lookup(&op, NOW).unwrap() {
        Lookup::NeedsVerification { verifier, stored } => {
            assert_eq!(verifier.expose(), PHC);
            assert_eq!(stored, create_effect("created").stored);
        }
        _ => panic!("successful create cannot Replay"),
    }
    let other = id(NOW, 2);
    let change = attempt(&other);
    assert!(matches!(
        db.write(&change, NOW, |tx| {
            delete_verifiers(tx, "created")?;
            Err(Error::Unsafe)
        }),
        Err(Error::Unsafe)
    ));
    assert_eq!(scalar(&db, "SELECT count(*) FROM op_verifiers"), 1);
    db.write(&change, NOW, |tx| {
        delete_verifiers(tx, "created")?;
        Ok(completed())
    })
    .unwrap();
    assert!(matches!(db.lookup(&op, NOW), Err(Error::OperationReused)));
}
#[test]
fn refusals_are_replayed_without_verifiers_and_unstored_errors_rollback() {
    let (_home, _secrets, db) = super::super::tests::fixture();
    for (seq, code) in [
        (1, org_proto::ErrorCode::AlreadyExists),
        (2, org_proto::ErrorCode::InvalidName),
    ] {
        let ident = id(NOW, seq);
        let op = Attempt {
            kind: "account.create",
            ..attempt(&ident)
        };
        db.write(&op, NOW, |_| Ok(rejected(code))).unwrap();
        assert_replay(&db, &op, NOW, rejected(code).stored);
    }
    assert_eq!(scalar(&db, "SELECT count(*) FROM op_verifiers"), 0);
    for code in [
        org_proto::ErrorCode::NotAuthorized,
        org_proto::ErrorCode::Unavailable,
        org_proto::ErrorCode::StorageError,
        org_proto::ErrorCode::Cancelled,
    ] {
        let ident = id(NOW, 3);
        assert!(matches!(
            db.write(&attempt(&ident), NOW, |_| Ok(rejected(code))),
            Err(Error::Unsafe)
        ));
    }
    assert_eq!(scalar(&db, "SELECT count(*) FROM op_journal"), 2);
}
#[test]
fn commit_failure_rolls_back_effect_and_clock() {
    let (_home, _secrets, db) = super::super::tests::fixture();
    let ident = id(NOW, 1);
    let op = Attempt {
        kind: "account.create",
        ..attempt(&ident)
    };
    let result = db.write(&op, NOW, |tx| {
        tx.execute_batch("PRAGMA defer_foreign_keys=ON;")?;
        tx.execute(
            "INSERT INTO op_verifiers VALUES ('missing-op','created','opaque')",
            [],
        )?;
        tx.execute(
            "INSERT INTO org_owners VALUES ('org','creator','member')",
            [],
        )?;
        Ok(create_effect("created"))
    });
    assert!(matches!(result, Err(Error::Store(StoreFault::Constraint))));
    for table in ["op_journal", "op_verifiers", "org_owners"] {
        assert_eq!(scalar(&db, &format!("SELECT count(*) FROM {table}")), 0);
    }
    assert_eq!(
        scalar(
            &db,
            "SELECT CAST(value AS INTEGER) FROM meta WHERE key='clock_high_ms'"
        ),
        123
    );
}
#[test]
fn pruning_is_bounded_and_exempts_reserved_or_staged_rows() {
    let (_home, _secrets, db) = super::super::tests::fixture();
    {
        let mut c = db.connection.lock().unwrap();
        let tx = c.transaction().unwrap();
        for seq in 0..302 {
            let ident = id(NOW - 31 * DAY, seq);
            let state = if seq == 300 {
                "reserved"
            } else if seq == 301 {
                "abandoned"
            } else {
                "rejected"
            };
            let result = (seq != 300).then(|| {
                serde_json::to_string(&rejected(org_proto::ErrorCode::InvalidName).stored).unwrap()
            });
            tx.execute(
                "INSERT INTO op_journal VALUES (?1,'organization.create',?2,'creator',?3,?4,?5)",
                (&ident, [7u8; 32], state, result, NOW - DAY),
            )
            .unwrap();
            if seq == 301 {
                tx.execute(
                    "INSERT INTO org_create_staging VALUES (?1,'name','org','office','member')",
                    [&ident],
                )
                .unwrap();
            }
        }
        tx.commit().unwrap();
    }
    assert_eq!(db.prune(NOW).unwrap(), 256);
    assert_eq!(scalar(&db, "SELECT count(*) FROM op_journal"), 46);
    let ident = id(NOW, 700);
    db.write(&attempt(&ident), NOW, |_| Ok(completed()))
        .unwrap();
    assert_eq!(scalar(&db, "SELECT count(*) FROM op_journal"), 3);
    assert_eq!(db.prune(NOW).unwrap(), 0);
    assert_eq!(scalar(&db, "SELECT count(*) FROM org_create_staging"), 1);
}
#[test]
fn typed_attempt_uses_the_shared_fingerprint_without_session_or_id() {
    use org_host_control::ops::OfficeRestartParams;
    let op = OfficeRestartParams {
        session: serde_json::from_value(json!("one")).unwrap(),
        operation_id: id(NOW, 1),
        office_id: "office".into(),
    };
    let a = Attempt::new(&op, "creator").unwrap();
    assert_eq!(a.id, op.operation_id);
    assert_eq!(a.kind, "office.restart");
    assert_eq!(
        a.fingerprint,
        org_host_control::journal::fingerprint(&op).unwrap()
    );
}

#[test]
fn unsupported_outcomes_and_verifier_mismatches_roll_back_the_effect() {
    let (_home, _secrets, db) = super::super::tests::fixture();
    let ident = id(NOW, 1);
    let op = attempt(&ident);
    let mut wrong_account = create_effect("created");
    wrong_account.stored = Stored::Completed {
        result: json!({"account":"different"}),
    };
    for (kind, effect) in [
        (
            "office.restart",
            Effect {
                stored: Stored::Accepted,
                verifier: None,
            },
        ),
        (
            "office.restart",
            Effect {
                stored: Stored::Unknown,
                verifier: None,
            },
        ),
        ("office.restart", create_effect("created")),
        ("account.create", wrong_account),
        ("bootstrap", completed()),
    ] {
        let op = Attempt {
            kind,
            ..attempt(&ident)
        };
        assert!(matches!(
            db.write(&op, NOW, |tx| {
                tx.execute(
                    "INSERT INTO org_owners VALUES ('org','creator','member')",
                    [],
                )?;
                Ok(effect)
            }),
            Err(Error::Unsafe)
        ));
        assert_eq!(scalar(&db, "SELECT count(*) FROM op_journal"), 0);
        assert_eq!(scalar(&db, "SELECT count(*) FROM org_owners"), 0);
    }
    assert!(matches!(
        db.write(&op, -1, |_| panic!("negative clock")),
        Err(Error::Unsafe)
    ));
}

#[test]
fn failure_at_each_sql_write_rolls_back_the_entire_transaction() {
    for (name, event, table) in [
        ("clock", "UPDATE", "meta"),
        ("row", "INSERT", "op_journal"),
        ("verifier", "INSERT", "op_verifiers"),
    ] {
        let (_home, _secrets, db) = super::super::tests::fixture();
        db.connection.lock().unwrap().execute_batch(&format!("CREATE TRIGGER fail_{name} BEFORE {event} ON {table} BEGIN SELECT RAISE(ABORT,'test failure'); END;")).unwrap();
        let ident = id(NOW, 1);
        let op = Attempt {
            kind: "account.create",
            ..attempt(&ident)
        };
        assert!(matches!(
            db.write(&op, NOW, |tx| {
                tx.execute(
                    "INSERT INTO org_owners VALUES ('org','creator','member')",
                    [],
                )?;
                Ok(create_effect("created"))
            }),
            Err(Error::Store(StoreFault::Constraint))
        ));
        for table in ["op_journal", "op_verifiers", "org_owners"] {
            assert_eq!(
                scalar(&db, &format!("SELECT count(*) FROM {table}")),
                0,
                "{name}"
            );
        }
        assert_eq!(
            scalar(
                &db,
                "SELECT CAST(value AS INTEGER) FROM meta WHERE key='clock_high_ms'"
            ),
            123
        );
    }
}

#[test]
fn reserved_conflicts_and_linked_or_abandoned_rows_replay_without_transitions() {
    let (_home, _secrets, db) = super::super::tests::fixture();
    for (sequence, state) in [(1, "reserved"), (2, "linked"), (3, "abandoned")] {
        let ident = id(NOW, sequence);
        let op = Attempt {
            kind: "organization.create",
            ..attempt(&ident)
        };
        let result = match state {
            "reserved" => None,
            "linked" => Some(serde_json::to_string(&completed().stored).unwrap()),
            _ => Some(
                serde_json::to_string(&rejected(org_proto::ErrorCode::Cancelled).stored).unwrap(),
            ),
        };
        db.connection
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO op_journal VALUES (?1,?2,?3,?4,?5,?6,?7)",
                (
                    op.id,
                    op.kind,
                    op.fingerprint,
                    op.account,
                    state,
                    result,
                    NOW + 30 * DAY,
                ),
            )
            .unwrap();
        match state {
            "reserved" => assert!(matches!(db.lookup(&op, NOW), Err(Error::Conflict))),
            "linked" => assert_replay(&db, &op, NOW, completed().stored),
            _ => assert_replay(
                &db,
                &op,
                NOW,
                rejected(org_proto::ErrorCode::Cancelled).stored,
            ),
        }
        assert!(matches!(
            db.write(&op, NOW, |_| panic!("immutable existing row")),
            Ok(Write::Existing(_)) | Err(Error::Conflict)
        ));
    }
    let ident = id(NOW, 4);
    let op = Attempt {
        kind: "organization.create",
        ..attempt(&ident)
    };
    assert!(matches!(
        db.write(&op, NOW, |_| Ok(completed())),
        Err(Error::Store(StoreFault::Constraint))
    ));
    assert_eq!(scalar(&db, "SELECT count(*) FROM op_journal"), 3);
}

#[test]
fn lookup_expiry_does_not_reopen_after_wall_rollback_or_restart() {
    let (home, secrets, db) = super::super::tests::fixture();
    let ident = id(NOW, 1);
    let op = attempt(&ident);
    db.write(&op, NOW, |_| Ok(completed())).unwrap();
    assert!(matches!(
        db.lookup(&op, NOW + 30 * DAY),
        Err(Error::Expired)
    ));
    let rollback_stays_expired = matches!(db.lookup(&op, NOW + 30 * DAY - 1), Err(Error::Expired));
    drop(db);
    let db = HostDb::open(home.path(), &secrets).unwrap();
    let restart_stays_expired = matches!(db.lookup(&op, NOW + 30 * DAY - 1), Err(Error::Expired));
    assert!(rollback_stays_expired && restart_stays_expired,
        "expired lookup must not resurrect: rollback={rollback_stays_expired}, restart={restart_stays_expired}");
}

#[test]
fn unchanged_lookup_clock_does_not_write_again() {
    let (_home, _secrets, db) = super::super::tests::fixture();
    let ident = id(NOW, 1);
    let op = attempt(&ident);
    db.write(&op, NOW, |_| Ok(completed())).unwrap();
    let changes = || db.connection.lock().unwrap().total_changes();
    let before = changes();
    assert_replay(&db, &op, NOW, completed().stored);
    assert_eq!(changes(), before);
    assert_replay(&db, &op, NOW + 1, completed().stored);
    let advanced = changes();
    assert_eq!(advanced, before + 1);
    assert_replay(&db, &op, NOW, completed().stored);
    assert_eq!(changes(), advanced);
}
#[test]
fn time_based_write_refusal_keeps_its_clock_after_rollback() {
    let (_home, _secrets, db) = super::super::tests::fixture();
    let ident = id(NOW, 1);
    let op = attempt(&ident);
    db.write(&op, NOW, |_| Ok(completed())).unwrap();
    assert!(matches!(
        db.write(&op, NOW + 30 * DAY, |_| panic!("expired")),
        Err(Error::Expired)
    ));
    assert!(matches!(
        db.lookup(&op, NOW + 30 * DAY - 1),
        Err(Error::Expired)
    ));
}

#[test]
fn pruning_at_exact_expiry_cascades_create_time_verifiers() {
    let (_home, _secrets, db) = super::super::tests::fixture();
    let ident = id(NOW, 1);
    let op = Attempt {
        kind: "account.create",
        ..attempt(&ident)
    };
    db.write(&op, NOW, |_| Ok(create_effect("created")))
        .unwrap();
    assert_eq!(scalar(&db, "SELECT count(*) FROM op_verifiers"), 1);
    assert_eq!(db.prune(NOW + 30 * DAY).unwrap(), 1);
    assert_eq!(scalar(&db, "SELECT count(*) FROM op_journal"), 0);
    assert_eq!(scalar(&db, "SELECT count(*) FROM op_verifiers"), 0);
}

// Exercise the same decision through the unlocked admission lookup and writer relookup.
fn decision(db: &HostDb, op: &Attempt<'_>, at: i64, writer: bool) -> Result<Lookup, Error> {
    if writer {
        match db.write(op, at, |_| {
            panic!("existing attempt must never run the effect")
        })? {
            Write::Existing(lookup) => Ok(lookup),
            Write::Committed(_) => panic!("existing row must not commit another effect"),
        }
    } else {
        db.lookup(op, at)
    }
}

#[test]
fn lookup_and_existing_write_decide_before_advancing_the_clock() {
    for writer in [false, true] {
        let (_home, _secrets, db) = super::super::tests::fixture();
        let ident = id(NOW, 1);
        let op = attempt(&ident);
        db.write(&op, NOW, |_| Ok(completed())).unwrap();
        // Deterministic ordering probe: the clock UPDATE removes the row AFTER its decision.
        db.connection.lock().unwrap().execute_batch("CREATE TRIGGER decision_order AFTER UPDATE OF value ON meta WHEN NEW.key='clock_high_ms' BEGIN DELETE FROM op_journal; END;").unwrap();
        assert!(matches!(
            decision(&db, &op, NOW + 1, writer),
            Ok(Lookup::Replay(_))
        ));
        assert_eq!(scalar(&db, "SELECT count(*) FROM op_journal"), 0);
        assert_eq!(
            scalar(
                &db,
                "SELECT CAST(value AS INTEGER) FROM meta WHERE key='clock_high_ms'"
            ),
            NOW + 1
        );
    }
}

#[test]
fn lookup_and_existing_write_never_escape_a_failed_clock_update_or_commit() {
    for writer in [false, true] {
        for expired in [false, true] {
            for commit in [false, true] {
                let (_home, _secrets, db) = super::super::tests::fixture();
                let ident = id(NOW, 1);
                let op = attempt(&ident);
                db.write(&op, NOW, |_| Ok(completed())).unwrap();
                let before = db.connection.lock().unwrap().total_changes();
                let fault = if commit {
                    "CREATE TABLE clock_commit_probe (operation_id TEXT REFERENCES op_journal(operation_id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER fail_clock AFTER UPDATE OF value ON meta WHEN NEW.key='clock_high_ms' BEGIN INSERT INTO clock_commit_probe VALUES ('missing'); END;"
                } else {
                    "CREATE TRIGGER fail_clock BEFORE UPDATE OF value ON meta WHEN NEW.key='clock_high_ms' BEGIN SELECT RAISE(ABORT,'clock write failed'); END;"
                };
                db.connection.lock().unwrap().execute_batch(fault).unwrap();
                let at = NOW + if expired { 30 * DAY } else { 1 };
                assert!(
                    matches!(
                        decision(&db, &op, at, writer),
                        Err(Error::Store(StoreFault::Constraint))
                    ),
                    "writer={writer}, expired={expired}, commit={commit}"
                );
                assert_eq!(
                    scalar(
                        &db,
                        "SELECT CAST(value AS INTEGER) FROM meta WHERE key='clock_high_ms'"
                    ),
                    NOW
                );
                assert_eq!(scalar(&db, "SELECT count(*) FROM op_journal"), 1);
                if commit {
                    assert_eq!(scalar(&db, "SELECT count(*) FROM clock_commit_probe"), 0);
                    assert!(
                        db.connection.lock().unwrap().total_changes() > before,
                        "UPDATE succeeded before deferred COMMIT failed"
                    );
                }
                db.connection
                    .lock()
                    .unwrap()
                    .execute_batch("DROP TRIGGER fail_clock;")
                    .unwrap();
                assert_replay(&db, &op, NOW, completed().stored);
            }
        }
    }
}

#[test]
fn each_decision_or_prune_uses_one_immediate_transaction_without_handoff() {
    let (_home, _secrets, db) = super::super::tests::fixture();
    let ident = id(NOW, 1);
    let op = attempt(&ident);
    db.write(&op, NOW, |_| Ok(completed())).unwrap();
    BEGIN_COUNT.with(|count| count.set(0));
    assert_replay(&db, &op, NOW + 1, completed().stored);
    assert_eq!(BEGIN_COUNT.with(|count| count.get()), 1);
    assert!(matches!(
        decision(&db, &op, NOW + 2, true),
        Ok(Lookup::Replay(_))
    ));
    assert_eq!(BEGIN_COUNT.with(|count| count.get()), 2);
    db.prune(NOW + 3).unwrap();
    assert_eq!(BEGIN_COUNT.with(|count| count.get()), 3);
}

#[test]
fn a_poisoned_connection_refuses_every_journal_entry_point() {
    let (_home, _secrets, db) = super::super::tests::fixture();
    let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _held = db.connection.lock().unwrap();
        panic!("simulate a panicking caller while it owns the registry");
    }));
    assert!(poisoned.is_err());
    let ident = id(NOW, 1);
    let op = attempt(&ident);
    assert!(matches!(db.lookup(&op, NOW), Err(Error::Unsafe)));
    assert!(matches!(
        db.write(&op, NOW, |_| panic!("poisoned")),
        Err(Error::Unsafe)
    ));
    assert_eq!(db.prune(NOW), Err(Error::Unsafe));
    let poison = db.connection.lock().err().unwrap();
    let c = poison.into_inner();
    assert_eq!(
        c.query_row("SELECT count(*) FROM op_journal", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn inconsistent_stored_outcomes_refuse_replay_without_changing_the_row() {
    for (state, kind) in [
        ("completed", "office.restart"),
        ("rejected", "office.restart"),
        ("linked", "organization.create"),
        ("abandoned", "organization.create"),
    ] {
        for stored in [
            completed().stored,
            rejected(org_proto::ErrorCode::AlreadyExists).stored,
            Stored::Accepted,
            Stored::Unknown,
        ] {
            if matches!(
                (state, &stored),
                ("completed" | "linked", Stored::Completed { .. })
                    | ("rejected" | "abandoned", Stored::Rejected { .. })
            ) {
                continue;
            }
            let (_home, _secrets, db) = super::super::tests::fixture();
            let ident = id(NOW, 1);
            let op = Attempt {
                kind,
                ..attempt(&ident)
            };
            let result = if matches!(stored, Stored::Unknown) {
                r#"{"status":"future_status"}"#.to_owned()
            } else {
                serde_json::to_string(&stored).unwrap()
            };
            let c = db.connection.lock().unwrap();
            c.execute(
                "INSERT INTO op_journal VALUES (?1,?2,?3,?4,?5,?6,?7)",
                (
                    op.id,
                    op.kind,
                    op.fingerprint,
                    op.account,
                    state,
                    &result,
                    NOW + 30 * DAY,
                ),
            )
            .unwrap();
            drop(c);
            assert!(
                matches!(db.lookup(&op, NOW), Err(Error::Unsafe)),
                "{state}/{result}"
            );
            assert!(matches!(
                db.write(&op, NOW, |_| panic!("invalid stored outcome")),
                Err(Error::Unsafe)
            ));
            let after: String = db
                .connection
                .lock()
                .unwrap()
                .query_row(
                    "SELECT result FROM op_journal WHERE operation_id=?1",
                    [op.id],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(after, result);
        }
    }
}

#[test]
fn malformed_stored_json_refuses_read_and_writer_relookup() {
    let (_home, _secrets, db) = super::super::tests::fixture();
    let ident = id(NOW, 1);
    let op = attempt(&ident);
    db.connection
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO op_journal VALUES (?1,?2,?3,?4,'completed','{',?5)",
            (op.id, op.kind, op.fingerprint, op.account, NOW + 30 * DAY),
        )
        .unwrap();
    assert!(matches!(db.lookup(&op, NOW), Err(Error::Unsafe)));
    assert!(matches!(
        db.write(&op, NOW, |_| panic!("malformed stored result")),
        Err(Error::Unsafe)
    ));
    assert_eq!(
        scalar(&db, "SELECT count(*) FROM op_journal WHERE result='{'"),
        1
    );
}

#[test]
fn pruning_and_clock_roll_back_with_the_effect_and_delete_failures() {
    let (_home, _secrets, db) = super::super::tests::fixture();
    let expired = id(NOW - 30 * DAY, 1);
    let stored = serde_json::to_string(&completed().stored).unwrap();
    db.connection
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO op_journal VALUES (?1,'office.restart',?2,'creator','completed',?3,?4)",
            (&expired, [7u8; 32], stored, NOW - 1),
        )
        .unwrap();
    let ident = id(NOW, 1);
    let op = attempt(&ident);
    assert!(matches!(
        db.write(&op, NOW, |_| Err(Error::Unsafe)),
        Err(Error::Unsafe)
    ));
    assert_eq!(scalar(&db, "SELECT count(*) FROM op_journal"), 1);
    assert_eq!(
        scalar(
            &db,
            "SELECT CAST(value AS INTEGER) FROM meta WHERE key='clock_high_ms'"
        ),
        123
    );
    db.connection.lock().unwrap().execute_batch("CREATE TRIGGER fail_prune BEFORE DELETE ON op_journal BEGIN SELECT RAISE(ABORT,'prune failed'); END;").unwrap();
    assert!(matches!(
        db.write(&op, NOW, |_| Ok(completed())),
        Err(Error::Store(StoreFault::Constraint))
    ));
    assert_eq!(db.prune(NOW), Err(Error::Store(StoreFault::Constraint)));
    assert_eq!(scalar(&db, "SELECT count(*) FROM op_journal"), 1);
    assert_eq!(
        scalar(
            &db,
            "SELECT CAST(value AS INTEGER) FROM meta WHERE key='clock_high_ms'"
        ),
        123
    );
}

#[test]
fn deleting_one_accounts_verifiers_preserves_another_accounts_create_replay() {
    let (_home, _secrets, db) = super::super::tests::fixture();
    let first = id(NOW, 1);
    let second = id(NOW, 2);
    let first = Attempt {
        kind: "account.create",
        ..attempt(&first)
    };
    let second = Attempt {
        kind: "account.create",
        ..attempt(&second)
    };
    db.write(&first, NOW, |_| Ok(create_effect("created-a")))
        .unwrap();
    db.write(&second, NOW, |_| {
        let mut effect = create_effect("created-b");
        effect.verifier = Some((
            "created-b".into(),
            Verifier::new(PHC.replace("$AAAAAAAA", "$BAAAAAAA")).unwrap(),
        ));
        Ok(effect)
    })
    .unwrap();
    assert_eq!(scalar(&db, "SELECT count(*) FROM op_verifiers"), 2);
    let change = id(NOW, 3);
    db.write(&attempt(&change), NOW, |tx| {
        delete_verifiers(tx, "created-a")?;
        Ok(completed())
    })
    .unwrap();
    assert_eq!(scalar(&db, "SELECT count(*) FROM op_verifiers"), 1);
    assert!(matches!(
        db.lookup(&first, NOW),
        Err(Error::OperationReused)
    ));
    match db.lookup(&second, NOW).unwrap() {
        Lookup::NeedsVerification { verifier, stored } => {
            assert_eq!(verifier.expose(), &PHC.replace("$AAAAAAAA", "$BAAAAAAA"));
            assert_eq!(stored, create_effect("created-b").stored);
        }
        _ => panic!("the other account must retain its own create-time verifier"),
    }
}

#[test]
fn a_late_recorded_operation_expires_thirty_days_after_its_id() {
    let (_home, _secrets, db) = super::super::tests::fixture();
    let ident = id(NOW, 1);
    let op = attempt(&ident);
    db.write(&op, NOW + DAY, |_| Ok(completed())).unwrap();
    assert_replay(&db, &op, NOW + 30 * DAY - 1, completed().stored);
    assert!(matches!(
        db.lookup(&op, NOW + 30 * DAY),
        Err(Error::Expired)
    ));
}

#[test]
fn a_failed_prune_commit_preserves_rows_verifiers_and_clock_after_reopen() {
    let (home, secrets, db) = super::super::tests::fixture();
    let ident = id(NOW, 1);
    let op = Attempt {
        kind: "account.create",
        ..attempt(&ident)
    };
    db.write(&op, NOW, |_| Ok(create_effect("created")))
        .unwrap();
    db.connection
        .lock()
        .unwrap()
        .execute_batch(
            "CREATE TABLE prune_commit_probe (
             operation_id TEXT REFERENCES op_journal(operation_id) DEFERRABLE INITIALLY DEFERRED
         );
         CREATE TRIGGER fail_prune_commit AFTER DELETE ON op_journal
         BEGIN INSERT INTO prune_commit_probe VALUES ('missing'); END;",
        )
        .unwrap();
    assert_eq!(
        db.prune(NOW + 30 * DAY),
        Err(Error::Store(StoreFault::Constraint))
    );
    for table in ["op_journal", "op_verifiers"] {
        assert_eq!(scalar(&db, &format!("SELECT count(*) FROM {table}")), 1);
    }
    assert_eq!(scalar(&db, "SELECT count(*) FROM prune_commit_probe"), 0);
    assert_eq!(
        scalar(
            &db,
            "SELECT CAST(value AS INTEGER) FROM meta WHERE key='clock_high_ms'"
        ),
        NOW
    );
    drop(db);
    let db = HostDb::open(home.path(), &secrets).unwrap();
    assert_eq!(scalar(&db, "SELECT count(*) FROM op_journal"), 1);
    assert_eq!(scalar(&db, "SELECT count(*) FROM op_verifiers"), 1);
    assert_eq!(scalar(&db, "SELECT count(*) FROM prune_commit_probe"), 0);
    assert_eq!(
        scalar(
            &db,
            "SELECT CAST(value AS INTEGER) FROM meta WHERE key='clock_high_ms'"
        ),
        NOW
    );
    match db.lookup(&op, NOW).unwrap() {
        Lookup::NeedsVerification { verifier, stored } => {
            assert_eq!(verifier.expose(), PHC);
            assert_eq!(stored, create_effect("created").stored);
        }
        _ => panic!("failed pruning must preserve create replay after reopen"),
    }
}

#[test]
fn malformed_stored_replay_verifier_refuses_instead_of_exposing_unchecked_phc() {
    let (_home, _secrets, db) = super::super::tests::fixture();
    let operation = id(NOW, 1);
    let op = Attempt {
        kind: "account.create",
        ..attempt(&operation)
    };
    db.write(&op, NOW, |_| Ok(create_effect("created")))
        .unwrap();
    db.connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE op_verifiers SET verifier=?1",
            ["invalid sensitive PHC"],
        )
        .unwrap();
    assert!(matches!(db.lookup(&op, NOW), Err(Error::Unsafe)));
    assert_eq!(
        db.connection
            .lock()
            .unwrap()
            .query_row("SELECT verifier FROM op_verifiers", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "invalid sensitive PHC"
    );
}

/// The real wall clock: what `Authentication::open` prunes at, unlike the fixed [`NOW`].
fn wall_now() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}
/// `count` settled rows plus one `reserved` and one staged row, all expired since `at - DAY`.
fn seed_expired(db: &HostDb, at: i64, count: u64) {
    let refusal =
        serde_json::to_string(&rejected(org_proto::ErrorCode::InvalidName).stored).unwrap();
    let mut c = db.connection.lock().unwrap();
    let tx = c.transaction().unwrap();
    for seq in 0..count + 2 {
        let ident = id(at - 31 * DAY, 10_000 + seq);
        let (state, result) = if seq == count {
            ("reserved", None)
        } else if seq == count + 1 {
            ("abandoned", Some(&refusal))
        } else {
            ("rejected", Some(&refusal))
        };
        tx.execute(
            "INSERT INTO op_journal VALUES (?1,'organization.create',?2,'creator',?3,?4,?5)",
            (&ident, [7u8; 32], state, result, at - DAY),
        )
        .unwrap();
        if state == "abandoned" {
            tx.execute(
                "INSERT INTO org_create_staging VALUES (?1,'name','org','office','member')",
                [&ident],
            )
            .unwrap();
        }
    }
    tx.commit().unwrap();
}
#[test]
fn prune_expired_drains_whole_batches_one_transaction_each() {
    let (_home, _secrets, db) = super::super::tests::fixture();
    let live = id(NOW - DAY, 1);
    db.write(&attempt(&live), NOW - DAY, |_| Ok(completed()))
        .unwrap();
    // An exact multiple of the batch: two full batches, then one that finds nothing.
    seed_expired(&db, NOW, 2 * u64::from(PRUNE_BATCH));
    BEGIN_COUNT.with(|count| count.set(0));
    assert_eq!(
        db.prune_expired(NOW).unwrap(),
        PruneOutcome {
            removed: 2 * u64::from(PRUNE_BATCH),
            more_remaining: false,
        }
    );
    assert_eq!(BEGIN_COUNT.with(|count| count.get()), 3);
    assert_eq!(scalar(&db, "SELECT count(*) FROM op_journal"), 3);
    assert_eq!(scalar(&db, "SELECT count(*) FROM org_create_staging"), 1);
    assert_replay(&db, &attempt(&live), NOW, completed().stored);
    assert_eq!(
        db.prune_expired(NOW).unwrap(),
        PruneOutcome {
            removed: 0,
            more_remaining: false,
        }
    );
}
/// TASK-235: a host start (`Authentication::open`, as `startup_authentication` opens the
/// registry) prunes every verifier that expired while the host was down, past one batch,
/// and keeps the live verifier and the rows still owed a reconciliation.
#[test]
fn host_start_prunes_expired_verifiers_and_keeps_live_ones() {
    const HOUR: i64 = 3_600_000;
    let (home, secrets, db) = super::super::tests::fixture();
    let now = wall_now();
    fn create(ident: &str) -> Attempt<'_> {
        Attempt {
            kind: "account.create",
            ..attempt(ident)
        }
    }
    // Expires an hour before the start; the later live write's own prune runs too early for it.
    let expired = id(now - 30 * DAY - HOUR, 1);
    db.write(&create(&expired), now - 30 * DAY - HOUR, |_| {
        Ok(create_effect("expired"))
    })
    .unwrap();
    let live = id(now - DAY, 2);
    db.write(&create(&live), now - DAY, |_| Ok(create_effect("live")))
        .unwrap();
    seed_expired(&db, now, u64::from(PRUNE_BATCH) + 44);
    assert_eq!(
        scalar(&db, "SELECT count(*) FROM op_journal"),
        2 + i64::from(PRUNE_BATCH) + 46
    );
    assert_eq!(scalar(&db, "SELECT count(*) FROM op_verifiers"), 2);
    drop(db);

    let custody: std::sync::Arc<dyn crate::host_secrets::HostSecrets> =
        std::sync::Arc::new(secrets);
    let instance = super::super::tests::init().instance_id;
    let started = crate::authentication::Authentication::open(
        home.path(),
        std::sync::Arc::clone(&custody),
        &instance,
    )
    .unwrap();
    drop(started);

    let db = HostDb::open(home.path(), &*custody).unwrap();
    // The live create, the reserved row and the staged row.
    assert_eq!(scalar(&db, "SELECT count(*) FROM op_journal"), 3);
    assert_eq!(scalar(&db, "SELECT count(*) FROM org_create_staging"), 1);
    let kept: Vec<String> = db
        .connection
        .lock()
        .unwrap()
        .prepare("SELECT created_account FROM op_verifiers")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(kept, ["live"]);
    assert!(matches!(
        db.lookup(&create(&live), now),
        Ok(Lookup::NeedsVerification { .. })
    ));
    assert!(matches!(
        db.lookup(&create(&expired), now),
        Err(Error::Expired)
    ));
}
/// A prune that fails at start is maintenance lost, not a refused start: the registry opens,
/// nothing is half-deleted, and the clock does not move past what was served.
#[test]
fn host_start_survives_a_failing_prune_without_deleting_anything() {
    let (home, secrets, db) = super::super::tests::fixture();
    seed_expired(&db, wall_now(), 3);
    db.connection
        .lock()
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER fail_prune BEFORE DELETE ON op_journal \
             BEGIN SELECT RAISE(ABORT,'prune failed'); END;",
        )
        .unwrap();
    drop(db);

    let custody: std::sync::Arc<dyn crate::host_secrets::HostSecrets> =
        std::sync::Arc::new(secrets);
    let instance = super::super::tests::init().instance_id;
    let (started, lines) = crate::host_log::capture(|| {
        crate::authentication::Authentication::open(
            home.path(),
            std::sync::Arc::clone(&custody),
            &instance,
        )
        .unwrap()
    });
    drop(started);
    assert!(
        lines
            .iter()
            .any(|line| line.contains("expired operation journal entries were not pruned at start")),
        "expected startup prune failure warning, got {lines:?}"
    );

    let db = HostDb::open(home.path(), &*custody).unwrap();
    assert_eq!(scalar(&db, "SELECT count(*) FROM op_journal"), 5);
    assert_eq!(
        scalar(
            &db,
            "SELECT CAST(value AS INTEGER) FROM meta WHERE key='clock_high_ms'"
        ),
        123
    );
}

#[test]
fn prune_expired_stops_deterministically_at_batch_cap_under_after_delete_trigger() {
    let (_home, _secrets, db) = super::super::tests::fixture();
    seed_expired(&db, NOW, u64::from(PRUNE_BATCH) + 5);
    // An AFTER DELETE trigger that re-inserts every deleted expired row would trap an
    // unbounded loop forever; prune_expired_capped must stop cleanly at the batch cap.
    db.connection
        .lock()
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reinsert_expired AFTER DELETE ON op_journal \
             BEGIN \
               INSERT INTO op_journal \
               (operation_id, kind, fingerprint, account, state, result, expires_at_ms) \
               VALUES (OLD.operation_id, OLD.kind, OLD.fingerprint, OLD.account, OLD.state, \
                       OLD.result, OLD.expires_at_ms); \
             END;",
        )
        .unwrap();
    BEGIN_COUNT.with(|count| count.set(0));
    let outcome = db.prune_expired_capped(NOW, 3).unwrap();
    assert_eq!(
        outcome,
        PruneOutcome {
            removed: 3 * u64::from(PRUNE_BATCH),
            more_remaining: true,
        }
    );
    assert_eq!(BEGIN_COUNT.with(|count| count.get()), 3);
}

#[test]
fn host_start_logs_warning_when_prune_reaches_batch_cap() {
    let (home, secrets, db) = super::super::tests::fixture();
    seed_expired(&db, wall_now(), u64::from(PRUNE_BATCH) + 5);
    // Re-inserting deleted expired rows forces startup prune_expired to hit MAX_PRUNE_BATCHES
    // without needing 262k fixture rows; verify startup completes and logs the cap warning.
    db.connection
        .lock()
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reinsert_for_cap AFTER DELETE ON op_journal \
             BEGIN \
               INSERT INTO op_journal \
               (operation_id, kind, fingerprint, account, state, result, expires_at_ms) \
               VALUES (OLD.operation_id, OLD.kind, OLD.fingerprint, OLD.account, OLD.state, \
                       OLD.result, OLD.expires_at_ms); \
             END;",
        )
        .unwrap();
    drop(db);

    let custody: std::sync::Arc<dyn crate::host_secrets::HostSecrets> =
        std::sync::Arc::new(secrets);
    let instance = super::super::tests::init().instance_id;
    let (started, lines) = crate::host_log::capture(|| {
        crate::authentication::Authentication::open(
            home.path(),
            std::sync::Arc::clone(&custody),
            &instance,
        )
        .unwrap()
    });
    drop(started);
    assert!(
        lines
            .iter()
            .any(|line| line.contains("reached its batch limit after removing")),
        "expected startup prune batch-cap warning, got {lines:?}"
    );
}

impl HostDb {
    /// Test fixture for `hostdb::contract_tests`: inserts one expired `reserved` row and one
    /// expired `abandoned` row referenced by `org_create_staging`.
    pub(crate) fn seed_expired_exempt_journal_rows(
        &self,
        reserved_id: &str,
        staged_id: &str,
        account: &str,
        expires_at_ms: i64,
    ) {
        let refusal =
            serde_json::to_string(&rejected(org_proto::ErrorCode::InvalidName).stored).unwrap();
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction().unwrap();
        tx.execute(
            "INSERT INTO op_journal VALUES (?1,'organization.create',?2,?3,'reserved',NULL,?4)",
            (reserved_id, [7u8; 32], account, expires_at_ms),
        )
        .unwrap();
        tx.execute(
            "INSERT INTO op_journal VALUES (?1,'organization.create',?2,?3,'abandoned',?4,?5)",
            (staged_id, [7u8; 32], account, refusal, expires_at_ms),
        )
        .unwrap();
        tx.execute(
            "INSERT INTO org_create_staging VALUES (?1,'name','org','office','member')",
            [staged_id],
        )
        .unwrap();
        tx.commit().unwrap();
    }
}
