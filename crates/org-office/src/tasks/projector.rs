//! Engine-independent projector for the unified tasks family (`TaskFiles`).

use std::collections::{BTreeMap, BTreeSet};

use org_store::{DurablePath, Engine, Id, ProjectionError, Projector};

use super::{
    ports::TasksWrite,
    records::{
        self, decode_json, ChecklistStepRecord, Kind, OfficeTaskCounterRecord, TaskCommentRecord,
        TaskEventRecord, TaskLinkRecord, TaskRecord, TaskWatcherRecord,
    },
};

pub struct TaskFiles;

impl TaskFiles {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TaskFiles {
    fn default() -> Self {
        Self::new()
    }
}

fn refuse(path: &DurablePath, reason: impl Into<String>) -> ProjectionError {
    ProjectionError::new(path, reason)
}

fn agree(path: &DurablePath, ok: bool, what: &str) -> Result<(), ProjectionError> {
    if ok {
        Ok(())
    } else {
        Err(refuse(path, format!("{what} does not match the path")))
    }
}

impl<E: Engine> Projector<E> for TaskFiles
where
    E::Tx: TasksWrite,
{
    fn owns(&self, path: &DurablePath) -> bool {
        records::kind_of(path).is_some()
    }

    fn apply(
        &self,
        tx: &mut E::Tx,
        path: &DurablePath,
        bytes: Option<&[u8]>,
    ) -> Result<(), ProjectionError> {
        let Some(kind) = records::kind_of(path) else {
            return Err(refuse(path, "not a task file"));
        };
        apply_kind(tx, path, kind, bytes)
    }

    fn validate(&self, tx: &E::Tx) -> Result<(), ProjectionError> {
        validate_tasks(tx)
    }
}

fn apply_kind<Tx: TasksWrite>(
    tx: &mut Tx,
    path: &DurablePath,
    kind: Kind,
    bytes: Option<&[u8]>,
) -> Result<(), ProjectionError> {
    match (kind, bytes) {
        (Kind::OfficeTaskCounter(code), Some(raw)) => {
            let rec: OfficeTaskCounterRecord =
                decode_json(raw).map_err(|e| refuse(path, e.to_string()))?;
            agree(path, rec.office_code == code, "office_code")?;
            tx.put_office_task_counter(&rec)?;
        }
        (Kind::OfficeTaskCounter(code), None) => tx.remove_office_task_counter(&code)?,
        (Kind::Task(id), Some(raw)) => {
            let rec: TaskRecord = decode_json(raw).map_err(|e| refuse(path, e.to_string()))?;
            agree(path, rec.id == id, "task_id")?;
            tx.put_task(&rec)?;
        }
        (Kind::Task(id), None) => tx.remove_task(&id)?,
        (Kind::Checklist(tid), Some(raw)) => {
            let steps: Vec<ChecklistStepRecord> =
                decode_json(raw).map_err(|e| refuse(path, e.to_string()))?;
            tx.put_checklist_steps(&tid, &steps)?;
        }
        (Kind::Checklist(tid), None) => tx.put_checklist_steps(&tid, &[])?,
        (Kind::Comment(tid, cid), Some(raw)) => {
            let rec: TaskCommentRecord =
                decode_json(raw).map_err(|e| refuse(path, e.to_string()))?;
            agree(path, rec.task_id == tid && rec.id == cid, "comment_id")?;
            tx.put_task_comment(&rec)?;
        }
        (Kind::Comment(tid, cid), None) => tx.remove_task_comment(&tid, &cid)?,
        (Kind::Watchers(tid), Some(raw)) => {
            let watchers: Vec<TaskWatcherRecord> =
                decode_json(raw).map_err(|e| refuse(path, e.to_string()))?;
            tx.put_task_watchers(&tid, &watchers)?;
        }
        (Kind::Watchers(tid), None) => tx.put_task_watchers(&tid, &[])?,
        (Kind::Links(tid), Some(raw)) => {
            let links: Vec<TaskLinkRecord> =
                decode_json(raw).map_err(|e| refuse(path, e.to_string()))?;
            for l in &links {
                agree(path, l.source_id == tid, "source_id")?;
            }
            tx.put_task_links(&tid, &links)?;
        }
        (Kind::Links(tid), None) => tx.put_task_links(&tid, &[])?,
        (Kind::Event(tid, rev), Some(raw)) => {
            let rec: TaskEventRecord = decode_json(raw).map_err(|e| refuse(path, e.to_string()))?;
            agree(path, rec.task_id == tid && rec.revision == rev, "event")?;
            tx.put_task_event(&rec)?;
        }
        (Kind::Event(_, _), None) => {
            return Err(refuse(
                path,
                "task events are immutable and cannot be deleted",
            ));
        }
    }
    Ok(())
}

/// Validates task referential integrity, unique office-coded keys, and dependency acyclicity
/// across both Memory and SQLite engines (`ADR-0018`).
fn validate_tasks<Tx: TasksWrite>(tx: &Tx) -> Result<(), ProjectionError> {
    let tasks = tx.tasks()?;
    let mut keys: BTreeSet<(String, u64)> = BTreeSet::new();
    let mut task_ids: BTreeSet<Id> = BTreeSet::new();
    for t in &tasks {
        let path = records::task_path(&t.id);
        if t.number == 0 {
            return Err(refuse(&path, "task number must be at least 1"));
        }
        if !keys.insert((t.office_code.clone(), t.number)) {
            return Err(refuse(&path, "duplicate task key"));
        }
        if tx.team(&t.team_id)?.is_none() {
            return Err(refuse(&path, "owning team does not exist"));
        }
        if let Some(proj_id) = &t.project_id {
            let Some(proj) = tx.project(proj_id)? else {
                return Err(refuse(&path, "project does not exist"));
            };
            if proj.owner_team_id != t.team_id {
                return Err(refuse(
                    &path,
                    "project task owning team must match project owner team",
                ));
            }
        }
        task_ids.insert(t.id.clone());
    }
    validate_dependency_dag(tx, &task_ids)
}

fn validate_dependency_dag<Tx: TasksWrite>(
    tx: &Tx,
    task_ids: &BTreeSet<Id>,
) -> Result<(), ProjectionError> {
    let links = tx.all_task_links()?;
    let mut adj: BTreeMap<Id, Vec<Id>> = BTreeMap::new();
    for link in &links {
        if link.kind != "depends_on" {
            continue;
        }
        let path = records::links_path(&link.source_id);
        if link.source_id == link.target_id {
            return Err(refuse(&path, "a task cannot depend on itself"));
        }
        if !task_ids.contains(&link.source_id) || !task_ids.contains(&link.target_id) {
            return Err(refuse(&path, "dependency link references unknown task"));
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
            return Err(refuse(
                &records::links_path(start),
                "dependency cycle is not allowed",
            ));
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
