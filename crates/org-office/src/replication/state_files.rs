//! Applying authoritative `state/` and `messaging/` files replicated from HQ onto a satellite's
//! own acceptance journal (`TASK-52` / `S1`). The command spans the organization and the task
//! families (an older replacement-HQ snapshot never downgrades a newer local task), so it lives
//! here, above both, rather than in either family.

use std::collections::BTreeSet;

use org_store::{Actor, Change, DurablePath, Engine, Id, Operation, Refusal, Stream, UnitOfWork};

use crate::organization::features::{invalid, operation, Op};
use crate::organization::ports::OrganizationRead;
use crate::organization::records::{self, OfficeRole};
use crate::tasks::ports::TasksRead;
use crate::tasks::records as task_records;

/// Applies authoritative `state/` file updates replicated from HQ onto a satellite's own
/// `org-store` acceptance journal and projection (`TASK-52` / `S1`).
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct ApplyReplicatedFiles {
    /// List of `(durable_path_str, Some(bytes) for put / None for delete)`.
    pub files: Vec<(String, Option<Vec<u8>>)>,
}

impl ApplyReplicatedFiles {
    pub fn operation(&self, id: Id, actor: Actor) -> Operation {
        operation(Op::ApplyReplicatedFiles, id, actor, self)
    }

    pub fn decide<E: Engine>(&self, uow: &mut UnitOfWork<'_, E>) -> Result<Change, Refusal>
    where
        E::Tx: OrganizationRead + TasksRead,
    {
        let _ = uow.next(Stream::Organization)?;
        let stale_tasks = collect_stale_replicated_tasks(uow.view(), &self.files);
        let mut change = Change::new(Op::ApplyReplicatedFiles.ack());
        let mut deferred_hq_offices: Vec<(DurablePath, Vec<u8>)> = Vec::new();
        for (raw_path, content) in &self.files {
            if !raw_path.starts_with("state/") && !raw_path.starts_with("messaging/") {
                return invalid("replicated files must stay strictly under state/ or messaging/");
            }
            let path = DurablePath::parse(raw_path)
                .map_err(|_| Refusal::invalid("invalid replicated durable path"))?;
            match content {
                Some(bytes) => {
                    if should_skip_stale_work_file(uow.view(), &path, bytes, &stale_tasks) {
                        continue;
                    }
                    // During D25 HQ replacement, a new HQ office file may sort lexicographically
                    // before the demoted previous HQ office file. Defer `role == Hq` office writes
                    // so the demoted office releases `slug = "hq"` before the new HQ claims it.
                    let is_hq_office =
                        matches!(records::kind_of(&path), Some(records::Kind::Office(_)))
                            && records::decode_office(bytes)
                                .is_ok_and(|o| o.role == OfficeRole::Hq);
                    if is_hq_office {
                        deferred_hq_offices.push((path, bytes.clone()));
                    } else {
                        change = change.put(path, bytes.clone());
                    }
                }
                None => change = change.remove(path),
            }
        }
        for (path, bytes) in deferred_hq_offices {
            change = change.put(path, bytes);
        }
        Ok(change)
    }
}

/// Identifies task IDs whose committed local revision is already >= the incoming replicated
/// task revision for the same owning `office_code` so an older replacement-HQ snapshot never
/// downgrades a local task while leaving higher-numbered immutable events behind (`TASK-52` P10-#4).
fn collect_stale_replicated_tasks(
    view: &impl TasksRead,
    files: &[(String, Option<Vec<u8>>)],
) -> BTreeSet<Id> {
    let mut stale = BTreeSet::new();
    for (raw_path, content) in files {
        let Some(bytes) = content else {
            continue;
        };
        let Ok(path) = DurablePath::parse(raw_path) else {
            continue;
        };
        let Some(task_records::Kind::Task(tid)) = task_records::kind_of(&path) else {
            continue;
        };
        let Ok(incoming) = serde_json::from_slice::<task_records::TaskRecord>(bytes) else {
            continue;
        };
        if let Ok(Some(local)) = view.task(&tid) {
            if local.office_code == incoming.office_code
                && local.meta.revision > incoming.meta.revision
            {
                stale.insert(tid);
            }
        }
    }
    stale
}

fn should_skip_stale_work_file(
    view: &impl TasksRead,
    path: &DurablePath,
    bytes: &[u8],
    stale_tasks: &BTreeSet<Id>,
) -> bool {
    match task_records::kind_of(path) {
        Some(task_records::Kind::OfficeTaskCounter(code)) => {
            let incoming_num = serde_json::from_slice::<serde_json::Value>(bytes)
                .ok()
                .and_then(|v| v.get("last_number").and_then(serde_json::Value::as_u64));
            matches!(
                (incoming_num, view.office_task_counter(&code)),
                (Some(in_num), Ok(Some(local))) if local.last_number >= in_num
            )
        }
        Some(
            task_records::Kind::Task(tid)
            | task_records::Kind::Checklist(tid)
            | task_records::Kind::Comment(tid, _)
            | task_records::Kind::Watchers(tid)
            | task_records::Kind::Links(tid)
            | task_records::Kind::Event(tid, _),
        ) => stale_tasks.contains(&tid),
        None => false,
    }
}
