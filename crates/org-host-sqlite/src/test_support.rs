//! Debug-only SQLite test fixtures for `agentic-org` and `org-host-setup` real-process tests.

use std::path::Path;

use rusqlite::Connection;

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
