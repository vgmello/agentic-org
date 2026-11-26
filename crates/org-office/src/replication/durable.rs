//! The durable replication store: one implementation for every engine (`TASK-52`, `FC-145`,
//! `ADR-0018` `ENG-3`, `TASK-172`).
//!
//! Durable source of truth lives under `<durable>/local/offices/<office_id>/replication/`
//! whatever the engine, so every engine leaves the same durable folder: `state.v1.json`
//! (written atomically via `full_sync` + parent-dir sync), the commit point, and the
//! append-only organization event log `events.v1.ndjson` it names (`TASK-140`, see
//! [`events_log`]). An engine adds only a rebuildable [`ReplicationProjection`] of the
//! committed state: the SQLite engine's lives in `crate::store::sqlite::replication`; the
//! memory engine serves every read from the state this store holds and adds none.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use org_sync::organization::OrgEvent;
use org_sync_proto::{Digest, Epoch, OfficeId, OrgId, OriginIncarnation};

use super::{memory::ReplicationCoreData, ReplicationStateFile};
use crate::error::OfficeError;
use crate::private_fs::{private_dir_synced, write_private_atomic};

mod events_log;
mod ports;
mod state_file;
#[cfg(test)]
mod tests;

use self::events_log::EventsLog;
use self::state_file::{core_from_file, encode_state_file};

/// A rebuildable projection of the committed replication state: an engine's index of it.
pub(crate) trait ReplicationProjection: Send {
    /// Brings the projection to `data`, the committed state.
    fn project(&mut self, data: &ReplicationCoreData) -> Result<(), OfficeError>;

    /// Brings the projection from `before`, the state it holds, to `after`, the state just
    /// committed. An engine updates only what changed (`TASK-140`); by default it projects
    /// `after` whole.
    fn project_change(
        &mut self,
        before: &ReplicationCoreData,
        after: &ReplicationCoreData,
    ) -> Result<(), OfficeError> {
        let _ = before;
        self.project(after)
    }
}

/// The events `after` adds to `before` in order, when `after`'s events are `before`'s followed
/// by new ones; `None` when the change is anything else (cleared, replaced). Compares every
/// retained position and digest, so a cleared and refilled history is never taken for an
/// append.
pub(crate) fn appended_events<'a>(
    before: &ReplicationCoreData,
    after: &'a ReplicationCoreData,
) -> Option<Vec<&'a OrgEvent>> {
    appended(&before.events, &after.events)
}

fn appended<'a>(
    before: &BTreeMap<(u64, u64), OrgEvent>,
    after: &'a BTreeMap<(u64, u64), OrgEvent>,
) -> Option<Vec<&'a OrgEvent>> {
    if after.len() < before.len() {
        return None;
    }
    let mut after_iter = after.iter();
    for (key, event) in before {
        let (after_key, after_event) = after_iter.next()?;
        if after_key != key || after_event.digest != event.digest {
            return None;
        }
    }
    Some(after_iter.map(|(_, event)| event).collect())
}

/// Checks the committed durable replication state of office `office_id` below `durable_root`
/// without changing either file (`restore --verify`, `TASK-104`): a `state.v1.json` that is
/// there must decode as this organization's and this office's, with every event it names
/// intact. An office that never replicated has none, which is not an error.
pub(crate) fn check_committed_state(
    durable_root: &Path,
    office_id: &str,
    org_id: &str,
) -> Result<(), OfficeError> {
    let state_dir = durable_root
        .join("local")
        .join("offices")
        .join(office_id)
        .join("replication");
    let state_path = state_dir.join("state.v1.json");
    match fs::symlink_metadata(&state_path) {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => {
            return Err(OfficeError::Invalid(
                "durable replication state is not a file",
            ))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(OfficeError::storage(e)),
    }
    let bytes = fs::read(&state_path).map_err(OfficeError::storage)?;
    let parsed: ReplicationStateFile = serde_json::from_slice(&bytes)
        .map_err(|_| OfficeError::Invalid("malformed durable replication state"))?;
    if parsed.org_id != org_id || parsed.local_office != office_id {
        return Err(OfficeError::Invalid(
            "durable replication state belongs to another office",
        ));
    }
    let (_, events) = EventsLog::open(&state_dir, &parsed)?;
    core_from_file(&parsed, &events).map(drop)
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
    log: EventsLog,
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

        let (data, log) = if state_path.is_file() {
            let bytes = fs::read(&state_path).map_err(OfficeError::storage)?;
            let parsed: ReplicationStateFile = serde_json::from_slice(&bytes)
                .map_err(|_| OfficeError::Invalid("malformed durable replication state"))?;
            let (log, events) = EventsLog::open(&state_dir, &parsed)?;
            let data = core_from_file(&parsed, &events)?;
            // Only a state file that decoded, with every event it names intact, may change
            // the log: a refused one leaves both files as they were.
            log.cut_uncommitted_tail()?;
            (data, log)
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
            let (log, extent) = EventsLog::empty(&state_dir);
            let encoded = encode_state_file(&initial, Some(&extent))?;
            write_private_atomic(&state_path, &encoded).map_err(OfficeError::storage)?;
            (initial, log)
        };

        let projection = Box::new(projection(&data)?);
        Ok(Self {
            state_path,
            inner: Mutex::new(DurableReplicationInner {
                data,
                log,
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

    /// Single durable commit point: append the new events to the log and write
    /// `state.v1.json` atomically ([`EventsLog::commit`]), update `guard.data` immediately so
    /// memory never diverges from the committed state, then bring the engine's rebuildable
    /// projection from the previous state up to it (marking `projection_dirty` if projection
    /// fails so the next operation reconciles from the committed state before mutating
    /// further).
    fn persist_and_project(
        &self,
        guard: &mut DurableReplicationInner,
        candidate: ReplicationCoreData,
    ) -> Result<(), OfficeError> {
        Self::reconcile_if_dirty(guard)?;
        guard
            .log
            .commit(&self.state_path, &guard.data, &candidate)?;
        let before = std::mem::replace(&mut guard.data, candidate);
        if let Err(err) = guard.projection.project_change(&before, &guard.data) {
            guard.projection_dirty = true;
            return Err(err);
        }
        Ok(())
    }
}
