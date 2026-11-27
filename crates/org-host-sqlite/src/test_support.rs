//! Debug-only SQLite test fixtures for `agentic-org`, `org-host-setup`, and SQLite conformance
//! integration tests.

use std::path::Path;

use org_host::ports::host_store::journal::{PruneOutcome, Stored};
use org_host::ports::host_store::Error;
use rusqlite::Connection;

use crate::HostDb;

pub mod custody;

/// Test-only helper kept inside the SQLite adapter so external tests never import `rusqlite`
/// or write SQL directly (ADR-0018 ENG-1).
pub fn corrupt_provisioning_prepared_for_test(registry_path: &Path) {
    let raw = Connection::open(registry_path).unwrap();
    raw.execute_batch("PRAGMA ignore_check_constraints=ON;")
        .unwrap();
    assert_eq!(
        raw.execute("UPDATE selected_provisioning SET prepared='{'", [])
            .unwrap(),
        1
    );
}

/// Test-only helper kept inside the SQLite adapter so process tests read `linked` on disk
/// without importing `rusqlite` or SQL outside `org-host-sqlite` (ADR-0018 ENG-1).
pub fn provisioning_linked_on_disk_for_test(registry_path: &Path, operation: &str) -> bool {
    Connection::open(registry_path)
        .unwrap()
        .query_row(
            "SELECT linked FROM selected_provisioning WHERE operation_id=?1",
            [operation],
            |r| r.get(0),
        )
        .expect("the reservation")
}

/// Skips fsync so conformance cases that insert thousands of rows (`SESSION_LIMIT`, journal
/// backlog) finish quickly; durability is covered separately by crash/unit tests.
pub fn relax_durability(db: &HostDb) {
    let conn = db.connection.lock().unwrap();
    conn.execute_batch("PRAGMA synchronous=OFF; PRAGMA fullfsync=OFF;")
        .unwrap();
}

/// Conformance fixture: inserts one expired `reserved` row and one expired `abandoned` row
/// referenced by `org_create_staging` so journal-pruning conformance can verify exemption.
pub fn seed_expired_exempt_journal_rows(
    db: &HostDb,
    reserved_id: &str,
    staged_id: &str,
    account: &str,
    expires_at_ms: i64,
) {
    let refusal = serde_json::to_string(&Stored::Rejected {
        refusal: org_proto::ApiError::new(org_proto::ErrorCode::InvalidName, "bad"),
    })
    .unwrap();
    let mut conn = db.connection.lock().unwrap();
    let tx = conn.transaction().unwrap();
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

/// Runs bounded batch pruning on `HostDb` for the shared journal-prune batch-cap conformance test.
pub fn journal_prune_expired_capped(
    db: &HostDb,
    wall_ms: i64,
    max_batches: usize,
) -> Result<PruneOutcome, Error> {
    db.prune_expired_capped(wall_ms, max_batches)
}

/// Lists all account IDs currently referenced in `op_verifiers` (sorted) for startup-prune
/// conformance verification.
pub fn op_verifier_accounts(db: &HostDb) -> Vec<String> {
    let conn = db.connection.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT created_account FROM op_verifiers ORDER BY created_account")
        .unwrap();
    stmt.query_map([], |row| row.get(0))
        .unwrap()
        .map(|row| row.unwrap())
        .collect()
}
