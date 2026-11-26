//! Classification of replicated snapshot paths, and filtering of stale snapshot work files and
//! self-echoed HQ org events on satellites.

use org_store::{DurablePath, Files, Id};

use crate::error::OfficeError;
use crate::office::{storage, Office};

use super::work_ingest::ReadOfficeCodesOp;
use super::ReplicatedFileBatch;

impl Office {
    /// Reconciles an entire incoming snapshot against committed local satellite-owned tasks and
    /// pending local outbox edits BEFORE chunking into <= 8 MiB batches (`TASK-52` P11-#2 / P12-#1):
    /// - Uses strict `local.revision > incoming.revision` so an interrupted snapshot resuming at
    ///   equal revision still applies any missing sidecars.
    /// - Scans all incoming `task.json` entries BEFORE removing `pending_work_paths` and protects
    ///   every task (and all its sidecars) touched by `pending_work_paths`.
    pub fn filter_stale_snapshot_work_files(
        &self,
        mut files: ReplicatedFileBatch,
        pending_work_paths: &std::collections::BTreeSet<String>,
    ) -> Result<ReplicatedFileBatch, OfficeError> {
        if self.info.role != crate::model::OfficeRole::Satellite {
            return Ok(files);
        }
        let mut protected_tasks = collect_task_ids_from_paths(pending_work_paths);
        for (path, opt_bytes) in &files {
            let Some(bytes) = opt_bytes else {
                continue;
            };
            let Ok(dp) = DurablePath::parse(path) else {
                continue;
            };
            let Some(crate::tasks::records::Kind::Task(tid)) = crate::tasks::records::kind_of(&dp)
            else {
                continue;
            };
            let incoming = serde_json::from_slice::<crate::tasks::records::TaskRecord>(bytes)
                .map_err(|_| OfficeError::Invalid("malformed TaskRecord in snapshot"))?;
            match self.task_inspect(tid.as_str())? {
                Ok(local) => {
                    if local.task.office_code == incoming.office_code
                        && local.task.revision > incoming.meta.revision
                    {
                        protected_tasks.insert(tid);
                    }
                }
                Err(api_err) if api_err.code == org_proto::ErrorCode::NotFound => {}
                Err(_) => {
                    return Err(OfficeError::Invalid(
                        "failed inspecting local task during snapshot reconciliation",
                    ));
                }
            }
        }
        if protected_tasks.is_empty() && pending_work_paths.is_empty() {
            return Ok(files);
        }
        files.retain(|(path, opt_bytes)| {
            if pending_work_paths.contains(path) {
                return false;
            }
            if opt_bytes.is_none() {
                return true;
            }
            let Ok(dp) = DurablePath::parse(path) else {
                return true;
            };
            match task_id_of_work_path(&dp) {
                Some(tid) => !protected_tasks.contains(&tid),
                None => true,
            }
        });
        Ok(files)
    }

    /// Filters out stale self-echoes of this satellite's own single-writer task counter, tasks,
    /// and sidecars from an HQ `OrgEvent` so a satellite creating task N+1 before pulling HQ's echo
    /// of task N never stalls its own org stream on SQLite's monotonic counter trigger (`TASK-52` P8-#1).
    pub fn filter_self_echoed_org_event_files(
        &self,
        files: ReplicatedFileBatch,
    ) -> Result<ReplicatedFileBatch, OfficeError> {
        if self.info.role != crate::model::OfficeRole::Satellite {
            return Ok(files);
        }
        let (codes, _) = self.store.read(ReadOfficeCodesOp).map_err(storage)?;
        let local_codes: std::collections::BTreeSet<String> = codes
            .into_iter()
            .filter(|c| c.office_id == self.identity.office_id)
            .map(|c| c.code)
            .collect();
        if local_codes.is_empty() {
            return Ok(files);
        }
        let durable_files = Files::open(&self.durable).map_err(OfficeError::storage)?;
        let mut skipped_tasks: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();
        let mut out = Vec::with_capacity(files.len());
        // Pass 1: identify stale echoed local tasks & counters.
        for (path, opt_bytes) in files {
            let Ok(dp) = DurablePath::parse(&path) else {
                out.push((path, opt_bytes));
                continue;
            };
            match crate::tasks::records::kind_of(&dp) {
                Some(crate::tasks::records::Kind::OfficeTaskCounter(ref code))
                    if local_codes.contains(code) =>
                {
                    if should_skip_echoed_counter(&durable_files, &dp, opt_bytes.as_deref()) {
                        continue;
                    }
                    out.push((path, opt_bytes));
                }
                Some(crate::tasks::records::Kind::Task(ref tid)) => {
                    if should_skip_echoed_task(self, &local_codes, tid, opt_bytes.as_deref()) {
                        skipped_tasks.insert(tid.as_str().to_owned());
                        continue;
                    }
                    out.push((path, opt_bytes));
                }
                _ => out.push((path, opt_bytes)),
            }
        }
        // Pass 2: drop already-present sidecars for skipped local tasks.
        out.retain(|(path, opt_bytes)| {
            let Ok(dp) = DurablePath::parse(path) else {
                return true;
            };
            let sidecar_tid = match crate::tasks::records::kind_of(&dp) {
                Some(
                    crate::tasks::records::Kind::Checklist(tid)
                    | crate::tasks::records::Kind::Comment(tid, _)
                    | crate::tasks::records::Kind::Watchers(tid)
                    | crate::tasks::records::Kind::Links(tid)
                    | crate::tasks::records::Kind::Event(tid, _),
                ) => tid,
                _ => return true,
            };
            if opt_bytes.is_some()
                && skipped_tasks.contains(sidecar_tid.as_str())
                && durable_files.read(&dp).ok().flatten().is_some()
            {
                return false;
            }
            true
        });
        Ok(out)
    }
}

fn task_id_of_work_path(dp: &DurablePath) -> Option<Id> {
    match crate::tasks::records::kind_of(dp) {
        Some(
            crate::tasks::records::Kind::Task(tid)
            | crate::tasks::records::Kind::Checklist(tid)
            | crate::tasks::records::Kind::Comment(tid, _)
            | crate::tasks::records::Kind::Watchers(tid)
            | crate::tasks::records::Kind::Links(tid)
            | crate::tasks::records::Kind::Event(tid, _),
        ) => Some(tid),
        _ => None,
    }
}

fn collect_task_ids_from_paths(
    paths: &std::collections::BTreeSet<String>,
) -> std::collections::BTreeSet<Id> {
    paths
        .iter()
        .filter_map(|p| DurablePath::parse(p).ok())
        .filter_map(|dp| task_id_of_work_path(&dp))
        .collect()
}

/// Returns the owning task ID string if `path` is a task record or task sidecar under `state/work/`.
pub fn task_id_for_work_path(path: &str) -> Option<String> {
    DurablePath::parse(path)
        .ok()
        .and_then(|dp| task_id_of_work_path(&dp))
        .map(|id| id.as_str().to_owned())
}

/// Returns `Ok(Some(task_id))` when `office` is a satellite that already holds a strictly newer
/// local revision for this incoming snapshot `task.json`, failing closed on storage or non-`NotFound`
/// lookup errors so newer sidecars can never be overwritten (`TASK-52` P12-#1 / P21-#2 / P27-#1).
pub fn is_stale_snapshot_task_entry(
    office: &Office,
    path: &str,
    bytes: &[u8],
) -> Result<Option<String>, OfficeError> {
    if office.info.role != crate::model::OfficeRole::Satellite {
        return Ok(None);
    }
    let dp = DurablePath::parse(path).map_err(storage)?;
    let Some(crate::tasks::records::Kind::Task(tid)) = crate::tasks::records::kind_of(&dp) else {
        return Ok(None);
    };
    let incoming: crate::tasks::records::TaskRecord = serde_json::from_slice(bytes)
        .map_err(|_| OfficeError::Invalid("malformed TaskRecord in snapshot"))?;
    match office.task_inspect(tid.as_str())? {
        Ok(local) => {
            if local.task.office_code == incoming.office_code
                && local.task.revision > incoming.meta.revision
            {
                Ok(Some(tid.as_str().to_owned()))
            } else {
                Ok(None)
            }
        }
        Err(api_err) if api_err.code == org_proto::ErrorCode::NotFound => Ok(None),
        Err(_) => Err(OfficeError::Invalid(
            "failed inspecting local task during snapshot indexing",
        )),
    }
}

/// Canonical dependency classification for replicated snapshot files derived directly from
/// owning domain record parsers (`TASK-52` P8-#5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotPathClass {
    OrgCoreStructure,
    OrgCoreEmployee,
    ProvidersAndHires,
    ModelProfilesAndProjects,
    DependentExecutionDocsAndBudgets,
    WorkCore,
    WorkSidecar,
}

/// Clears employee attribution on typed canonical `state/repositories.yaml` records for the
/// neutral pre-pass of > 8 MiB org-core snapshots (`TASK-52` P11-#1).
pub fn strip_org_core_employee_refs_typed(path: &str, bytes: &[u8]) -> Option<Vec<u8>> {
    if path != "state/repositories.yaml" {
        return None;
    }
    let mut repos = crate::organization::records::decode_repositories(bytes).ok()?;
    for repo in &mut repos {
        repo.meta.created_by = crate::organization::records::By::System;
        repo.meta.updated_by = crate::organization::records::By::System;
    }
    Some(crate::organization::records::encode_repositories(&repos))
}

/// Classifies a replicated `state/` path strictly using canonical record parsers so large
/// snapshots never misclassify files such as `execution.md` or `processes/*.md` (`TASK-52` P8-#5).
pub fn classify_replicated_snapshot_path(path: &str) -> SnapshotPathClass {
    let Ok(dp) = DurablePath::parse(path) else {
        return SnapshotPathClass::DependentExecutionDocsAndBudgets;
    };
    if let Some(tk) = crate::tasks::records::kind_of(&dp) {
        return match tk {
            crate::tasks::records::Kind::OfficeTaskCounter(_)
            | crate::tasks::records::Kind::Task(_) => SnapshotPathClass::WorkCore,
            _ => SnapshotPathClass::WorkSidecar,
        };
    }
    if crate::processes::records::parse_path(&dp).is_some() {
        return SnapshotPathClass::DependentExecutionDocsAndBudgets;
    }
    if crate::projects::records::kind_of(&dp).is_some() {
        return SnapshotPathClass::ModelProfilesAndProjects;
    }
    if let Some(ok) = crate::organization::records::kind_of(&dp) {
        use crate::organization::records::Kind as OrgKind;
        return match ok {
            OrgKind::Employee(_, _, _) => SnapshotPathClass::OrgCoreEmployee,
            OrgKind::ProviderConnection(_) | OrgKind::HireRequest(_, _) => {
                SnapshotPathClass::ProvidersAndHires
            }
            OrgKind::ModelProfile(_) => SnapshotPathClass::ModelProfilesAndProjects,
            OrgKind::AgentExecution(_, _) | OrgKind::WatchingModel => {
                SnapshotPathClass::DependentExecutionDocsAndBudgets
            }
            _ => SnapshotPathClass::OrgCoreStructure,
        };
    }
    if matches!(
        crate::messaging::records::kind_of(&dp),
        Some(crate::messaging::records::Kind::Conversation(_))
    ) || path.starts_with("state/budgets/")
    {
        return SnapshotPathClass::DependentExecutionDocsAndBudgets;
    }
    SnapshotPathClass::OrgCoreStructure
}

pub(super) fn is_replicated_op_kind(kind: &str) -> bool {
    matches!(
        kind,
        "organization.apply_replicated_files"
            | "replication.apply_message"
            | "replication.apply_conversation"
            | "replication.apply_origin_close"
            | "replication.apply_handoff_accept"
    )
}

/// Determines provenance directly from durable journal operation kind (with bounded-ring fallback),
/// preventing long catch-ups (>2,048 ops) from ever re-exporting replicated HQ work (`TASK-52` P8-#3).
pub(super) fn is_replicated_journal_entry(
    entry: &org_store::Entry,
    replication: &dyn crate::replication::ReplicationStore,
) -> bool {
    entry.header.operation.as_ref().is_some_and(|op| {
        is_replicated_op_kind(op.kind.as_str())
            || replication
                .is_replicated_op_id(op.id.as_str())
                .unwrap_or(false)
    })
}

fn should_skip_echoed_counter(
    durable_files: &Files,
    dp: &DurablePath,
    incoming_bytes: Option<&[u8]>,
) -> bool {
    let Some(in_raw) = incoming_bytes else {
        return false;
    };
    let Ok(in_val) = serde_json::from_slice::<serde_json::Value>(in_raw) else {
        return false;
    };
    let Some(in_num) = in_val
        .get("last_number")
        .and_then(serde_json::Value::as_u64)
    else {
        return false;
    };
    let Ok(Some(local_raw)) = durable_files.read(dp) else {
        return false;
    };
    serde_json::from_slice::<serde_json::Value>(&local_raw)
        .ok()
        .and_then(|v| v.get("last_number").and_then(serde_json::Value::as_u64))
        .is_some_and(|local_num| local_num >= in_num)
}

fn should_skip_echoed_task(
    office: &Office,
    local_codes: &std::collections::BTreeSet<String>,
    tid: &Id,
    incoming_bytes: Option<&[u8]>,
) -> bool {
    let Some(in_raw) = incoming_bytes else {
        return false;
    };
    let Ok(in_rec) = serde_json::from_slice::<crate::tasks::records::TaskRecord>(in_raw) else {
        return false;
    };
    if !local_codes.contains(&in_rec.office_code) {
        return false;
    }
    office
        .task_inspect(tid.as_str())
        .ok()
        .and_then(|r| r.ok())
        .is_some_and(|local| local.task.revision >= in_rec.meta.revision)
}
