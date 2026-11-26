//! The tasks family's files in a replicated batch (`TASK-52`, `doc-77`, `doc-79`):
//!
//! - a task whose local effective revision is already past the batch's is stale, and the
//!   batch's files for it are skipped (an older replacement-HQ snapshot never downgrades a
//!   newer local task);
//! - on HQ ingesting a satellite's work, each task folder file goes to the folder HQ's own
//!   projection names for its task ([`rehome`]);
//! - when a batch moves a task's `task.md` to another folder (HQ's `MoveTeam`), the files it
//!   left behind in the old folder follow it ([`relocation_leftovers`]).

use std::collections::{BTreeMap, BTreeSet};

use org_store::{DurablePath, Id, Refusal};

use crate::tasks::ports::TasksRead;
use crate::tasks::records::{self, Kind, Location, TaskRecord};
use crate::tasks::relocation::task_folder_moves;
use crate::tasks::replica::incoming_tasks;

type Files = Vec<(String, Option<Vec<u8>>)>;
/// Files to write (`Some`) or remove (`None`) at their paths.
type Moves = Vec<(DurablePath, Option<Vec<u8>>)>;

fn as_refs(files: &[(String, Option<Vec<u8>>)]) -> impl Iterator<Item = (&str, Option<&[u8]>)> {
    files.iter().map(|(p, b)| (p.as_str(), b.as_deref()))
}

/// Tasks whose committed local revision is already past the revision the batch brings them
/// to, for the same owning office code. A batch whose `task.md` does not decode has no stale
/// task: the projector refuses it whole.
pub(super) fn stale_tasks(
    view: &impl TasksRead,
    files: &[(String, Option<Vec<u8>>)],
) -> BTreeSet<Id> {
    let Ok(incoming) = incoming_tasks(as_refs(files)) else {
        return BTreeSet::new();
    };
    let mut stale = BTreeSet::new();
    for (id, incoming) in incoming {
        if let Ok(Some(local)) = view.task(&id) {
            if local.office_code == incoming.task.office_code
                && local.meta.revision > incoming.revision
            {
                stale.insert(id);
            }
        }
    }
    stale
}

/// Whether `path` is a file of a stale task.
pub(super) fn is_stale_file(path: &DurablePath, stale: &BTreeSet<Id>) -> bool {
    records::kind_of(path).is_some_and(|kind| stale.contains(kind.task_id()))
}

fn location_of(view: &impl TasksRead, task: &TaskRecord) -> Option<Location> {
    match &task.project_id {
        Some(project) => Some(Location::Project(project.clone())),
        None => view
            .team_chain(&task.team_id)
            .ok()
            .flatten()
            .map(Location::Team),
    }
}

/// `files`, with every task folder file in the folder this office's projection names for
/// its task: from the batch's own `task.md` when it has one, else from the task as it
/// stands. A file of a task neither names is left where it is, for the projector to judge.
pub(super) fn rehome(
    view: &impl TasksRead,
    files: &[(String, Option<Vec<u8>>)],
) -> Result<Files, Refusal> {
    let incoming = incoming_tasks(as_refs(files)).unwrap_or_default();
    let mut folders: BTreeMap<Id, Option<String>> = BTreeMap::new();
    let mut out = Vec::with_capacity(files.len());
    for (raw, bytes) in files {
        let dir = DurablePath::parse(raw)
            .ok()
            .and_then(|p| records::task_dir_of(&p));
        let Some(dir) = dir else {
            out.push((raw.clone(), bytes.clone()));
            continue;
        };
        let folder = folders.entry(dir.task_id.clone()).or_insert_with(|| {
            let task = match incoming.get(&dir.task_id) {
                Some(incoming) => Some(incoming.task.clone()),
                None => view.task(&dir.task_id).ok().flatten(),
            };
            task.and_then(|t| location_of(view, &t).map(|l| records::task_folder(&l, &t.id)))
        });
        let from = dir.folder();
        let moved = match folder {
            Some(folder) if *folder != from => {
                let path = format!("{folder}{}", &raw[from.len()..]);
                DurablePath::parse(&path).map_err(|_| Refusal::invalid("a rehomed task path"))?;
                path
            }
            _ => raw.clone(),
        };
        out.push((moved, bytes.clone()));
    }
    Ok(out)
}

/// For each task whose `task.md` the batch writes in a folder other than the one the task
/// is in here, the files of the old folder the batch does not write: removed there and
/// written in the new folder from the task's records. A stale task's own files are not
/// written by the batch, so all of them move from here, its newer local `task.md` included.
/// The projector ignores the removal of a file left in a folder its task has moved out of,
/// so the order of the moves is free.
pub(super) fn relocation_leftovers(
    view: &impl TasksRead,
    files: &[(String, Option<Vec<u8>>)],
    stale: &BTreeSet<Id>,
) -> Result<Moves, Refusal> {
    let named: BTreeSet<&str> = files
        .iter()
        .filter(|(p, bytes)| {
            let skipped = bytes.is_some()
                && DurablePath::parse(p).is_ok_and(|path| is_stale_file(&path, stale));
            !skipped
        })
        .map(|(p, _)| p.as_str())
        .collect();
    let mut removes = Vec::new();
    let mut puts = Vec::new();
    for (raw, bytes) in files {
        let Ok(path) = DurablePath::parse(raw) else {
            continue;
        };
        let (Some(parsed), Some(_)) = (records::parse_path(&path), bytes) else {
            continue;
        };
        let (Kind::Task(id), Some(to)) = (parsed.kind, parsed.dir) else {
            continue;
        };
        let Some(task) = view.task(&id)? else {
            continue;
        };
        let Some(from) = location_of(view, &task) else {
            continue;
        };
        if from == to.location {
            continue;
        }
        for moved in task_folder_moves(view, &task, &from, &to.location)? {
            let Ok(old) = moved.from else {
                continue;
            };
            if !named.contains(old.as_str()) {
                removes.push((old, None));
            }
            if let Some(new) = moved.to {
                if !named.contains(new.as_str()) {
                    puts.push((new, Some(moved.bytes)));
                }
            }
        }
    }
    removes.extend(puts);
    Ok(removes)
}
