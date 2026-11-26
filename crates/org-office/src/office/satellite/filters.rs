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
    ///   equal revision still applies any missing sidecars. The incoming revision is the
    ///   effective one: `task.md`'s, or the task's latest history event in the snapshot.
    /// - Scans all incoming `task.md` entries BEFORE removing `pending_work_paths` and protects
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
        let refs = files.iter().map(|(p, b)| (p.as_str(), b.as_deref()));
        let incoming = crate::tasks::replica::incoming_tasks(refs)
            .map_err(|_| OfficeError::Invalid("malformed task.md in snapshot"))?;
        for (tid, incoming) in incoming {
            match self.task_inspect(tid.as_str())? {
                Ok(local) => {
                    if local.task.office_code == incoming.task.office_code
                        && local.task.revision > incoming.revision
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

    /// Filters out stale self-echoes of this satellite's own single-writer tasks and their
    /// sidecars from an HQ `OrgEvent` so an echo of an older local revision never rolls a task
    /// back (`TASK-52` P8-#1). The office's task counter is office-local and never replicated
    /// (`doc-80`), so it never echoes.
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
        let mut skipped_tasks: std::collections::BTreeSet<Id> = std::collections::BTreeSet::new();
        let mut out = Vec::with_capacity(files.len());
        // Pass 1: identify stale echoed local tasks.
        for (path, opt_bytes) in files {
            let Ok(dp) = DurablePath::parse(&path) else {
                out.push((path, opt_bytes));
                continue;
            };
            if let Some(crate::tasks::records::Kind::Task(tid)) =
                crate::tasks::records::kind_of(&dp)
            {
                let here = durable_files.read(&dp).ok().flatten().is_some();
                if here && should_skip_echoed_task(self, &local_codes, &tid, opt_bytes.as_deref()) {
                    skipped_tasks.insert(tid);
                    continue;
                }
            }
            out.push((path, opt_bytes));
        }
        // Pass 2: drop already-present sidecars for skipped local tasks.
        out.retain(|(path, opt_bytes)| {
            let Ok(dp) = DurablePath::parse(path) else {
                return true;
            };
            let Some(kind) = crate::tasks::records::kind_of(&dp) else {
                return true;
            };
            !(opt_bytes.is_some()
                && skipped_tasks.contains(kind.task_id())
                && durable_files.read(&dp).ok().flatten().is_some())
        });
        Ok(out)
    }
}

fn task_id_of_work_path(dp: &DurablePath) -> Option<Id> {
    crate::tasks::records::kind_of(dp).map(|kind| kind.task_id().clone())
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

/// Returns the owning task ID string if `path` is a task folder file (`task.md` or a sidecar)
/// or a task history event.
pub fn task_id_for_work_path(path: &str) -> Option<String> {
    DurablePath::parse(path)
        .ok()
        .and_then(|dp| task_id_of_work_path(&dp))
        .map(|id| id.as_str().to_owned())
}

/// The local task folder files a snapshot moves elsewhere: each is of a task whose `task.md`
/// the snapshot (`incoming`) carries in another folder, as HQ's `MoveTeam` leaves it. Snapshot
/// reconciliation removes them, history and comments included, so no copy of a moved task
/// is left in its old folder; the projector keeps the moved task's rows (`doc-77`).
pub fn relocated_task_paths<'a>(
    local: impl IntoIterator<Item = &'a String>,
    incoming: &std::collections::BTreeSet<String>,
) -> std::collections::BTreeSet<String> {
    let folders: std::collections::BTreeMap<Id, String> = incoming
        .iter()
        .filter_map(|p| DurablePath::parse(p).ok())
        .filter(|p| {
            matches!(
                crate::tasks::records::kind_of(p),
                Some(crate::tasks::records::Kind::Task(_))
            )
        })
        .filter_map(|p| crate::tasks::records::task_dir_of(&p))
        .map(|dir| (dir.task_id.clone(), dir.folder()))
        .collect();
    local
        .into_iter()
        .filter(|raw| {
            let dir = DurablePath::parse(raw)
                .ok()
                .and_then(|p| crate::tasks::records::task_dir_of(&p));
            dir.is_some_and(|dir| {
                folders
                    .get(&dir.task_id)
                    .is_some_and(|folder| *folder != dir.folder())
            })
        })
        .cloned()
        .collect()
}

/// Returns `Ok(Some(task_id))` when `office` is a satellite that already holds a strictly newer
/// local revision for this incoming snapshot `task.md`, failing closed on storage or non-`NotFound`
/// lookup errors so newer sidecars can never be overwritten (`TASK-52` P12-#1 / P21-#2 / P27-#1).
/// Seen one file at a time, the incoming revision is `task.md`'s own: never above the task's
/// effective revision, so a task this protects may have newer history in the snapshot, never
/// the other way round.
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
    let incoming = crate::tasks::records::decode_task_md(bytes)
        .map_err(|_| OfficeError::Invalid("malformed task.md in snapshot"))?
        .task;
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
            crate::tasks::records::Kind::Task(_) => SnapshotPathClass::WorkCore,
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

fn should_skip_echoed_task(
    office: &Office,
    local_codes: &std::collections::BTreeSet<String>,
    tid: &Id,
    incoming_bytes: Option<&[u8]>,
) -> bool {
    let Some(in_raw) = incoming_bytes else {
        return false;
    };
    let Ok(in_rec) = crate::tasks::records::decode_task_md(in_raw).map(|doc| doc.task) else {
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
