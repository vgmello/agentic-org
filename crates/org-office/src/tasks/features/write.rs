//! What a task mutation leaves behind (`doc-79`): a core mutation rewrites `task.md` at
//! its next revision with that revision's history event; a child-only mutation (a comment,
//! a watcher, a checklist change) writes only its own file beside `task.md` and the next
//! history event, never `task.md`. Either way the task's effective revision is its latest
//! event's.

use org_proto::ErrorCode;
use org_store::{Change, Id, Refusal};
use serde_json::Value;

use super::{refuse, storage_refusal};
use crate::tasks::ports::TasksRead;
use crate::tasks::records::{self, EventInput, Location, TaskLinkRecord, TaskRecord};

/// Who changes a task, when, and for which command.
pub(crate) struct Writer<'a> {
    pub organization_id: Id,
    pub actor_id: &'a Id,
    pub at: &'a str,
    pub command_id: &'a str,
}

impl<'a> Writer<'a> {
    pub(crate) fn new<R: TasksRead>(
        view: &R,
        actor_id: &'a Id,
        at: &'a str,
        command_id: &'a str,
    ) -> Result<Self, Refusal> {
        let organization = view.organization().map_err(storage_refusal)?;
        let organization_id = organization
            .map(|o| o.id)
            .ok_or_else(|| refuse(ErrorCode::StorageError, "the organization is missing"))?;
        Ok(Self {
            organization_id,
            actor_id,
            at,
            command_id,
        })
    }

    fn event(&self, task_id: &Id, revision: u64, change: Value) -> Result<Vec<u8>, Refusal> {
        let (_, bytes) = records::encode_event(EventInput {
            organization_id: &self.organization_id,
            task_id,
            revision,
            occurred_at: self.at,
            actor_id: self.actor_id,
            operation_id: self.command_id,
            change,
        })
        .map_err(|e| refuse(ErrorCode::StorageError, format!("encode task event: {e}")))?;
        Ok(bytes)
    }

    /// `task.md` at `task.meta.revision` (already advanced), with `links`, and the event of
    /// that revision.
    pub(crate) fn task_and_event(
        &self,
        change: Change,
        location: &Location,
        task: &TaskRecord,
        links: &[TaskLinkRecord],
        event: Value,
    ) -> Result<Change, Refusal> {
        let path = records::task_md_path(location, &task.id).map_err(too_deep)?;
        let bytes =
            records::encode_task_md(task, links).map_err(|e| refuse(ErrorCode::TooLarge, e))?;
        let event = self.event(&task.id, task.meta.revision, event)?;
        Ok(change
            .put(path, bytes)
            .create(records::event_path(&task.id, task.meta.revision), event))
    }

    /// The next history event of a child-only change, without `task.md`: the task's
    /// effective revision advances to it. Returns the change and that revision.
    pub(crate) fn child_event(
        &self,
        change: Change,
        task: &TaskRecord,
        event: Value,
    ) -> Result<(Change, u64), Refusal> {
        let revision = super::validate_i64_u64(task.meta.revision + 1, "revision")?;
        let bytes = self.event(&task.id, revision, event)?;
        let path = records::event_path(&task.id, revision);
        Ok((change.create(path, bytes), revision))
    }
}

pub(crate) fn too_deep(_: org_store::StoreError) -> Refusal {
    refuse(
        ErrorCode::InvalidRequest,
        "the owning team is too deep in the team tree for task files",
    )
}

/// Where `task`'s folder is: its project's, or its owning team's chain.
pub(crate) fn task_location<R: TasksRead>(
    view: &R,
    task: &TaskRecord,
) -> Result<Location, Refusal> {
    if let Some(project) = &task.project_id {
        return Ok(Location::Project(project.clone()));
    }
    let chain = view.team_chain(&task.team_id).map_err(storage_refusal)?;
    chain.map(Location::Team).ok_or_else(|| {
        refuse(
            ErrorCode::StorageError,
            "the task's owning team has no chain to a top-level team",
        )
    })
}

/// The task's links as they stand, for a `task.md` rewrite that does not change them.
pub(crate) fn current_links<R: TasksRead>(
    view: &R,
    task: &TaskRecord,
) -> Result<Vec<TaskLinkRecord>, Refusal> {
    view.task_links_from(&task.id).map_err(storage_refusal)
}

/// A child file's bytes, or the refusal for a file over its bound.
pub(crate) fn child_bytes(encoded: Result<Vec<u8>, String>) -> Result<Vec<u8>, Refusal> {
    encoded.map_err(|e| refuse(ErrorCode::TooLarge, e))
}
