//! Secondary task mutations: comments, watchers, dependency edges, and checklists (`TASK-49`, `TASK-77`).

use org_proto::tasks::{
    TasksAddDependencyParams, TasksChecklistAddParams, TasksChecklistCheckParams,
    TasksChecklistEditParams, TasksChecklistMoveParams, TasksChecklistRemoveParams,
    TasksCommentParams, TasksRemoveDependencyParams, TasksUnwatchParams, TasksWatchParams,
    TASK_COMMENT_MAX_CHARS, TASK_MAX_CHECKLIST_STEPS, TASK_STEP_TEXT_MAX_CHARS,
};
use org_proto::ErrorCode;
use org_store::{Change, Engine, Id, Refusal, UnitOfWork};
use serde_json::json;

use crate::tasks::ports::TasksRead;
use crate::tasks::records::{
    checklist_path, comment_path, encode_json, links_path, watchers_path, ChecklistStepRecord,
    TaskCommentRecord, TaskWatcherRecord,
};
use crate::time::format_rfc3339_ms;

use super::core::{add_dependency_edge, derive_link_id};
use super::{
    parse_uuid_v7, refuse, require_active_employee, require_task, storage_refusal,
    write_task_and_event,
};

pub struct CommentTask {
    pub actor_member_id: String,
    pub params: TasksCommentParams,
}

impl CommentTask {
    pub fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
    ) -> Result<(Change, Id, TaskCommentRecord), Refusal>
    where
        E::Tx: TasksRead,
    {
        let actor = require_active_employee(uow.view(), &self.actor_member_id)?;
        let _cmd_id = parse_uuid_v7(&self.params.command_id, "command_id")?;
        let comment_id = parse_uuid_v7(&self.params.comment_id, "comment_id")?;
        let mut task = require_task(uow.view(), &self.params.task)?;
        let body = self.params.body_markdown.trim();
        if body.is_empty() {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                "comment body_markdown cannot be empty",
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
        let comment = TaskCommentRecord {
            task_id: task.id.clone(),
            id: comment_id.clone(),
            position: next_pos,
            author_id: actor.id.clone(),
            created_at: at.clone(),
            body_markdown: body.to_owned(),
        };
        task.meta.touch(&actor.id, &at);
        let comment_bytes = encode_json(&comment)
            .map_err(|e| refuse(ErrorCode::StorageError, format!("encode comment: {e}")))?;
        let change = write_task_and_event(
            Change::new(org_store::Ack::Durable)
                .put(comment_path(&task.id, &comment_id), comment_bytes),
            &task,
            &actor.id,
            &at,
            &self.params.command_id,
            json!({ "type": "comment_added", "comment_id": comment_id.as_str() }),
        )?;
        Ok((change, task.id, comment))
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
        let mut task = require_task(uow.view(), &self.params.task)?;
        let mut watchers = uow
            .view()
            .task_watchers(&task.id)
            .map_err(storage_refusal)?;
        if watchers
            .iter()
            .any(|w| w.watcher_kind == "employee" && w.watcher_id.as_ref() == Some(&actor.id))
        {
            return Ok((Change::new(org_store::Ack::Durable), task.id));
        }
        watchers.push(TaskWatcherRecord {
            id: derive_link_id(&task.id, &actor.id, 1),
            watcher_kind: "employee".into(),
            watcher_id: Some(actor.id.clone()),
        });
        let at = format_rfc3339_ms(uow.now().0);
        task.meta.touch(&actor.id, &at);
        let bytes = encode_json(&watchers)
            .map_err(|e| refuse(ErrorCode::StorageError, format!("encode watchers: {e}")))?;
        let change = write_task_and_event(
            Change::new(org_store::Ack::Durable).put(watchers_path(&task.id), bytes),
            &task,
            &actor.id,
            &at,
            &self.params.command_id,
            json!({ "type": "watcher_added", "employee_id": actor.id.as_str() }),
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
        let mut task = require_task(uow.view(), &self.params.task)?;
        let mut watchers = uow
            .view()
            .task_watchers(&task.id)
            .map_err(storage_refusal)?;
        let before = watchers.len();
        watchers.retain(|w| {
            !(w.watcher_kind == "employee" && w.watcher_id.as_ref() == Some(&actor.id))
        });
        if watchers.len() == before {
            return Ok((Change::new(org_store::Ack::Durable), task.id));
        }
        let at = format_rfc3339_ms(uow.now().0);
        task.meta.touch(&actor.id, &at);
        let bytes = encode_json(&watchers)
            .map_err(|e| refuse(ErrorCode::StorageError, format!("encode watchers: {e}")))?;
        let change = write_task_and_event(
            Change::new(org_store::Ack::Durable).put(watchers_path(&task.id), bytes),
            &task,
            &actor.id,
            &at,
            &self.params.command_id,
            json!({ "type": "watcher_removed", "employee_id": actor.id.as_str() }),
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
            return Ok((Change::new(org_store::Ack::Durable), task.id));
        }
        let at = format_rfc3339_ms(uow.now().0);
        task.meta.touch(&actor.id, &at);
        let bytes = encode_json(&links)
            .map_err(|e| refuse(ErrorCode::StorageError, format!("encode links: {e}")))?;
        let change = write_task_and_event(
            Change::new(org_store::Ack::Durable).put(links_path(&task.id), bytes),
            &task,
            &actor.id,
            &at,
            &self.params.command_id,
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
        let bytes = encode_json(&links)
            .map_err(|e| refuse(ErrorCode::StorageError, format!("encode links: {e}")))?;
        let change = write_task_and_event(
            Change::new(org_store::Ack::Durable).put(links_path(&task.id), bytes),
            &task,
            &actor.id,
            &at,
            &self.params.command_id,
            json!({
                "type": "dependency_removed",
                "depends_on_task_id": target.id.as_str(),
                "depends_on_key": target.key(),
            }),
        )?;
        Ok((change, task.id))
    }
}

fn validate_step_text(raw: &str) -> Result<String, Refusal> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(refuse(
            ErrorCode::InvalidRequest,
            "checklist step text cannot be empty",
        ));
    }
    if trimmed.chars().count() > TASK_STEP_TEXT_MAX_CHARS {
        return Err(refuse(
            ErrorCode::TooLarge,
            format!("checklist step text exceeds {TASK_STEP_TEXT_MAX_CHARS} characters"),
        ));
    }
    Ok(trimmed.to_owned())
}

fn reindex_steps(steps: &mut [ChecklistStepRecord]) {
    for (idx, step) in steps.iter_mut().enumerate() {
        step.position = idx as u64;
    }
}

pub struct ChecklistAdd {
    pub actor_member_id: String,
    pub params: TasksChecklistAddParams,
}

impl ChecklistAdd {
    pub fn decide<E: Engine>(&self, uow: &mut UnitOfWork<'_, E>) -> Result<(Change, Id), Refusal>
    where
        E::Tx: TasksRead,
    {
        let actor = require_active_employee(uow.view(), &self.actor_member_id)?;
        let _cmd_id = parse_uuid_v7(&self.params.command_id, "command_id")?;
        let step_id = parse_uuid_v7(&self.params.step_id, "step_id")?;
        let mut task = require_task(uow.view(), &self.params.task)?;
        let text = validate_step_text(&self.params.text)?;
        let mut steps = uow
            .view()
            .checklist_steps(&task.id)
            .map_err(storage_refusal)?;
        if steps.len() >= TASK_MAX_CHECKLIST_STEPS {
            return Err(refuse(
                ErrorCode::TooLarge,
                format!("at most {TASK_MAX_CHECKLIST_STEPS} checklist steps allowed"),
            ));
        }
        if steps.iter().any(|s| s.id == step_id) {
            return Err(refuse(
                ErrorCode::AlreadyExists,
                format!("checklist step `{step_id}` already exists"),
            ));
        }
        steps.push(ChecklistStepRecord {
            id: step_id.clone(),
            position: steps.len() as u64,
            text: text.clone(),
            required: self.params.required,
            done: false,
            linked_task_id: None,
            checked_by_employee_id: None,
            checked_at: None,
            revision: 1,
        });
        reindex_steps(&mut steps);
        let at = format_rfc3339_ms(uow.now().0);
        task.meta.touch(&actor.id, &at);
        let bytes = encode_json(&steps)
            .map_err(|e| refuse(ErrorCode::StorageError, format!("encode checklist: {e}")))?;
        let change = write_task_and_event(
            Change::new(org_store::Ack::Durable).put(checklist_path(&task.id), bytes),
            &task,
            &actor.id,
            &at,
            &self.params.command_id,
            json!({ "type": "checklist_step_added", "step_id": step_id.as_str(), "text": text }),
        )?;
        Ok((change, task.id))
    }
}

pub struct ChecklistCheck {
    pub actor_member_id: String,
    pub params: TasksChecklistCheckParams,
}

impl ChecklistCheck {
    pub fn decide<E: Engine>(&self, uow: &mut UnitOfWork<'_, E>) -> Result<(Change, Id), Refusal>
    where
        E::Tx: TasksRead,
    {
        let actor = require_active_employee(uow.view(), &self.actor_member_id)?;
        let _cmd_id = parse_uuid_v7(&self.params.command_id, "command_id")?;
        let step_id = parse_uuid_v7(&self.params.step_id, "step_id")?;
        let mut task = require_task(uow.view(), &self.params.task)?;
        let mut steps = uow
            .view()
            .checklist_steps(&task.id)
            .map_err(storage_refusal)?;
        let at = format_rfc3339_ms(uow.now().0);
        let Some(step) = steps.iter_mut().find(|s| s.id == step_id) else {
            return Err(refuse(
                ErrorCode::NotFound,
                format!("checklist step `{step_id}` not found"),
            ));
        };
        step.done = self.params.done;
        if self.params.done {
            step.checked_by_employee_id = Some(actor.id.clone());
            step.checked_at = Some(at.clone());
        } else {
            step.checked_by_employee_id = None;
            step.checked_at = None;
        }
        step.revision = step.revision.saturating_add(1);
        task.meta.touch(&actor.id, &at);
        let bytes = encode_json(&steps)
            .map_err(|e| refuse(ErrorCode::StorageError, format!("encode checklist: {e}")))?;
        let change = write_task_and_event(
            Change::new(org_store::Ack::Durable).put(checklist_path(&task.id), bytes),
            &task,
            &actor.id,
            &at,
            &self.params.command_id,
            json!({ "type": "checklist_step_checked", "step_id": step_id.as_str(), "done": self.params.done }),
        )?;
        Ok((change, task.id))
    }
}

pub struct ChecklistEdit {
    pub actor_member_id: String,
    pub params: TasksChecklistEditParams,
}

impl ChecklistEdit {
    pub fn decide<E: Engine>(&self, uow: &mut UnitOfWork<'_, E>) -> Result<(Change, Id), Refusal>
    where
        E::Tx: TasksRead,
    {
        let actor = require_active_employee(uow.view(), &self.actor_member_id)?;
        let _cmd_id = parse_uuid_v7(&self.params.command_id, "command_id")?;
        let step_id = parse_uuid_v7(&self.params.step_id, "step_id")?;
        let mut task = require_task(uow.view(), &self.params.task)?;
        let mut steps = uow
            .view()
            .checklist_steps(&task.id)
            .map_err(storage_refusal)?;
        let Some(step) = steps.iter_mut().find(|s| s.id == step_id) else {
            return Err(refuse(
                ErrorCode::NotFound,
                format!("checklist step `{step_id}` not found"),
            ));
        };
        if self.params.text.is_none() && self.params.required.is_none() {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                "checklist.edit requires `text` or `required`",
            ));
        }
        if let Some(ref text) = self.params.text {
            step.text = validate_step_text(text)?;
        }
        if let Some(req) = self.params.required {
            step.required = req;
        }
        step.revision = step.revision.saturating_add(1);
        let at = format_rfc3339_ms(uow.now().0);
        task.meta.touch(&actor.id, &at);
        let bytes = encode_json(&steps)
            .map_err(|e| refuse(ErrorCode::StorageError, format!("encode checklist: {e}")))?;
        let change = write_task_and_event(
            Change::new(org_store::Ack::Durable).put(checklist_path(&task.id), bytes),
            &task,
            &actor.id,
            &at,
            &self.params.command_id,
            json!({ "type": "checklist_step_edited", "step_id": step_id.as_str() }),
        )?;
        Ok((change, task.id))
    }
}

pub struct ChecklistRemove {
    pub actor_member_id: String,
    pub params: TasksChecklistRemoveParams,
}

impl ChecklistRemove {
    pub fn decide<E: Engine>(&self, uow: &mut UnitOfWork<'_, E>) -> Result<(Change, Id), Refusal>
    where
        E::Tx: TasksRead,
    {
        let actor = require_active_employee(uow.view(), &self.actor_member_id)?;
        let _cmd_id = parse_uuid_v7(&self.params.command_id, "command_id")?;
        let step_id = parse_uuid_v7(&self.params.step_id, "step_id")?;
        let mut task = require_task(uow.view(), &self.params.task)?;
        let mut steps = uow
            .view()
            .checklist_steps(&task.id)
            .map_err(storage_refusal)?;
        let before = steps.len();
        steps.retain(|s| s.id != step_id);
        if steps.len() == before {
            return Err(refuse(
                ErrorCode::NotFound,
                format!("checklist step `{step_id}` not found"),
            ));
        }
        reindex_steps(&mut steps);
        let at = format_rfc3339_ms(uow.now().0);
        task.meta.touch(&actor.id, &at);
        let bytes = encode_json(&steps)
            .map_err(|e| refuse(ErrorCode::StorageError, format!("encode checklist: {e}")))?;
        let change = write_task_and_event(
            Change::new(org_store::Ack::Durable).put(checklist_path(&task.id), bytes),
            &task,
            &actor.id,
            &at,
            &self.params.command_id,
            json!({ "type": "checklist_step_removed", "step_id": step_id.as_str() }),
        )?;
        Ok((change, task.id))
    }
}

pub struct ChecklistMove {
    pub actor_member_id: String,
    pub params: TasksChecklistMoveParams,
}

impl ChecklistMove {
    pub fn decide<E: Engine>(&self, uow: &mut UnitOfWork<'_, E>) -> Result<(Change, Id), Refusal>
    where
        E::Tx: TasksRead,
    {
        let actor = require_active_employee(uow.view(), &self.actor_member_id)?;
        let _cmd_id = parse_uuid_v7(&self.params.command_id, "command_id")?;
        let step_id = parse_uuid_v7(&self.params.step_id, "step_id")?;
        let mut task = require_task(uow.view(), &self.params.task)?;
        let mut steps = uow
            .view()
            .checklist_steps(&task.id)
            .map_err(storage_refusal)?;
        let Some(pos) = steps.iter().position(|s| s.id == step_id) else {
            return Err(refuse(
                ErrorCode::NotFound,
                format!("checklist step `{step_id}` not found"),
            ));
        };
        let mut step = steps.remove(pos);
        step.revision = step.revision.saturating_add(1);
        let dest = (self.params.position as usize).min(steps.len());
        steps.insert(dest, step);
        reindex_steps(&mut steps);
        let at = format_rfc3339_ms(uow.now().0);
        task.meta.touch(&actor.id, &at);
        let bytes = encode_json(&steps)
            .map_err(|e| refuse(ErrorCode::StorageError, format!("encode checklist: {e}")))?;
        let change = write_task_and_event(
            Change::new(org_store::Ack::Durable).put(checklist_path(&task.id), bytes),
            &task,
            &actor.id,
            &at,
            &self.params.command_id,
            json!({ "type": "checklist_step_moved", "step_id": step_id.as_str(), "position": dest }),
        )?;
        Ok((change, task.id))
    }
}
