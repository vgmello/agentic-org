use super::*;
use crate::hostdb::lockout::{Outcome, Socket};

#[test]
fn sixth_failure_starts_one_second_lock_and_survives_reopen() {
    let (home, secrets, db) = fixture();
    for n in 1..=6 {
        let state = db
            .record_login(Socket::Control, "sage", 1_000, Outcome::Failure)
            .unwrap();
        assert_eq!(state.now_ms(), 1_000);
        assert_eq!(state.is_locked(), n == 6, "failure {n}");
    }
    let row: (i64,i64,i64) = db.connection.lock().unwrap().query_row(
        "SELECT failures,locked_until_ms,last_failure_ms FROM login_lockout WHERE socket='control' AND login_key='sage'",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(row, (6, 2_000, 1_000));
    drop(db);
    let db = HostDb::open(home.path(), &secrets).unwrap();
    assert!(db
        .login_lockout(Socket::Control, "sage", 1_999)
        .unwrap()
        .is_locked());
    assert!(!db
        .login_lockout(Socket::Control, "sage", 2_000)
        .unwrap()
        .is_locked());
}

fn seed(db: &HostDb, socket: &str, key: &str, failures: i64, until: i64, last: i64) {
    db.connection
        .lock()
        .unwrap()
        .execute(
            "INSERT OR REPLACE INTO login_lockout VALUES (?1,?2,?3,?4,?5)",
            (socket, key, failures, until, last),
        )
        .unwrap();
}
fn row(db: &HostDb, socket: &str, key: &str) -> Option<(i64, i64, i64)> {
    use rusqlite::OptionalExtension;
    db.connection.lock().unwrap().query_row(
        "SELECT failures,locked_until_ms,last_failure_ms FROM login_lockout WHERE socket=?1 AND login_key=?2",
        (socket,key), |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
    ).optional().unwrap()
}
fn high(db: &HostDb) -> i64 {
    db.connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM meta WHERE key='clock_high_ms'",
            [],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn exponential_backoff_caps_at_fifteen_minutes_and_count_at_sixty_three() {
    let (_home, _secrets, db) = fixture();
    let mut time = 1_000;
    for failures in 1..=64 {
        let expected = if failures < 6 {
            0
        } else {
            (1_000_i64 << (failures - 6).min(10)).min(900_000)
        };
        let result = db
            .record_login(Socket::Local, "unknown", time as u64, Outcome::Failure)
            .unwrap();
        assert_eq!(result.is_locked(), failures >= 6);
        assert_eq!(
            row(&db, "local", "unknown"),
            Some((failures.min(63), time + expected, time))
        );
        assert_eq!(high(&db), time);
        if expected > 0 {
            assert!(db
                .login_lockout(Socket::Local, "unknown", (time + expected - 1) as u64)
                .unwrap()
                .is_locked());
            assert!(!db
                .login_lockout(Socket::Local, "unknown", (time + expected) as u64)
                .unwrap()
                .is_locked());
        }
        time += expected;
    }
}

#[test]
fn locked_attempts_cannot_update_counters_or_clock_but_authorized_reset_clears() {
    let (_home, _secrets, db) = fixture();
    seed(&db, "control", "sage", 6, 3_000, 1_000);
    for outcome in [Outcome::Failure, Outcome::Success] {
        let state = db
            .record_login(Socket::Control, "sage", 2_000, outcome)
            .unwrap();
        assert!(state.is_locked());
        assert_eq!(state.now_ms(), 2_000);
        assert_eq!(row(&db, "control", "sage"), Some((6, 3_000, 1_000)));
        assert_eq!(high(&db), 123);
    }
    assert!(!db
        .record_login(Socket::Control, "sage", 2_000, Outcome::Reset)
        .unwrap()
        .is_locked());
    assert_eq!(row(&db, "control", "sage"), None);
    assert_eq!(high(&db), 2_000);
}

#[test]
fn unknown_keys_socket_and_login_pairs_are_independent_and_bound_as_sql_parameters() {
    let (_home, _secrets, db) = fixture();
    for (socket, name) in [(Socket::Control, "control"), (Socket::Local, "local")] {
        for key in ["sage", "unknown", "' OR 1=1 --"] {
            assert!(!db.login_lockout(socket, key, 1_000).unwrap().is_locked());
            db.record_login(socket, key, 1_000, Outcome::Failure)
                .unwrap();
            assert_eq!(row(&db, name, key), Some((1, 1_000, 1_000)));
        }
    }
    db.record_login(Socket::Control, "unknown", 2_000, Outcome::Success)
        .unwrap();
    assert_eq!(row(&db, "control", "unknown"), None);
    for (name, key) in [
        ("control", "sage"),
        ("local", "unknown"),
        ("control", "' OR 1=1 --"),
    ] {
        assert_eq!(row(&db, name, key), Some((1, 1_000, 1_000)));
    }
    db.record_login(Socket::Local, "unknown", 2_000, Outcome::Reset)
        .unwrap();
    assert_eq!(row(&db, "local", "unknown"), None);
    // Eligible success includes the exact expiry boundary.
    seed(&db, "local", "expired", 6, 2_000, 1_000);
    assert!(!db
        .record_login(Socket::Local, "expired", 2_000, Outcome::Success)
        .unwrap()
        .is_locked());
    assert_eq!(row(&db, "local", "expired"), None);
}

#[test]
fn prune_uses_exact_thirty_minute_and_twenty_four_hour_boundaries() {
    for (failures, age) in [(5, 1_800_000), (6, 86_400_000)] {
        let (_home, _secrets, db) = fixture();
        seed(&db, "control", "old", failures, 1_000, 1_000);
        seed(&db, "local", "new", failures, 1_001, 1_001);
        assert_eq!(
            db.prune_login_lockouts((1_000 + age - 1) as u64).unwrap(),
            0
        );
        assert_eq!(db.prune_login_lockouts((1_000 + age) as u64).unwrap(), 1);
        assert_eq!(row(&db, "control", "old"), None);
        assert_eq!(row(&db, "local", "new"), Some((failures, 1_001, 1_001)));
        assert_eq!(db.prune_login_lockouts((1_001 + age) as u64).unwrap(), 1);
        assert_eq!(high(&db), 1_001 + age);
    }
}

#[test]
fn corrupt_counters_fail_closed_without_repair_or_clock_write_and_sweep_skips_them() {
    for bad in [
        (-1, 1_000, 1_000),
        (64, 1_000, 1_000),
        (1, 0, -1),
        (1, -1, 0),
        (1, 999, 1_000),
    ] {
        let (_home, _secrets, db) = fixture();
        seed(&db, "control", "bad", bad.0, bad.1, bad.2);
        assert!(db
            .login_lockout(Socket::Control, "bad", 100_000_000)
            .is_err());
        for outcome in [Outcome::Failure, Outcome::Success, Outcome::Reset] {
            assert!(db
                .record_login(Socket::Control, "bad", 100_000_000, outcome)
                .is_err());
            assert_eq!(row(&db, "control", "bad"), Some(bad));
            assert_eq!(high(&db), 123);
        }
        seed(&db, "local", "good", 1, 1_000, 1_000);
        assert_eq!(db.prune_login_lockouts(100_000_000).unwrap(), 1);
        assert_eq!(row(&db, "control", "bad"), Some(bad));
        assert_eq!(row(&db, "local", "good"), None);
    }
}

#[test]
fn unreadable_counter_values_propagate_errors_and_sweep_retains_them() {
    let (_home, _secrets, db) = fixture();
    // Simulate damaged storage: STRICT production DDL normally prevents this value.
    db.connection.lock().unwrap().execute_batch("DROP TABLE login_lockout;
        CREATE TABLE login_lockout (socket TEXT,login_key TEXT,failures,locked_until_ms,last_failure_ms,PRIMARY KEY(socket,login_key));
        INSERT INTO login_lockout VALUES ('control','bad','not a counter',1000,1000);").unwrap();
    assert!(db.login_lockout(Socket::Control, "bad", 2_000).is_err());
    for outcome in [Outcome::Failure, Outcome::Success, Outcome::Reset] {
        assert!(db
            .record_login(Socket::Control, "bad", 2_000, outcome)
            .is_err());
    }
    assert_eq!(high(&db), 123);
    assert_eq!(db.prune_login_lockouts(100_000_000).unwrap(), 0);
    let c = db.connection.lock().unwrap();
    assert_eq!(
        c.query_row("SELECT failures FROM login_lockout", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "not a counter"
    );
}

#[test]
fn nondecreasing_clock_is_shared_with_journal_and_reads_never_advance_it() {
    let (home, secrets, db) = fixture();
    db.record_login(Socket::Control, "sage", 5_000, Outcome::Failure)
        .unwrap();
    assert_eq!(high(&db), 5_000);
    assert_eq!(
        db.login_lockout(Socket::Local, "unknown", 7_000)
            .unwrap()
            .now_ms(),
        7_000
    );
    assert_eq!(high(&db), 5_000);
    assert_eq!(
        db.record_login(Socket::Control, "sage", 4_000, Outcome::Failure)
            .unwrap()
            .now_ms(),
        5_000
    );
    assert_eq!(row(&db, "control", "sage"), Some((2, 5_000, 5_000)));
    drop(db);
    let db = HostDb::open(home.path(), &secrets).unwrap();
    assert_eq!(
        db.login_lockout(Socket::Local, "unknown", 1)
            .unwrap()
            .now_ms(),
        5_000
    );
    db.connection.lock().unwrap().execute_batch("CREATE TRIGGER forbid_same_clock BEFORE UPDATE OF value ON meta WHEN NEW.value=OLD.value BEGIN SELECT RAISE(ABORT,'unchanged clock'); END;").unwrap();
    db.record_login(Socket::Local, "unknown", 1, Outcome::Failure)
        .unwrap();
    assert_eq!(db.prune_login_lockouts(1).unwrap(), 0);
}

#[test]
fn bad_clocks_and_time_overflow_cannot_mutate_counter_or_clock() {
    for clock in ["-1", "broken"] {
        let (_home, _secrets, db) = fixture();
        db.connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE meta SET value=?1 WHERE key='clock_high_ms'",
                [clock],
            )
            .unwrap();
        assert!(db.login_lockout(Socket::Control, "sage", 1_000).is_err());
        assert!(db
            .record_login(Socket::Control, "sage", 1_000, Outcome::Failure)
            .is_err());
        assert!(db.prune_login_lockouts(1_000).is_err());
        assert_eq!(row(&db, "control", "sage"), None);
        assert_eq!(
            db.connection
                .lock()
                .unwrap()
                .query_row(
                    "SELECT value FROM meta WHERE key='clock_high_ms'",
                    [],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            clock
        );
    }
    let (_home, _secrets, db) = fixture();
    assert!(db.login_lockout(Socket::Control, "sage", u64::MAX).is_err());
    assert!(db
        .record_login(Socket::Control, "sage", u64::MAX, Outcome::Failure)
        .is_err());
    assert!(db.prune_login_lockouts(u64::MAX).is_err());
    seed(&db, "control", "sage", 5, 1_000, 1_000);
    assert!(db
        .record_login(Socket::Control, "sage", i64::MAX as u64, Outcome::Failure)
        .is_err());
    assert_eq!(row(&db, "control", "sage"), Some((5, 1_000, 1_000)));
    assert_eq!(high(&db), 123);
}

#[test]
fn storage_errors_never_become_unlocked_answers() {
    let (_home, _secrets, db) = fixture();
    db.connection
        .lock()
        .unwrap()
        .execute_batch("DROP TABLE login_lockout;")
        .unwrap();
    assert!(db.login_lockout(Socket::Control, "unknown", 1_000).is_err());
    assert!(db
        .record_login(Socket::Control, "unknown", 1_000, Outcome::Success)
        .is_err());
    assert!(db.prune_login_lockouts(1_000).is_err());
    assert_eq!(high(&db), 123);
}

#[test]
fn statement_and_commit_faults_roll_back_counter_and_clock_through_reopen() {
    for action in ["insert", "update", "delete", "prune"] {
        for commit in [false, true] {
            let (home, secrets, db) = fixture();
            if action != "insert" {
                seed(&db, "control", "sage", 1, 1_000, 1_000);
            }
            let event = match action {
                "insert" => "INSERT",
                "update" => "UPDATE",
                _ => "DELETE",
            };
            let sql = if commit {
                format!("CREATE TABLE commit_probe (id TEXT REFERENCES accounts(account_id) DEFERRABLE INITIALLY DEFERRED);
                    CREATE TRIGGER fail_write AFTER {event} ON login_lockout BEGIN INSERT INTO commit_probe VALUES ('missing'); END;")
            } else {
                format!("CREATE TRIGGER fail_write BEFORE {event} ON login_lockout BEGIN SELECT RAISE(ABORT,'write failed'); END;")
            };
            db.connection.lock().unwrap().execute_batch(&sql).unwrap();
            let result = match action {
                "delete" => db
                    .record_login(Socket::Control, "sage", 100_000_000, Outcome::Success)
                    .map(|_| ()),
                "prune" => db.prune_login_lockouts(100_000_000).map(|_| ()),
                _ => db
                    .record_login(Socket::Control, "sage", 100_000_000, Outcome::Failure)
                    .map(|_| ()),
            };
            assert!(result.is_err(), "{action}, commit={commit}");
            assert_eq!(high(&db), 123);
            let expected = if action == "insert" {
                None
            } else {
                Some((1, 1_000, 1_000))
            };
            assert_eq!(row(&db, "control", "sage"), expected);
            drop(db);
            let db = HostDb::open(home.path(), &secrets).unwrap();
            assert_eq!(row(&db, "control", "sage"), expected);
            assert_eq!(high(&db), 123);
        }
    }
}

#[test]
fn audit_lines_contain_only_the_pinned_hash_never_the_login() {
    const LOGIN: &str = "sensitive\nlogin' OR 1=1";
    if std::env::var_os("SAGE_LOCKOUT_AUDIT_CHILD").is_some() {
        let (_home, _secrets, db) = fixture();
        seed(&db, "control", LOGIN, 64, 1_000, 1_000);
        assert!(db.login_lockout(Socket::Control, LOGIN, 2_000).is_err());
        assert!(db
            .record_login(Socket::Control, LOGIN, 2_000, Outcome::Failure)
            .is_err());
        assert_eq!(db.prune_login_lockouts(100_000_000).unwrap(), 0);
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "hostdb::tests::lockout::audit_lines_contain_only_the_pinned_hash_never_the_login",
            "--nocapture",
        ])
        .env("SAGE_LOCKOUT_AUDIT_CHILD", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!stderr.contains("sensitive") && !stderr.contains("login' OR 1=1"));
    let lines: Vec<_> = stderr
        .lines()
        .filter(|l| l.contains("invalid login lockout"))
        .collect();
    assert_eq!(lines.len(), 3);
    // Independently calculated SHA256(b"control\\0sensitive\\nlogin' OR 1=1")[:8].hex().
    for line in lines {
        assert_eq!(
            line,
            "org-office-host: invalid login lockout key 8fe917c340c5f9eb"
        );
    }
}

#[test]
fn sweep_retains_future_rows_and_invalid_socket_rows() {
    let (_home, _secrets, db) = fixture();
    seed(&db, "control", "future", 1, 100_000_001, 100_000_001);
    db.connection
        .lock()
        .unwrap()
        .execute_batch("PRAGMA ignore_check_constraints=ON;")
        .unwrap();
    seed(&db, "invalid", "bad", 1, 1_000, 1_000);
    assert_eq!(db.prune_login_lockouts(100_000_000).unwrap(), 0);
    assert_eq!(
        row(&db, "control", "future"),
        Some((1, 100_000_001, 100_000_001))
    );
    assert_eq!(row(&db, "invalid", "bad"), Some((1, 1_000, 1_000)));
}

#[test]
fn poisoned_mutex_refuses_all_lockout_operations() {
    let (_home, _secrets, db) = fixture();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = db.connection.lock().unwrap();
        panic!("poison fixture");
    }));
    assert_eq!(
        db.login_lockout(Socket::Control, "sage", 1_000),
        Err(Error::Unsafe)
    );
    assert_eq!(
        db.record_login(Socket::Control, "sage", 1_000, Outcome::Failure),
        Err(Error::Unsafe)
    );
    assert_eq!(db.prune_login_lockouts(1_000), Err(Error::Unsafe));
}

#[test]
fn clock_write_faults_roll_back_counter_operations_and_pruning() {
    for action in ["failure", "success", "reset", "prune"] {
        let (_home, _secrets, db) = fixture();
        seed(&db, "control", "sage", 1, 1_000, 1_000);
        db.connection.lock().unwrap().execute_batch("CREATE TRIGGER fail_clock BEFORE UPDATE OF value ON meta WHEN NEW.key='clock_high_ms' BEGIN SELECT RAISE(ABORT,'clock write failed'); END;").unwrap();
        let result = match action {
            "prune" => db.prune_login_lockouts(100_000_000).map(|_| ()),
            "success" => db
                .record_login(Socket::Control, "sage", 100_000_000, Outcome::Success)
                .map(|_| ()),
            "reset" => db
                .record_login(Socket::Control, "sage", 100_000_000, Outcome::Reset)
                .map(|_| ()),
            _ => db
                .record_login(Socket::Control, "sage", 100_000_000, Outcome::Failure)
                .map(|_| ()),
        };
        assert!(result.is_err(), "{action}");
        assert_eq!(row(&db, "control", "sage"), Some((1, 1_000, 1_000)));
        assert_eq!(high(&db), 123);
    }
}

#[test]
fn independent_connections_serialize_failure_increments() {
    let (home, secrets, db) = fixture();
    let other = HostDb::open(home.path(), &secrets).unwrap();
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            barrier.wait();
            for _ in 0..3 {
                db.record_login(Socket::Control, "sage", 1_000, Outcome::Failure)
                    .unwrap();
            }
        });
        barrier.wait();
        for _ in 0..3 {
            other
                .record_login(Socket::Control, "sage", 1_000, Outcome::Failure)
                .unwrap();
        }
    });
    assert_eq!(row(&db, "control", "sage"), Some((6, 2_000, 1_000)));
}

#[test]
fn prune_deletes_only_the_selected_socket_key_pair() {
    let (_home, _secrets, db) = fixture();
    seed(&db, "control", "old", 1, 1_000, 1_000);
    seed(&db, "control", "new", 1, 1_001, 1_001);
    seed(&db, "local", "old", 1, 1_001, 1_001);
    assert_eq!(db.prune_login_lockouts(1_801_000).unwrap(), 1);
    assert_eq!(row(&db, "control", "old"), None);
    assert_eq!(row(&db, "control", "new"), Some((1, 1_001, 1_001)));
    assert_eq!(row(&db, "local", "old"), Some((1, 1_001, 1_001)));
}
