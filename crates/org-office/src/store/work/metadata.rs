//! Work transaction metadata, frozen at agents f6d4db9 (v5 opener revision 2).

use org_proto::{work::WorkAccepted, ApiError};
use org_work::{NumberScope, StoredResult};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

use crate::OfficeError;

/// Shared by fresh creation and every pre-v5 upgrade; v5 is closed.
pub(crate) fn create_tables(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    let numbered: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM work_items WHERE json_extract(data,'$.number') IS NOT NULL)",
        [],
        |row| row.get(0),
    )?;
    if numbered {
        // Unreleased v4 had no writer. Refuse inconsistent history rather than reuse numbers.
        return Err(rusqlite::Error::InvalidQuery);
    }
    tx.execute_batch(
        "CREATE TABLE work_change_results (
             change_id TEXT PRIMARY KEY CHECK (length(change_id) BETWEEN 1 AND 128),
             params_hash BLOB NOT NULL CHECK (length(params_hash) = 32),
             answer TEXT NOT NULL,
             expires_at_ms INTEGER NOT NULL CHECK (expires_at_ms >= 0),
             principal_account TEXT, principal_member TEXT,
             actor_kind TEXT CHECK (actor_kind IS NULL OR length(actor_kind) > 0)
         ) STRICT, WITHOUT ROWID;
         CREATE INDEX idx_work_change_results_expiry ON work_change_results(expires_at_ms);
         CREATE TABLE work_number_counters (
             scope TEXT PRIMARY KEY, last INTEGER NOT NULL CHECK (last >= 0)
         ) STRICT, WITHOUT ROWID;
         CREATE TABLE work_clock (
             id INTEGER PRIMARY KEY CHECK (id = 1),
             high_water_ms INTEGER NOT NULL CHECK (high_water_ms >= 0)
         ) STRICT;",
    )
}

// Deliberately define the lowercase storage envelope, independently of Result's Ok/Err.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Answer<T, E> {
    Ok(T),
    Err(E),
}

pub(super) fn put_change_result(
    db: &Connection,
    change_id: &str,
    result: &StoredResult,
) -> Result<(), OfficeError> {
    let answer = match &result.answer {
        Ok(value) => Answer::Ok(value),
        Err(error) => Answer::Err(error),
    };
    let answer = serde_json::to_string(&answer).map_err(OfficeError::storage)?;
    db.execute(
        "INSERT INTO work_change_results
         (change_id,params_hash,answer,expires_at_ms,principal_account,principal_member,actor_kind)
         VALUES (?1,?2,?3,?4,NULL,NULL,NULL)",
        params![
            change_id,
            result.params_hash.as_slice(),
            answer,
            super::integer(result.expires_at_ms)?
        ],
    )
    .map_err(OfficeError::storage)?;
    Ok(())
}

pub(super) fn change_result(
    db: &Connection,
    change_id: &str,
) -> Result<Option<StoredResult>, OfficeError> {
    let row: Option<(Vec<u8>, String, i64)> = db
        .query_row(
            "SELECT params_hash,answer,expires_at_ms FROM work_change_results WHERE change_id=?1",
            [change_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(OfficeError::storage)?;
    row.map(|(hash, answer, expires_at_ms)| {
        let params_hash = hash
            .try_into()
            .map_err(|_| OfficeError::Invalid("work_change_results contains an invalid hash"))?;
        let answer: Answer<WorkAccepted, ApiError> = serde_json::from_str(&answer)
            .map_err(|_| OfficeError::Invalid("work_change_results contains an invalid answer"))?;
        Ok(StoredResult {
            params_hash,
            answer: match answer {
                Answer::Ok(value) => Ok(value),
                Answer::Err(error) => Err(error),
            },
            expires_at_ms: u64::try_from(expires_at_ms).map_err(|_| {
                OfficeError::Invalid("work_change_results contains an invalid expiry")
            })?,
        })
    })
    .transpose()
}

pub(super) fn allocate_number(db: &Connection, scope: &NumberScope) -> Result<u64, OfficeError> {
    if let NumberScope::Tasks { project_id } | NumberScope::Bugs { project_id } = scope {
        if project_id.is_empty() || project_id.contains(':') {
            return Err(OfficeError::Invalid("invalid work number scope"));
        }
    }
    let value: i64 = db
        .query_row(
            "INSERT INTO work_number_counters(scope,last) VALUES (?1,1)
         ON CONFLICT(scope) DO UPDATE SET last=last+1
         RETURNING last",
            [scope.key()],
            |row| row.get(0),
        )
        .map_err(OfficeError::storage)?;
    u64::try_from(value)
        .map_err(|_| OfficeError::Invalid("work_number_counters contains an invalid number"))
}

pub(super) fn raise_high_water(db: &Connection, ms: u64) -> Result<(), OfficeError> {
    db.execute(
        "INSERT INTO work_clock(id,high_water_ms) VALUES (1,?1)
         ON CONFLICT(id) DO UPDATE SET high_water_ms=MAX(high_water_ms,excluded.high_water_ms)",
        [super::integer(ms)?],
    )
    .map_err(OfficeError::storage)?;
    Ok(())
}

pub(super) fn high_water(db: &Connection) -> Result<u64, OfficeError> {
    let value: Option<i64> = db
        .query_row(
            "SELECT high_water_ms FROM work_clock WHERE id=1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(OfficeError::storage)?;
    u64::try_from(value.unwrap_or(0))
        .map_err(|_| OfficeError::Invalid("work_clock contains an invalid high water"))
}
