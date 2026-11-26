//! Replicated `state/` application on satellites, HQ ingest of satellite work batches, and
//! the org journal and satellite work outbox feeds into `org-sync`.

use std::collections::BTreeMap;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_store::{Actor, DurablePath, Id, Outcome};
use org_sync_proto::{Digest, Epoch, OfficeId};
use serde::{Deserialize, Serialize};

use crate::office::{storage, Office};
use crate::{
    error::OfficeError, model::OfficeRole, replication::state_files::ApplyReplicatedFiles,
    store::open::ApplyReplicatedFilesOp,
};

use super::filters::is_replicated_journal_entry;
use super::work_ingest::{
    validate_typed_work_files_against_hq, validate_work_file_paths, ReadOfficeCodesOp,
};
use super::ReplicatedFileBatch;

impl Office {
    /// Applies an HQ-authoritative batch of `state/` files onto this satellite's `org-store`.
    pub fn apply_replicated_state_files(
        &self,
        operation_id: &str,
        files: Vec<(String, Option<Vec<u8>>)>,
    ) -> Result<(), OfficeError> {
        if self.info.role != OfficeRole::Satellite {
            return Err(OfficeError::Invalid(
                "only satellites apply replicated state batches",
            ));
        }
        self.apply_replicated_files_inner(operation_id, files, false, BTreeMap::new())
    }

    /// Applies one batch of an HQ snapshot applied in several (`TASK-52` P21-#2):
    /// `task_revisions` names, for the tasks of the batch, the effective revision the whole
    /// snapshot brings each to, so a batch holding a task's `task.md` without its later
    /// history (or the reverse) is never taken for stale (`doc-79`).
    pub fn apply_replicated_snapshot_batch(
        &self,
        operation_id: &str,
        files: Vec<(String, Option<Vec<u8>>)>,
        task_revisions: BTreeMap<String, u64>,
    ) -> Result<(), OfficeError> {
        if self.info.role != OfficeRole::Satellite {
            return Err(OfficeError::Invalid(
                "only satellites apply replicated state batches",
            ));
        }
        let mut revisions = BTreeMap::new();
        for (task, revision) in task_revisions {
            let id = Id::parse(&task)
                .map_err(|_| OfficeError::Invalid("invalid task id in snapshot revisions"))?;
            revisions.insert(id, revision);
        }
        self.apply_replicated_files_inner(operation_id, files, false, revisions)
    }

    /// Applies a validated satellite-originated work file batch strictly under projector-owned
    /// work families (`tasks` or `messaging`), refusing HQ-owned organization/office/project paths
    /// and any unowned paths (`TASK-52` P2-1).
    pub fn apply_replicated_work_files(
        &self,
        operation_id: &str,
        files: Vec<(String, Option<Vec<u8>>)>,
    ) -> Result<(), OfficeError> {
        if self.info.role != OfficeRole::Hq {
            return Err(OfficeError::Invalid(
                "only HQ ingests satellite work batches",
            ));
        }
        validate_work_file_paths(&files)?;
        self.apply_replicated_files_inner(operation_id, files, true, BTreeMap::new())
    }

    /// Validates a satellite's typed work batch against HQ's organization snapshot (actor/team
    /// authority, monotonic task revision, immutable events) and commits it atomically with
    /// `origin` high-water under `federation_io_lock` (`TASK-52` H-7).
    pub fn ingest_validated_satellite_work(
        &self,
        origin: &org_sync::work::Origin,
        seq: org_sync_proto::SourceSeq,
        expected_record_id: &str,
        operation_id: &str,
        files: Vec<(String, Option<Vec<u8>>)>,
    ) -> Result<u64, OfficeError> {
        if self.info.role != OfficeRole::Hq {
            return Err(OfficeError::Invalid(
                "only HQ ingests satellite work batches",
            ));
        }
        let _guard = self
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let prev_hw = self.replication.remote_work_high_water(origin)?;
        if seq.get() <= prev_hw {
            return Ok(prev_hw);
        }
        if seq.get() != prev_hw.saturating_add(1) {
            return Err(OfficeError::Invalid("non-contiguous satellite work seq"));
        }
        validate_work_file_paths(&files)?;
        validate_typed_work_files_against_hq(
            self,
            origin.office.as_str(),
            expected_record_id,
            &files,
        )?;
        self.apply_replicated_files_inner(operation_id, files, true, BTreeMap::new())?;
        self.replication.record_remote_work_seq(origin, seq)?;
        Ok(prev_hw)
    }

    /// `rehome_tasks`: HQ ingesting a satellite's work writes each task folder file in the
    /// folder its own projection names for the task (`ApplyReplicatedFiles::rehome_tasks`).
    fn apply_replicated_files_inner(
        &self,
        operation_id: &str,
        files: Vec<(String, Option<Vec<u8>>)>,
        rehome_tasks: bool,
        task_revisions: BTreeMap<Id, u64>,
    ) -> Result<(), OfficeError> {
        let op_id = Id::parse(operation_id)
            .map_err(|_| OfficeError::Invalid("invalid replication operation_id"))?;
        self.replication.record_replicated_op_id(operation_id)?;
        let reply = self
            .store
            .run(ApplyReplicatedFilesOp {
                operation_id: op_id,
                actor: Actor::System,
                command: ApplyReplicatedFiles {
                    files,
                    rehome_tasks,
                    task_revisions,
                },
            })
            .map_err(storage)?;
        match reply.outcome {
            Outcome::Completed => {
                self.store.sync().map_err(storage)?;
                Ok(())
            }
            Outcome::Rejected => Err(OfficeError::Invalid("replicated state batch rejected")),
        }
    }

    /// Publishes any custody-confirmed, un-replicated HQ `org-store` journal entries under
    /// `state/` as typed `org-sync` `OrgEvent` deltas using the durable `ARCH-03` consumer
    /// `"org-sync-hq"`, atomically recording `(journal_incarnation, ordinal)` in one transaction
    /// and splitting oversized multi-file entries within `MAX_EVENT_PAYLOAD_BYTES` (`TASK-52` H-12/M-16).
    pub fn sync_org_journal_to_replication(&self) -> Result<usize, OfficeError> {
        if self.info.role != OfficeRole::Hq {
            return Ok(0);
        }
        let _guard = self
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let confirmed = self.store.status().confirmed;
        sync_org_journal_locked_up_to(self, confirmed)
    }

    /// Feeds locally accepted satellite task journal entries into this satellite's durable
    /// `org-sync` work outbox via the `ARCH-03` consumer `"org-sync-work-outbox"`, skipping entries
    /// produced by inbound HQ replication applies (`TASK-52` H-6).
    pub fn sync_satellite_work_outbox(&self) -> Result<usize, OfficeError> {
        if self.info.role != OfficeRole::Satellite {
            return Ok(0);
        }
        let _guard = self
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        sync_satellite_work_outbox_locked(self)
    }
}

pub(super) fn sync_org_journal_locked_up_to(
    office: &Office,
    max_ordinal: u64,
) -> Result<usize, OfficeError> {
    let wm = office.register_replication_consumer("org-sync-hq")?;
    let entries = office.store.entries_after_contiguous(wm).map_err(storage)?;
    if entries.is_empty() {
        return Ok(0);
    }
    let incarnation = office.identity.journal_incarnation.as_str().to_owned();
    let codec = org_sync_proto::Codec::new("org.native.changes", 1)
        .map_err(|_| OfficeError::Invalid("codec"))?;
    let mut published = 0usize;
    let mut last_ordinal = wm;
    for entry in entries {
        let ord = entry.ordinal();
        if ord > max_ordinal {
            break;
        }
        let files: Vec<(String, Option<String>)> = entry
            .versions()
            .filter(|fv| is_hq_org_event_version(fv.path, fv.bytes))
            .map(|fv| {
                (
                    fv.path.as_str().to_owned(),
                    fv.bytes.map(|b| STANDARD.encode(b)),
                )
            })
            .collect();
        if !files.is_empty() {
            let payloads = chunk_org_delta_payloads(files)?;
            published += office.replication.publish_hq_journal_events(
                codec.clone(),
                payloads,
                &incarnation,
                ord,
            )?;
        }
        last_ordinal = last_ordinal.max(ord);
    }
    if last_ordinal > wm {
        office.advance_replication_consumer("org-sync-hq", last_ordinal)?;
    }
    Ok(published)
}

/// Checkpoint-consistent snapshot of HQ's authoritative `state/` files (`TASK-52` P2-8).
#[derive(Clone, Debug)]
pub struct ConsistentStateSnapshot {
    pub checkpoint: org_sync::organization::OrgCheckpoint,
    pub files: Vec<(String, Vec<u8>)>,
}

/// Typed `state/` file delta payload carried inside HQ-published `org-sync` `OrgEvent`s
/// (`TASK-52` F2) and satellite-originated `WorkChange` payloads (`TASK-52` F3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgDeltaPayload {
    pub v: u32,
    /// `(durable_path_under_state, Some(base64_bytes) for put / None for delete)`.
    pub files: Vec<(String, Option<String>)>,
}

impl OrgDeltaPayload {
    pub fn decode_files(&self) -> Result<ReplicatedFileBatch, OfficeError> {
        if self.v != 1 {
            return Err(OfficeError::Invalid(
                "unsupported org delta payload version",
            ));
        }
        let mut out = Vec::with_capacity(self.files.len());
        for (path, b64) in &self.files {
            if (!path.starts_with("state/") && !path.starts_with("messaging/"))
                || path.contains("..")
            {
                return Err(OfficeError::Invalid("invalid org delta file path"));
            }
            let bytes = match b64 {
                Some(s) => Some(
                    STANDARD
                        .decode(s)
                        .map_err(|_| OfficeError::Invalid("invalid org delta file base64"))?,
                ),
                None => None,
            };
            out.push((path.clone(), bytes));
        }
        Ok(out)
    }
}

fn is_hq_org_event_version(path: &DurablePath, bytes: Option<&[u8]>) -> bool {
    if path.as_str().starts_with("state/") {
        return true;
    }
    matches!(
        crate::messaging::records::kind_of(path),
        Some(crate::messaging::records::Kind::Conversation(_))
    ) && bytes.is_some_and(|raw| {
        serde_json::from_slice::<crate::messaging::records::ConversationRecord>(raw)
            .is_ok_and(|c| c.kind == org_proto::chat::ConversationKind::Channel)
    })
}

/// Splits a batch of encoded `(path, Option<b64>)` entries into one or more `OrgDeltaPayload`
/// JSON byte slices each strictly within `org_sync::organization::MAX_PAYLOAD_BYTES` (`TASK-52` H-12).
fn chunk_org_delta_payloads(
    files: Vec<(String, Option<String>)>,
) -> Result<Vec<std::sync::Arc<[u8]>>, OfficeError> {
    let max_bytes = org_sync::organization::MAX_PAYLOAD_BYTES;
    let mut chunks = Vec::new();
    let mut current: Vec<(String, Option<String>)> = Vec::new();
    for item in files {
        current.push(item);
        let encoded = serde_json::to_vec(&OrgDeltaPayload {
            v: 1,
            files: current.clone(),
        })
        .map_err(OfficeError::storage)?;
        if encoded.len() <= max_bytes {
            continue;
        }
        let popped = current.pop().expect("just pushed");
        if !current.is_empty() {
            let flush = serde_json::to_vec(&OrgDeltaPayload {
                v: 1,
                files: std::mem::take(&mut current),
            })
            .map_err(OfficeError::storage)?;
            chunks.push(std::sync::Arc::from(flush));
        }
        let single = serde_json::to_vec(&OrgDeltaPayload {
            v: 1,
            files: vec![popped.clone()],
        })
        .map_err(OfficeError::storage)?;
        if single.len() > max_bytes {
            return Err(OfficeError::Invalid(
                "single replicated file exceeds org-sync event payload limit",
            ));
        }
        current.push(popped);
    }
    if !current.is_empty() {
        let flush = serde_json::to_vec(&OrgDeltaPayload {
            v: 1,
            files: current,
        })
        .map_err(OfficeError::storage)?;
        chunks.push(std::sync::Arc::from(flush));
    }
    Ok(chunks)
}

fn sync_satellite_work_outbox_locked(office: &Office) -> Result<usize, OfficeError> {
    let wm = office.register_replication_consumer("org-sync-work-outbox")?;
    let confirmed = office.store.status().confirmed;
    let entries = office.store.entries_after_contiguous(wm).map_err(storage)?;
    if entries.is_empty() {
        return Ok(0);
    }
    let (codes, _) = office.store.read(ReadOfficeCodesOp).map_err(storage)?;
    let local_codes: std::collections::BTreeSet<String> = codes
        .into_iter()
        .filter(|c| c.office_id == office.identity.office_id)
        .map(|c| c.code)
        .collect();
    let incarnation = office.identity.journal_incarnation.as_str().to_owned();
    let codec = org_sync_proto::Codec::new("org.native.changes", 1)
        .map_err(|_| OfficeError::Invalid("codec"))?;
    let mut enqueued = 0usize;
    let mut last_ordinal = wm;
    for entry in entries {
        let ord = entry.ordinal();
        if ord > confirmed {
            break;
        }
        let from_replication = is_replicated_journal_entry(&entry, office.replication.as_ref());
        if !from_replication {
            let task_versions: Vec<(&DurablePath, Option<&[u8]>)> = entry
                .versions()
                .filter(|fv| crate::tasks::records::kind_of(fv.path).is_some())
                .map(|fv| (fv.path, fv.bytes))
                .collect();
            if !task_versions.is_empty()
                && is_locally_owned_satellite_work_entry(office, &local_codes, &task_versions)?
            {
                let files: Vec<(String, Option<String>)> = task_versions
                    .into_iter()
                    .map(|(path, bytes)| {
                        (path.as_str().to_owned(), bytes.map(|b| STANDARD.encode(b)))
                    })
                    .collect();
                let record_id =
                    extract_primary_task_id(&files).unwrap_or_else(|| format!("task-ord-{ord}"));
                let payloads = chunk_org_delta_payloads(files)?;
                let created_ms = office.monotonic_now_ms();
                enqueued += office.replication.publish_satellite_work_entries(
                    codec.clone(),
                    &record_id,
                    payloads,
                    &incarnation,
                    ord,
                    created_ms,
                )?;
            }
        }
        last_ordinal = last_ordinal.max(ord);
    }
    if last_ordinal > wm {
        office.advance_replication_consumer("org-sync-work-outbox", last_ordinal)?;
    }
    Ok(enqueued)
}

struct ReadTaskOfficeCodeOp(Id);

impl crate::store::open::ReadOp for ReadTaskOfficeCodeOp {
    type Out = Option<String>;

    fn read<S: crate::tasks::ports::TasksRead>(
        self,
        snapshot: &S,
    ) -> Result<Self::Out, org_store::StoreError> {
        Ok(snapshot.task(&self.0)?.map(|t| t.office_code))
    }
}

/// Belt-and-suspenders guard (`TASK-192`) ensuring a satellite never enqueues a `WorkChange`
/// touching another office's task or an immutable-history deletion (which HQ would reject
/// and stall sync), while propagating transient storage errors so the outbox consumer watermark
/// never advances past an unverified local entry.
fn is_locally_owned_satellite_work_entry(
    office: &Office,
    local_codes: &std::collections::BTreeSet<String>,
    versions: &[(&DurablePath, Option<&[u8]>)],
) -> Result<bool, OfficeError> {
    if local_codes.is_empty() {
        return Ok(false);
    }
    let mut verified_tasks = std::collections::BTreeSet::new();
    let mut sidecar_tasks = Vec::new();
    for (path, bytes) in versions {
        match crate::tasks::records::kind_of(path) {
            Some(crate::tasks::records::Kind::Task(tid)) => {
                let Some(raw) = bytes else {
                    return Ok(false);
                };
                let Ok(doc) = crate::tasks::records::decode_task_md(raw) else {
                    return Ok(false);
                };
                if doc.task.id != tid || !local_codes.contains(&doc.task.office_code) {
                    return Ok(false);
                }
                verified_tasks.insert(tid);
            }
            Some(
                crate::tasks::records::Kind::Comment(tid, _)
                | crate::tasks::records::Kind::Event(tid, _),
            ) => {
                if bytes.is_none() {
                    return Ok(false);
                }
                sidecar_tasks.push(tid);
            }
            Some(
                crate::tasks::records::Kind::Checklist(tid)
                | crate::tasks::records::Kind::Operations(tid),
            ) => sidecar_tasks.push(tid),
            None => return Ok(false),
        }
    }
    for tid in sidecar_tasks {
        if verified_tasks.contains(&tid) {
            continue;
        }
        let (office_code, _) = office
            .store
            .read(ReadTaskOfficeCodeOp(tid.clone()))
            .map_err(storage)?;
        if !office_code.is_some_and(|code| local_codes.contains(&code)) {
            return Ok(false);
        }
        verified_tasks.insert(tid);
    }
    Ok(true)
}

fn extract_primary_task_id(files: &[(String, Option<String>)]) -> Option<String> {
    let kinds: Vec<crate::tasks::records::Kind> = files
        .iter()
        .filter_map(|(path, _)| DurablePath::parse(path).ok())
        .filter_map(|dp| crate::tasks::records::kind_of(&dp))
        .collect();
    let primary = kinds
        .iter()
        .find(|k| matches!(k, crate::tasks::records::Kind::Task(_)))
        .or_else(|| kinds.first())?;
    Some(primary.task_id().as_str().to_owned())
}

pub(crate) fn reconcile_satellite_replication_on_open(
    replication: &dyn crate::replication::ReplicationStore,
    hq_office_id: &str,
    ca_digest: Digest,
    epoch: u64,
) -> Result<(), OfficeError> {
    let cp = replication.org_checkpoint()?;
    let pending_snapshot = replication
        .sync_status()
        .is_ok_and(|s| s.initial_snapshot_required);
    if cp.hq_office.as_str() != hq_office_id
        || cp.ca != ca_digest
        || cp.position.epoch.get() < epoch
    {
        if epoch < cp.highest_epoch.get() {
            return Err(OfficeError::Invalid(
                "satellite identity epoch regressed below replication checkpoint",
            ));
        }
        // If re-enrollment already persisted replacement HQ/CA pins (`initial_snapshot_required`)
        // and crashed before swapping TLS credentials at the same epoch, preserve the committed
        // replacement checkpoint pins so the old HQ cannot be re-trusted on restart (`TASK-52` P10-#2).
        if pending_snapshot && epoch == cp.highest_epoch.get() {
            return Ok(());
        }
        let new_hq =
            OfficeId::new(hq_office_id).map_err(|_| OfficeError::Invalid("hq_office_id"))?;
        let new_ep = Epoch::new(epoch.max(1)).map_err(|_| OfficeError::Invalid("epoch"))?;
        replication.reset_for_re_enrollment(new_hq, ca_digest, new_ep)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "replication_tests.rs"]
mod work_outbox_filter_tests;
