//! SQLite-only browser-session cases: digests at rest (SES-4) and opening
//! registries created before B3.
use std::os::unix::fs::PermissionsExt;

use rusqlite::OpenFlags;

use super::super::tests::connection_for_test;
use crate::{HostDb, DB};
use org_host::ports::accounts::Verifier;
use org_host::ports::host_secrets::{self, HostSecrets};
use org_host::ports::host_store::{Error, HostDbInit};
use org_host::ports::sessions::{BrowserSessions, NewSession, Origin, Scheme, SessionDigest};

const INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071b0";
const NOW: i64 = 1_800_000_000_000;
const ALICE: &str = "01J0000000000000000000000A";
const BOB: &str = "01J0000000000000000000000B";
const PHC: &str = "$argon2id$v=19$m=65536,t=3,p=1$MDEyMzQ1Njc4OWFiY2RlZg$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

pub(crate) struct NoSecrets;
impl HostSecrets for NoSecrets {
    fn put(
        &self,
        _: &host_secrets::Name,
        _: &host_secrets::Value,
    ) -> Result<(), host_secrets::Error> {
        Ok(())
    }
    fn load(
        &self,
        _: &host_secrets::Name,
    ) -> Result<Option<host_secrets::Value>, host_secrets::Error> {
        Ok(None)
    }
    fn delete(&self, _: &host_secrets::Name) -> Result<(), host_secrets::Error> {
        Ok(())
    }
    fn list(&self) -> Result<Vec<host_secrets::Name>, host_secrets::Error> {
        Ok(Vec::new())
    }
}

pub(crate) fn sqlite() -> (tempfile::TempDir, HostDb) {
    let home = tempfile::tempdir().unwrap();
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let init = HostDbInit {
        instance_id: INSTANCE.into(),
        clock_high_ms: NOW,
    };
    let db = HostDb::create(home.path(), &NoSecrets, &init).unwrap();
    for (id, login) in [(ALICE, "alice"), (BOB, "bob")] {
        let verifier = Verifier::new(PHC.into()).unwrap();
        db.seed_account(id, login, &verifier, id == ALICE, NOW)
            .unwrap();
    }
    db.relax_durability_for_tests();
    (home, db)
}

fn origin() -> Origin {
    Origin::new(Scheme::Https, "office.example", 8443).unwrap()
}

fn raw(n: u32) -> ([u8; 32], [u8; 32]) {
    let mut cookie = [0xC0; 32];
    let mut proof = [0x9F; 32];
    cookie[..4].copy_from_slice(&n.to_be_bytes());
    proof[..4].copy_from_slice(&n.to_be_bytes());
    (cookie, proof)
}

fn new_session(account: &str, n: u32) -> NewSession {
    let (cookie, proof) = raw(n);
    NewSession::new(
        account,
        SessionDigest::cookie(&cookie),
        SessionDigest::proof(&proof),
        origin(),
    )
    .unwrap()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}
/// Every byte the registry keeps on disk: the database and any sidecar.
fn at_rest(home: &std::path::Path) -> Vec<u8> {
    let mut bytes = Vec::new();
    for suffix in ["", "-wal", "-shm", "-journal"] {
        if let Ok(b) = std::fs::read(home.join(format!("{DB}{suffix}"))) {
            bytes.extend(b);
        }
    }
    bytes
}
fn raw_db(home: &std::path::Path) -> rusqlite::Connection {
    connection_for_test(home.join(DB), OpenFlags::SQLITE_OPEN_READ_WRITE).unwrap()
}
fn names(db: &rusqlite::Connection) -> Vec<String> {
    db.prepare("SELECT name FROM sqlite_schema WHERE name LIKE 'browser_sessions%' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn only_digests_are_at_rest_and_a_copied_registry_yields_no_usable_cookie() {
    let (home, db) = sqlite();
    for n in 1..=3 {
        db.create(&new_session(ALICE, n), NOW).unwrap();
    }
    // Before and after the close checkpoint.
    let mut db = Some(db);
    for closed in [false, true] {
        if closed {
            drop(db.take());
        }
        let bytes = at_rest(home.path());
        for n in 1..=3 {
            let (cookie, proof) = raw(n);
            assert!(!contains(&bytes, &cookie), "raw cookie {n} at rest");
            assert!(!contains(&bytes, &proof), "raw proof {n} at rest");
        }
        if closed {
            // The scan sees stored data: the digest itself is present.
            let (cookie, _) = raw(1);
            assert!(contains(&bytes, &SessionDigest::cookie(&cookie).0));
        }
    }

    let copy = tempfile::tempdir().unwrap();
    std::fs::set_permissions(copy.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::copy(home.path().join(DB), copy.path().join(DB)).unwrap();
    let copied = HostDb::open(copy.path(), &NoSecrets).unwrap();
    let (cookie, proof) = raw(1);
    let (cookie_d, proof_d) = (SessionDigest::cookie(&cookie), SessionDigest::proof(&proof));
    // Presenting what the copy holds (the digests) as cookie and proof finds nothing.
    let replayed = copied
        .find(
            &SessionDigest::cookie(&cookie_d.0),
            &SessionDigest::proof(&proof_d.0),
            &origin(),
            NOW,
        )
        .unwrap();
    assert_eq!(replayed, None);
    // Only the raw values, which the copy does not contain, would work.
    assert!(copied
        .find(&cookie_d, &proof_d, &origin(), NOW)
        .unwrap()
        .is_some());
}

#[test]
fn opening_twice_is_idempotent_and_keeps_sessions() {
    let (home, db) = sqlite();
    let s = new_session(ALICE, 1);
    db.create(&s, NOW).unwrap();
    drop(db);
    for _ in 0..2 {
        let db = HostDb::open(home.path(), &NoSecrets).unwrap();
        let (cookie, proof) = raw(1);
        let found = db
            .find(
                &SessionDigest::cookie(&cookie),
                &SessionDigest::proof(&proof),
                &origin(),
                NOW,
            )
            .unwrap()
            .unwrap();
        assert_eq!(found.id, s.id);
    }
    assert_eq!(
        names(&raw_db(home.path())),
        [
            "browser_sessions",
            "browser_sessions_account",
            "browser_sessions_activity"
        ]
    );
}

#[test]
fn a_registry_from_before_the_session_table_gains_it_at_open() {
    let (home, db) = sqlite();
    drop(db);
    raw_db(home.path())
        .execute_batch(
            "DROP INDEX browser_sessions_account; DROP INDEX browser_sessions_activity; DROP TABLE browser_sessions;",
        )
        .unwrap();
    assert!(names(&raw_db(home.path())).is_empty());
    let db = HostDb::open(home.path(), &NoSecrets).unwrap();
    db.create(&new_session(ALICE, 1), NOW).unwrap();
    assert_eq!(names(&raw_db(home.path())).len(), 3);
}

#[test]
fn a_session_table_with_other_columns_refuses_the_open_and_creates_nothing() {
    let (home, db) = sqlite();
    drop(db);
    let handmade = "CREATE TABLE browser_sessions (id TEXT PRIMARY KEY, account_id TEXT) STRICT";
    raw_db(home.path())
        .execute_batch(&format!(
            "DROP INDEX browser_sessions_account; DROP INDEX browser_sessions_activity; DROP TABLE browser_sessions; {handmade};"
        ))
        .unwrap();
    for _ in 0..2 {
        assert!(matches!(
            HostDb::open(home.path(), &NoSecrets),
            Err(Error::Outdated)
        ));
        let db = raw_db(home.path());
        assert_eq!(names(&db), ["browser_sessions"]);
        let sql: String = db
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE name='browser_sessions'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sql, handmade);
    }
}

/// Replaces the session objects with `table` (and no indexes) on a closed registry.
fn handmade_table(home: &std::path::Path, table: &str) {
    raw_db(home)
        .execute_batch(&format!(
            "DROP INDEX browser_sessions_account; DROP INDEX browser_sessions_activity; DROP TABLE browser_sessions; {table};"
        ))
        .unwrap();
}

#[test]
fn a_session_table_with_the_same_columns_but_no_cascade_is_refused() {
    let (home, db) = sqlite();
    drop(db);
    let table = super::TABLE_DDL.replace(" ON DELETE CASCADE", "");
    assert_ne!(table, super::TABLE_DDL);
    handmade_table(home.path(), &table);
    assert!(matches!(
        HostDb::open(home.path(), &NoSecrets),
        Err(Error::Outdated)
    ));
    assert_eq!(names(&raw_db(home.path())), ["browser_sessions"]);
}

#[test]
fn a_present_index_with_another_definition_is_refused() {
    let (home, db) = sqlite();
    drop(db);
    raw_db(home.path())
        .execute_batch(
            "DROP INDEX browser_sessions_activity; CREATE INDEX browser_sessions_activity ON browser_sessions(created_ms);",
        )
        .unwrap();
    assert!(matches!(
        HostDb::open(home.path(), &NoSecrets),
        Err(Error::Outdated)
    ));
}

#[test]
fn a_missing_index_alone_is_recreated() {
    let (home, db) = sqlite();
    drop(db);
    raw_db(home.path())
        .execute_batch("DROP INDEX browser_sessions_activity;")
        .unwrap();
    HostDb::open(home.path(), &NoSecrets).unwrap();
    assert_eq!(names(&raw_db(home.path())).len(), 3);
}
