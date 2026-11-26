//! The deferred-reference check a transaction must pass before it commits.

use org_store::EngineError;
use rusqlite::{ffi, Connection};

use super::failed;

/// Zero exactly when every reference of the open transaction is resolved, deferred or not.
fn unresolved_references(db: &Connection) -> Result<i32, EngineError> {
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
