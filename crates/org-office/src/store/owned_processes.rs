//! `owned_processes`: the SQL behind `crate::owned_processes` (B1-12). Runs on the writer for
//! writes and on a reader for reads; every function takes the caller's connection.

use std::io;
use std::time::Instant;

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::OfficeError;
use crate::owned_processes::{OwnedProcess, OwnedProcessKey, SavedLeader, SavedTarget};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredInventory {
    v: u8,
    targets: Vec<SavedTarget>,
}

pub(crate) fn create_table(tx: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE owned_processes (
             id INTEGER PRIMARY KEY AUTOINCREMENT, generation INTEGER NOT NULL,
             mark TEXT NOT NULL UNIQUE CHECK(mark <> ''),
             leader_pid INTEGER, leader_pgid INTEGER, leader_started TEXT, leader_boot TEXT,
             tool_sessions TEXT NOT NULL, inventory TEXT NOT NULL, created_at TEXT NOT NULL,
             CHECK((leader_pid IS NULL) = (leader_pgid IS NULL)
                 AND (leader_pid IS NULL) = (leader_started IS NULL)
                 AND (leader_pid IS NULL) = (leader_boot IS NULL))
         ) STRICT;",
    )
}

const STALE: OfficeError = OfficeError::Invalid("stale owned-process key or mark");
const COLUMNS: &str = "id, generation, mark, leader_pid, leader_pgid, leader_started, \
                       leader_boot, tool_sessions, inventory FROM owned_processes";

fn missed(deadline: Instant) -> Result<(), OfficeError> {
    if Instant::now() < deadline {
        return Ok(());
    }
    let late = "owned-process checkpoint missed its deadline";
    Err(OfficeError::storage(io::Error::new(
        io::ErrorKind::TimedOut,
        late,
    )))
}

fn decode_inventory(text: &str) -> Result<Vec<SavedTarget>, OfficeError> {
    let stored: StoredInventory = serde_json::from_str(text).map_err(OfficeError::storage)?;
    if stored.v != 1 {
        return Err(OfficeError::storage(io::Error::other(
            "unknown inventory version",
        )));
    }
    Ok(stored.targets)
}

fn encode_inventory(targets: &[SavedTarget]) -> Result<String, OfficeError> {
    let stored = StoredInventory {
        v: 1,
        targets: targets.to_vec(),
    };
    serde_json::to_string(&stored).map_err(OfficeError::storage)
}

fn get<T: rusqlite::types::FromSql>(row: &rusqlite::Row<'_>, at: usize) -> Result<T, OfficeError> {
    row.get(at).map_err(OfficeError::storage)
}

fn decode_row(row: &rusqlite::Row<'_>) -> Result<OwnedProcess, OfficeError> {
    let leader = match get::<Option<u32>>(row, 3)? {
        Some(pid) => Some(SavedLeader {
            pid,
            pgid: get(row, 4)?,
            started: get(row, 5)?,
            boot: get(row, 6)?,
        }),
        None => None,
    };
    let sessions: String = get(row, 7)?;
    Ok(OwnedProcess {
        key: OwnedProcessKey {
            id: get(row, 0)?,
            generation: get(row, 1)?,
        },
        mark: get(row, 2)?,
        leader,
        tool_sessions: serde_json::from_str(&sessions).map_err(OfficeError::storage)?,
        inventory: decode_inventory(&get::<String>(row, 8)?)?,
    })
}

/// Every row (`None`) or the row `key`; one row that can't be decoded fails the whole read.
pub(crate) fn select(
    db: &Connection,
    key: Option<OwnedProcessKey>,
) -> Result<Vec<OwnedProcess>, OfficeError> {
    let (sql, args) = match key {
        Some(key) => (
            format!("SELECT {COLUMNS} WHERE id=?1 AND generation=?2"),
            vec![key.id, key.generation],
        ),
        None => (format!("SELECT {COLUMNS} ORDER BY id"), vec![]),
    };
    let mut statement = db.prepare(&sql).map_err(OfficeError::storage)?;
    let mut rows = statement
        .query(rusqlite::params_from_iter(args))
        .map_err(OfficeError::storage)?;
    let mut found = Vec::new();
    while let Some(row) = rows.next().map_err(OfficeError::storage)? {
        found.push(decode_row(row)?);
    }
    Ok(found)
}

fn current(db: &Connection, key: OwnedProcessKey) -> Result<OwnedProcess, OfficeError> {
    select(db, Some(key))?.pop().ok_or(STALE)
}

pub(crate) fn insert(
    db: &Connection,
    mark: &str,
    now: &str,
) -> Result<OwnedProcessKey, OfficeError> {
    let taken = db.query_row(
        "SELECT 1 FROM owned_processes WHERE mark=?1",
        [mark],
        |_| Ok(()),
    );
    if taken.optional().map_err(OfficeError::storage)?.is_some() {
        return Err(OfficeError::AlreadyExists);
    }
    db.execute(
        "INSERT INTO owned_processes (generation, mark, tool_sessions, inventory, created_at)
         VALUES (1, ?1, '[]', ?2, ?3)",
        (mark, encode_inventory(&[])?, now),
    )
    .map_err(OfficeError::storage)?;
    Ok(OwnedProcessKey {
        id: db.last_insert_rowid(),
        generation: 1,
    })
}

pub(crate) fn set_leader(
    db: &Connection,
    key: OwnedProcessKey,
    leader: &SavedLeader,
) -> Result<(), OfficeError> {
    match current(db, key)?.leader {
        Some(saved) if saved == *leader => return Ok(()),
        Some(_) => return Err(OfficeError::Invalid("a different leader is recorded")),
        None => {}
    }
    let SavedLeader {
        pid,
        pgid,
        started,
        boot,
    } = leader;
    db.execute(
        "UPDATE owned_processes SET leader_pid=?3, leader_pgid=?4, leader_started=?5,
         leader_boot=?6 WHERE id=?1 AND generation=?2",
        (key.id, key.generation, pid, pgid, started, boot),
    )
    .map_err(OfficeError::storage)?;
    Ok(())
}

pub(crate) fn add_tool_session(
    db: &Connection,
    key: OwnedProcessKey,
    session: u32,
) -> Result<(), OfficeError> {
    let mut sessions = current(db, key)?.tool_sessions;
    if !sessions.contains(&session) {
        sessions.push(session);
        let text = serde_json::to_string(&sessions).map_err(OfficeError::storage)?;
        db.execute(
            "UPDATE owned_processes SET tool_sessions=?3 WHERE id=?1 AND generation=?2",
            (key.id, key.generation, text),
        )
        .map_err(OfficeError::storage)?;
    }
    Ok(())
}

/// See `Office::checkpoint_owned_inventory`. The deadline is checked again on every attempt.
pub(crate) fn checkpoint(
    db: &Connection,
    key: OwnedProcessKey,
    mark: &str,
    inventory: &[SavedTarget],
    deadline: Instant,
) -> Result<(), OfficeError> {
    let row = current(db, key)?;
    if row.mark != mark {
        return Err(STALE);
    }
    if !row.inventory.iter().all(|saved| inventory.contains(saved)) {
        return Err(OfficeError::Invalid(
            "the inventory omits or changes a saved entry",
        ));
    }
    missed(deadline)?;
    db.execute(
        "UPDATE owned_processes SET inventory=?3 WHERE id=?1 AND generation=?2",
        (key.id, key.generation, encode_inventory(inventory)?),
    )
    .map_err(OfficeError::storage)?;
    #[cfg(test)]
    crate::owned_processes::tests::pause_before_commit(mark, deadline);
    Ok(())
}

/// After an `Office` call returned: a commit that finished past `deadline` answers TimedOut.
pub(crate) fn committed_in_time(deadline: Instant) -> Result<(), OfficeError> {
    missed(deadline)
}

pub(crate) fn retire(db: &Connection, key: OwnedProcessKey) -> Result<bool, OfficeError> {
    let sql = "DELETE FROM owned_processes WHERE id=?1 AND generation=?2";
    let removed = db
        .execute(sql, (key.id, key.generation))
        .map_err(OfficeError::storage)?;
    Ok(removed == 1)
}
