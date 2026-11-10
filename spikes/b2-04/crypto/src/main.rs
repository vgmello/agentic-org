use rusqlite::{Connection, OpenFlags};
use std::path::Path;

fn open(path: &Path, key: Option<&str>) -> Connection {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE).unwrap();
    if let Some(key) = key {
        db.pragma_update(None, "key", key).unwrap();
    }
    db
}

fn main() {
    let path = std::env::args().nth(1).expect("temporary database path");
    let path = Path::new(&path);
    // Public synthetic fixture. This is never an office database or credential.
    let fixture_key = "sage synthetic SQLCipher fixture";
    if !path.exists() {
        let db = Connection::open(path).unwrap();
        db.pragma_update(None, "key", fixture_key).unwrap();
        db.execute_batch(
            "CREATE TABLE proof (value TEXT NOT NULL); INSERT INTO proof VALUES ('retained');",
        )
        .unwrap();
        println!("PASS encrypted fixture created");
    }
    let before = std::fs::read(path).unwrap();
    assert!(!before.starts_with(b"SQLite format 3"));
    for key in [None, Some("wrong synthetic fixture")] {
        let db = open(path, key);
        assert!(
            db.query_row("SELECT value FROM proof", [], |r| r.get::<_, String>(0))
                .is_err()
        );
    }
    assert_eq!(before, std::fs::read(path).unwrap());
    println!("PASS missing/wrong keys refused without changing database");
    let db = open(path, Some(fixture_key));
    let value: String = db
        .query_row("SELECT value FROM proof", [], |r| r.get(0))
        .unwrap();
    assert_eq!(value, "retained");
    let version: String = db
        .query_row("PRAGMA cipher_version", [], |r| r.get(0))
        .unwrap();
    let provider: String = db
        .query_row("PRAGMA cipher_provider", [], |r| r.get(0))
        .unwrap();
    let integrity: String = db
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    println!(
        "PASS restored key opens same fixture; integrity=ok; cipher={version}; provider={provider}"
    );
}
