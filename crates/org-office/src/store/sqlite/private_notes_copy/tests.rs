use std::os::unix::fs::PermissionsExt;

use super::*;
use crate::store::sqlite::private_notes::SqlitePrivateNotesStore;

fn notes_db() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let durable = dir.path().canonicalize().unwrap().join("durable");
    std::fs::create_dir(&durable).unwrap();
    drop(SqlitePrivateNotesStore::open(&durable).unwrap());
    (dir, durable.join("private/task-notes.db"))
}

#[test]
fn a_copy_carries_pages_still_in_the_wal_and_is_owner_only() {
    let (dir, db) = notes_db();
    let writer = Connection::open(&db).unwrap();
    writer
        .execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA wal_autocheckpoint = 0;
             INSERT INTO private_note_authors VALUES ('org', 'only in the wal');",
        )
        .unwrap();
    let wal = std::fs::metadata(db.with_extension("db-wal")).unwrap();
    assert!(wal.len() > 0, "the row is still in the WAL");
    let copy = dir.path().canonicalize().unwrap().join("copy.db");
    snapshot_task_notes(&db, &copy).unwrap();
    drop(writer);
    let mode = std::fs::metadata(&copy).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let read = Connection::open_with_flags(&copy, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let value: String = read
        .query_row("SELECT author_id FROM private_note_authors", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(value, "only in the wal");
    // A copy never replaces a file that is there.
    assert!(snapshot_task_notes(&db, &copy).is_err());
}

#[test]
fn a_damaged_copy_or_another_database_fails_the_check() {
    let (dir, db) = notes_db();
    let root = dir.path().canonicalize().unwrap();
    let copy = root.join("copy.db");
    snapshot_task_notes(&db, &copy).unwrap();
    check_task_notes(&copy).unwrap();
    let mut bytes = std::fs::read(&copy).unwrap();
    let page = 4096.min(bytes.len() / 2);
    for b in &mut bytes[page..page + 512] {
        *b = 0xa5;
    }
    let damaged = root.join("damaged.db");
    std::fs::write(&damaged, &bytes).unwrap();
    assert!(check_task_notes(&damaged).is_err());
    let other = root.join("other.db");
    Connection::open(&other)
        .unwrap()
        .execute_batch("CREATE TABLE t (x)")
        .unwrap();
    assert!(check_task_notes(&other).is_err());
    assert!(check_task_notes(std::path::Path::new("relative.db")).is_err());
}

fn user_space_db() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let durable = dir.path().canonicalize().unwrap().join("durable");
    std::fs::create_dir(&durable).unwrap();
    drop(crate::store::sqlite::user_space::SqliteUserSpaceStore::open(&durable).unwrap());
    (dir, durable.join("private/user-space.db"))
}

#[test]
fn a_user_space_copy_carries_rows_still_in_the_wal_and_passes_only_its_own_check() {
    let (dir, db) = user_space_db();
    let writer = Connection::open(&db).unwrap();
    writer
        .execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA wal_autocheckpoint = 0;
             CREATE TABLE probe (v TEXT); INSERT INTO probe VALUES ('only in the wal');",
        )
        .unwrap();
    assert!(
        std::fs::metadata(db.with_extension("db-wal"))
            .unwrap()
            .len()
            > 0
    );
    let root = dir.path().canonicalize().unwrap();
    let copy = root.join("user-space.db");
    snapshot_user_space(&db, &copy).unwrap();
    drop(writer);
    assert_eq!(
        std::fs::metadata(&copy).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let read = Connection::open_with_flags(&copy, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let value: String = read
        .query_row("SELECT v FROM probe", [], |row| row.get(0))
        .unwrap();
    assert_eq!(value, "only in the wal");
    check_user_space(&copy).unwrap();
    // Each database is checked for its own schema: the two never stand in for each other.
    assert!(check_task_notes(&copy).is_err());
    let (_notes_dir, notes) = notes_db();
    assert!(check_user_space(&notes).is_err());
    assert!(
        snapshot_user_space(&db, &copy).is_err(),
        "never replaces a file"
    );
}

#[test]
fn a_copy_or_check_keeps_temporary_data_in_memory() {
    let (_dir, db) = notes_db();
    let conn = open_read_only(&db).unwrap();
    let temp_store: i64 = conn
        .query_row("PRAGMA temp_store", [], |row| row.get(0))
        .unwrap();
    assert_eq!(temp_store, 2, "MEMORY");
}
