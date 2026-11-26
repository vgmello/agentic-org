//! Checklist mutations (`TASK-49`): each is child-only (`doc-79`), writing `checklist.yaml`
//! and the next history event, never `task.md`. Checking or unchecking a step is answered
//! `committed`; adding, editing, removing or moving one is answered `durable` (`ADR-0014`).

use org_proto::tasks::{
    TasksChecklistAddParams, TasksChecklistCheckParams, TasksChecklistEditParams,
    TasksChecklistMoveParams, TasksChecklistRemoveParams, TASK_MAX_CHECKLIST_STEPS,
    TASK_STEP_TEXT_MAX_CHARS,
};
use org_proto::ErrorCode;
use org_store::{Ack, Change, Engine, Id, Refusal, UnitOfWork};
use serde_json::json;

use crate::tasks::ports::TasksRead;
use crate::tasks::records::{self, ChecklistStepRecord, TaskRecord};
use crate::time::format_rfc3339_ms;

use super::write::{child_bytes, task_location, too_deep, Writer};
use super::{parse_uuid_v7, refuse, require_active_employee, require_task, storage_refusal};

/// `checklist.yaml` with `steps`, and the next history event: never `task.md`.
fn write_checklist<R: TasksRead>(
    view: &R,
    task: &TaskRecord,
    steps: &[ChecklistStepRecord],
    ack: Ack,
    writer: Writer<'_>,
    event: serde_json::Value,
) -> Result<Change, Refusal> {
    let location = task_location(view, task)?;
    let path = records::checklist_path(&location, &task.id).map_err(too_deep)?;
    let bytes = child_bytes(records::encode_checklist(steps))?;
    let (change, _) = writer.child_event(Change::new(ack).put(path, bytes), task, event)?;
    Ok(change)
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
        let task = require_task(uow.view(), &self.params.task)?;
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
        let change = write_checklist(
            uow.view(),
            &task,
            &steps,
            Ack::Durable,
            Writer::new(uow.view(), &actor.id, &at, &self.params.command_id)?,
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
        let task = require_task(uow.view(), &self.params.task)?;
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
        let change = write_checklist(
            uow.view(),
            &task,
            &steps,
            Ack::Committed,
            Writer::new(uow.view(), &actor.id, &at, &self.params.command_id)?,
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
        let task = require_task(uow.view(), &self.params.task)?;
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
        let change = write_checklist(
            uow.view(),
            &task,
            &steps,
            Ack::Durable,
            Writer::new(uow.view(), &actor.id, &at, &self.params.command_id)?,
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
        let task = require_task(uow.view(), &self.params.task)?;
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
        let change = write_checklist(
            uow.view(),
            &task,
            &steps,
            Ack::Durable,
            Writer::new(uow.view(), &actor.id, &at, &self.params.command_id)?,
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
        let task = require_task(uow.view(), &self.params.task)?;
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
        let change = write_checklist(
            uow.view(),
            &task,
            &steps,
            Ack::Durable,
            Writer::new(uow.view(), &actor.id, &at, &self.params.command_id)?,
            json!({ "type": "checklist_step_moved", "step_id": step_id.as_str(), "position": dest }),
        )?;
        Ok((change, task.id))
    }
}
