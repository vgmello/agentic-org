//! The rebuildable `replication.db` projection: its format, pragmas, DDL and the
//! projection of the replication core into it.

use rusqlite::{params, Connection};

use crate::error::OfficeError;
use crate::replication::memory::ReplicationCoreData;

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

pub(super) fn project_into_sqlite(
    conn: &mut Connection,
    data: &ReplicationCoreData,
) -> Result<(), OfficeError> {
    let tx = conn.transaction().map_err(OfficeError::storage)?;
    let cp = &data.checkpoint;
    tx.execute("DELETE FROM replication_meta", [])
        .map_err(OfficeError::storage)?;
    tx.execute(
        "INSERT INTO replication_meta \
         (id, org_id, local_office, hq_office, epoch, seq, highest_epoch, protocol_level, \
          restore_pending, initial_snapshot_required) \
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            cp.org_id.as_str(),
            cp.local_office.as_str(),
            cp.hq_office.as_str(),
            cp.position.epoch.get() as i64,
            cp.position.seq as i64,
            cp.highest_epoch.get() as i64,
            i64::from(cp.protocol_level),
            i64::from(cp.restore_pending),
            i64::from(data.initial_snapshot_required),
        ],
    )
    .map_err(OfficeError::storage)?;

    tx.execute("DELETE FROM replication_org_events", [])
        .map_err(OfficeError::storage)?;
    for ev in data.events.values() {
        tx.execute(
            "INSERT INTO replication_org_events (epoch, seq, digest_hex, codec_name, codec_version) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                ev.position.epoch.get() as i64,
                ev.position.seq as i64,
                ev.digest.to_hex(),
                ev.codec.id(),
                i64::from(ev.codec.version()),
            ],
        )
        .map_err(OfficeError::storage)?;
    }

    tx.execute("DELETE FROM replication_work_outbox", [])
        .map_err(OfficeError::storage)?;
    for change in data.work_outbox.pending() {
        tx.execute(
            "INSERT INTO replication_work_outbox (seq, record_id, digest_hex) VALUES (?1, ?2, ?3)",
            params![
                change.key.seq.get() as i64,
                &change.record_id,
                change.digest.to_hex(),
            ],
        )
        .map_err(OfficeError::storage)?;
    }

    tx.execute("DELETE FROM replication_message_outbox", [])
        .map_err(OfficeError::storage)?;
    for msg in &data.message_queue.pending {
        tx.execute(
            "INSERT INTO replication_message_outbox (message_id, destination_office, deadline_ms) \
             VALUES (?1, ?2, ?3)",
            params![&msg.id, msg.destination.as_str(), msg.deadline_ms as i64],
        )
        .map_err(OfficeError::storage)?;
    }
    tx.commit().map_err(OfficeError::storage)
}
