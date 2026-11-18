//! What the SQLite engine rests on, asserted against the contract schema
//! (`docs/design/organization-data/schema.sql`) on the bundled SQLite:
//!
//! - the deferred-foreign-key counter (`SQLITE_DBSTATUS_DEFERRED_FKS`) tells, inside an open
//!   transaction, whether a commit would fail, and follows `ROLLBACK TO` a savepoint;
//! - `PRAGMA defer_foreign_keys = ON` after `BEGIN` defers the immediate references too,
//!   lasts through savepoints and ends with the transaction;
//! - `PRAGMA foreign_key_check` names the table and row of a pending violation.

use rusqlite::{ffi, Connection};

const SCHEMA: &str = include_str!("../../../../../docs/design/organization-data/schema.sql");

const ORG: &str = "01900000-0000-7000-8000-0000000000a2";
const OFFICE: &str = "01900000-0000-7000-8000-0000000000a3";
const REPOSITORY: &str = "01900000-0000-7000-8000-000000000010";
const TEAM: &str = "01900000-0000-7000-8000-000000000011";
const CREATOR: &str = "01900000-0000-7000-8000-000000000012";
const HR: &str = "01900000-0000-7000-8000-000000000013";

fn open() -> Connection {
    let db = Connection::open_in_memory().expect("a database");
    db.execute_batch(SCHEMA).expect("the contract schema loads");
    assert_eq!(number(&db, "PRAGMA foreign_keys"), 1);
    db
}

/// Zero exactly when every reference of the open transaction is resolved.
fn unresolved(db: &Connection) -> i32 {
    let (mut current, mut high) = (0, 0);
    // SAFETY: the handle is this open connection's, and the two out-pointers are valid for
    // the call.
    let code = unsafe {
        ffi::sqlite3_db_status(
            db.handle(),
            ffi::SQLITE_DBSTATUS_DEFERRED_FKS,
            &mut current,
            &mut high,
            0,
        )
    };
    assert_eq!(code, ffi::SQLITE_OK);
    current
}

fn number(db: &Connection, sql: &str) -> i64 {
    db.query_row(sql, [], |row| row.get(0)).expect("a number")
}

fn deferring(db: &Connection) -> bool {
    number(db, "PRAGMA defer_foreign_keys") == 1
}

fn run(db: &Connection, sql: &str) -> rusqlite::Result<()> {
    db.execute_batch(sql)
}

fn must(db: &Connection, sql: &str) {
    run(db, sql).unwrap_or_else(|error| panic!("{sql}: {error}"));
}

/// Every pending violation, as `(table, parent table)`.
fn violations(db: &Connection) -> Vec<(String, Option<i64>, String)> {
    let mut check = db.prepare("PRAGMA foreign_key_check").unwrap();
    let rows = check.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)));
    rows.unwrap().map(Result::unwrap).collect()
}

fn organization() -> String {
    format!(
        "INSERT INTO organizations (id, slug, schema_version, revision, name, \
         organization_repository_id, state_branch) \
         VALUES ('{ORG}', 'example', 1, 1, 'Example', '{REPOSITORY}', 'org/state')"
    )
}

fn repository() -> String {
    format!(
        "INSERT INTO repositories (organization_id, id, key, schema_version, revision, kind, \
         remote_url) VALUES ('{ORG}', '{REPOSITORY}', 'organization', 1, 1, 'remote', \
         'https://example.test/org.git')"
    )
}

fn office() -> String {
    format!(
        "INSERT INTO offices (organization_id, id, slug, schema_version, revision, name, role) \
         VALUES ('{ORG}', '{OFFICE}', 'hq', 1, 1, 'HQ', 'hq')"
    )
}

fn office_code() -> String {
    format!(
        "INSERT INTO office_codes (organization_id, code, office_id, issued_at) \
         VALUES ('{ORG}', 'A', '{OFFICE}', '2026-11-14T10:00:00.000Z')"
    )
}

fn team(id: &str, number: u64, parent: Option<&str>, lead: Option<&str>) -> String {
    let text = |value: Option<&str>| value.map_or("NULL".to_owned(), |v| format!("'{v}'"));
    format!(
        "INSERT INTO teams (organization_id, id, number, schema_version, revision, name, \
         parent_team_id, lead_employee_id) VALUES ('{ORG}', '{id}', {number}, 1, 1, 'Team', \
         {}, {})",
        text(parent),
        text(lead)
    )
}

fn employee(id: &str, handle: &str, kind: &str, home: &str) -> String {
    format!(
        "INSERT INTO employees (organization_id, id, handle, schema_version, revision, kind, \
         name, home_team_id, created_by_kind, created_by_id) VALUES ('{ORG}', '{id}', \
         '{handle}', 1, 1, '{kind}', 'Name', '{home}', 'employee', '{CREATOR}')"
    )
}

fn is_foreign_key(error: &rusqlite::Error) -> bool {
    error.sqlite_error().is_some_and(|e| {
        e.extended_code == ffi::SQLITE_CONSTRAINT_FOREIGNKEY
            || e.code == rusqlite::ErrorCode::ConstraintViolation
    })
}

/// Q2: the counter is non-zero while a deferred reference is unresolved, a commit then
/// fails and leaves the transaction open, and `ROLLBACK TO` takes the counter back with
/// the rows.
#[test]
fn the_deferred_counter_tells_whether_a_commit_would_fail_and_follows_a_savepoint() {
    let db = open();
    assert_eq!(unresolved(&db), 0);
    must(&db, "BEGIN IMMEDIATE");
    // The organization names its repository through a deferred reference.
    must(&db, &organization());
    assert!(unresolved(&db) > 0);
    let refused = run(&db, "COMMIT").expect_err("a commit with a dangling reference");
    assert!(is_foreign_key(&refused), "{refused}");
    assert!(
        !db.is_autocommit(),
        "the refused commit left the transaction open"
    );
    must(&db, &repository());
    assert_eq!(unresolved(&db), 0);

    // One operation: a team under a parent that does not exist. Undone alone.
    must(&db, "SAVEPOINT op");
    must(&db, &team(TEAM, 1, Some(HR), None));
    assert!(unresolved(&db) > 0);
    must(&db, "ROLLBACK TO op; RELEASE op");
    assert_eq!(unresolved(&db), 0);

    // The next operation is valid and the batch commits with the first one's rows.
    must(&db, "SAVEPOINT op");
    must(&db, &team(TEAM, 1, None, None));
    assert_eq!(unresolved(&db), 0);
    must(&db, "RELEASE op");
    // Removing a row something refers to counts too, and putting it back resolves it.
    must(
        &db,
        &format!("DELETE FROM repositories WHERE id = '{REPOSITORY}'"),
    );
    assert!(unresolved(&db) > 0);
    must(&db, &repository());
    assert_eq!(unresolved(&db), 0);
    must(&db, "COMMIT");
    assert_eq!(unresolved(&db), 0);
    assert_eq!(number(&db, "SELECT count(*) FROM teams"), 1);
}

/// Q3: without the pragma an immediate reference fails at its statement; with it, issued
/// after `BEGIN`, files may arrive in any order. It survives `ROLLBACK TO` and is off
/// after `COMMIT` and after `ROLLBACK`.
#[test]
fn defer_foreign_keys_after_begin_lasts_the_transaction_and_no_longer() {
    let db = open();
    // `offices.organization_id REFERENCES organizations(id)` is immediate.
    must(&db, "BEGIN IMMEDIATE");
    let refused = run(&db, &office()).expect_err("an immediate reference");
    assert!(is_foreign_key(&refused), "{refused}");
    assert_eq!(
        unresolved(&db),
        0,
        "the refused statement left nothing pending"
    );
    must(&db, "ROLLBACK");

    must(&db, "BEGIN IMMEDIATE");
    assert!(!deferring(&db));
    must(&db, "PRAGMA defer_foreign_keys = ON");
    assert!(deferring(&db));
    must(&db, &office());
    assert!(
        unresolved(&db) > 0,
        "the immediate reference is now counted"
    );
    must(&db, "SAVEPOINT op");
    must(&db, &office_code());
    must(&db, "ROLLBACK TO op; RELEASE op");
    assert!(deferring(&db), "the pragma survives ROLLBACK TO");
    assert!(unresolved(&db) > 0, "the office is still pending");
    // Still deferring after the savepoint: another early row is accepted.
    must(&db, &office_code());
    must(&db, &organization());
    must(&db, &repository());
    assert_eq!(unresolved(&db), 0);
    must(&db, "COMMIT");
    assert!(!deferring(&db), "the pragma is off after COMMIT");

    // The next transaction is immediate again until the pragma is issued again.
    must(&db, "BEGIN IMMEDIATE");
    let other = office()
        .replace(OFFICE, HR)
        .replace("'hq', 1", "'other', 1");
    let other = other.replace(ORG, CREATOR);
    let refused = run(&db, &other).expect_err("immediate again");
    assert!(is_foreign_key(&refused), "{refused}");
    must(&db, "PRAGMA defer_foreign_keys = ON");
    must(&db, &other);
    assert!(unresolved(&db) > 0);
    let refused = run(&db, "COMMIT").expect_err("a commit with a dangling reference");
    assert!(is_foreign_key(&refused), "{refused}");
    assert!(
        deferring(&db),
        "a refused commit keeps the transaction and the pragma"
    );
    must(&db, "ROLLBACK");
    assert!(!deferring(&db), "the pragma is off after ROLLBACK");
    assert_eq!(unresolved(&db), 0);
    assert_eq!(number(&db, "SELECT count(*) FROM offices"), 1);
}

/// The pragma issued before `BEGIN`, in autocommit, carries into the transaction that
/// follows: the flag is the connection's, and only `COMMIT` and `ROLLBACK` clear it.
/// Pinned so a change in the bundled SQLite shows; the engine issues it after `BEGIN`, as
/// the documentation states it.
#[test]
fn defer_foreign_keys_before_begin_is_pinned() {
    let db = open();
    must(&db, "PRAGMA defer_foreign_keys = ON");
    let before_begin = deferring(&db);
    must(&db, "BEGIN IMMEDIATE");
    let in_transaction = deferring(&db);
    let early_row = run(&db, &office());
    must(&db, "ROLLBACK");
    assert!(!deferring(&db));
    assert_eq!(
        (before_begin, in_transaction, early_row.is_ok()),
        (true, true, true)
    );
}

/// What `replace` uses when the counter is not zero: the table, the row and the table
/// referred to, for every pending violation.
#[test]
fn foreign_key_check_names_the_table_and_row_of_a_pending_violation() {
    let db = open();
    must(&db, "BEGIN IMMEDIATE; PRAGMA defer_foreign_keys = ON");
    assert!(violations(&db).is_empty());
    must(&db, &office());
    let rowid = number(&db, "SELECT rowid FROM offices");
    let pending = violations(&db);
    assert_eq!(
        pending,
        [(
            "offices".to_owned(),
            Some(rowid),
            "organizations".to_owned()
        )]
    );
    must(&db, &organization());
    let tables: Vec<_> = violations(&db).into_iter().map(|v| (v.0, v.2)).collect();
    assert_eq!(
        tables,
        [("organizations".to_owned(), "repositories".to_owned())]
    );
    must(&db, &repository());
    assert!(violations(&db).is_empty());
    assert_eq!(unresolved(&db), 0);
    must(&db, "COMMIT");
}

/// The rows of a valid creation, inserted in the byte order a rebuild walks the files in
/// (`identity/`, `offices/`, `organization.yaml`, `repositories.yaml`, then a team's
/// `agents/`, `people/` and `team.yaml`), leave the counter at zero and commit. The same
/// rows with the lead outside the team do not.
#[test]
fn a_valid_creation_in_rebuild_order_ends_with_the_counter_at_zero() {
    let rows = |lead_home: &str| {
        let db = open();
        must(&db, "BEGIN IMMEDIATE; PRAGMA defer_foreign_keys = ON");
        let other = "01900000-0000-7000-8000-000000000021";
        for sql in [
            office_code(),
            office(),
            organization(),
            repository(),
            employee(HR, "hr", "agent", TEAM),
            employee(CREATOR, "ana", "person", lead_home),
            team(TEAM, 1, None, Some(CREATOR)),
            team(other, 2, Some(TEAM), None),
            format!(
                "INSERT INTO record_refs (organization_id, kind, id, is_deleted) \
                 VALUES ('{ORG}', 'team', '{TEAM}', 0)"
            ),
        ] {
            must(&db, &sql);
        }
        db
    };

    let db = rows(TEAM);
    assert_eq!(unresolved(&db), 0);
    assert!(violations(&db).is_empty());
    must(&db, "COMMIT");

    // The lead's home is another team: the reference from the team to its lead dangles.
    let db = rows("01900000-0000-7000-8000-000000000021");
    assert!(unresolved(&db) > 0);
    let tables: Vec<_> = violations(&db).into_iter().map(|v| (v.0, v.2)).collect();
    assert_eq!(tables, [("teams".to_owned(), "employees".to_owned())]);
    assert!(run(&db, "COMMIT").is_err());
}
