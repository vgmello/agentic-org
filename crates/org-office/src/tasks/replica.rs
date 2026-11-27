//! What replication reads of the task files in a batch (`TASK-52`, `doc-79`): which tasks a
//! batch touches, and the revision it brings each to.
//!
//! A task's effective revision is `max(task.md revision, latest history event revision)`, so
//! a batch's revision for a task counts the history events it carries beside `task.md`, and
//! a batch of child-only changes (a comment, a watcher, a checklist step) carries no
//! `task.md` at all: its revision is its latest event's. Comparing a local revision with
//! `task.md`'s alone would call a newer batch stale and an older one new.

use std::collections::BTreeMap;

use org_store::{DurablePath, Id};

use super::records::{self, Kind, TaskRecord};

/// A task a batch writes `task.md` or history events for.
#[derive(Clone, Debug)]
pub struct IncomingTask {
    /// The batch's `task.md` for the task, decoded; `None` when the batch carries only
    /// child files and history (a child-only change).
    pub task: Option<TaskRecord>,
    /// The revision the batch brings the task to: the later of `task.md`'s and its latest
    /// history event's in the batch.
    pub revision: u64,
}

/// Every task whose `task.md` or history events the batch puts, with the revision the batch
/// brings it to. A `task.md` that does not decode is refused with its path.
pub fn incoming_tasks<'a>(
    files: impl IntoIterator<Item = (&'a str, Option<&'a [u8]>)>,
) -> Result<BTreeMap<Id, IncomingTask>, String> {
    let mut tasks: BTreeMap<Id, IncomingTask> = BTreeMap::new();
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
                let entry = tasks.entry(id).or_insert(IncomingTask {
                    task: None,
                    revision: 0,
                });
                entry.revision = entry.revision.max(doc.task.meta.revision);
                entry.task = Some(doc.task);
            }
            (Some(Kind::Event(id, revision)), Some(_)) => {
                let entry = tasks.entry(id).or_insert(IncomingTask {
                    task: None,
                    revision: 0,
                });
                entry.revision = entry.revision.max(revision);
            }
            _ => {}
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

/// What a whole snapshot brings one task to, kept as its pages stream past.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SnapshotTask {
    /// The owning office code from the snapshot's `task.md`, once a page has carried it.
    pub office_code: Option<String>,
    /// The snapshot's effective revision for the task: the later of its `task.md`'s and its
    /// latest history event's, over every page.
    pub revision: u64,
}

/// The tasks of a snapshot that arrives in pages (`TASK-52` P21-#2), indexed one file at a
/// time without keeping any file: per task, only its owning office code and its effective
/// revision over every page. A task's `task.md` and its history are in different pages
/// (and different apply tiers), so neither alone gives the revision the snapshot brings it
/// to: a task with child-only changes after its last `task.md` write is at its latest event.
#[derive(Clone, Debug, Default)]
pub struct SnapshotTasks {
    tasks: BTreeMap<Id, SnapshotTask>,
}

impl SnapshotTasks {
    /// Takes one written file of the snapshot into account. A `task.md` that does not decode
    /// is refused with its path.
    pub fn observe(&mut self, raw: &str, bytes: &[u8]) -> Result<(), String> {
        let Ok(path) = DurablePath::parse(raw) else {
            return Ok(());
        };
        match records::kind_of(&path) {
            Some(Kind::Task(id)) => {
                let doc = records::decode_task_md(bytes).map_err(|_| raw.to_owned())?;
                if doc.task.id != id {
                    return Err(raw.to_owned());
                }
                let entry = self.tasks.entry(id).or_default();
                entry.revision = entry.revision.max(doc.task.meta.revision);
                entry.office_code = Some(doc.task.office_code);
            }
            Some(Kind::Event(id, revision)) => {
                let entry = self.tasks.entry(id).or_default();
                entry.revision = entry.revision.max(revision);
            }
            _ => {}
        }
        Ok(())
    }

    /// Every task the snapshot carries, with what it brings each to.
    pub fn iter(&self) -> impl Iterator<Item = (&Id, &SnapshotTask)> {
        self.tasks.iter()
    }

    /// The effective revision the snapshot brings each task of `files` to.
    pub fn revisions_of<'a>(
        &self,
        files: impl IntoIterator<Item = &'a str>,
    ) -> BTreeMap<String, u64> {
        files
            .into_iter()
            .filter_map(task_of_path)
            .filter_map(|id| {
                let revision = self.tasks.get(&id)?.revision;
                Some((id.as_str().to_owned(), revision))
            })
            .collect()
    }
}
