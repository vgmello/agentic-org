//! Where the tasks family's files live (`doc-77`, `doc-79`, `doc-80`):
//!
//! - `state/projects/<project-id>/work/tasks/<task-id>/` for a project task, and
//!   `state/teams/<top>/teams/<child>/.../work/tasks/<task-id>/` for a projectless task, the
//!   full chain of its owning team's folder as the organization family keeps it;
//! - `state/work-history/tasks/<task-id>/<revision>.json` for its immutable history, the
//!   revision in decimal without leading zeros (`canonical-json/2` filename identity);
//! - `local/offices/<office-id>/task-counter.yaml` for an office's own task counter.
//!
//! Every function decides by the shape of the path alone, so every path has at most one
//! owner.

use org_store::{DurablePath, Id, StoreError};

use crate::organization::records::{is_stable_id, path_number, team_folder};

/// Where a task's folder is: under its project, or under its owning team's chain.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Location {
    Project(Id),
    /// The owning team's chain, from the top-level team down to it.
    Team(Vec<Id>),
}

impl Location {
    /// The folder the task folders of this location are in, without a trailing `/`.
    pub fn folder(&self) -> String {
        match self {
            Location::Project(project) => format!("state/projects/{project}"),
            Location::Team(chain) => team_folder(chain),
        }
    }
}

/// One task's folder.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TaskDir {
    pub location: Location,
    pub task_id: Id,
}

impl TaskDir {
    pub fn new(location: Location, task_id: Id) -> Self {
        Self { location, task_id }
    }

    /// `<location>/work/tasks/<task-id>`.
    pub fn folder(&self) -> String {
        task_folder(&self.location, &self.task_id)
    }
}

/// The file of the tasks family a `state/` path holds.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Kind {
    /// `task.md`.
    Task(Id),
    /// `comments/<comment-id>.md`: the task and the comment.
    Comment(Id, Id),
    /// `checklist.yaml`.
    Checklist(Id),
    /// `operations.yaml`: watchers, owner questions and waiting spans.
    Operations(Id),
    /// `state/work-history/tasks/<task-id>/<revision>.json`.
    Event(Id, u64),
}

impl Kind {
    pub fn task_id(&self) -> &Id {
        match self {
            Kind::Task(id)
            | Kind::Comment(id, _)
            | Kind::Checklist(id)
            | Kind::Operations(id)
            | Kind::Event(id, _) => id,
        }
    }
}

/// A parsed path of the family: its kind, and the task folder it is in (`None` for a
/// history event, which is not under the task's folder).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TaskPath {
    pub kind: Kind,
    pub dir: Option<TaskDir>,
}

fn stable(text: &str) -> Option<Id> {
    is_stable_id(text).then(|| Id::parse(text).ok()).flatten()
}

/// `<location>/work/tasks/<task-id>`.
pub fn task_folder(location: &Location, task_id: &Id) -> String {
    format!("{}/work/tasks/{task_id}", location.folder())
}

fn in_folder(location: &Location, task_id: &Id, file: &str) -> Result<DurablePath, StoreError> {
    DurablePath::parse(&format!("{}/{file}", task_folder(location, task_id)))
}

/// `task.md` of the task in `location`. Refused when the team tree is too deep for a path.
pub fn task_md_path(location: &Location, task_id: &Id) -> Result<DurablePath, StoreError> {
    in_folder(location, task_id, "task.md")
}

pub fn comment_path(
    location: &Location,
    task_id: &Id,
    comment_id: &Id,
) -> Result<DurablePath, StoreError> {
    in_folder(location, task_id, &format!("comments/{comment_id}.md"))
}

pub fn checklist_path(location: &Location, task_id: &Id) -> Result<DurablePath, StoreError> {
    in_folder(location, task_id, "checklist.yaml")
}

pub fn operations_path(location: &Location, task_id: &Id) -> Result<DurablePath, StoreError> {
    in_folder(location, task_id, "operations.yaml")
}

/// `state/work-history/tasks/<task-id>`: the folder of a task's history events.
pub fn history_folder(task_id: &Id) -> DurablePath {
    DurablePath::parse(&format!("state/work-history/tasks/{task_id}")).expect("a short path")
}

/// `state/work-history/tasks/<task-id>/<revision>.json`.
pub fn event_path(task_id: &Id, revision: u64) -> DurablePath {
    history_folder(task_id)
        .join(&format!("{revision}.json"))
        .expect("a decimal file name")
}

/// `local/offices/<office-id>/task-counter.yaml`: the office's own task counter.
pub fn counter_path(office_id: &Id) -> DurablePath {
    DurablePath::parse(&format!("local/offices/{office_id}/task-counter.yaml"))
        .expect("a short path")
}

/// Whether `path` is `office_id`'s task counter.
pub fn is_counter_path(path: &DurablePath, office_id: &Id) -> bool {
    *path == counter_path(office_id)
}

/// The file of a task folder: what is below `<location>/work/tasks/<task-id>/`.
fn task_file(task_id: Id, rest: &[&str]) -> Option<Kind> {
    match rest {
        ["task.md"] => Some(Kind::Task(task_id)),
        ["checklist.yaml"] => Some(Kind::Checklist(task_id)),
        ["operations.yaml"] => Some(Kind::Operations(task_id)),
        ["comments", name] => {
            let comment = stable(name.strip_suffix(".md")?)?;
            Some(Kind::Comment(task_id, comment))
        }
        _ => None,
    }
}

/// A task's folder and file below a location: `work/tasks/<task-id>/<file>`.
fn below_location(location: Location, rest: &[&str]) -> Option<TaskPath> {
    let ["work", "tasks", task, file @ ..] = rest else {
        return None;
    };
    let task_id = stable(task)?;
    let kind = task_file(task_id.clone(), file)?;
    Some(TaskPath {
        kind,
        dir: Some(TaskDir::new(location, task_id)),
    })
}

/// The family's file `path` holds, with its folder; `None` when `path` is not one of the
/// family's `state/` files. The office-local counter is not a `state/` file
/// ([`is_counter_path`]).
pub fn parse_path(path: &DurablePath) -> Option<TaskPath> {
    let parts: Vec<&str> = path.components().collect();
    match parts.as_slice() {
        ["state", "work-history", "tasks", task, file] => {
            let task_id = stable(task)?;
            let revision = file.strip_suffix(".json").and_then(path_number)?;
            (revision <= i64::MAX as u64).then_some(TaskPath {
                kind: Kind::Event(task_id, revision),
                dir: None,
            })
        }
        ["state", "projects", project, rest @ ..] => {
            below_location(Location::Project(stable(project)?), rest)
        }
        ["state", "teams", ..] => {
            let mut chain = Vec::new();
            let mut rest = &parts[1..];
            while let ["teams", team, below @ ..] = rest {
                chain.push(stable(team)?);
                rest = below;
            }
            below_location(Location::Team(chain), rest)
        }
        _ => None,
    }
}

/// Which file of the family `path` holds; `None` when it is not one of its `state/` files.
pub fn kind_of(path: &DurablePath) -> Option<Kind> {
    parse_path(path).map(|parsed| parsed.kind)
}

/// The task folder `path` is in; `None` for a history event or a path not of the family.
pub fn task_dir_of(path: &DurablePath) -> Option<TaskDir> {
    parse_path(path).and_then(|parsed| parsed.dir)
}

#[cfg(test)]
#[path = "paths_tests.rs"]
mod tests;
