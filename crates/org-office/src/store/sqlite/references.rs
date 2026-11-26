//! The deferred-reference check a transaction must pass before it commits.

use org_store::EngineError;
use rusqlite::{ffi, Connection};

use super::failed;

/// Zero exactly when every reference of the open transaction is resolved, deferred or not.
///
/// The workspace's one `unsafe` block outside `org-platform` (`tests/rust_hygiene.rs` pins
/// it here): rusqlite 0.42 has no safe wrapper for `sqlite3_db_status`, and the deferred-
/// foreign-key counter has no SQL equivalent (a trial `COMMIT` would end the transaction and
/// `PRAGMA foreign_key_check` scans every table). The engine boundary keeps rusqlite here,
/// so the call cannot move to `org-platform`, which links only the C library.
#[expect(
    unsafe_code,
    reason = "rusqlite has no safe sqlite3_db_status; the deferred-reference counter needs the connection handle"
)]
pub(super) fn unresolved_references(db: &Connection) -> Result<i32, EngineError> {
    let (mut current, mut high) = (0, 0);
    // SAFETY: `handle` is this open connection's own, `db` is borrowed for the whole call
    // so it cannot close meanwhile, and both out-pointers are valid for writes. rusqlite
    // 0.42 has no safe wrapper for `sqlite3_db_status`.
    let code = unsafe {
        ffi::sqlite3_db_status(
            db.handle(),
            ffi::SQLITE_DBSTATUS_DEFERRED_FKS,
            &mut current,
            &mut high,
            0,
        )
    };
    if code != ffi::SQLITE_OK {
        return Err(EngineError::new(format!(
            "reading the unresolved-reference count: code {code}"
        )));
    }
    Ok(current)
}

/// Fails when the transaction could not commit, naming the rows whose references dangle.
pub(super) fn check_references(db: &Connection) -> Result<(), EngineError> {
    if unresolved_references(db)? == 0 {
        return Ok(());
    }
    let named = db
        .prepare("PRAGMA foreign_key_check")
        .and_then(|mut check| {
            let rows = check.query_map([], |row| {
                let (table, parent): (String, String) = (row.get(0)?, row.get(2)?);
                let rowid: Option<i64> = row.get(1)?;
                let rowid = rowid.map_or("?".to_owned(), |rowid| rowid.to_string());
                Ok(format!(
                    "{table} row {rowid} refers to a missing row of {parent}"
                ))
            })?;
            rows.take(8).collect::<rusqlite::Result<Vec<String>>>()
        })
        .map_err(|error| failed(db, "naming unresolved references", &error))?;
    Err(EngineError::new(format!(
        "unresolved references: {}",
        named.join("; ")
    )))
}
