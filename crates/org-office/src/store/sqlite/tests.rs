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
