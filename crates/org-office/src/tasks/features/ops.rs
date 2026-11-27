//! Secondary task mutations: comments, watchers and dependency edges (`TASK-49`, `TASK-77`).
//!
//! A comment and a watch or unwatch are child-only (`doc-79`): each writes its own file
//! beside `task.md` (`comments/<comment-id>.md`, `operations.yaml`) and the next history
//! event, never `task.md`, answered `committed` (`ADR-0014`). A dependency edge is a link
//! in `task.md`: a core mutation, answered `durable`.

use org_proto::attachments::{check_task_comment_attachment_ids, AttachmentTargetKind};
use org_proto::tasks::{
    TasksAddDependencyParams, TasksCommentParams, TasksRemoveDependencyParams, TasksUnwatchParams,
    TasksWatchParams, TASK_COMMENT_MAX_CHARS,
};
use org_proto::ErrorCode;
use org_store::{Ack, Change, Engine, Id, Refusal, UnitOfWork};
use serde_json::json;

use crate::attachments::records::AttachmentRecord;
use crate::tasks::ports::TasksRead;
use crate::tasks::records::{self, TaskCommentRecord, TaskRecord, TaskWatcherRecord};
use crate::time::format_rfc3339_ms;

use super::attachments::PreparedTaskFiles;
use super::core::{add_dependency_edge, derive_link_id};
use super::write::{child_bytes, current_attachments, task_location, too_deep, Writer};
use super::{parse_uuid_v7, refuse, require_active_employee, require_task, storage_refusal};

pub struct CommentTask {
    pub actor_member_id: String,
    pub params: TasksCommentParams,
    /// Validated, blob-promoted files bound to this comment (`TASK-98.4`, max 10).
    pub prepared_files: PreparedTaskFiles,
}

impl CommentTask {
    pub fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
    ) -> Result<(Change, Id, TaskCommentRecord, Vec<AttachmentRecord>), Refusal>
    where
        E::Tx: TasksRead,
    {
        let actor = require_active_employee(uow.view(), &self.actor_member_id)?;
        let _cmd_id = parse_uuid_v7(&self.params.command_id, "command_id")?;
        let comment_id = parse_uuid_v7(&self.params.comment_id, "comment_id")?;
        if let Err(err) = check_task_comment_attachment_ids(&self.params.attachment_ids) {
            return Err(refuse(err.code, err.message));
        }
        let task = require_task(uow.view(), &self.params.task)?;
        let body = self.params.body_markdown.trim();
        if body.is_empty() && self.params.attachment_ids.is_empty() {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                "comment body_markdown cannot be empty without attachments",
            ));
        }
        if body.chars().count() > TASK_COMMENT_MAX_CHARS {
            return Err(refuse(
                ErrorCode::TooLarge,
                format!("comment exceeds {TASK_COMMENT_MAX_CHARS} characters"),
            ));
        }
        let existing = uow
            .view()
            .task_comments(&task.id)
            .map_err(storage_refusal)?;
        if existing.iter().any(|c| c.id == comment_id) {
            return Err(refuse(
                ErrorCode::AlreadyExists,
                format!("comment `{comment_id}` already exists"),
            ));
        }
        let next_pos = existing.iter().map(|c| c.position + 1).max().unwrap_or(0);
        let at = format_rfc3339_ms(uow.now().0);
        let attachments = if self.params.attachment_ids.is_empty() {
            Vec::new()
        } else {
            self.prepared_files.to_records(
                AttachmentTargetKind::TaskComment,
                &comment_id,
                &self.params.command_id,
                &at,
            )?
        };
        let comment = TaskCommentRecord {
            task_id: task.id.clone(),
            id: comment_id.clone(),
            position: next_pos,
            author_id: actor.id.clone(),
            created_at: at.clone(),
            body_markdown: body.to_owned(),
        };
        let location = task_location(uow.view(), &task)?;
        let path = records::comment_path(&location, &task.id, &comment_id).map_err(too_deep)?;
        let bytes = child_bytes(records::encode_comment(&comment, &attachments))?;
        let writer = Writer::new(uow.view(), &actor.id, &at, &self.params.command_id)?;
        let mut event = json!({ "type": "comment_added", "comment_id": comment_id.as_str() });
        if !attachments.is_empty() {
            let ids: Vec<&str> = attachments.iter().map(|a| a.id.as_str()).collect();
            event["attachment_ids"] = json!(ids);
        }
        let (change, _) = writer.child_event(
            Change::new(Ack::Committed).create(path, bytes),
            &task,
            event,
        )?;
        Ok((change, task.id, comment, attachments))
    }
}

pub struct WatchTask {
    pub actor_member_id: String,
    pub params: TasksWatchParams,
}

impl WatchTask {
    pub fn decide<E: Engine>(&self, uow: &mut UnitOfWork<'_, E>) -> Result<(Change, Id), Refusal>
    where
        E::Tx: TasksRead,
    {
        let actor = require_active_employee(uow.view(), &self.actor_member_id)?;
        let _cmd_id = parse_uuid_v7(&self.params.command_id, "command_id")?;
        let task = require_task(uow.view(), &self.params.task)?;
        let mut watchers = uow
            .view()
            .task_watchers(&task.id)
            .map_err(storage_refusal)?;
        if watchers
            .iter()
            .any(|w| w.watcher_kind == "employee" && w.watcher_id.as_ref() == Some(&actor.id))
        {
            return Ok((Change::new(Ack::Committed), task.id));
        }
        watchers.push(TaskWatcherRecord {
            id: derive_link_id(&task.id, &actor.id, 1),
            watcher_kind: "employee".into(),
            watcher_id: Some(actor.id.clone()),
        });
        let at = format_rfc3339_ms(uow.now().0);
        let event = json!({ "type": "watcher_added", "employee_id": actor.id.as_str() });
        let change = write_operations(
            uow.view(),
            &task,
            &watchers,
            &actor.id,
            &at,
            &self.params.command_id,
            event,
        )?;
        Ok((change, task.id))
    }
}

pub struct UnwatchTask {
    pub actor_member_id: String,
    pub params: TasksUnwatchParams,
}

impl UnwatchTask {
    pub fn decide<E: Engine>(&self, uow: &mut UnitOfWork<'_, E>) -> Result<(Change, Id), Refusal>
    where
        E::Tx: TasksRead,
    {
        let actor = require_active_employee(uow.view(), &self.actor_member_id)?;
        let _cmd_id = parse_uuid_v7(&self.params.command_id, "command_id")?;
        let task = require_task(uow.view(), &self.params.task)?;
        let mut watchers = uow
            .view()
            .task_watchers(&task.id)
            .map_err(storage_refusal)?;
        let before = watchers.len();
        watchers.retain(|w| {
            !(w.watcher_kind == "employee" && w.watcher_id.as_ref() == Some(&actor.id))
        });
        if watchers.len() == before {
            return Ok((Change::new(Ack::Committed), task.id));
        }
        let at = format_rfc3339_ms(uow.now().0);
        let event = json!({ "type": "watcher_removed", "employee_id": actor.id.as_str() });
        let change = write_operations(
            uow.view(),
            &task,
            &watchers,
            &actor.id,
            &at,
            &self.params.command_id,
            event,
        )?;
        Ok((change, task.id))
    }
}

pub struct AddDependency {
    pub actor_member_id: String,
    pub params: TasksAddDependencyParams,
}

impl AddDependency {
    pub fn decide<E: Engine>(&self, uow: &mut UnitOfWork<'_, E>) -> Result<(Change, Id), Refusal>
    where
        E::Tx: TasksRead,
    {
        let actor = require_active_employee(uow.view(), &self.actor_member_id)?;
        let _cmd_id = parse_uuid_v7(&self.params.command_id, "command_id")?;
        let mut task = require_task(uow.view(), &self.params.task)?;
        let target = require_task(uow.view(), &self.params.depends_on_task)?;
        let (links, added) = add_dependency_edge(uow.view(), &task.id, &target.id)?;
        if !added {
            return Ok((Change::new(Ack::Durable), task.id));
        }
        let at = format_rfc3339_ms(uow.now().0);
        task.meta.touch(&actor.id, &at);
        let change = rewrite_links(
            uow.view(),
            &task,
            &links,
            &Writer::new(uow.view(), &actor.id, &at, &self.params.command_id)?,
            json!({
                "type": "dependency_added",
                "depends_on_task_id": target.id.as_str(),
                "depends_on_key": target.key(),
            }),
        )?;
        Ok((change, task.id))
    }
}

pub struct RemoveDependency {
    pub actor_member_id: String,
    pub params: TasksRemoveDependencyParams,
}

impl RemoveDependency {
    pub fn decide<E: Engine>(&self, uow: &mut UnitOfWork<'_, E>) -> Result<(Change, Id), Refusal>
    where
        E::Tx: TasksRead,
    {
        let actor = require_active_employee(uow.view(), &self.actor_member_id)?;
        let _cmd_id = parse_uuid_v7(&self.params.command_id, "command_id")?;
        let mut task = require_task(uow.view(), &self.params.task)?;
        let target = require_task(uow.view(), &self.params.depends_on_task)?;
        let mut links = uow
            .view()
            .task_links_from(&task.id)
            .map_err(storage_refusal)?;
        let before = links.len();
        links.retain(|l| !(l.kind == "depends_on" && l.target_id == target.id));
        if links.len() == before {
            return Err(refuse(
                ErrorCode::NotFound,
                format!(
                    "task `{}` does not depend on `{}`",
                    task.key(),
                    target.key()
                ),
            ));
        }
        let at = format_rfc3339_ms(uow.now().0);
        task.meta.touch(&actor.id, &at);
        let change = rewrite_links(
            uow.view(),
            &task,
            &links,
            &Writer::new(uow.view(), &actor.id, &at, &self.params.command_id)?,
            json!({
                "type": "dependency_removed",
                "depends_on_task_id": target.id.as_str(),
                "depends_on_key": target.key(),
            }),
        )?;
        Ok((change, task.id))
    }
}

/// `task.md` with its new links and existing attachments, and the event of its next revision (`durable`).
fn rewrite_links<R: TasksRead>(
    view: &R,
    task: &TaskRecord,
    links: &[records::TaskLinkRecord],
    writer: &Writer<'_>,
    event: serde_json::Value,
) -> Result<Change, Refusal> {
    let location = task_location(view, task)?;
    let attachments = current_attachments(view, task)?;
    writer.task_and_event(
        Change::new(Ack::Durable),
        &location,
        task,
        links,
        &attachments,
        event,
    )
}

/// A watch or an unwatch: `operations.yaml` and the next event, never `task.md`
/// (`committed`, `doc-79`).
fn write_operations<R: TasksRead>(
    view: &R,
    task: &TaskRecord,
    watchers: &[TaskWatcherRecord],
    actor_id: &Id,
    at: &str,
    command_id: &str,
    event: serde_json::Value,
) -> Result<Change, Refusal> {
    let location = task_location(view, task)?;
    let path = records::operations_path(&location, &task.id).map_err(too_deep)?;
    let bytes = child_bytes(records::encode_operations(watchers))?;
    let writer = Writer::new(view, actor_id, at, command_id)?;
    let (change, _) =
        writer.child_event(Change::new(Ack::Committed).put(path, bytes), task, event)?;
    Ok(change)
}
