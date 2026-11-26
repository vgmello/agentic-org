//! SQLite projection of the durable replication state (`TASK-52`, `FC-145`, `ADR-0018` `ENG-3`).
//!
//! The durable source of truth, `<durable>/local/offices/<office_id>/replication/state.v1.json`,
//! is engine-independent: [`DurableReplicationStore`] keeps it for every engine (`TASK-172`).
//! This adapter adds the rebuildable SQLite projection at
//! `<runtime>/<office_id>/protected/replication.db`, reconciled via
//! [`super::companion::open_and_reconcile_companion_db`].
//!
//! [`DurableReplicationStore`]: crate::replication::durable::DurableReplicationStore

use std::path::Path;

use org_sync_proto::OfficeId;
use rusqlite::Connection;

use super::companion::open_and_reconcile_companion_db;
use crate::error::OfficeError;
use crate::private_fs::{private_dir, tighten_dir};
use crate::replication::{durable::ReplicationProjection, memory::ReplicationCoreData};

mod projection;
#[cfg(test)]
mod tests;

use self::projection::{
    project_into_sqlite, REPLICATION_ADAPTER_FORMAT, REPLICATION_DDL, REPLICATION_PRAGMAS,
};

/// The SQLite engine's [`ReplicationProjection`]: `replication.db` under the office's
/// `protected/` runtime folder.
pub(crate) struct SqliteReplicationProjection {
    conn: Connection,
}

impl SqliteReplicationProjection {
    /// Opens `<runtime_root>/<local_office>/protected/replication.db`, rebuilding it from
    /// `data` whenever it is missing, empty, corrupt or schema-mismatched.
    pub(crate) fn open(
        runtime_root: &Path,
        local_office: &OfficeId,
        data: &ReplicationCoreData,
    ) -> Result<Self, OfficeError> {
        let protected_dir = runtime_root.join(local_office.as_str()).join("protected");
        private_dir(&protected_dir).map_err(OfficeError::storage)?;
        tighten_dir(&protected_dir).map_err(OfficeError::storage)?;

        let db_path = protected_dir.join("replication.db");
        let conn = open_and_reconcile_companion_db(
            &db_path,
            "replication",
            REPLICATION_ADAPTER_FORMAT,
            REPLICATION_DDL,
            REPLICATION_PRAGMAS,
            |conn| project_into_sqlite(conn, data),
        )?;
        Ok(Self { conn })
    }
}

impl ReplicationProjection for SqliteReplicationProjection {
    fn project(&mut self, data: &ReplicationCoreData) -> Result<(), OfficeError> {
        project_into_sqlite(&mut self.conn, data)
    }
}
