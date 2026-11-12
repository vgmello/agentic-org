//! Shared office origin identity and the private local work-version ledger.
//!
//! Work has a gap-free sequence per incarnation, independent of message_sequence.
//! Only record_change appends, in the same writer savepoint as its item and feed rows.
//! Restore rotates meta.incarnation; old rows and their counters remain available.

use std::collections::BTreeMap;
use std::ops::RangeInclusive;

use org_proto::Position;
use rusqlite::{params, Connection, OptionalExtension};

use crate::OfficeError;

const INCARNATION_KEY: &str = "incarnation";

pub(crate) fn create_tables(tx: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE work_feed (
             seq INTEGER PRIMARY KEY AUTOINCREMENT,
             change_id TEXT, targets TEXT NOT NULL
         ) STRICT;
         CREATE TABLE work_origin_counter (
             origin_incarnation TEXT PRIMARY KEY,
             last_seq INTEGER NOT NULL CHECK (last_seq >= 0)
         ) STRICT, WITHOUT ROWID;
         CREATE TABLE work_origin_log (
             origin_office_id TEXT NOT NULL, origin_incarnation TEXT NOT NULL,
             origin_seq INTEGER NOT NULL CHECK (origin_seq >= 1),
             feed_seq INTEGER NOT NULL REFERENCES work_feed(seq),
             target_id TEXT NOT NULL, generation INTEGER NOT NULL CHECK (generation >= 1),
             version INTEGER NOT NULL CHECK (version >= 0),
             org_epoch INTEGER NOT NULL CHECK (org_epoch >= 0),
             org_seq INTEGER NOT NULL CHECK (org_seq >= 0),
             committed_at_ms INTEGER NOT NULL CHECK (committed_at_ms >= 0),
             UNIQUE (target_id, generation, version),
             PRIMARY KEY (origin_office_id, origin_incarnation, origin_seq)
         ) STRICT, WITHOUT ROWID;
         CREATE INDEX idx_work_origin_stream ON work_origin_log(origin_incarnation, origin_seq);
         CREATE INDEX idx_work_origin_target_version ON work_origin_log(target_id, version);",
    )
}

/// This office.db's incarnation: a ULID made the first time it is asked for and kept in `meta`.
/// Message and work origins both use (office, incarnation, origin_seq). The value lives inside
/// the file, so a restore brings the old one back: whoever restores must make a new one (that
/// is the restore tool's job, not this function's). Runs on the writer.
pub(crate) fn incarnation(db: &Connection) -> Result<String, OfficeError> {
    let existing: Option<String> = db
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            [INCARNATION_KEY],
            |row| row.get(0),
        )
        .optional()
        .map_err(OfficeError::storage)?;
    if let Some(existing) = existing {
        return Ok(existing);
    }
    let made = crate::ulid::generate().map_err(OfficeError::storage)?;
    db.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)",
        (INCARNATION_KEY, &made),
    )
    .map_err(OfficeError::storage)?;
    Ok(made)
}

/// Shared by messages and work; a restore tool rotates only the incarnation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalOrigin {
    pub(crate) office_id: String,
    pub(crate) incarnation: String,
}

/// The committed version, enriched with the stored item's ownership generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OriginRef {
    pub(crate) target_id: String,
    pub(crate) generation: u64,
    pub(crate) version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OriginEntry {
    pub(crate) origin: LocalOrigin,
    pub(crate) seq: u64,
    pub(crate) feed_seq: u64,
    pub(crate) target: OriginRef,
    pub(crate) org_position: Position,
    pub(crate) committed_at_ms: u64,
}

pub(crate) fn local_origin(db: &Connection) -> Result<LocalOrigin, OfficeError> {
    let office_id: String = db
        .query_row(
            "SELECT value FROM meta WHERE key = 'office_id'",
            [],
            |row| row.get(0),
        )
        .map_err(OfficeError::storage)?;
    let origin = LocalOrigin {
        office_id,
        incarnation: incarnation(db)?,
    };
    validate_origin(&origin)?;
    Ok(origin)
}

pub(crate) fn high_water(db: &Connection, incarnation: &str) -> Result<u64, OfficeError> {
    db.query_row(
        "SELECT last_seq FROM work_origin_counter WHERE origin_incarnation = ?1",
        [incarnation],
        |row| read_integer(row, 0),
    )
    .optional()
    .map(|value| value.unwrap_or(0))
    .map_err(OfficeError::storage)
}

/// At most 1,024 references, in sequence order, within one incarnation.
pub(crate) fn entries_after(
    db: &Connection,
    incarnation: &str,
    after: u64,
    limit: u32,
) -> Result<Vec<OriginEntry>, OfficeError> {
    let mut query = db
        .prepare(
            "SELECT origin_office_id, origin_incarnation, origin_seq, feed_seq, target_id,
                generation, version, org_epoch, org_seq, committed_at_ms
         FROM work_origin_log WHERE origin_incarnation = ?1 AND origin_seq > ?2
         ORDER BY origin_seq LIMIT ?3",
        )
        .map_err(OfficeError::storage)?;
    let rows = query
        .query_map(
            params![incarnation, integer(after)?, limit.min(1024)],
            |row| {
                Ok(OriginEntry {
                    origin: LocalOrigin {
                        office_id: row.get(0)?,
                        incarnation: row.get(1)?,
                    },
                    seq: read_integer(row, 2)?,
                    feed_seq: read_integer(row, 3)?,
                    target: OriginRef {
                        target_id: row.get(4)?,
                        generation: read_integer(row, 5)?,
                        version: read_integer(row, 6)?,
                    },
                    org_position: Position {
                        epoch: read_integer(row, 7)?,
                        seq: read_integer(row, 8)?,
                    },
                    committed_at_ms: read_integer(row, 9)?,
                })
            },
        )
        .map_err(OfficeError::storage)?;
    let entries = rows
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(OfficeError::storage)?;
    for entry in &entries {
        validate_origin(&entry.origin)?;
        validate_ref(&entry.target)?;
    }
    Ok(entries)
}

/// Called only by record_change inside the office writer's savepoint, after put_item.
/// The caller propagates any error so its item/feed/log/counter writes roll back together.
/// The next WorkTxn slice supplies this integration; this capability stays private.
pub(crate) fn append(
    db: &Connection,
    position: Position,
    refs: &[OriginRef],
    feed_seq: u64,
    at_ms: u64,
) -> Result<RangeInclusive<u64>, OfficeError> {
    if db.is_autocommit() {
        return Err(OfficeError::Invalid(
            "work origin append requires a writer transaction",
        ));
    }
    if refs.is_empty() || feed_seq == 0 {
        return Err(OfficeError::Invalid(
            "work origin append requires versions and a feed row",
        ));
    }
    for value in [feed_seq, at_ms, position.epoch, position.seq] {
        integer(value)?;
    }
    let mut seen = BTreeMap::new();
    for item in refs {
        validate_ref(item)?;
        if seen
            .insert(&item.target_id, item.version)
            .is_some_and(|previous| item.version <= previous)
        {
            return Err(OfficeError::Invalid(
                "work origin versions must advance within an append",
            ));
        }
    }
    let origin = local_origin(db)?;
    let previous = high_water(db, &origin.incarnation)?;
    let last = previous
        .checked_add(refs.len() as u64)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or(OfficeError::Invalid("work origin sequence is exhausted"))?;
    // Every input and the final sequence fit SQLite's signed integer above.
    for (offset, item) in refs.iter().enumerate() {
        db.execute(
            "INSERT INTO work_origin_log
             (origin_office_id, origin_incarnation, origin_seq, feed_seq, target_id,
              generation, version, org_epoch, org_seq, committed_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                origin.office_id,
                origin.incarnation,
                (previous + offset as u64 + 1) as i64,
                feed_seq as i64,
                item.target_id,
                item.generation as i64,
                item.version as i64,
                position.epoch as i64,
                position.seq as i64,
                at_ms as i64,
            ],
        )
        .map_err(OfficeError::storage)?;
    }
    db.execute(
        "INSERT INTO work_origin_counter (origin_incarnation, last_seq) VALUES (?1, ?2)
         ON CONFLICT(origin_incarnation) DO UPDATE SET last_seq = excluded.last_seq",
        params![origin.incarnation, last as i64],
    )
    .map_err(OfficeError::storage)?;
    Ok(previous + 1..=last)
}

fn integer(value: u64) -> Result<i64, OfficeError> {
    i64::try_from(value).map_err(|_| OfficeError::Invalid("work origin integer is out of range"))
}

fn validate_origin(origin: &LocalOrigin) -> Result<(), OfficeError> {
    if !crate::office_file::is_plain_id(&origin.office_id)
        || crate::ulid::time_ms(&origin.incarnation).is_none()
    {
        return Err(OfficeError::Invalid("invalid work origin identity"));
    }
    Ok(())
}

fn validate_ref(item: &OriginRef) -> Result<(), OfficeError> {
    if crate::ulid::time_ms(&item.target_id).is_none() || item.generation == 0 {
        return Err(OfficeError::Invalid("invalid work origin reference"));
    }
    integer(item.generation)?;
    integer(item.version)?;
    Ok(())
}

fn read_integer(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<u64> {
    let value = row.get::<_, i64>(column)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(column, value))
}

#[cfg(test)]
mod tests;
