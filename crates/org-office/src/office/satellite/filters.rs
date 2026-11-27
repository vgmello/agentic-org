//! Classification of replicated snapshot paths, and filtering of stale snapshot work files and
//! self-echoed HQ org events on satellites.

use std::collections::{BTreeMap, BTreeSet};

use org_store::{DurablePath, Files, Id, StoreError};

use crate::error::OfficeError;
use crate::office::{storage, Office};
use crate::store::open::ReadOp;
use crate::tasks::ports::TasksRead;
use crate::tasks::replica::{incoming_tasks, SnapshotTasks};

use super::work_ingest::ReadOfficeCodesOp;
use super::ReplicatedFileBatch;

/// Each named task this office has, with its owning office code and effective revision.
struct ReadTaskRevisionsOp(Vec<Id>);

impl ReadOp for ReadTaskRevisionsOp {
    type Out = BTreeMap<Id, (String, u64)>;

    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        let mut out = BTreeMap::new();
        for id in self.0 {
            if let Some(task) = snapshot.task(&id)? {
                out.insert(id, (task.office_code, task.meta.revision));
            }
        }
        Ok(out)
    }
}

impl Office {
    /// The local tasks of `ids`, with their owning office code and effective revision.
    fn local_task_revisions(
        &self,
        ids: impl IntoIterator<Item = Id>,
    ) -> Result<BTreeMap<Id, (String, u64)>, OfficeError> {
        let ids: Vec<Id> = ids.into_iter().collect();
        if ids.is_empty() {
            return Ok(BTreeMap::new());
        }
        let (local, _) = self.store.read(ReadTaskRevisionsOp(ids)).map_err(storage)?;
        Ok(local)
    }

    /// The tasks of a whole snapshot (every page indexed, `TASK-52` P21-#2) this satellite
    /// keeps rather than take from it: those it holds at a strictly later effective revision
    /// than the snapshot brings them to, for the same owning office code. Both sides count
    /// child-only history, so a task with comments, checklist or watcher changes after its
    /// last `task.md` write is never taken for newer here than it is (`doc-79`). Strict, so an
    /// interrupted snapshot resuming at the same revision still applies missing files.
    pub fn protected_snapshot_tasks(
        &self,
        snapshot: &SnapshotTasks,
    ) -> Result<BTreeSet<String>, OfficeError> {
        if self.info.role != crate::model::OfficeRole::Satellite {
            return Ok(BTreeSet::new());
        }
        let local = self.local_task_revisions(snapshot.iter().map(|(id, _)| id.clone()))?;
        let mut protected = BTreeSet::new();
        for (id, incoming) in snapshot.iter() {
            let Some((code, revision)) = local.get(id) else {
                continue;
            };
            let same_office = incoming.office_code.as_ref().is_none_or(|c| c == code);
            if same_office && *revision > incoming.revision {
                protected.insert(id.as_str().to_owned());
            }
        }
        Ok(protected)
    }

    /// Reconciles an entire incoming snapshot against committed local satellite-owned tasks and
    /// pending local outbox edits BEFORE chunking into <= 8 MiB batches (`TASK-52` P11-#2 / P12-#1):
    /// protects every task [`Office::protected_snapshot_tasks`] names, and every task (and all
    /// its files) touched by `pending_work_paths`.
    pub fn filter_stale_snapshot_work_files(
        &self,
        mut files: ReplicatedFileBatch,
        pending_work_paths: &BTreeSet<String>,
    ) -> Result<ReplicatedFileBatch, OfficeError> {
        if self.info.role != crate::model::OfficeRole::Satellite {
            return Ok(files);
        }
        let mut snapshot = SnapshotTasks::default();
        for (path, bytes) in &files {
            if let Some(bytes) = bytes {
                snapshot
                    .observe(path, bytes)
                    .map_err(|_| OfficeError::Invalid("malformed task.md in snapshot"))?;
            }
        }
        let mut protected_tasks = collect_task_ids_from_paths(pending_work_paths);
        for tid in self.protected_snapshot_tasks(&snapshot)? {
            protected_tasks.insert(Id::parse(&tid).map_err(storage)?);
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

    /// Filters out stale self-echoes of this satellite's own single-writer tasks from an HQ
    /// `OrgEvent` so an echo of an older local revision never rolls a task back (`TASK-52`
    /// P8-#1). An echo is judged per task by the revision it brings the task to: its
    /// `task.md`'s or its latest history event's, so a child-only change (a checklist step,
    /// a watcher, a comment: no `task.md`) echoed back after a later local one never
    /// rewrites `checklist.yaml` or `operations.yaml` to the older content. Every written
    /// file of a task this office owns and already holds at that revision or later is
    /// dropped, unless the echo writes its `task.md` at another path: a relocation
    /// (`MoveTeam`) passes whole. The office's task counter is office-local and never
    /// replicated (`doc-80`), so it never echoes.
    pub fn filter_self_echoed_org_event_files(
        &self,
        files: ReplicatedFileBatch,
    ) -> Result<ReplicatedFileBatch, OfficeError> {
        if self.info.role != crate::model::OfficeRole::Satellite {
            return Ok(files);
        }
        let (codes, _) = self.store.read(ReadOfficeCodesOp).map_err(storage)?;
        let local_codes: BTreeSet<String> = codes
            .into_iter()
            .filter(|c| c.office_id == self.identity.office_id)
            .map(|c| c.code)
            .collect();
        if local_codes.is_empty() {
            return Ok(files);
        }
        // A `task.md` that does not decode: the projector refuses the batch whole.
        let Ok(incoming) = incoming_tasks(files.iter().map(|(p, b)| (p.as_str(), b.as_deref())))
        else {
            return Ok(files);
        };
        let local = self.local_task_revisions(incoming.keys().cloned())?;
        let durable_files = Files::open(&self.durable).map_err(OfficeError::storage)?;
        let here = |path: &str| {
            DurablePath::parse(path)
                .ok()
                .is_some_and(|dp| durable_files.read(&dp).ok().flatten().is_some())
        };
        let mut echoed: BTreeSet<Id> = BTreeSet::new();
        for (tid, batch) in &incoming {
            let Some((local_code, local_revision)) = local.get(tid) else {
                continue;
            };
            let code = batch.task.as_ref().map_or(local_code, |t| &t.office_code);
            if !local_codes.contains(code) || *local_revision < batch.revision {
                continue;
            }
            let relocated = files.iter().any(|(path, bytes)| {
                bytes.is_some()
                    && DurablePath::parse(path).is_ok_and(|dp| {
                        matches!(
                            crate::tasks::records::kind_of(&dp),
                            Some(crate::tasks::records::Kind::Task(ref id)) if id == tid
                        )
                    })
                    && !here(path)
            });
            if !relocated {
                echoed.insert(tid.clone());
            }
        }
        if echoed.is_empty() {
            return Ok(files);
        }
        Ok(files
            .into_iter()
            .filter(|(path, bytes)| {
                let task = crate::tasks::replica::task_of_path(path);
                !(bytes.is_some() && task.is_some_and(|tid| echoed.contains(&tid)))
            })
            .collect())
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
    // A shared file's record names its project and its uploader: applied after both
    // (`TASK-98.6`).
    if crate::attachments::records::kind_of(&dp).is_some() {
        return SnapshotPathClass::DependentExecutionDocsAndBudgets;
    }
    if let Some(tk) = crate::tasks::records::kind_of(&dp) {
        return match tk {
            crate::tasks::records::Kind::Task(_) => SnapshotPathClass::WorkCore,
            _ => SnapshotPathClass::WorkSidecar,
        };
    }
    if crate::reviews::files::parse_path(&dp).is_some() {
        return SnapshotPathClass::WorkSidecar;
    }
    if crate::processes::records::parse_path(&dp).is_some() {
        return SnapshotPathClass::DependentExecutionDocsAndBudgets;
    }
    // A project's documentation tree names its project and the employees who wrote it:
    // applied after both, and never through the org-core employee pre-pass (`TASK-55`).
    if crate::project_docs::paths::parse_durable(&dp).is_some() {
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
