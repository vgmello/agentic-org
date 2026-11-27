//! The rebuildable `replication.db` projection: its format, pragmas, DDL and the
//! projection of the replication core into it.

use std::collections::BTreeMap;

use org_sync::organization::OrgEvent;
use rusqlite::{params, types::ToSql, Connection, Transaction};

use org_office::replication::{durable::appended_events, memory::ReplicationCoreData};
use org_office::OfficeError;

pub(super) const REPLICATION_ADAPTER_FORMAT: u32 = 1;
pub(super) const REPLICATION_PRAGMAS: &str = "
PRAGMA journal_mode = WAL;
PRAGMA synchronous = FULL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
";

pub(super) const REPLICATION_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS replication_meta (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    org_id TEXT NOT NULL,
    local_office TEXT NOT NULL,
    hq_office TEXT NOT NULL,
    epoch INTEGER NOT NULL,
    seq INTEGER NOT NULL,
    highest_epoch INTEGER NOT NULL,
    protocol_level INTEGER NOT NULL,
    restore_pending INTEGER NOT NULL,
    initial_snapshot_required INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS replication_org_events (
    epoch INTEGER NOT NULL,
    seq INTEGER NOT NULL,
    digest_hex TEXT NOT NULL,
    codec_name TEXT NOT NULL,
    codec_version INTEGER NOT NULL,
    PRIMARY KEY (epoch, seq)
) STRICT;

CREATE TABLE IF NOT EXISTS replication_work_outbox (
    seq INTEGER PRIMARY KEY,
    record_id TEXT NOT NULL,
    digest_hex TEXT NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS replication_message_outbox (
    message_id TEXT PRIMARY KEY,
    destination_office TEXT NOT NULL,
    deadline_ms INTEGER NOT NULL
) STRICT;
"#;

/// Rebuilds every table from `data`: at open, after a failed projection, and whenever a
/// change cannot be projected incrementally.
pub(super) fn project_into_sqlite(
    conn: &mut Connection,
    data: &ReplicationCoreData,
) -> Result<(), OfficeError> {
    let tx = conn.transaction().map_err(OfficeError::storage)?;
    write_meta(&tx, data)?;
    tx.execute("DELETE FROM replication_org_events", [])
        .map_err(OfficeError::storage)?;
    for ev in data.events.values() {
        insert_event(&tx, ev)?;
    }
    sync_rows(&tx, &WORK_ROWS, None, &work_rows(data))?;
    sync_rows(&tx, &MESSAGE_ROWS, None, &message_rows(data))?;
    tx.commit().map_err(OfficeError::storage)
}

/// Brings the tables from `before`, the state they hold, to `after` in one transaction,
/// touching only the rows that changed (`TASK-140`): new events are inserted, a cleared or
/// replaced history is rewritten, and the small outbox tables are diffed by key, so a commit
/// no longer deletes and re-inserts every row.
pub(super) fn project_change_into_sqlite(
    conn: &mut Connection,
    before: &ReplicationCoreData,
    after: &ReplicationCoreData,
) -> Result<(), OfficeError> {
    let tx = conn.transaction().map_err(OfficeError::storage)?;
    if meta_of(before) != meta_of(after) {
        write_meta(&tx, after)?;
    }
    match appended_events(before, after) {
        Some(added) => {
            for ev in added {
                insert_event(&tx, ev)?;
            }
        }
        None => {
            tx.execute("DELETE FROM replication_org_events", [])
                .map_err(OfficeError::storage)?;
            for ev in after.events.values() {
                insert_event(&tx, ev)?;
            }
        }
    }
    sync_rows(&tx, &WORK_ROWS, Some(&work_rows(before)), &work_rows(after))?;
    sync_rows(
        &tx,
        &MESSAGE_ROWS,
        Some(&message_rows(before)),
        &message_rows(after),
    )?;
    tx.commit().map_err(OfficeError::storage)
}

type Meta<'a> = (&'a str, &'a str, &'a str, i64, i64, i64, i64, i64, i64);

fn meta_of(data: &ReplicationCoreData) -> Meta<'_> {
    let cp = &data.checkpoint;
    (
        cp.org_id.as_str(),
        cp.local_office.as_str(),
        cp.hq_office.as_str(),
        cp.position.epoch.get() as i64,
        cp.position.seq as i64,
        cp.highest_epoch.get() as i64,
        i64::from(cp.protocol_level),
        i64::from(cp.restore_pending),
        i64::from(data.initial_snapshot_required),
    )
}

fn write_meta(tx: &Transaction<'_>, data: &ReplicationCoreData) -> Result<(), OfficeError> {
    let m = meta_of(data);
    tx.execute("DELETE FROM replication_meta", [])
        .map_err(OfficeError::storage)?;
    tx.execute(
        "INSERT INTO replication_meta \
         (id, org_id, local_office, hq_office, epoch, seq, highest_epoch, protocol_level, \
          restore_pending, initial_snapshot_required) \
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![m.0, m.1, m.2, m.3, m.4, m.5, m.6, m.7, m.8],
    )
    .map_err(OfficeError::storage)?;
    Ok(())
}

fn insert_event(tx: &Transaction<'_>, ev: &OrgEvent) -> Result<(), OfficeError> {
    tx.prepare_cached(
        "INSERT INTO replication_org_events (epoch, seq, digest_hex, codec_name, codec_version) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .and_then(|mut stmt| {
        stmt.execute(params![
            ev.position.epoch.get() as i64,
            ev.position.seq as i64,
            ev.digest.to_hex(),
            ev.codec.id(),
            i64::from(ev.codec.version()),
        ])
    })
    .map_err(OfficeError::storage)?;
    Ok(())
}

/// A keyed outbox table: its rows are `(key, a, b)`.
struct KeyedTable {
    clear: &'static str,
    delete: &'static str,
    insert: &'static str,
}

const WORK_ROWS: KeyedTable = KeyedTable {
    clear: "DELETE FROM replication_work_outbox",
    delete: "DELETE FROM replication_work_outbox WHERE seq = ?1",
    insert: "INSERT INTO replication_work_outbox (seq, record_id, digest_hex) VALUES (?1, ?2, ?3)",
};

const MESSAGE_ROWS: KeyedTable = KeyedTable {
    clear: "DELETE FROM replication_message_outbox",
    delete: "DELETE FROM replication_message_outbox WHERE message_id = ?1",
    insert: "INSERT INTO replication_message_outbox (message_id, destination_office, deadline_ms) \
             VALUES (?1, ?2, ?3)",
};

fn work_rows(data: &ReplicationCoreData) -> BTreeMap<i64, (String, String)> {
    data.work_outbox
        .pending()
        .iter()
        .map(|c| {
            let row = (c.record_id.clone(), c.digest.to_hex());
            (c.key.seq.get() as i64, row)
        })
        .collect()
}

fn message_rows(data: &ReplicationCoreData) -> BTreeMap<&str, (&str, i64)> {
    data.message_queue
        .pending
        .iter()
        .map(|m| {
            let row = (m.destination.as_str(), m.deadline_ms as i64);
            (m.id.as_str(), row)
        })
        .collect()
}

/// Deletes the rows `before` has that `after` lacks or changes, then inserts the rows `after`
/// adds or changes. Without a `before` the table is cleared and refilled: a full rebuild.
fn sync_rows<K, A, B>(
    tx: &Transaction<'_>,
    table: &KeyedTable,
    before: Option<&BTreeMap<K, (A, B)>>,
    after: &BTreeMap<K, (A, B)>,
) -> Result<(), OfficeError>
where
    K: Ord + ToSql,
    A: PartialEq + ToSql,
    B: PartialEq + ToSql,
{
    let empty = BTreeMap::new();
    let before = match before {
        Some(before) => before,
        None => {
            tx.execute(table.clear, []).map_err(OfficeError::storage)?;
            &empty
        }
    };
    for (key, row) in before {
        if after.get(key) != Some(row) {
            tx.prepare_cached(table.delete)
                .and_then(|mut stmt| stmt.execute(params![key]))
                .map_err(OfficeError::storage)?;
        }
    }
    for (key, row) in after {
        if before.get(key) != Some(row) {
            tx.prepare_cached(table.insert)
                .and_then(|mut stmt| stmt.execute(params![key, row.0, row.1]))
                .map_err(OfficeError::storage)?;
        }
    }
    Ok(())
}
