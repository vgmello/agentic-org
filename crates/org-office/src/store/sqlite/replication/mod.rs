//! SQLite + durable-JSON adapter for [`crate::replication::ReplicationStore`]
//! (`TASK-52`, `FC-145`, `ADR-0018` `ENG-3`).
//!
//! Durable source of truth lives under `<durable>/local/offices/<office_id>/replication/state.v1.json`
//! (written atomically via `full_sync` + parent-dir sync). The rebuildable SQLite projection lives
//! at `<runtime>/<office_id>/protected/replication.db` and is reconciled via
//! [`super::companion::open_and_reconcile_companion_db`].

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use org_sync_proto::{Digest, Epoch, OfficeId, OrgId, OriginIncarnation};
use rusqlite::Connection;

use super::companion::open_and_reconcile_companion_db;
use crate::error::OfficeError;
use crate::private_fs::{private_dir, private_dir_synced, tighten_dir, write_private_atomic};
use crate::replication::{memory::ReplicationCoreData, ReplicationStateFile};

mod ports;
mod projection;
mod state_file;
#[cfg(test)]
mod tests;

use self::projection::{
    project_into_sqlite, REPLICATION_ADAPTER_FORMAT, REPLICATION_DDL, REPLICATION_PRAGMAS,
};
use self::state_file::{core_from_file, encode_state_file};

struct SqliteReplicationInner {
    data: ReplicationCoreData,
    conn: Connection,
    /// True if a prior SQLite projection failed after the durable JSON commit point succeeded;
    /// reconciled before serving any subsequent mutation (`TASK-52` review F19).
    projection_dirty: bool,
}

pub struct SqliteReplicationStore {
    state_path: PathBuf,
    inner: Mutex<SqliteReplicationInner>,
}

pub struct OpenReplicationParams<'a> {
    pub durable_root: &'a Path,
    pub runtime_root: &'a Path,
    pub org_id: OrgId,
    pub local_office: OfficeId,
    pub hq_office: OfficeId,
    pub ca: Digest,
    pub epoch: Epoch,
    pub incarnation: OriginIncarnation,
    pub initial_snapshot_required: bool,
}

impl SqliteReplicationStore {
    pub fn open(p: OpenReplicationParams<'_>) -> Result<Self, OfficeError> {
        let state_dir = p
            .durable_root
            .join("local")
            .join("offices")
            .join(p.local_office.as_str())
            .join("replication");
        let state_path = state_dir.join("state.v1.json");
        let protected_dir = p
            .runtime_root
            .join(p.local_office.as_str())
            .join("protected");
        private_dir_synced(p.durable_root, &state_dir).map_err(OfficeError::storage)?;
        private_dir(&protected_dir).map_err(OfficeError::storage)?;
        tighten_dir(&protected_dir).map_err(OfficeError::storage)?;

        let data = if state_path.is_file() {
            let bytes = fs::read(&state_path).map_err(OfficeError::storage)?;
            let parsed: ReplicationStateFile = serde_json::from_slice(&bytes)
                .map_err(|_| OfficeError::Invalid("malformed durable replication state"))?;
            core_from_file(&parsed)?
        } else {
            let initial = ReplicationCoreData::new(
                p.org_id,
                p.local_office.clone(),
                p.hq_office,
                p.ca,
                p.epoch,
                p.incarnation,
                p.initial_snapshot_required,
            );
            let encoded = encode_state_file(&initial)?;
            write_private_atomic(&state_path, &encoded).map_err(OfficeError::storage)?;
            initial
        };

        let db_path = protected_dir.join("replication.db");
        let conn = open_and_reconcile_companion_db(
            &db_path,
            "replication",
            REPLICATION_ADAPTER_FORMAT,
            REPLICATION_DDL,
            REPLICATION_PRAGMAS,
            |conn| project_into_sqlite(conn, &data),
        )?;

        Ok(Self {
            state_path,
            inner: Mutex::new(SqliteReplicationInner {
                data,
                conn,
                projection_dirty: false,
            }),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, SqliteReplicationInner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn reconcile_if_dirty(guard: &mut SqliteReplicationInner) -> Result<(), OfficeError> {
        if guard.projection_dirty {
            project_into_sqlite(&mut guard.conn, &guard.data)?;
            guard.projection_dirty = false;
        }
        Ok(())
    }

    /// Single durable commit point: write `state.v1.json` atomically, update `guard.data`
    /// immediately so memory never diverges from the committed JSON, then project into the
    /// rebuildable SQLite index (marking `projection_dirty` if projection fails so the next
    /// operation reconciles from the committed state before mutating further).
    fn persist_and_project(
        &self,
        guard: &mut SqliteReplicationInner,
        candidate: ReplicationCoreData,
    ) -> Result<(), OfficeError> {
        Self::reconcile_if_dirty(guard)?;
        let encoded = encode_state_file(&candidate)?;
        write_private_atomic(&self.state_path, &encoded).map_err(OfficeError::storage)?;
        guard.data = candidate;
        if let Err(err) = project_into_sqlite(&mut guard.conn, &guard.data) {
            guard.projection_dirty = true;
            return Err(err);
        }
        Ok(())
    }
}
