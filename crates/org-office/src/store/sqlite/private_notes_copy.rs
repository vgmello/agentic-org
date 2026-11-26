//! A consistent copy of the private task notes database for a backup, and the integrity check
//! of a restored copy (`TASK-104`, `doc-79`). Kept in the SQLite adapter, the only code that
//! names the engine (ENG-1).
//!
//! `task-notes.db` runs in WAL mode, so its committed pages may still be in `-wal`: copying the
//! three files byte for byte can tear them. `VACUUM INTO` writes one self-contained database
//! from a read transaction instead; the sidecars are never copied.

use std::fs::OpenOptions;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use crate::error::OfficeError;
use crate::private_fs::full_sync;

/// The tables `private-notes-schema.sql` creates; a copy without them is not a notes database.
const TABLES: &str = "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN \
     ('private_note_authors', 'private_note_tasks', 'private_task_notes', \
     'private_note_command_receipts')";

fn open_read_only(path: &Path) -> Result<Connection, OfficeError> {
    if !path.is_absolute() {
        // The bundled SQLite reads a path starting with `file:` as a URI.
        return Err(OfficeError::Invalid(
            "a notes database path must be absolute",
        ));
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(path, flags).map_err(OfficeError::storage)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(OfficeError::storage)?;
    Ok(conn)
}

/// Writes a transactionally consistent copy of `source` (`<durable>/private/task-notes.db`)
/// to `target`, a path that must not exist: it is created owner-only (`0600`, never following
/// a link) before SQLite fills it, and fully flushed after. `source` is opened read-only; the
/// caller holds the office lock, so no writer runs meanwhile.
pub fn snapshot_task_notes(source: &Path, target: &Path) -> Result<(), OfficeError> {
    let conn = open_read_only(source)?;
    let made = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(org_platform::fs::O_NOFOLLOW | org_platform::fs::O_CLOEXEC)
        .open(target)
        .map_err(OfficeError::storage)?;
    let target_text =
        target
            .to_str()
            .filter(|_| target.is_absolute())
            .ok_or(OfficeError::Invalid(
                "a notes copy path must be absolute UTF-8",
            ))?;
    conn.execute("VACUUM INTO ?1", [target_text])
        .map_err(OfficeError::storage)?;
    drop(conn);
    full_sync(&made).map_err(OfficeError::storage)?;
    check_task_notes(target)
}

/// Opens a copy read-only and requires `PRAGMA integrity_check` to answer exactly `ok` and the
/// notes schema's tables to be there.
pub fn check_task_notes(path: &Path) -> Result<(), OfficeError> {
    let conn = open_read_only(path)?;
    let mut statement = conn
        .prepare("PRAGMA integrity_check")
        .map_err(OfficeError::storage)?;
    let answers: Vec<String> = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(OfficeError::storage)?
        .collect::<Result<_, _>>()
        .map_err(OfficeError::storage)?;
    if answers != ["ok"] {
        return Err(OfficeError::Invalid(
            "the private task notes database fails its integrity check",
        ));
    }
    let tables: i64 = conn
        .query_row(TABLES, [], |row| row.get(0))
        .map_err(OfficeError::storage)?;
    if tables != 4 {
        return Err(OfficeError::Invalid(
            "the private task notes database lacks its schema",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
