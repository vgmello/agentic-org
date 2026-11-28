//! What a rebuild of the tasks family checks across files (`ADR-0018`, `doc-79`):
//!
//! - every task is in the folder its project or its owning team's chain names, and every
//!   file of a task folder is in its task's folder;
//! - every task's history is contiguous from revision 1, no event is for an unknown task,
//!   and the task's effective revision is its latest event's: a `task.md` whose revision is
//!   above its history's is refused;
//! - task keys are unique, owners and repositories exist, and dependencies are acyclic.
//!
//! The checks read in bulk (`TASK-105`): every task, every history outline and every source
//! under the task folders at once, and each owning team, team chain and project once
//! however many tasks name it, so a rebuild costs no reads per task.

use std::collections::{BTreeMap, BTreeSet};

use org_store::{DurablePath, Id, ProjectionError};

use crate::tasks::{
    ports::TasksWrite,
    query::TaskHistoryOutline,
    records::{self, Location, TaskRecord},
};

/// The prefixes every task folder is under.
const TASK_FOLDER_ROOTS: [&str; 2] = ["state/projects", "state/teams"];

fn refuse(path: &DurablePath, reason: impl Into<String>) -> ProjectionError {
    ProjectionError::new(path, reason)
}

fn fixed(text: &str) -> DurablePath {
    DurablePath::parse(text).expect("a fixed path")
}

/// Owners looked up once each, however many tasks name them.
#[derive(Default)]
struct Owners {
    chains: BTreeMap<Id, Option<Vec<Id>>>,
    teams: BTreeMap<Id, bool>,
    /// A project's owning team, `None` when there is no such project.
    projects: BTreeMap<Id, Option<Id>>,
}

impl Owners {
    fn chain<Tx: TasksWrite>(
        &mut self,
        tx: &Tx,
        team: &Id,
    ) -> Result<Option<Vec<Id>>, ProjectionError> {
        if let Some(found) = self.chains.get(team) {
            return Ok(found.clone());
        }
        let found = tx.team_chain(team)?;
        self.chains.insert(team.clone(), found.clone());
        Ok(found)
    }

    fn team_exists<Tx: TasksWrite>(&mut self, tx: &Tx, team: &Id) -> Result<bool, ProjectionError> {
        if let Some(found) = self.teams.get(team) {
            return Ok(*found);
        }
        let found = tx.team(team)?.is_some();
        self.teams.insert(team.clone(), found);
        Ok(found)
    }

    fn project_owner<Tx: TasksWrite>(
        &mut self,
        tx: &Tx,
        project: &Id,
    ) -> Result<Option<Id>, ProjectionError> {
        if let Some(found) = self.projects.get(project) {
            return Ok(found.clone());
        }
        let found = tx.project(project)?.map(|p| p.owner_team_id);
        self.projects.insert(project.clone(), found.clone());
        Ok(found)
    }
}

/// The location a task's own fields name: its project, or its owning team's chain.
fn location_of<Tx: TasksWrite>(
    tx: &Tx,
    owners: &mut Owners,
    task: &TaskRecord,
) -> Result<Location, ProjectionError> {
    if let Some(project) = &task.project_id {
        return Ok(Location::Project(project.clone()));
    }
    let chain = owners.chain(tx, &task.team_id)?;
    let path = fixed("state/teams");
    chain
        .map(Location::Team)
        .ok_or_else(|| refuse(&path, format!("task {} has no owning team chain", task.id)))
}

/// Validates task referential integrity, placement, history, unique office-coded keys, and
/// dependency acyclicity across both Memory and SQLite engines (`ADR-0018`).
pub(super) fn validate_tasks<Tx: TasksWrite>(tx: &Tx) -> Result<(), ProjectionError> {
    let tasks = tx.tasks()?;
    let repo_ids: BTreeSet<Id> = tx.repositories()?.into_iter().map(|r| r.id).collect();
    let histories = tx.task_history_outlines()?;
    let mut folder_files = Vec::new();
    for root in TASK_FOLDER_ROOTS {
        folder_files.extend(tx.sources_under(&fixed(root))?);
    }
    let sources: BTreeSet<&DurablePath> = folder_files.iter().collect();
    let mut owners = Owners::default();
    let mut keys: BTreeSet<(String, u64)> = BTreeSet::new();
    let mut folders: BTreeMap<Id, String> = BTreeMap::new();
    for t in &tasks {
        let location = location_of(tx, &mut owners, t)?;
        let path = records::task_md_path(&location, &t.id)
            .map_err(|_| refuse(&fixed("state/teams"), "a task folder is too deep"))?;
        if !sources.contains(&path) {
            return Err(refuse(
                &path,
                "the task is not in the folder its fields name",
            ));
        }
        if t.number == 0 {
            return Err(refuse(&path, "task number must be at least 1"));
        }
        if !keys.insert((t.office_code.clone(), t.number)) {
            return Err(refuse(&path, "duplicate task key"));
        }
        if !owners.team_exists(tx, &t.team_id)? {
            return Err(refuse(&path, "owning team does not exist"));
        }
        if let Some(proj_id) = &t.project_id {
            let Some(owner_team_id) = owners.project_owner(tx, proj_id)? else {
                return Err(refuse(&path, "project does not exist"));
            };
            if owner_team_id != t.team_id {
                return Err(refuse(
                    &path,
                    "project task owning team must match project owner team",
                ));
            }
        }
        validate_task_delivery_refs(tx, t, &path)?;
        let mut seen_repos = BTreeSet::new();
        for tr in &t.repositories {
            if !seen_repos.insert(&tr.repository_id) {
                return Err(refuse(&path, "duplicate repository on task"));
            }
            if !repo_ids.contains(&tr.repository_id) {
                return Err(refuse(&path, "task repository does not exist"));
            }
        }
        if let Some(summary) = &t.final_summary {
            if tx.employee(&summary.by_id)?.is_none() {
                return Err(refuse(&path, "final_summary author does not exist"));
            }
        }
        validate_history(histories.get(&t.id), t, &path)?;
        folders.insert(t.id.clone(), records::task_folder(&location, &t.id));
    }
    validate_placement(tx, &folder_files, &folders)?;
    validate_dependency_dag(tx, &folders)
}

/// Validates task delivery references (`feature_id`, `milestone_id`, `details.affects_feature_id`)
/// belong to the task's project and are absent on projectless team tasks.
fn validate_task_delivery_refs<Tx: TasksWrite>(
    tx: &Tx,
    t: &TaskRecord,
    path: &DurablePath,
) -> Result<(), ProjectionError> {
    let affects = t
        .details
        .as_ref()
        .and_then(|d| d.affects_feature_id.as_ref());
    let Some(proj_id) = &t.project_id else {
        if t.feature_id.is_some() || t.milestone_id.is_some() || affects.is_some() {
            return Err(refuse(
                path,
                "team task cannot set feature_id, milestone_id, or affects_feature_id without project_id",
            ));
        }
        return Ok(());
    };
    if let Some(fid) = &t.feature_id {
        if !tx.feature_in_project(proj_id, fid)? {
            return Err(refuse(path, "task feature_id does not exist in project"));
        }
    }
    if let Some(mid) = &t.milestone_id {
        if !tx.milestone_in_project(proj_id, mid)? {
            return Err(refuse(path, "task milestone_id does not exist in project"));
        }
    }
    if let Some(afid) = affects {
        if !tx.feature_in_project(proj_id, afid)? {
            return Err(refuse(
                path,
                "task details.affects_feature_id does not exist in project",
            ));
        }
    }
    Ok(())
}

/// Every task folder file (`folder_files`: the sources under [`TASK_FOLDER_ROOTS`]) is in
/// its task's folder, and every history event is a task's.
fn validate_placement<Tx: TasksWrite>(
    tx: &Tx,
    folder_files: &[DurablePath],
    folders: &BTreeMap<Id, String>,
) -> Result<(), ProjectionError> {
    for path in folder_files {
        let Some(dir) = records::task_dir_of(path) else {
            continue;
        };
        match folders.get(&dir.task_id) {
            Some(folder) if *folder == dir.folder() => {}
            Some(_) => return Err(refuse(path, "not in its task's folder")),
            None => return Err(refuse(path, "a file of a task that does not exist")),
        }
    }
    for path in tx.sources_under(&fixed("state/work-history/tasks"))? {
        let Some(kind) = records::kind_of(&path) else {
            continue;
        };
        if !folders.contains_key(kind.task_id()) {
            return Err(refuse(&path, "history of a task that does not exist"));
        }
    }
    Ok(())
}

/// The history runs 1, 2, ... with no gap, and the task's effective revision is its last
/// event's (`max(task.md revision, latest event revision)` equals the latest event's only
/// when `task.md` is not ahead of its history).
fn validate_history(
    outline: Option<&TaskHistoryOutline>,
    task: &TaskRecord,
    path: &DurablePath,
) -> Result<(), ProjectionError> {
    let revisions = outline.map_or(&[][..], |o| o.revisions.as_slice());
    for (expected, revision) in (1u64..).zip(revisions) {
        if *revision != expected {
            let at = records::event_path(&task.id, expected);
            return Err(refuse(&at, "the task's history has a gap here"));
        }
    }
    let latest = revisions.last().copied().unwrap_or(0);
    if latest == 0 {
        return Err(refuse(path, "the task has no history"));
    }
    // Revision 1 is the task's `created` change (`doc-79`), whatever wrote the row. The gap
    // check above has made revision 1 the first.
    let created = outline
        .and_then(|o| o.created_change_json.as_deref())
        .is_some_and(|change| {
            serde_json::from_str::<serde_json::Value>(change)
                .is_ok_and(|change| change.get("type") == Some(&serde_json::json!("created")))
        });
    if !created {
        let at = records::event_path(&task.id, 1);
        return Err(refuse(&at, "revision 1 is not the task's `created` change"));
    }
    if task.meta.revision > latest {
        return Err(refuse(
            path,
            format!(
                "task.md is at revision {} but its history ends at {latest}",
                task.meta.revision
            ),
        ));
    }
    Ok(())
}

/// Dependencies name existing tasks and never form a cycle. A refusal names the `task.md`
/// of the task whose link is at fault.
fn validate_dependency_dag<Tx: TasksWrite>(
    tx: &Tx,
    folders: &BTreeMap<Id, String>,
) -> Result<(), ProjectionError> {
    let links = tx.all_task_links()?;
    let mut adj: BTreeMap<Id, Vec<Id>> = BTreeMap::new();
    let task_ids: BTreeSet<&Id> = folders.keys().collect();
    let at = |id: &Id| match folders.get(id) {
        Some(folder) => fixed(&format!("{folder}/task.md")),
        None => fixed("state"),
    };
    for link in &links {
        if link.kind != "depends_on" {
            continue;
        }
        if link.source_id == link.target_id {
            return Err(refuse(
                &at(&link.source_id),
                "a task cannot depend on itself",
            ));
        }
        if !task_ids.contains(&link.source_id) || !task_ids.contains(&link.target_id) {
            return Err(refuse(
                &at(&link.source_id),
                "dependency link references unknown task",
            ));
        }
        adj.entry(link.source_id.clone())
            .or_default()
            .push(link.target_id.clone());
    }
    // 3-color DFS cycle detection (0 = unvisited, 1 = visiting, 2 = done).
    let mut color: BTreeMap<&Id, u8> = BTreeMap::new();
    for start in adj.keys() {
        if *color.get(start).unwrap_or(&0) != 0 {
            continue;
        }
        if has_cycle(start, &adj, &mut color) {
            return Err(refuse(&at(start), "dependency cycle is not allowed"));
        }
    }
    Ok(())
}

fn has_cycle<'a>(
    node: &'a Id,
    adj: &'a BTreeMap<Id, Vec<Id>>,
    color: &mut BTreeMap<&'a Id, u8>,
) -> bool {
    color.insert(node, 1);
    if let Some(neighbors) = adj.get(node) {
        for next in neighbors {
            let state = *color.get(next).unwrap_or(&0);
            match state {
                1 => return true,
                0 if has_cycle(next, adj, color) => return true,
                _ => {}
            }
        }
    }
    color.insert(node, 2);
    false
}
