use std::{fs, path::PathBuf};

use org_store::{
    testkit::engine_suite::cursor, CorePort, CoreRead, DurablePath, Engine, Sha256, StoreError,
};

use super::{beside, schema::Schema, SqliteEngine};

/// A fresh projection in a temporary directory, which the guard keeps.
fn open() -> (SqliteEngine, tempfile::TempDir) {
    let (path, dir) = place();
    (SqliteEngine::open(&path).expect("a new projection"), dir)
}

fn place() -> (PathBuf, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let root = dir.path().canonicalize().expect("a real path");
    (root.join("office.db"), dir)
}

fn path(text: &str) -> DurablePath {
    DurablePath::parse(text).expect("a valid test path")
}

fn source(n: u8) -> (DurablePath, Sha256) {
    (path(&format!("state/file-{n}")), Sha256::of(&[n]))
}

/// Commits one source at `ordinal`.
fn commit(engine: &SqliteEngine, ordinal: u64) {
    let mut tx = engine.begin().unwrap();
    let (path, hash) = source(ordinal as u8);
    tx.set_source(&path, Some(hash)).unwrap();
    engine.commit(tx, &cursor(ordinal)).unwrap();
}

fn sources(engine: &SqliteEngine) -> usize {
    let snapshot = engine.snapshot().unwrap();
    snapshot.sources_under(&path("state")).unwrap().len()
}

const ORG: &str = "01900000-0000-7000-8000-0000000000a2";

/// An office row, whose organization is an immediate reference in the contract schema.
fn office(tx: &super::SqliteTx, id: &str) {
    let sql = "INSERT INTO offices (organization_id, id, slug, schema_version, revision, \
               name, role) VALUES (?1, ?2, ?2, 1, 1, 'Office', 'hq')";
    tx.db().execute(sql, [ORG, id]).expect("an office row");
}

fn organization(tx: &super::SqliteTx) {
    let sql = "INSERT INTO organizations (id, slug, schema_version, revision, name, \
               organization_repository_id, state_branch) \
               VALUES (?1, 'example', 1, 1, 'Example', 'repo', 'org/state'); \
               INSERT INTO repositories (organization_id, id, schema_version, revision, kind, \
               remote_url) VALUES (?1, 'repo', 1, 1, 'remote', 'https://example.test/o.git');"
        .replace("?1", &format!("'{ORG}'"));
    tx.db().execute_batch(&sql).expect("an organization");
}

org_store::engine_conformance_tests!(open);

#[test]
fn a_relative_path_is_refused() {
    let refused = SqliteEngine::open("office.db".as_ref())
        .err()
        .expect("refused");
    assert!(refused.message.contains("absolute"), "{refused}");
    assert!(!PathBuf::from("office.db").exists());
}

#[test]
fn the_file_is_plain_sqlite_that_any_connection_reads_without_a_key() {
    let (engine, dir) = open();
    commit(&engine, 1);
    let file = dir.path().join("office.db");
    let small = fs::metadata(&file).unwrap().len();
    // Nothing is folded into the file until the engine is told to.
    engine.checkpoint().unwrap();
    assert!(fs::metadata(&file).unwrap().len() > small);
    let bytes = fs::read(&file).unwrap();
    assert_eq!(&bytes[..16], b"SQLite format 3\0");
    // Write-ahead log, as the header records it.
    assert_eq!(&bytes[18..20], [2, 2]);
    let plain = rusqlite::Connection::open(&file).unwrap();
    let tables: i64 = plain
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type = 'table'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(tables >= 80, "{tables}");
    // TASK-10 (H-05): sensitive messaging and budget tables are isolated out of shared office.db.
    for isolated in [
        "conversations",
        "messages",
        "message_recipient_states",
        "budgets",
        "budget_policy_versions",
        "budget_holds",
    ] {
        let count: i64 = plain
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name = ?1",
                [isolated],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 0,
            "isolated table {isolated} must not be in office.db"
        );
    }
    let stored: String = plain
        .query_row("SELECT digest FROM store_schema", [], |row| row.get(0))
        .unwrap();
    assert_eq!(stored, Schema::current().digest());
    assert_eq!(stored.len(), 64);
}

#[test]
fn reopening_keeps_the_projection_and_another_schema_digest_replaces_it() {
    let (path, _dir) = place();
    let engine = SqliteEngine::open(&path).unwrap();
    commit(&engine, 1);
    drop(engine);

    let engine = SqliteEngine::open(&path).unwrap();
    assert_eq!(
        engine.snapshot().unwrap().cursor().unwrap(),
        Some(cursor(1))
    );
    assert_eq!(sources(&engine), 1);
    drop(engine);

    // Another build's DDL: the projection is deleted and made anew, empty, for a rebuild.
    let other = || Schema::current().with("CREATE TABLE store_extra (n INTEGER) STRICT;");
    assert_ne!(other().digest(), Schema::current().digest());
    let engine = SqliteEngine::open_as(&path, other()).unwrap();
    assert_eq!(engine.snapshot().unwrap().cursor().unwrap(), None);
    assert_eq!(sources(&engine), 0);
    let tx = engine.begin().unwrap();
    tx.db()
        .execute("INSERT INTO store_extra (n) VALUES (1)", [])
        .unwrap();
    engine.commit(tx, &cursor(5)).unwrap();
    drop(engine);
    let engine = SqliteEngine::open_as(&path, other()).unwrap();
    assert_eq!(
        engine.snapshot().unwrap().cursor().unwrap(),
        Some(cursor(5))
    );
    drop(engine);

    // And back: this build does not read the other's either.
    let engine = SqliteEngine::open(&path).unwrap();
    assert_eq!(engine.snapshot().unwrap().cursor().unwrap(), None);
    let tx = engine.begin().unwrap();
    assert!(tx
        .db()
        .execute("INSERT INTO store_extra (n) VALUES (1)", [])
        .is_err());
}

#[test]
fn a_file_that_is_no_finished_projection_is_made_anew() {
    for unfinished in [&b"not a database at all, just some bytes"[..], b""] {
        let (path, _dir) = place();
        fs::write(&path, unfinished).unwrap();
        fs::write(beside(&path, ".new"), b"left by an interrupted replace").unwrap();
        let engine = SqliteEngine::open(&path).unwrap();
        assert_eq!(engine.snapshot().unwrap().cursor().unwrap(), None);
        commit(&engine, 1);
        assert_eq!(sources(&engine), 1);
        assert!(!beside(&path, ".new").exists());
    }
}

#[test]
fn dropping_the_engine_does_no_durable_work() {
    let (path, _dir) = place();
    let engine = SqliteEngine::open(&path).unwrap();
    commit(&engine, 1);
    let _ = engine.snapshot().unwrap();
    let log = beside(&path, "-wal");
    let before = (
        fs::metadata(&path).unwrap().len(),
        fs::metadata(&log).unwrap().len(),
    );
    assert!(before.1 > 0);
    drop(engine);
    // No checkpoint on close: the log is where it was, and so is the file.
    let after = (
        fs::metadata(&path).unwrap().len(),
        fs::metadata(&log).unwrap().len(),
    );
    assert_eq!(after, before);
    let engine = SqliteEngine::open(&path).unwrap();
    assert_eq!(
        engine.snapshot().unwrap().cursor().unwrap(),
        Some(cursor(1))
    );
}

#[test]
fn a_read_on_another_thread_runs_while_the_writer_holds_its_transaction() {
    let (engine, _dir) = open();
    commit(&engine, 1);
    let mut tx = engine.begin().unwrap();
    let (other, hash) = source(2);
    tx.set_source(&other, Some(hash)).unwrap();
    std::thread::scope(|scope| {
        let read = scope.spawn(|| {
            let snapshot = engine.snapshot().unwrap();
            let seen = snapshot.source_hash(&other).unwrap();
            (snapshot.cursor().unwrap(), seen)
        });
        assert_eq!(read.join().unwrap(), (Some(cursor(1)), None));
    });
    // The checkpoint has its own connection, and neither waits for the writer nor stops it.
    engine.checkpoint().unwrap();
    assert!(
        engine.begin().is_err(),
        "one writer's transaction at a time"
    );
    engine.commit(tx, &cursor(2)).unwrap();
    assert_eq!(sources(&engine), 2);
    engine.checkpoint().unwrap();
}

#[test]
fn more_snapshots_than_the_pool_keeps_open_and_close_without_harm() {
    let (engine, _dir) = open();
    commit(&engine, 1);
    let many: Vec<_> = (0..super::MAX_IDLE_READERS + 3)
        .map(|_| engine.snapshot().unwrap())
        .collect();
    for snapshot in &many {
        assert_eq!(snapshot.cursor().unwrap(), Some(cursor(1)));
    }
    drop(many);
    let pool = super::lock(&engine.readers.pool);
    assert_eq!((pool.open, pool.idle.len()), (0, super::MAX_IDLE_READERS));
}

#[test]
fn a_read_connection_cannot_write() {
    let (engine, _dir) = open();
    let snapshot = engine.snapshot().unwrap();
    let written = snapshot.db().execute("DELETE FROM store_schema", []);
    assert!(written.is_err());
}

#[test]
fn rows_may_arrive_before_what_they_refer_to_and_check_tells_when_a_commit_would_fail() {
    let (engine, _dir) = open();
    let mut tx = engine.begin().unwrap();
    tx.check().unwrap();
    // The office before its organization: an immediate reference in the contract schema.
    let mark = tx.mark().unwrap();
    office(&tx, "office-a");
    let pending = tx.check().expect_err("the organization is missing");
    assert!(!pending.transaction_ended);
    assert!(pending.message.contains("offices row"), "{pending}");
    assert!(pending.message.contains("organizations"), "{pending}");
    // The operation is undone alone, and the count goes back with it.
    tx.undo_to(mark).unwrap();
    tx.check().unwrap();

    office(&tx, "office-a");
    organization(&tx);
    tx.check().unwrap();
    engine.commit(tx, &cursor(1)).unwrap();

    // Deferring ended with the transaction and is issued again by the next `begin`.
    let mut tx = engine.begin().unwrap();
    tx.db()
        .execute("DELETE FROM repositories WHERE id = 'repo'", [])
        .unwrap();
    assert!(tx.check().is_err());
    // A commit that would fail is refused whole, and the writer goes on.
    let refused = engine
        .commit(tx, &cursor(2))
        .expect_err("a dangling reference");
    assert!(refused.message.contains("organizations row"), "{refused}");
    assert_eq!(
        engine.snapshot().unwrap().cursor().unwrap(),
        Some(cursor(1))
    );
    let tx = engine.begin().unwrap();
    let repositories: i64 = tx
        .db()
        .query_row("SELECT count(*) FROM repositories", [], |row| row.get(0))
        .unwrap();
    assert_eq!(repositories, 1);
}

#[test]
fn a_replacement_with_a_dangling_reference_is_refused_by_row_and_the_projection_stays() {
    let (engine, dir) = open();
    commit(&engine, 1);
    let refused = engine.replace(&mut |tx| {
        office(tx, "office-a");
        Ok(cursor(9))
    });
    let Err(StoreError::Engine(refused)) = refused else {
        panic!("{refused:?}");
    };
    assert!(refused.message.contains("offices row"), "{refused}");
    assert_eq!(
        engine.snapshot().unwrap().cursor().unwrap(),
        Some(cursor(1))
    );
    assert_eq!(sources(&engine), 1);
    commit(&engine, 2);

    // In any order, as a rebuild applies files, a whole set installs.
    engine
        .replace(&mut |tx| {
            office(tx, "office-a");
            organization(tx);
            tx.check()?;
            Ok(cursor(9))
        })
        .unwrap();
    assert_eq!(
        engine.snapshot().unwrap().cursor().unwrap(),
        Some(cursor(9))
    );
    assert_eq!(sources(&engine), 0);
    // One database file, its WAL pair, and the isolated `protected/` folder for `messaging.db`:
    // nothing of the build staging or old log is left.
    let mut names: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(
        names,
        ["office.db", "office.db-shm", "office.db-wal", "protected"]
    );
    commit(&engine, 10);
    drop(engine);
    let engine = SqliteEngine::open(&dir.path().canonicalize().unwrap().join("office.db"));
    assert_eq!(
        engine.unwrap().snapshot().unwrap().cursor().unwrap(),
        Some(cursor(10))
    );

    // TASK-166: `open()` cleans interrupted `protected/office.db.new`, and `replace()` recreates
    // `protected/` with `0700` if it was deleted while the engine was open.
    let protected = dir.path().join("protected");
    fs::write(protected.join("office.db.new"), b"stale").unwrap();
    let engine = SqliteEngine::open(&dir.path().canonicalize().unwrap().join("office.db")).unwrap();
    assert!(!protected.join("office.db.new").exists());
    fs::remove_dir_all(&protected).unwrap();
    engine.replace(&mut |_| Ok(cursor(11))).unwrap();
    assert!(protected.is_dir());
    assert!(!protected.join("office.db.new").exists());
    assert_eq!(
        engine.snapshot().unwrap().cursor().unwrap(),
        Some(cursor(11))
    );
}

#[test]
fn a_replacement_is_refused_while_the_writer_holds_a_transaction() {
    let (engine, _dir) = open();
    commit(&engine, 1);
    let tx = engine.begin().unwrap();
    let refused = engine.replace(&mut |_| Ok(cursor(9)));
    assert!(matches!(refused, Err(StoreError::Engine(_))), "{refused:?}");
    engine.commit(tx, &cursor(2)).unwrap();
    assert_eq!(
        engine.snapshot().unwrap().cursor().unwrap(),
        Some(cursor(2))
    );
    engine.replace(&mut |_| Ok(cursor(9))).unwrap();
    assert_eq!(
        engine.snapshot().unwrap().cursor().unwrap(),
        Some(cursor(9))
    );
}

/// `count` commits from `from`, each of forty sources: enough to add pages to the file.
fn commit_many(engine: &SqliteEngine, from: u64, count: u64) {
    for ordinal in from..from + count {
        let mut tx = engine.begin().unwrap();
        for n in 0..40u64 {
            let name = format!("state/many/{ordinal}-{n}");
            tx.set_source(&path(&name), Some(Sha256::of(name.as_bytes())))
                .unwrap();
        }
        engine.commit(tx, &cursor(ordinal)).unwrap();
    }
}

#[test]
fn a_checkpoint_during_a_replacement_never_keeps_a_connection_to_the_old_file() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let (engine, dir) = open();
    let file = dir.path().join("office.db");
    commit(&engine, 1);
    let replacing = AtomicBool::new(true);
    std::thread::scope(|scope| {
        // Checkpoints as fast as they go, through every moment of every replacement.
        let checkpoints = scope.spawn(|| {
            while replacing.load(Ordering::Acquire) {
                engine.checkpoint().expect("a checkpoint beside a replace");
            }
        });
        for round in 0..12 {
            engine.replace(&mut |_| Ok(cursor(10 + round))).unwrap();
        }
        replacing.store(false, Ordering::Release);
        checkpoints.join().unwrap();
    });

    // The next checkpoint folds the NEW file's log: the file under the name grows, and
    // the log is then written from its start again instead of growing.
    let before = fs::metadata(&file).unwrap().len();
    commit_many(&engine, 100, 8);
    assert_eq!(fs::metadata(&file).unwrap().len(), before);
    let log = fs::metadata(beside(&file, "-wal")).unwrap().len();
    engine.checkpoint().unwrap();
    assert!(fs::metadata(&file).unwrap().len() > before);
    // Fewer writes than the log already has room for: it is written over, not added to.
    commit_many(&engine, 200, 3);
    let after = fs::metadata(beside(&file, "-wal")).unwrap().len();
    assert_eq!(after, log);
}

#[test]
fn another_adapter_format_is_another_schema_and_the_projection_is_made_anew() {
    let other = || Schema::current().encoded_as(u32::MAX);
    assert_ne!(other().digest(), Schema::current().digest());
    let (path, _dir) = place();
    let engine = SqliteEngine::open(&path).unwrap();
    commit(&engine, 1);
    drop(engine);
    // The same DDL, written another way: not read, made anew for a rebuild.
    let engine = SqliteEngine::open_as(&path, other()).unwrap();
    assert_eq!(engine.snapshot().unwrap().cursor().unwrap(), None);
    assert_eq!(sources(&engine), 0);
}

#[test]
fn a_projection_that_cannot_be_opened_is_an_error_and_is_not_deleted() {
    use std::os::unix::fs::PermissionsExt;

    let (path, _dir) = place();
    let engine = SqliteEngine::open(&path).unwrap();
    commit(&engine, 1);
    engine.checkpoint().unwrap();
    drop(engine);
    let whole = fs::read(&path).unwrap();
    let allow = |mode| fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();

    // No permission to open it: not "no database", so nothing is deleted.
    allow(0o000);
    if fs::read(&path).is_ok() {
        // This user reads any file (root): the refusal cannot be staged here.
        allow(0o600);
        return;
    }
    let refused = SqliteEngine::open(&path).err().expect("refused");
    assert!(!refused.message.is_empty());
    allow(0o600);
    assert_eq!(fs::read(&path).unwrap(), whole);
    let engine = SqliteEngine::open(&path).unwrap();
    assert_eq!(
        engine.snapshot().unwrap().cursor().unwrap(),
        Some(cursor(1))
    );
    assert_eq!(sources(&engine), 1);
}

#[test]
fn persistent_checkpoint_failures_surface_upkeep_error_and_clear_on_recovery() {
    let (path, _dir) = place();
    let engine = SqliteEngine::open(&path).unwrap();
    commit(&engine, 1);
    engine.after_round();
    assert_eq!(engine.checkpoint_failures(), 0);
    assert!(engine.upkeep_error().is_none());

    // Hold a real SQLite read snapshot while committing new WAL frames across 3 flusher rounds:
    // `PRAGMA wal_checkpoint(PASSIVE)` genuinely reports `busy=1, checkpointed < log`.
    let held_reader = engine.snapshot().unwrap();
    commit(&engine, 2);
    engine.after_round();
    assert_eq!(engine.checkpoint_failures(), 1);
    assert!(engine.upkeep_error().is_none());

    commit(&engine, 3);
    engine.after_round();
    assert_eq!(engine.checkpoint_failures(), 2);
    assert!(engine.upkeep_error().is_none());

    commit(&engine, 4);
    engine.after_round();
    assert_eq!(engine.checkpoint_failures(), 3);
    let warning = engine
        .upkeep_error()
        .expect("3 consecutive blocked WAL checkpoints must surface upkeep_error");
    assert!(warning.message.contains("3 consecutive"));
    assert!(warning.message.contains("WAL checkpoint incomplete"));

    // Dropping the long reader lets the next real checkpoint fold the WAL and clear upkeep_error.
    drop(held_reader);
    engine.after_round();
    assert_eq!(engine.checkpoint_failures(), 0);
    assert!(engine.upkeep_error().is_none());
}

#[test]
fn concurrent_snapshots_never_see_diverged_office_and_messaging_cursors() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    let (path, _dir) = place();
    let engine = Arc::new(SqliteEngine::open(&path).unwrap());
    commit(&engine, 1);

    let done = Arc::new(AtomicBool::new(false));
    let reader_engine = Arc::clone(&engine);
    let reader_done = Arc::clone(&done);
    let reader = std::thread::spawn(move || {
        let mut checks = 0usize;
        while !reader_done.load(Ordering::Relaxed) || checks < 20 {
            let snap = reader_engine.snapshot().unwrap();
            let office_cur = snap.cursor().unwrap();
            let msg_cur = super::messaging::read_messaging_cursor(snap.msg_db()).unwrap();
            assert_eq!(
                office_cur, msg_cur,
                "snapshot observed diverged cursors between office.db and protected/messaging.db"
            );
            checks += 1;
        }
    });

    for ordinal in 2..=40 {
        commit(&engine, ordinal);
    }
    done.store(true, Ordering::Relaxed);
    reader.join().unwrap();
}

#[test]
fn partial_commit_failure_poisons_engine_and_reopen_resets_diverged_cursors() {
    let (path, _dir) = place();
    let engine = SqliteEngine::open(&path).unwrap();
    commit(&engine, 1);

    // Inject an office.db COMMIT failure AFTER protected/messaging.db commits:
    // commit() MUST return EngineError::ended and MUST NOT return the diverged pair to `writer`.
    let mut tx = engine.begin().unwrap();
    tx.fail_office_commit = true;
    let err = engine.commit(tx, &cursor(2)).unwrap_err();
    assert!(
        err.transaction_ended,
        "partial commit failure must mark transaction ended: {err}"
    );
    assert!(
        err.message.contains("diverged"),
        "error should explain divergence: {err}"
    );
    assert!(
        engine.begin().is_err(),
        "engine must refuse further transactions after partial dual-DB commit divergence"
    );
    assert!(
        engine.snapshot().is_err(),
        "engine must refuse new read snapshots after partial dual-DB commit divergence"
    );
    drop(engine);

    // Reopening detects the diverged cursors and clears both so `org-store` rebuilds atomically.
    let reopened = SqliteEngine::open(&path).unwrap();
    let snap = reopened.snapshot().unwrap();
    assert_eq!(snap.cursor().unwrap(), None);
    assert_eq!(
        super::messaging::read_messaging_cursor(snap.msg_db()).unwrap(),
        None
    );
}

#[test]
fn messaging_db_and_sidecars_are_owner_only_0600_inside_0700_protected_dir() {
    use std::os::unix::fs::PermissionsExt as _;

    let (path, _dir) = place();
    let engine = SqliteEngine::open(&path).unwrap();
    commit(&engine, 1);

    let msg_path = super::messaging::messaging_db_path(&path);
    let protected_dir = msg_path.parent().unwrap();
    assert_eq!(
        fs::symlink_metadata(protected_dir)
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o700
    );
    assert_eq!(
        fs::symlink_metadata(&msg_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o600
    );

    // Loosen sidecars and verify reopening tightens messaging.db + -wal/-shm back to 0600.
    for suffix in ["-wal", "-shm"] {
        let sidecar = beside(&msg_path, suffix);
        if sidecar.exists() {
            fs::set_permissions(&sidecar, fs::Permissions::from_mode(0o644)).unwrap();
        }
    }
    drop(engine);

    let _reopened = SqliteEngine::open(&path).unwrap();
    for suffix in ["", "-wal", "-shm"] {
        let target = beside(&msg_path, suffix);
        if target.exists() {
            assert_eq!(
                fs::symlink_metadata(&target).unwrap().permissions().mode() & 0o7777,
                0o600,
                "expected 0600 on {}",
                target.display()
            );
        }
    }
}

#[test]
fn corrupt_companion_databases_rebuild_cleanly_and_torn_jsonl_tail_is_tolerated() {
    use org_office::accounting::AccountingStore;
    use org_office::budgets::{BudgetAggregate, BudgetPolicyVersion, BudgetRecord, BudgetStore};
    use org_office::providers::{OfficeConnectionBinding, ProviderPoolStore};
    use org_proto::budgets::{
        BudgetPeriodKind, BudgetPolicyState, BudgetScopeKind, BudgetTimeZoneSource, BudgetUnit,
    };
    use org_proto::costs::{
        AttributionBasis, CompletenessStatus, ModelPriceView, UsageActorKind, UsageCostBasis,
        UsageRecordView, UsageScopeKind,
    };

    let tmp = tempfile::tempdir().unwrap();
    let durable = tmp.path().join("durable");
    let runtime = tmp.path().join("runtime");
    fs::create_dir_all(&durable).unwrap();
    fs::create_dir_all(&runtime).unwrap();
    let org_id = "01900000-0000-7000-8000-0000000000a2";
    let office_id = "01900000-0000-7000-8000-0000000000b2";

    // 1. Seed accounting (price + usage), budget, and provider binding.
    let acc = super::accounting::SqliteAccountingStore::open(&durable, &runtime, office_id, org_id)
        .unwrap();
    acc.set_price(
        org_id,
        ModelPriceView {
            id: "01900000-0000-7000-8000-000000000101".into(),
            provider: "openai".into(),
            model: "gpt-5".into(),
            effective_from: "2026-01-01T00:00:00Z".into(),
            currency: "USD".into(),
            input_micro_usd_per_million: Some(1_000_000),
            output_micro_usd_per_million: Some(2_000_000),
            cache_read_micro_usd_per_million: Some(500_000),
            cache_write_micro_usd_per_million: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            created_by_kind: "employee".into(),
            created_by_id: Some("01900000-0000-7000-8000-000000000001".into()),
        },
    )
    .unwrap()
    .unwrap();
    let mut rec = UsageRecordView {
        id: "01900000-0000-7000-8000-000000000201".into(),
        origin_office_id: office_id.into(),
        producer_id: "prod-1".into(),
        source_event_id: "evt-1".into(),
        source_sequence: 1,
        execution_office_id: Some(office_id.into()),
        run_id: None,
        actor_kind: UsageActorKind::Agent,
        employee_id: Some("01900000-0000-7000-8000-0000000000aa".into()),
        runtime: "codex".into(),
        provider: Some("openai".into()),
        model: Some("gpt-5".into()),
        occurred_at: "2026-01-02T00:00:00Z".into(),
        recorded_at: "2026-01-02T00:00:00Z".into(),
        recorded_by_kind: "system".into(),
        recorded_by_id: None,
        normalization_version: 1,
        source_metadata_json: "{}".into(),
        payload_sha256: String::new(),
        input_tokens: Some(100),
        output_tokens: Some(50),
        cache_read_tokens: Some(0),
        cache_write_tokens: Some(0),
        reasoning_tokens: Some(0),
        token_status: CompletenessStatus::Complete,
        token_issue: None,
        price_id: Some("01900000-0000-7000-8000-000000000101".into()),
        cost_micro_usd: Some(200),
        cost_basis: UsageCostBasis::TokenEstimate,
        cost_status: CompletenessStatus::Complete,
        cost_issue: None,
        scope_kind: UsageScopeKind::Organization,
        task_id: None,
        project_id: None,
        team_id: None,
        feature_id: None,
        milestone_id: None,
        release_id: None,
        task_type_snapshot: None,
        work_type: None,
        attribution_basis: AttributionBasis::Unattributed,
        supersedes_id: None,
        correction_kind: None,
        correction_reason: None,
    };
    rec.payload_sha256 = org_office::accounting::compute_usage_payload_sha256(org_id, &rec);
    acc.append_usage(org_id, rec.clone()).unwrap().unwrap();
    drop(acc);

    let bud =
        super::budgets::SqliteBudgetStore::open(&durable, &runtime, office_id, org_id).unwrap();
    let agg = BudgetAggregate {
        budget: BudgetRecord {
            organization_id: org_id.into(),
            id: "01900000-0000-7000-8000-000000000301".into(),
            current_revision: 1,
            scope_kind: BudgetScopeKind::Agent,
            task_id: None,
            agent_id: Some("01900000-0000-7000-8000-0000000000aa".into()),
            release_id: None,
            period_kind: BudgetPeriodKind::Day,
            time_zone: "UTC".into(),
            time_zone_source: BudgetTimeZoneSource::Hq,
            time_zone_subject_ref: None,
            created_at: "2026-01-02T00:00:00Z".into(),
        },
        versions: vec![BudgetPolicyVersion {
            organization_id: org_id.into(),
            budget_id: "01900000-0000-7000-8000-000000000301".into(),
            revision: 1,
            state: BudgetPolicyState::Active,
            unit: BudgetUnit::MicroUsd,
            limit_amount: Some(1_000_000),
            time_zone: "UTC".into(),
            time_zone_source: BudgetTimeZoneSource::Hq,
            time_zone_subject_ref: None,
            changed_by_id: "01900000-0000-7000-8000-000000000001".into(),
            changed_at: "2026-01-02T00:00:00Z".into(),
            reason: None,
        }],
        periods: vec![],
        events: vec![],
        notices: vec![],
        holds: vec![],
        pending_stop: None,
    };
    bud.save_aggregate(
        &agg,
        &[
            ("01900000-0000-7000-8000-000000000001".into(), "person"),
            ("01900000-0000-7000-8000-0000000000aa".into(), "agent"),
        ],
    )
    .unwrap()
    .unwrap();
    drop(bud);

    let prov = super::providers::SqliteProviderPoolStore::open(
        &durable, &runtime, office_id, org_id, true,
    )
    .unwrap();
    prov.save_binding_and_invalidate(&OfficeConnectionBinding {
        organization_id: org_id.into(),
        office_id: office_id.into(),
        connection_id: "01900000-0000-7000-8000-000000000401".into(),
        revision: 1,
        auth_context_ref: "local:test".into(),
        runtime_binary_path: None,
        account_identity_ref: None,
        updated_at: "2026-01-02T00:00:00Z".into(),
        last_operation_id: None,
        last_operation_hash: None,
    })
    .unwrap();
    drop(prov);

    // 2. Corrupt ALL three protected companion SQLite DBs with garbage bytes AND append a
    //    crash-torn trailing fragment (no trailing newline) to the usage JSONL file!
    let protected = runtime.join(office_id).join("protected");
    for name in ["accounting.db", "budgets.db", "providers.db"] {
        fs::write(
            protected.join(name),
            b"NOT A SQLITE DATABASE CORRUPT GARBAGE",
        )
        .unwrap();
    }
    let jsonl_path = durable
        .join("accounting/usage")
        .join(office_id)
        .join("prod-1/2026-01-02.jsonl");
    {
        use std::io::Write;
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(&jsonl_path)
            .unwrap();
        f.write_all(b"{\"id\":\"truncated-crash-tail").unwrap();
    }

    // 3. Reopen all three: each deletes & rebuilds cleanly from durable files, AND repairs
    //    the torn JSONL tail on disk so a subsequent same-day append + reopen succeeds (`Finding #4`)!
    let acc2 =
        super::accounting::SqliteAccountingStore::open(&durable, &runtime, office_id, org_id)
            .unwrap();
    assert_eq!(acc2.effective_records(org_id).unwrap().len(), 1);
    assert_eq!(
        acc2.list_prices(org_id, &Default::default()).unwrap().len(),
        1
    );
    let mut rec2 = rec.clone();
    rec2.id = "01900000-0000-7000-8000-000000000202".into();
    rec2.source_event_id = "evt-2".into();
    rec2.payload_sha256 = org_office::accounting::compute_usage_payload_sha256(org_id, &rec2);
    acc2.append_usage(org_id, rec2).unwrap().unwrap();
    drop(acc2);
    let acc3 =
        super::accounting::SqliteAccountingStore::open(&durable, &runtime, office_id, org_id)
            .unwrap();
    assert_eq!(
        acc3.effective_records(org_id).unwrap().len(),
        2,
        "repaired torn tail must allow subsequent same-day append and reopen cleanly"
    );
    drop(acc3);

    // 3b. Verify atomic budget aggregate updates + immutable historical version conflict refusal
    //     + full rebuild after corrupting `protected/budgets.db` (`Pass 2 Finding #2`).
    let bud2 =
        super::budgets::SqliteBudgetStore::open(&durable, &runtime, office_id, org_id).unwrap();
    let loaded_bud = bud2.list_all(org_id).unwrap();
    assert_eq!(loaded_bud.len(), 1);
    assert_eq!(loaded_bud[0].versions.len(), 1);
    // Attempting to mutate historical committed rev 1 MUST fail closed with Conflict!
    let mut tampered = agg.clone();
    tampered.versions[0].limit_amount = Some(5);
    assert!(
        matches!(
            bud2.save_aggregate(
                &tampered,
                &[
                    ("01900000-0000-7000-8000-000000000001".into(), "person"),
                    ("01900000-0000-7000-8000-0000000000aa".into(), "agent"),
                ],
            ),
            Ok(Err(ref e)) if e.code == org_proto::ErrorCode::Conflict
        ),
        "mutating an already-committed budget policy revision must fail closed with Conflict"
    );
    let mut real_v2 = agg.versions[0].clone();
    real_v2.revision = 2;
    real_v2.limit_amount = Some(9_999_999);
    let mut agg_v2 = agg.clone();
    agg_v2.budget.current_revision = 2;
    agg_v2.versions.push(real_v2);
    bud2.save_aggregate(
        &agg_v2,
        &[
            ("01900000-0000-7000-8000-000000000001".into(), "person"),
            ("01900000-0000-7000-8000-0000000000aa".into(), "agent"),
        ],
    )
    .unwrap()
    .unwrap();
    drop(bud2);
    fs::write(
        protected.join("budgets.db"),
        b"CORRUPT AGAIN TO VERIFY REBUILD",
    )
    .unwrap();
    let bud3 =
        super::budgets::SqliteBudgetStore::open(&durable, &runtime, office_id, org_id).unwrap();
    let rebuilt_bud = bud3.list_all(org_id).unwrap();
    assert_eq!(rebuilt_bud[0].versions.len(), 2);
    assert_eq!(rebuilt_bud[0].versions[1].limit_amount, Some(9_999_999));
    drop(bud3);

    let prov2 = super::providers::SqliteProviderPoolStore::open(
        &durable, &runtime, office_id, org_id, true,
    )
    .unwrap();
    assert!(prov2
        .get_binding(org_id, office_id, "01900000-0000-7000-8000-000000000401")
        .unwrap()
        .is_some());
    drop(prov2);

    // 4. Conflicting duplicate usage record ID in JSONL MUST fail closed (`Pass 2 Finding #8`).
    let mut conflicting = rec.clone();
    conflicting.payload_sha256 = "different-sha".into();
    conflicting.source_sequence = 99;
    let saved_jsonl = fs::read(&jsonl_path).unwrap();
    {
        use std::io::Write;
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(&jsonl_path)
            .unwrap();
        let mut line = serde_json::to_vec(&conflicting).unwrap();
        line.push(b'\n');
        f.write_all(&line).unwrap();
    }
    assert!(
        super::accounting::SqliteAccountingStore::open(&durable, &runtime, office_id, org_id)
            .is_err(),
        "conflicting duplicate usage record id in durable JSONL must fail closed"
    );
    fs::write(&jsonl_path, saved_jsonl).unwrap();

    // 5. Mid-file corruption (newline-terminated corrupt line) MUST still fail closed!
    {
        use std::io::Write;
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(&jsonl_path)
            .unwrap();
        f.write_all(b"{\"id\":\"corrupt-midfile\n").unwrap();
    }
    assert!(
        super::accounting::SqliteAccountingStore::open(&durable, &runtime, office_id, org_id)
            .is_err(),
        "newline-terminated corrupt JSONL line must fail closed"
    );
}

#[test]
fn staged_orphan_corrections_and_chain_tamper_prevent_collision_replay_and_overcount() {
    use org_office::accounting::{
        compute_usage_payload_sha256, AccountingStore, UsageStreamCheckpoint,
    };
    use org_proto::costs::{
        AttributionBasis, CompletenessStatus, UsageActorKind, UsageCostBasis, UsageRecordView,
        UsageScopeKind,
    };

    let tmp = tempfile::tempdir().unwrap();
    let durable = tmp.path().join("durable");
    let runtime = tmp.path().join("runtime");
    fs::create_dir_all(&durable).unwrap();
    fs::create_dir_all(&runtime).unwrap();
    let office_id = "01900000-0000-7000-8000-000000000010";
    let org_id = "01900000-0000-7000-8000-000000000001";

    let acc = super::accounting::SqliteAccountingStore::open(&durable, &runtime, office_id, org_id)
        .unwrap();
    let make_rec = |id: &str, evt: &str, supersedes: Option<&str>, runtime_name: &str| {
        let mut r = UsageRecordView {
            id: id.into(),
            origin_office_id: office_id.into(),
            producer_id: "prod-1".into(),
            source_event_id: evt.into(),
            source_sequence: 0,
            execution_office_id: Some(office_id.into()),
            run_id: None,
            actor_kind: UsageActorKind::Agent,
            employee_id: Some("01900000-0000-7000-8000-0000000000aa".into()),
            runtime: runtime_name.into(),
            provider: Some("anthropic".into()),
            model: Some("claude".into()),
            occurred_at: "2026-01-02T00:00:00Z".into(),
            recorded_at: "2026-01-02T00:00:01Z".into(),
            recorded_by_kind: "system".into(),
            recorded_by_id: None,
            normalization_version: 1,
            source_metadata_json: "{}".into(),
            payload_sha256: String::new(),
            input_tokens: Some(10),
            output_tokens: Some(5),
            cache_read_tokens: Some(0),
            cache_write_tokens: Some(0),
            reasoning_tokens: None,
            token_status: CompletenessStatus::Complete,
            token_issue: None,
            price_id: None,
            cost_micro_usd: Some(100),
            cost_basis: UsageCostBasis::RuntimeEstimate,
            cost_status: CompletenessStatus::Complete,
            cost_issue: None,
            scope_kind: UsageScopeKind::Organization,
            task_id: None,
            project_id: None,
            team_id: None,
            feature_id: None,
            milestone_id: None,
            release_id: None,
            task_type_snapshot: None,
            work_type: None,
            attribution_basis: AttributionBasis::Unattributed,
            supersedes_id: supersedes.map(str::to_owned),
            correction_kind: None,
            correction_reason: supersedes.map(|_| "adjust".into()),
        };
        r.payload_sha256 = compute_usage_payload_sha256(org_id, &r);
        r
    };

    // 1. Append rec1 and verify `runtime` affects `payload_sha256`, `head_sha256 == rec1.payload_sha256`,
    //    and `chain_sha256` is populated in the durable checkpoint file.
    let rec1 = make_rec(
        "01900000-0000-7000-8000-000000000301",
        "evt-1",
        None,
        "codex",
    );
    let rec1_diff_runtime = make_rec(
        "01900000-0000-7000-8000-000000000301",
        "evt-1",
        None,
        "claude",
    );
    assert_ne!(
        rec1.payload_sha256, rec1_diff_runtime.payload_sha256,
        "runtime must be covered by canonical payload_sha256"
    );
    let (saved1, _) = acc.append_usage(org_id, rec1.clone()).unwrap().unwrap();
    assert_eq!(saved1.source_sequence, 1);
    let cp_path = durable
        .join("accounting/checkpoints")
        .join(office_id)
        .join("prod-1.json");
    let cp_on_disk: UsageStreamCheckpoint =
        serde_json::from_slice(&fs::read(&cp_path).unwrap()).unwrap();
    assert_eq!(
        cp_on_disk.head_sha256.as_deref(),
        Some(saved1.payload_sha256.as_str()),
        "head_sha256 must match last event payload_sha256 per schema.sql:1129"
    );
    assert!(
        cp_on_disk.chain_sha256.is_some(),
        "chain_sha256 must be recorded separately from head_sha256"
    );
    drop(acc);

    // 2. Inject TWO orphan corrections (seq 2 and seq 3 on the same checkpointed stream) whose
    //    parents are missing. Reopen must:
    //    - stage both without failing startup
    //    - count distinct incomplete streams as 1 (not 3!)
    //    - refuse appending a new record onto that stream (Conflict, never reusing seq 2)
    //    - refuse replaying a staged orphan (Conflict, never `replayed: true`)
    let jsonl_path = durable
        .join("accounting/usage")
        .join(office_id)
        .join("prod-1/2026-01-02.jsonl");
    let saved_jsonl = fs::read(&jsonl_path).unwrap();
    let mut orphan2 = make_rec(
        "01900000-0000-7000-8000-000000000302",
        "evt-orphan-2",
        Some("01900000-0000-7000-8000-000000000998"),
        "codex",
    );
    orphan2.source_sequence = 2;
    let mut orphan3 = make_rec(
        "01900000-0000-7000-8000-000000000303",
        "evt-orphan-3",
        Some("01900000-0000-7000-8000-000000000999"),
        "codex",
    );
    orphan3.source_sequence = 3;
    {
        use std::io::Write;
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(&jsonl_path)
            .unwrap();
        for o in [&orphan2, &orphan3] {
            let mut line = serde_json::to_vec(o).unwrap();
            line.push(b'\n');
            f.write_all(&line).unwrap();
        }
    }
    let acc_orphan =
        super::accounting::SqliteAccountingStore::open(&durable, &runtime, office_id, org_id)
            .unwrap();
    let cov = acc_orphan.coverage_summary(org_id).unwrap();
    assert_eq!(
        cov.incomplete_streams, 1,
        "two staged corrections on one stream must count as 1 distinct incomplete stream"
    );
    let fresh = make_rec(
        "01900000-0000-7000-8000-000000000304",
        "evt-4",
        None,
        "codex",
    );
    assert!(
        matches!(
            acc_orphan.append_usage(org_id, fresh),
            Ok(Err(ref e)) if e.code == org_proto::ErrorCode::Conflict
        ),
        "append must refuse to advance or reuse sequence on a stream with staged records"
    );
    assert!(
        matches!(
            acc_orphan.append_usage(org_id, orphan2.clone()),
            Ok(Err(ref e)) if e.code == org_proto::ErrorCode::Conflict
        ),
        "replaying a staged record must fail closed with Conflict"
    );
    drop(acc_orphan);

    // 3. Restore clean JSONL and tamper ONLY rec1's UUIDv7 (same payload_sha256, broken chain_sha256).
    //    Reopen must quarantine rec1 out of `effective_records`, report `incomplete_streams == 1`,
    //    and refuse to acknowledge a retry as `replayed: true`.
    let mut tampered1 = saved1.clone();
    tampered1.id = "01900000-0000-7000-8000-000000000399".into();
    assert_eq!(tampered1.payload_sha256, saved1.payload_sha256);
    let mut tampered_bytes = serde_json::to_vec(&tampered1).unwrap();
    tampered_bytes.push(b'\n');
    fs::write(&jsonl_path, &tampered_bytes).unwrap();
    let acc_tampered =
        super::accounting::SqliteAccountingStore::open(&durable, &runtime, office_id, org_id)
            .unwrap();
    assert!(
        acc_tampered.effective_records(org_id).unwrap().is_empty(),
        "chain-mismatched record must be quarantined out of effective_records"
    );
    assert_eq!(
        acc_tampered
            .coverage_summary(org_id)
            .unwrap()
            .incomplete_streams,
        1
    );
    assert!(
        matches!(
            acc_tampered.append_usage(org_id, rec1.clone()),
            Ok(Err(ref e)) if e.code == org_proto::ErrorCode::Conflict
        ),
        "retry against a chain-tampered stream must return Conflict, never replayed: true"
    );
    drop(acc_tampered);
    fs::write(&jsonl_path, &saved_jsonl).unwrap();

    // 4. Unanchored restore (checkpoint file deleted) MUST NOT fabricate a checkpoint (`Pass 6 #1`).
    let saved_cp = fs::read(&cp_path).unwrap();
    fs::remove_file(&cp_path).unwrap();
    let acc_unanchored =
        super::accounting::SqliteAccountingStore::open(&durable, &runtime, office_id, org_id)
            .unwrap();
    assert!(
        !cp_path.is_file(),
        "rebuild must never fabricate a checkpoint for an unanchored restore"
    );
    assert!(acc_unanchored.effective_records(org_id).unwrap().is_empty());
    assert_eq!(
        acc_unanchored
            .coverage_summary(org_id)
            .unwrap()
            .incomplete_streams,
        1
    );
    drop(acc_unanchored);
    fs::write(&cp_path, &saved_cp).unwrap();

    // 5. Verified stream with valid correction #1 + orphan correction #2 (`Pass 6 #2`):
    //    valid correction #1 MUST remain projected while ONLY orphan #2 is staged.
    let acc_corr =
        super::accounting::SqliteAccountingStore::open(&durable, &runtime, office_id, org_id)
            .unwrap();
    let mut corr1 = make_rec(
        "01900000-0000-7000-8000-000000000311",
        "evt-corr-1",
        Some(&saved1.id),
        "codex",
    );
    corr1.producer_id = "prod-corr".into();
    corr1.cost_micro_usd = Some(55);
    corr1.payload_sha256 = compute_usage_payload_sha256(org_id, &corr1);
    let (saved_corr1, _) = acc_corr.append_usage(org_id, corr1).unwrap().unwrap();
    assert_eq!(saved_corr1.source_sequence, 1);
    drop(acc_corr);
    let corr_jsonl = durable
        .join("accounting/usage")
        .join(office_id)
        .join("prod-corr/2026-01-02.jsonl");
    let mut orphan_on_corr = make_rec(
        "01900000-0000-7000-8000-000000000312",
        "evt-corr-orphan-2",
        Some("01900000-0000-7000-8000-000000000997"),
        "codex",
    );
    orphan_on_corr.producer_id = "prod-corr".into();
    orphan_on_corr.source_sequence = 2;
    orphan_on_corr.payload_sha256 = compute_usage_payload_sha256(org_id, &orphan_on_corr);
    {
        use std::io::Write;
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(&corr_jsonl)
            .unwrap();
        let mut line = serde_json::to_vec(&orphan_on_corr).unwrap();
        line.push(b'\n');
        f.write_all(&line).unwrap();
    }
    let acc_mixed =
        super::accounting::SqliteAccountingStore::open(&durable, &runtime, office_id, org_id)
            .unwrap();
    let eff = acc_mixed.effective_records(org_id).unwrap();
    assert_eq!(
        eff.len(),
        1,
        "valid correction #1 must supersede rec1 and stay projected even when orphan #2 is staged"
    );
    assert_eq!(eff[0].id, saved_corr1.id);
    assert_eq!(eff[0].cost_micro_usd, Some(55));
    assert_eq!(
        acc_mixed
            .coverage_summary(org_id)
            .unwrap()
            .incomplete_streams,
        1
    );
    drop(acc_mixed);

    // 5b. Also test Pass 6 #2 when the durable checkpoint ALREADY covers both corr1 and orphan #2
    //     (hw = 2, chain over [corr1, orphan2]): valid corr1 must STILL stay projected!
    let corr_cp_path = durable
        .join("accounting/checkpoints")
        .join(office_id)
        .join("prod-corr.json");
    let mut corr_cp: UsageStreamCheckpoint =
        serde_json::from_slice(&fs::read(&corr_cp_path).unwrap()).unwrap();
    corr_cp.high_water_sequence = 2;
    corr_cp.head_sha256 = Some(orphan_on_corr.payload_sha256.clone());
    corr_cp.chain_sha256 = Some(org_office::accounting::chain_usage_stream_head(
        corr_cp.chain_sha256.as_deref(),
        &orphan_on_corr,
    ));
    fs::write(&corr_cp_path, serde_json::to_vec_pretty(&corr_cp).unwrap()).unwrap();
    let acc_mixed_hw2 =
        super::accounting::SqliteAccountingStore::open(&durable, &runtime, office_id, org_id)
            .unwrap();
    let eff2 = acc_mixed_hw2.effective_records(org_id).unwrap();
    assert_eq!(
        eff2.len(),
        1,
        "with checkpoint hw=2 covering [corr1, orphan2], valid corr1 must stay projected while only orphan2 is staged"
    );
    assert_eq!(eff2[0].id, saved_corr1.id);
    assert_eq!(
        acc_mixed_hw2
            .coverage_summary(org_id)
            .unwrap()
            .incomplete_streams,
        1
    );
}
