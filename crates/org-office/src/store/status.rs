//! `office.db`'s `member_status`: each agent's own status line (`status.set`), kept so it
//! survives a server restart (ADR-0003 §1.1 item 6; `current_task_id` is the work slice's, and
//! clearing a line deletes the row until that exists). One row per member; a line lapses at
//! `expires_at`, and a lapsed row is never shown (the card derivation compares instants) and is
//! dropped on the next write.
//! Part of the store module, so SQL and rusqlite types stay in it (ADR-0002 §5).

use rusqlite::Connection;

use crate::error::OfficeError;

/// A stored line: the member, the text, and when it lapses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StatusLine {
    pub member_id: String,
    pub text: String,
    pub expires_at: String,
}

pub(crate) fn create_table(tx: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE member_status (
             member_id TEXT PRIMARY KEY, status_text TEXT NOT NULL, current_task_id TEXT,
             updated_at TEXT NOT NULL, expires_at TEXT NOT NULL
         ) STRICT;",
    )
}

fn storage(error: rusqlite::Error) -> OfficeError {
    OfficeError::storage(error)
}

/// Sets `member_id`'s line, or clears it with `None`, and drops every line that lapsed by
/// `now` (all times `YYYY-MM-DDTHH:MM:SS.mmmZ`, so they order as text). Runs on the writer.
pub(crate) fn write(
    db: &Connection,
    member_id: &str,
    line: Option<(&str, &str)>,
    now: &str,
) -> Result<(), OfficeError> {
    db.execute("DELETE FROM member_status WHERE expires_at <= ?1", [now])
        .map_err(storage)?;
    match line {
        Some((text, expires_at)) => db.execute(
            "INSERT INTO member_status (member_id, status_text, updated_at, expires_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(member_id) DO UPDATE SET status_text = excluded.status_text,
                 updated_at = excluded.updated_at, expires_at = excluded.expires_at",
            (member_id, text, now, expires_at),
        ),
        None => db.execute(
            "DELETE FROM member_status WHERE member_id = ?1",
            [member_id],
        ),
    }
    .map(|_| ())
    .map_err(storage)
}

/// Every stored line (lapsed ones included: the reader compares instants).
pub(crate) fn all(db: &Connection) -> Result<Vec<StatusLine>, OfficeError> {
    let mut statement = db
        .prepare("SELECT member_id, status_text, expires_at FROM member_status")
        .map_err(storage)?;
    let rows = statement
        .query_map([], |row| {
            Ok(StatusLine {
                member_id: row.get(0)?,
                text: row.get(1)?,
                expires_at: row.get(2)?,
            })
        })
        .map_err(storage)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(storage)
}
