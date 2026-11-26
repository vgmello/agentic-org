//! The tasks family's files in a replicated batch (`TASK-52`, `doc-77`, `doc-79`):
//!
//! - a task whose local effective revision is already past the batch's is stale, and the
//!   batch's files for it are skipped (an older replacement-HQ snapshot or a self-echo never
//!   downgrades a newer local task), whether or not the batch writes its `task.md`;
//! - on HQ ingesting a satellite's work, each task folder file goes to the folder HQ's own
//!   projection names for its task ([`rehome`]);
//! - when a batch moves a task to another folder (HQ's `MoveTeam`, which moves every
//!   projectless task folder of the moved subtree), the files of the old folder follow it
//!   ([`relocation_leftovers`]), the tasks this office has and HQ never named included;
//! - a removal never takes a task's only copy ([`Removal`]).

use std::collections::{BTreeMap, BTreeSet};

use org_store::{DurablePath, Id, Refusal};

use crate::organization::records as org;
use crate::tasks::ports::TasksRead;
use crate::tasks::records::{self, Kind, Location, TaskDir, TaskRecord};
use crate::tasks::relocation::task_folder_moves;
use crate::tasks::replica::incoming_tasks;

type Files = Vec<(String, Option<Vec<u8>>)>;

fn as_refs(files: &[(String, Option<Vec<u8>>)]) -> impl Iterator<Item = (&str, Option<&[u8]>)> {
    files.iter().map(|(p, b)| (p.as_str(), b.as_deref()))
}

/// Tasks whose committed local revision is already past the revision the batch brings them
/// to: the later of the batch's `task.md` and its latest history event, so a batch of
/// child-only changes (no `task.md`) is judged by its events, or with neither, by its hint
/// alone. `hints` is the revision the
/// whole replicated set the batch is one part of brings a task to (a snapshot applied in
/// several batches, whose `task.md` and history are in different ones). A batch writing
/// `task.md` with another owning office code is never stale; one whose `task.md` does not
/// decode has no stale task: the projector refuses it whole.
pub(super) fn stale_tasks(
    view: &impl TasksRead,
    files: &[(String, Option<Vec<u8>>)],
    hints: &BTreeMap<Id, u64>,
) -> BTreeSet<Id> {
    let Ok(incoming) = incoming_tasks(as_refs(files)) else {
        return BTreeSet::new();
    };
    let mut stale = BTreeSet::new();
    for (id, incoming) in &incoming {
        let Ok(Some(local)) = view.task(id) else {
            continue;
        };
        let other_office = incoming
            .task
            .as_ref()
            .is_some_and(|task| task.office_code != local.office_code);
        let revision = hints
            .get(id)
            .map_or(incoming.revision, |hint| incoming.revision.max(*hint));
        if !other_office && local.meta.revision > revision {
            stale.insert(id.clone());
        }
    }
    // A task the batch has only child files of (no `task.md`, no event) is judged by its hint.
    for (id, &hint) in hints {
        if incoming.contains_key(id) {
            continue;
        }
        if let Ok(Some(local)) = view.task(id) {
            if local.meta.revision > hint {
                stale.insert(id.clone());
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

/// Where each task's files are here, by the files themselves: for a task whose owning team
/// is not known here while a move of it arrives in pieces.
type OnDisk = Option<BTreeMap<Id, Location>>;

/// The folder `task`'s files are in here: the one its fields name, or, when its owning team
/// has no chain here, the one its files are found in.
fn current_location(
    view: &impl TasksRead,
    task: &TaskRecord,
    on_disk: &mut OnDisk,
) -> Result<Option<Location>, Refusal> {
    if let Some(location) = location_of(view, task) {
        return Ok(Some(location));
    }
    if on_disk.is_none() {
        let teams = DurablePath::parse("state/teams").expect("a fixed path");
        let mut found = BTreeMap::new();
        for path in view.sources_under(&teams)? {
            if let Some(dir) = records::task_dir_of(&path) {
                found.entry(dir.task_id).or_insert(dir.location);
            }
        }
        *on_disk = Some(found);
    }
    Ok(on_disk
        .as_ref()
        .and_then(|found| found.get(&task.id).cloned()))
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
            let task = match incoming.get(&dir.task_id).and_then(|i| i.task.as_ref()) {
                Some(task) => Some(task.clone()),
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

/// The chain each team has once the batch applies, for every team whose chain it changes:
/// each team whose `team.yaml` it writes at a chain other than the one the team has here
/// (HQ's `MoveTeam`, or a team this office no longer knows while a move arrives in pieces),
/// and every team below one of those, whose own file may come in a later piece.
fn moved_team_chains(
    view: &impl TasksRead,
    files: &[(String, Option<Vec<u8>>)],
) -> Result<BTreeMap<Id, Vec<Id>>, Refusal> {
    let mut moved = BTreeMap::new();
    for (raw, bytes) in files {
        let path = match (DurablePath::parse(raw), bytes) {
            (Ok(path), Some(_)) => path,
            _ => continue,
        };
        let Some(org::Kind::Team(chain)) = org::kind_of(&path) else {
            continue;
        };
        let Some(team) = chain.last() else {
            continue;
        };
        if view.team_chain(team)?.as_ref() != Some(&chain) {
            moved.insert(team.clone(), chain);
        }
    }
    if moved.is_empty() {
        return Ok(moved);
    }
    let named = moved.clone();
    for team in view.teams()? {
        if named.contains_key(&team.id) {
            continue;
        }
        let Some(chain) = view.team_chain(&team.id)? else {
            continue;
        };
        let above = chain.len().saturating_sub(1);
        let nearest = (0..above)
            .rev()
            .find_map(|at| named.get(&chain[at]).map(|new| (at, new)));
        if let Some((at, new)) = nearest {
            let mut to = new.clone();
            to.extend_from_slice(&chain[at + 1..]);
            moved.insert(team.id, to);
        }
    }
    Ok(moved)
}

/// What a batch moves out of the folders its tasks leave, beside its own files.
#[derive(Default)]
pub(super) struct Leftovers {
    /// The tasks the batch moves to another folder.
    pub relocated: BTreeSet<Id>,
    /// Files of an old folder the batch does not name, written again in the new folder.
    pub puts: Vec<(DurablePath, Vec<u8>)>,
    /// Those files, removed from the old folder.
    pub removes: Vec<DurablePath>,
}

type MovedTaskMap = BTreeMap<Id, (TaskRecord, Location, Location)>;

fn collect_directly_moved_tasks(
    view: &impl TasksRead,
    files: &[(String, Option<Vec<u8>>)],
    on_disk: &mut OnDisk,
) -> Result<MovedTaskMap, Refusal> {
    let mut moving = MovedTaskMap::new();
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
        if let Some(from) = current_location(view, &task, on_disk)? {
            if from != to.location {
                moving.insert(id, (task, from, to.location));
            }
        }
    }
    Ok(moving)
}

fn collect_team_moved_tasks(
    view: &impl TasksRead,
    files: &[(String, Option<Vec<u8>>)],
    on_disk: &mut OnDisk,
    moving: &mut MovedTaskMap,
) -> Result<(), Refusal> {
    let teams = moved_team_chains(view, files)?;
    if teams.is_empty() {
        return Ok(());
    }
    for task in view.tasks()? {
        if task.project_id.is_some() || moving.contains_key(&task.id) {
            continue;
        }
        let Some(chain) = teams.get(&task.team_id) else {
            continue;
        };
        let to = Location::Team(chain.clone());
        if let Some(from) = current_location(view, &task, on_disk)? {
            if from != to {
                moving.insert(task.id.clone(), (task, from, to));
            }
        }
    }
    Ok(())
}

/// Every task the batch moves to another folder, and the files of its old folder the batch
/// does not write or remove. A task moves when the batch writes its `task.md` in another
/// folder, or writes its owning team (or a team above it) at another chain: every local
/// projectless task of a moved subtree moves with `team.yaml` in the same entry, including
/// the tasks HQ has never seen (made here offline) and those whose own files come in a later
/// piece of the move. A stale task's own files are not written by the batch, so all of them
/// move from here, its newer local `task.md` included.
pub(super) fn relocation_leftovers(
    view: &impl TasksRead,
    files: &[(String, Option<Vec<u8>>)],
    stale: &BTreeSet<Id>,
) -> Result<Leftovers, Refusal> {
    let named: BTreeSet<&str> = files
        .iter()
        .filter(|(p, bytes)| {
            let skipped = bytes.is_some()
                && DurablePath::parse(p).is_ok_and(|path| is_stale_file(&path, stale));
            !skipped
        })
        .map(|(p, _)| p.as_str())
        .collect();
    let mut on_disk: OnDisk = None;
    let mut moving = collect_directly_moved_tasks(view, files, &mut on_disk)?;
    collect_team_moved_tasks(view, files, &mut on_disk, &mut moving)?;
    let mut out = Leftovers::default();
    for (id, (task, from, to)) in moving {
        for moved in task_folder_moves(view, &task, &from, &to)? {
            // A file no path can hold under the new chain stays where it is.
            let (Ok(old), Some(new)) = (moved.from, moved.to) else {
                continue;
            };
            if !named.contains(old.as_str()) {
                out.removes.push(old);
            }
            if !named.contains(new.as_str()) {
                out.puts.push((new, moved.bytes));
            }
        }
        out.relocated.insert(id);
    }
    Ok(out)
}

/// What becomes of a removal the batch carries of a task folder file.
pub(super) enum Removal {
    /// Not a task folder file, or one of no task here: removed where it stands.
    Now,
    /// A file of the old folder of a task the batch moves: removed once every write of the
    /// batch stands, so the moved task's records are never taken with it.
    AfterWrites,
    /// A file of the folder the task is in here, which the batch does not move: removing it
    /// would take the task's only copy (a move arriving in pieces, whose new folder comes in
    /// a later piece that moves the files). It stays.
    Kept,
}

/// Judges the batch's removals of task folder files against the tasks it moves.
pub(super) struct Removals<'a> {
    relocated: &'a BTreeSet<Id>,
    on_disk: OnDisk,
}

impl<'a> Removals<'a> {
    /// `relocated`: the tasks the batch moves.
    pub(super) fn new(relocated: &'a BTreeSet<Id>) -> Self {
        Self {
            relocated,
            on_disk: None,
        }
    }

    /// How the batch's removal of `path` applies.
    pub(super) fn of(
        &mut self,
        view: &impl TasksRead,
        path: &DurablePath,
    ) -> Result<Removal, Refusal> {
        let Some(TaskDir { location, task_id }) = records::task_dir_of(path) else {
            return Ok(Removal::Now);
        };
        if self.relocated.contains(&task_id) {
            return Ok(Removal::AfterWrites);
        }
        let Some(task) = view.task(&task_id)? else {
            return Ok(Removal::Now);
        };
        let here = current_location(view, &task, &mut self.on_disk)?;
        Ok(if here.as_ref() == Some(&location) {
            Removal::Kept
        } else {
            Removal::Now
        })
    }
}
