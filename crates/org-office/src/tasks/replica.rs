//! What replication reads of the task files in a batch (`TASK-52`, `doc-79`): which tasks a
//! batch writes `task.md` for, and the revision it brings each to.
//!
//! A task's effective revision is `max(task.md revision, latest history event revision)`, so
//! a batch's revision for a task counts the history events it carries beside `task.md`:
//! comparing a local revision with `task.md`'s alone would call a newer batch stale.

use std::collections::BTreeMap;

use org_store::{DurablePath, Id};

use super::records::{self, Kind, TaskRecord};

/// A task whose `task.md` a batch writes.
#[derive(Clone, Debug)]
pub struct IncomingTask {
    pub task: TaskRecord,
    /// The revision the batch brings the task to: `task.md`'s, or its latest event in the
    /// batch when that is later.
    pub revision: u64,
}

/// Every task whose `task.md` the batch puts, decoded, with the revision the batch brings it
/// to. A `task.md` that does not decode is refused with its path.
pub fn incoming_tasks<'a>(
    files: impl IntoIterator<Item = (&'a str, Option<&'a [u8]>)>,
) -> Result<BTreeMap<Id, IncomingTask>, String> {
    let mut tasks = BTreeMap::new();
    let mut latest: BTreeMap<Id, u64> = BTreeMap::new();
    for (raw, bytes) in files {
        let Ok(path) = DurablePath::parse(raw) else {
            continue;
        };
        match (records::kind_of(&path), bytes) {
            (Some(Kind::Task(id)), Some(raw_bytes)) => {
                let doc = records::decode_task_md(raw_bytes).map_err(|_| raw.to_owned())?;
                if doc.task.id != id {
                    return Err(raw.to_owned());
                }
                let revision = doc.task.meta.revision;
                tasks.insert(
                    id,
                    IncomingTask {
                        task: doc.task,
                        revision,
                    },
                );
            }
            (Some(Kind::Event(id, revision)), Some(_)) => {
                let entry = latest.entry(id).or_insert(0);
                *entry = (*entry).max(revision);
            }
            _ => {}
        }
    }
    for (id, incoming) in &mut tasks {
        if let Some(event) = latest.get(id) {
            incoming.revision = incoming.revision.max(*event);
        }
    }
    Ok(tasks)
}

/// The task a task folder file or history event is of; `None` for any other path.
pub fn task_of_path(raw: &str) -> Option<Id> {
    DurablePath::parse(raw)
        .ok()
        .and_then(|path| records::kind_of(&path))
        .map(|kind| kind.task_id().clone())
}
