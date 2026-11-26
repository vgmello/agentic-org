//! Applying authoritative `state/` and `messaging/` files replicated from HQ onto a satellite's
//! own acceptance journal (`TASK-52` / `S1`). The command spans the organization and the task
//! families (an older replacement-HQ snapshot never downgrades a newer local task), so it lives
//! here, above both, rather than in either family.

use org_store::{Actor, Change, DurablePath, Engine, Id, Operation, Refusal, Stream, UnitOfWork};

use crate::organization::features::{invalid, operation, Op};
use crate::organization::ports::OrganizationRead;
use crate::organization::records::{self, OfficeRole};
use crate::tasks::ports::TasksRead;

mod tasks;

/// Applies authoritative `state/` file updates replicated from HQ onto a satellite's own
/// `org-store` acceptance journal and projection (`TASK-52` / `S1`).
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct ApplyReplicatedFiles {
    /// List of `(durable_path_str, Some(bytes) for put / None for delete)`.
    pub files: Vec<(String, Option<Vec<u8>>)>,
    /// HQ ingesting a satellite's work: each task folder file is written in the folder HQ's
    /// own projection names for its task, so a team HQ moved meanwhile keeps its tasks
    /// together (`MoveTeam`, `doc-77`). Never set for HQ's authoritative batches, whose
    /// paths already follow the chains they carry.
    #[serde(default)]
    pub rehome_tasks: bool,
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
        let rehomed;
        let files = if self.rehome_tasks {
            rehomed = tasks::rehome(uow.view(), &self.files)?;
            &rehomed
        } else {
            &self.files
        };
        let stale_tasks = tasks::stale_tasks(uow.view(), files);
        let leftovers = tasks::relocation_leftovers(uow.view(), files, &stale_tasks)?;
        let mut change = Change::new(Op::ApplyReplicatedFiles.ack());
        let mut deferred_hq_offices: Vec<(DurablePath, Vec<u8>)> = Vec::new();
        for (raw_path, content) in files {
            if !raw_path.starts_with("state/") && !raw_path.starts_with("messaging/") {
                return invalid("replicated files must stay strictly under state/ or messaging/");
            }
            let path = DurablePath::parse(raw_path)
                .map_err(|_| Refusal::invalid("invalid replicated durable path"))?;
            match content {
                Some(bytes) => {
                    if tasks::is_stale_file(&path, &stale_tasks) {
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
        // Files the batch left in a folder it moved a task out of follow the task.
        for (path, bytes) in leftovers {
            change = match bytes {
                Some(bytes) => change.put(path, bytes),
                None => change.remove(path),
            };
        }
        Ok(change)
    }
}
