//! Engine-independent projector for the unified tasks family (`TaskFiles`, `doc-77`,
//! `doc-79`, `doc-80`).
//!
//! It owns every task folder file under `state/projects/<id>/work/tasks/` and
//! `state/teams/<chain>/work/tasks/`, the history under `state/work-history/tasks/`, and the
//! opening office's own `local/offices/<office-id>/task-counter.yaml`: a rebuild walks only
//! that office's local folder, so the counter is rebuilt for the opening office alone.

use org_store::{DurablePath, Engine, Id, ProjectionError, Projector};

use super::{
    ports::TasksWrite,
    records::{self, Kind, Location, TaskDir},
};

mod validate;

/// The tasks family's projector: for one organization and, when it projects an office's
/// files, that office's counter.
pub struct TaskFiles {
    organization_id: Id,
    office_id: Option<Id>,
}

impl TaskFiles {
    /// The projector an office's store opens with: the shared files and this office's
    /// task counter.
    pub fn new(organization_id: Id, office_id: Id) -> Self {
        Self {
            organization_id,
            office_id: Some(office_id),
        }
    }

    /// The shared `state/` files alone, as an organization's Git state carries them: no
    /// office's local counter (`TASK-97` import validation).
    pub fn shared(organization_id: Id) -> Self {
        Self {
            organization_id,
            office_id: None,
        }
    }

    fn owns_counter(&self, path: &DurablePath) -> bool {
        self.office_id
            .as_ref()
            .is_some_and(|office| records::is_counter_path(path, office))
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
        records::kind_of(path).is_some() || self.owns_counter(path)
    }

    fn apply(
        &self,
        tx: &mut E::Tx,
        path: &DurablePath,
        bytes: Option<&[u8]>,
    ) -> Result<(), ProjectionError> {
        if self.owns_counter(path) {
            return apply_counter(tx, path, self.office_id.as_ref(), bytes);
        }
        let Some(parsed) = records::parse_path(path) else {
            return Err(refuse(path, "not a task file"));
        };
        match (parsed.kind, parsed.dir) {
            (Kind::Event(task, revision), _) => {
                let Some(raw) = bytes else {
                    return Err(refuse(path, "task history is immutable: never removed"));
                };
                let event = records::verify_event(raw, &self.organization_id, &task, revision)
                    .map_err(|e| refuse(path, e))?;
                tx.put_task_event(&event)?;
                Ok(())
            }
            (kind, Some(dir)) => apply_task_file(tx, path, &dir, kind, bytes),
            (_, None) => Err(refuse(path, "a task file is in a task folder")),
        }
    }

    fn validate(&self, tx: &E::Tx) -> Result<(), ProjectionError> {
        validate::validate_tasks(tx)
    }
}

/// The office's counter: the projection holds this office's counter alone, so a file
/// replaces every counter row and a removal clears them.
fn apply_counter<Tx: TasksWrite>(
    tx: &mut Tx,
    path: &DurablePath,
    office: Option<&Id>,
    bytes: Option<&[u8]>,
) -> Result<(), ProjectionError> {
    for old in tx.office_task_counters()? {
        tx.remove_office_task_counter(&old.office_code)?;
    }
    let Some(raw) = bytes else {
        return Ok(());
    };
    let (owner, counter) = records::decode_counter(raw).map_err(|e| refuse(path, e.to_string()))?;
    agree(path, Some(&owner) == office, "office_id")?;
    tx.put_office_task_counter(&counter)?;
    Ok(())
}

/// Whether removing a file of `dir` removes the task's rows: it does unless the task is
/// known to be in another folder. A file left in a folder the task has moved out of (a
/// relocation, `MoveTeam`) is a stale copy whose removal must not take the moved task's
/// rows with it, whatever order the moves are applied in.
fn removes_rows<Tx: TasksWrite>(tx: &Tx, dir: &TaskDir) -> Result<bool, ProjectionError> {
    let Some(task) = tx.task(&dir.task_id)? else {
        return Ok(true);
    };
    let current = match &task.project_id {
        Some(project) => Some(Location::Project(project.clone())),
        None => tx.team_chain(&task.team_id)?.map(Location::Team),
    };
    Ok(current.is_none_or(|location| location == dir.location))
}

fn apply_task_file<Tx: TasksWrite>(
    tx: &mut Tx,
    path: &DurablePath,
    dir: &TaskDir,
    kind: Kind,
    bytes: Option<&[u8]>,
) -> Result<(), ProjectionError> {
    let tid = &dir.task_id;
    if bytes.is_none() && !removes_rows(tx, dir)? {
        return Ok(());
    }
    match (kind, bytes) {
        (Kind::Task(_), Some(raw)) => {
            let doc = records::decode_task_md(raw).map_err(|e| refuse(path, e.to_string()))?;
            agree(path, doc.task.id == *tid, "id")?;
            let placed = match (&dir.location, &doc.task.project_id) {
                (Location::Project(project), Some(own)) => project == own,
                (Location::Team(chain), None) => chain.last() == Some(&doc.task.team_id),
                _ => false,
            };
            agree(path, placed, "the task's project or owning team")?;
            tx.put_task(&doc.task)?;
            tx.put_task_links(tid, &doc.links)?;
        }
        (Kind::Task(_), None) => tx.remove_task(tid)?,
        (Kind::Comment(_, comment), Some(raw)) => {
            let rec = records::decode_comment(raw).map_err(|e| refuse(path, e.to_string()))?;
            agree(path, rec.task_id == *tid && rec.id == comment, "comment_id")?;
            tx.put_task_comment(&rec)?;
        }
        (Kind::Comment(_, comment), None) => tx.remove_task_comment(tid, &comment)?,
        (Kind::Checklist(_), Some(raw)) => {
            let steps = records::decode_checklist(raw).map_err(|e| refuse(path, e.to_string()))?;
            tx.put_checklist_steps(tid, &steps)?;
        }
        (Kind::Checklist(_), None) => tx.put_checklist_steps(tid, &[])?,
        (Kind::Operations(_), Some(raw)) => {
            let watchers =
                records::decode_operations(raw).map_err(|e| refuse(path, e.to_string()))?;
            tx.put_task_watchers(tid, &watchers)?;
        }
        (Kind::Operations(_), None) => tx.put_task_watchers(tid, &[])?,
        (Kind::Event(..), _) => return Err(refuse(path, "history is not in a task folder")),
    }
    Ok(())
}
