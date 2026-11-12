//! Native current work state, immutable events, and permanent ID claims.
//! All writes run on the office writer; callers propagate errors so its savepoint
//! rolls back item, ID, event and feed writes together.

use std::collections::BTreeSet;

use org_proto::work::WorkEvent;
use org_work::StoredItem;
use rusqlite::{params, Connection, OptionalExtension};

use crate::OfficeError;

pub(crate) mod assembly;
mod call;
mod metadata;
pub(crate) use metadata::create_tables as create_v5_tables;

pub(crate) fn create_tables(tx: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE work_items (
             id TEXT PRIMARY KEY, generation INTEGER NOT NULL CHECK (generation >= 1),
             version INTEGER NOT NULL CHECK (version >= 1), data TEXT NOT NULL,
             CHECK (json_extract(data, '$.id') IS id
                 AND json_extract(data, '$.generation') IS generation
                 AND json_extract(data, '$.version') IS version)
         ) STRICT, WITHOUT ROWID;
         CREATE TABLE work_ids (
             id TEXT PRIMARY KEY, item_id TEXT NOT NULL REFERENCES work_items(id),
             kind TEXT NOT NULL CHECK (kind IN ('item', 'step', 'link', 'comment'))
         ) STRICT, WITHOUT ROWID;
         CREATE INDEX idx_work_ids_item ON work_ids(item_id);
         CREATE TABLE work_events (
             target_id TEXT NOT NULL REFERENCES work_items(id),
             version INTEGER NOT NULL CHECK (version >= 1),
             feed_seq INTEGER NOT NULL CHECK (feed_seq >= 1) REFERENCES work_feed(seq),
             data TEXT NOT NULL,
             CHECK (json_extract(data, '$.target_id') IS target_id
                 AND json_extract(data, '$.version') IS version),
             PRIMARY KEY (target_id, version)
         ) STRICT, WITHOUT ROWID;
         CREATE TRIGGER work_events_no_replace BEFORE INSERT ON work_events
             WHEN EXISTS (SELECT 1 FROM work_events
                 WHERE target_id = NEW.target_id AND version = NEW.version)
             BEGIN SELECT RAISE(ABORT, 'work events are immutable'); END;
         CREATE TRIGGER work_events_no_update BEFORE UPDATE ON work_events
             BEGIN SELECT RAISE(ABORT, 'work events are immutable'); END;
         CREATE TRIGGER work_events_no_delete BEFORE DELETE ON work_events
             BEGIN SELECT RAISE(ABORT, 'work events are immutable'); END;",
    )
}

fn integer(value: u64) -> Result<i64, OfficeError> {
    i64::try_from(value).map_err(|_| OfficeError::Invalid("work integer is out of range"))
}

fn last_version(previous: u64, count: usize) -> Result<u64, OfficeError> {
    previous
        .checked_add(count as u64)
        .filter(|n| *n <= i64::MAX as u64)
        .ok_or(OfficeError::Invalid("work version is exhausted"))
}

fn ids(item: &StoredItem) -> Result<Vec<(&str, &str)>, OfficeError> {
    let ids = std::iter::once((item.id.as_str(), "item"))
        .chain(item.checklist.iter().map(|s| (s.id.as_str(), "step")))
        .chain(item.links.iter().map(|l| (l.id.as_str(), "link")))
        .chain(item.comments.iter().map(|c| (c.id.as_str(), "comment")));
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for (id, kind) in ids {
        if !org_work::is_ulid(id) || !seen.insert(id) {
            return Err(OfficeError::Invalid("invalid or duplicate work id"));
        }
        result.push((id, kind));
    }
    Ok(result)
}

fn validate_item(item: &StoredItem) -> Result<Vec<(&str, &str)>, OfficeError> {
    if item.generation == 0 || item.version == 0 {
        return Err(OfficeError::Invalid(
            "invalid work item version or generation",
        ));
    }
    integer(item.generation)?;
    integer(item.version)?;
    ids(item)
}

fn validate_event(event: &WorkEvent) -> Result<(), OfficeError> {
    if !org_work::is_ulid(&event.target_id) || event.version == 0 {
        return Err(OfficeError::Invalid("invalid work event identity"));
    }
    integer(event.version)?;
    integer(event.org_position.epoch)?;
    integer(event.org_position.seq)?;
    // The parser accepts exactly a four-digit year, so its result fits SQLite's integer.
    org_work::parse_ms(&event.at).ok_or(OfficeError::Invalid("invalid work event timestamp"))?;
    Ok(())
}

/// Reads one native item inside a caller-held office.db transaction.
pub(crate) fn get_in(db: &Connection, id: &str) -> Result<Option<StoredItem>, OfficeError> {
    get(db, id)
}

/// Composes the item and its ID index from one caller-held transaction.
fn get(db: &Connection, id: &str) -> Result<Option<StoredItem>, OfficeError> {
    if db.is_autocommit() {
        return Err(OfficeError::Invalid(
            "work item read requires a transaction",
        ));
    }
    let row: Option<(i64, i64, String, Option<i64>)> = db
        .query_row(
            "SELECT generation, version, data,
                (SELECT max(version) FROM work_events WHERE target_id = work_items.id)
             FROM work_items WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(OfficeError::storage)?;
    row.map(|(generation, version, data, last_event)| {
        let item: StoredItem = serde_json::from_str(&data).map_err(OfficeError::storage)?;
        let mut expected = validate_item(&item)?;
        if item.id != id
            || integer(item.generation)? != generation
            || integer(item.version)? != version
            || last_event != Some(version)
        {
            return Err(OfficeError::Invalid("stored work item is inconsistent"));
        }
        expected.sort_unstable();
        let mut query = db
            .prepare("SELECT id, kind FROM work_ids WHERE item_id = ?1 ORDER BY id, kind")
            .map_err(OfficeError::storage)?;
        let indexed = query
            .query_map([id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(OfficeError::storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(OfficeError::storage)?;
        if !indexed
            .iter()
            .map(|(id, kind)| (id.as_str(), kind.as_str()))
            .eq(expected)
        {
            return Err(OfficeError::Invalid(
                "stored work id claims are inconsistent",
            ));
        }
        Ok(item)
    })
    .transpose()
}

/// The feed is allocated lazily by the later WorkTxn adapter before its first put;
/// record_change finalizes that feed and appends origin rows in the same savepoint.
fn put_item(
    db: &Connection,
    item: &StoredItem,
    events: &[WorkEvent],
    feed_seq: u64,
) -> Result<(), OfficeError> {
    if db.is_autocommit() {
        return Err(OfficeError::Invalid(
            "work item write requires a writer transaction",
        ));
    }
    if feed_seq == 0 {
        return Err(OfficeError::Invalid("work item write requires a feed row"));
    }
    let feed_seq = integer(feed_seq)?;
    let claimed = validate_item(item)?;
    let previous = get(db, &item.id)?;
    let version = previous.as_ref().map_or(0, |item| item.version);
    let last = last_version(version, events.len())?;
    if events.is_empty()
        || item.version != last
        || events
            .iter()
            .enumerate()
            .any(|(i, e)| e.target_id != item.id || e.version != version + 1 + i as u64)
    {
        return Err(OfficeError::Invalid("work versions must be contiguous"));
    }
    for event in events {
        validate_event(event)?;
    }
    if let Some(old) = &previous {
        if ids(old)?.iter().any(|id| !claimed.contains(id)) {
            return Err(OfficeError::Invalid("work ids cannot be forgotten"));
        }
    }
    db.execute("INSERT INTO work_items (id, generation, version, data) VALUES (?1, ?2, ?3, ?4)
        ON CONFLICT(id) DO UPDATE SET generation=excluded.generation, version=excluded.version, data=excluded.data",
        params![item.id, integer(item.generation)?, integer(item.version)?, serde_json::to_string(item).map_err(OfficeError::storage)?])
        .map_err(OfficeError::storage)?;
    for (id, kind) in claimed {
        claim_id(db, id, &item.id, kind)?;
    }
    for event in events {
        db.execute(
            "INSERT INTO work_events (target_id, version, feed_seq, data) VALUES (?1, ?2, ?3, ?4)",
            params![
                item.id,
                integer(event.version)?,
                feed_seq,
                serde_json::to_string(event).map_err(OfficeError::storage)?
            ],
        )
        .map_err(OfficeError::storage)?;
    }
    Ok(())
}

/// Reserve an ID for one item and role; a repeated claim must match both.
fn claim_id(db: &Connection, id: &str, item: &str, kind: &str) -> Result<(), OfficeError> {
    let written = db
        .execute(
            "INSERT INTO work_ids (id, item_id, kind) VALUES (?1, ?2, ?3)
        ON CONFLICT(id) DO UPDATE SET item_id=excluded.item_id
        WHERE work_ids.item_id=excluded.item_id AND work_ids.kind=excluded.kind",
            params![id, item, kind],
        )
        .map_err(OfficeError::storage)?;
    if written != 1 {
        return Err(OfficeError::Invalid("work id is already claimed"));
    }
    Ok(())
}

fn owner_of(db: &Connection, id: &str) -> Result<Option<(String, String)>, OfficeError> {
    db.query_row(
        "SELECT item_id, kind FROM work_ids WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(OfficeError::storage)
}

/// Immutable event records with their committed call's feed sequence, oldest first.
fn event_records(
    db: &Connection,
    target: &str,
    after: u64,
) -> Result<Vec<(u64, WorkEvent)>, OfficeError> {
    let mut query = db
        .prepare(
            "SELECT version, feed_seq, data FROM work_events
        WHERE target_id = ?1 AND version > ?2 ORDER BY version",
        )
        .map_err(OfficeError::storage)?;
    let rows = query
        .query_map(params![target, integer(after)?], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(OfficeError::storage)?;
    rows.map(|row| {
        let (version, feed, data) = row.map_err(OfficeError::storage)?;
        let event: WorkEvent = serde_json::from_str(&data).map_err(OfficeError::storage)?;
        validate_event(&event)?;
        if event.target_id != target || integer(event.version)? != version || feed <= 0 {
            return Err(OfficeError::Invalid("stored work event is inconsistent"));
        }
        Ok((feed as u64, event))
    })
    .collect()
}

#[cfg(test)]
mod tests;
