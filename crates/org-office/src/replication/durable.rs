//! The durable replication store: one implementation for every engine (`TASK-52`, `FC-145`,
//! `ADR-0018` `ENG-3`, `TASK-172`).
//!
//! Durable source of truth lives under `<durable>/local/offices/<office_id>/replication/state.v1.json`
//! (written atomically via `full_sync` + parent-dir sync) whatever the engine, so every engine
//! leaves the same durable folder. An engine adds only a rebuildable [`ReplicationProjection`]
//! of the committed state: the SQLite engine's lives in `crate::store::sqlite::replication`;
//! the memory engine serves every read from the state this store holds and adds none.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use org_sync_proto::{Digest, Epoch, OfficeId, OrgId, OriginIncarnation};

use super::{memory::ReplicationCoreData, ReplicationStateFile};
use crate::error::OfficeError;
use crate::private_fs::{private_dir_synced, write_private_atomic};

mod ports;
mod state_file;
#[cfg(test)]
mod tests;

use self::state_file::{core_from_file, encode_state_file};

/// A rebuildable projection of the committed replication state: an engine's index of it.
pub(crate) trait ReplicationProjection: Send {
    /// Brings the projection to `data`, the state `state.v1.json` holds.
    fn project(&mut self, data: &ReplicationCoreData) -> Result<(), OfficeError>;
}

/// The projection of an engine that serves reads from the state the store holds.
#[cfg(any(test, feature = "testkit"))]
struct NoProjection;

#[cfg(any(test, feature = "testkit"))]
impl ReplicationProjection for NoProjection {
    fn project(&mut self, _data: &ReplicationCoreData) -> Result<(), OfficeError> {
        Ok(())
    }
}

struct DurableReplicationInner {
    data: ReplicationCoreData,
    projection: Box<dyn ReplicationProjection>,
    /// True if a prior projection failed after the durable JSON commit point succeeded;
    /// reconciled before serving any subsequent mutation (`TASK-52` review F19).
    projection_dirty: bool,
}

pub struct DurableReplicationStore {
    state_path: PathBuf,
    inner: Mutex<DurableReplicationInner>,
}

/// Where the durable state lives and what a new one starts from.
pub struct DurableReplicationParams<'a> {
    pub durable_root: &'a Path,
    pub org_id: OrgId,
    pub local_office: OfficeId,
    pub hq_office: OfficeId,
    pub ca: Digest,
    pub epoch: Epoch,
    pub incarnation: OriginIncarnation,
    pub initial_snapshot_required: bool,
}

impl DurableReplicationStore {
    /// Opens the durable state, writing the initial one when there is none, then the
    /// engine's projection over it: `projection` receives the state it starts from.
    pub(crate) fn open<P: ReplicationProjection + 'static>(
        p: DurableReplicationParams<'_>,
        projection: impl FnOnce(&ReplicationCoreData) -> Result<P, OfficeError>,
    ) -> Result<Self, OfficeError> {
        let state_dir = p
            .durable_root
            .join("local")
            .join("offices")
            .join(p.local_office.as_str())
            .join("replication");
        let state_path = state_dir.join("state.v1.json");
        private_dir_synced(p.durable_root, &state_dir).map_err(OfficeError::storage)?;

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

        let projection = Box::new(projection(&data)?);
        Ok(Self {
            state_path,
            inner: Mutex::new(DurableReplicationInner {
                data,
                projection,
                projection_dirty: false,
            }),
        })
    }

    /// Opens the durable state for an engine without a projection (the memory engine).
    #[cfg(any(test, feature = "testkit"))]
    pub(crate) fn open_unprojected(p: DurableReplicationParams<'_>) -> Result<Self, OfficeError> {
        Self::open(p, |_| Ok(NoProjection))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, DurableReplicationInner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn reconcile_if_dirty(guard: &mut DurableReplicationInner) -> Result<(), OfficeError> {
        if guard.projection_dirty {
            guard.projection.project(&guard.data)?;
            guard.projection_dirty = false;
        }
        Ok(())
    }

    /// Single durable commit point: write `state.v1.json` atomically, update `guard.data`
    /// immediately so memory never diverges from the committed JSON, then bring the
    /// engine's rebuildable projection up to it (marking `projection_dirty` if projection
    /// fails so the next operation reconciles from the committed state before mutating
    /// further).
    fn persist_and_project(
        &self,
        guard: &mut DurableReplicationInner,
        candidate: ReplicationCoreData,
    ) -> Result<(), OfficeError> {
        Self::reconcile_if_dirty(guard)?;
        let encoded = encode_state_file(&candidate)?;
        write_private_atomic(&self.state_path, &encoded).map_err(OfficeError::storage)?;
        guard.data = candidate;
        if let Err(err) = guard.projection.project(&guard.data) {
            guard.projection_dirty = true;
            return Err(err);
        }
        Ok(())
    }
}
