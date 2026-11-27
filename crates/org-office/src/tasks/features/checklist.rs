//! Checklist mutations (`TASK-49`, `TASK-88`): each is child-only (`doc-79`), writing
//! `checklist.yaml` and the next history event, never `task.md`. Checking or unchecking a step
//! is answered `committed`; adding, editing, removing or moving one is answered `durable`
//! (`ADR-0014`). A checklist has three sections (`acceptance`, `dod`, `work`), and every
//! mutation leaves each section's positions 0-based and contiguous.

use org_proto::tasks::{
    ChecklistSection, TasksChecklistAddParams, TasksChecklistCheckParams, TasksChecklistEditParams,
    TasksChecklistMoveParams, TasksChecklistRemoveParams, TASK_MAX_CHECKLIST_STEPS,
    TASK_STEP_TEXT_MAX_CHARS,
};
use org_proto::ErrorCode;
use org_store::{Ack, Change, Engine, Id, Refusal, UnitOfWork};
use serde_json::json;

use crate::tasks::ops::{
    child_bytes, require_local_task_write_authority, task_location, too_deep, TaskWriteAuthority,
    Writer,
};
use crate::tasks::ports::TasksRead;
use crate::tasks::records::{self, ChecklistStepRecord, TaskRecord};
use crate::time::format_rfc3339_ms;

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

/// Orders `steps` section by section, keeping their order within each, and numbers each
/// section's steps from 0.
fn reindex_steps(steps: &mut [ChecklistStepRecord]) {
    steps.sort_by_key(|s| s.section);
    let mut next = [0u64; ChecklistSection::ALL.len()];
    for step in steps {
        let slot = &mut next[step.section as usize];
        step.position = *slot;
        *slot += 1;
    }
}

pub struct ChecklistAdd {
    pub actor_member_id: String,
    pub params: TasksChecklistAddParams,
    /// Single-office task write authority verified atomically inside the `UnitOfWork` (`TASK-218`).
    pub authority: TaskWriteAuthority,
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
        require_local_task_write_authority(uow.view(), &self.authority, &task)?;
        let text = validate_step_text(&self.params.text)?;
        let section = self.params.section.unwrap_or_default();
        let required = self
            .params
            .required
            .unwrap_or_else(|| section.required_by_default());
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
        // Last in its section: `reindex_steps` keeps the order within a section.
        steps.push(ChecklistStepRecord {
            id: step_id.clone(),
            section,
            position: u64::MAX,
            text: text.clone(),
            required,
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
            json!({
                "type": "checklist_step_added",
                "step_id": step_id.as_str(),
                "section": section.as_str(),
                "text": text,
                "required": required,
            }),
        )?;
        Ok((change, task.id))
    }
}

pub struct ChecklistCheck {
    pub actor_member_id: String,
    pub params: TasksChecklistCheckParams,
    /// Single-office task write authority verified atomically inside the `UnitOfWork` (`TASK-218`).
    pub authority: TaskWriteAuthority,
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
        require_local_task_write_authority(uow.view(), &self.authority, &task)?;
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
    /// Single-office task write authority verified atomically inside the `UnitOfWork` (`TASK-218`).
    pub authority: TaskWriteAuthority,
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
        require_local_task_write_authority(uow.view(), &self.authority, &task)?;
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
    /// Single-office task write authority verified atomically inside the `UnitOfWork` (`TASK-218`).
    pub authority: TaskWriteAuthority,
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
        require_local_task_write_authority(uow.view(), &self.authority, &task)?;
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
    /// Single-office task write authority verified atomically inside the `UnitOfWork` (`TASK-218`).
    pub authority: TaskWriteAuthority,
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
        require_local_task_write_authority(uow.view(), &self.authority, &task)?;
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
        let section = self.params.section.unwrap_or(step.section);
        step.section = section;
        // Insert before the `dest`-th step of the target section, or after its last one.
        let in_section: Vec<usize> = (0..steps.len())
            .filter(|&i| steps[i].section == section)
            .collect();
        let dest = (self.params.position as usize).min(in_section.len());
        let index = match in_section.get(dest) {
            Some(&index) => index,
            None => in_section.last().map_or_else(
                || steps.iter().take_while(|s| s.section < section).count(),
                |&last| last + 1,
            ),
        };
        steps.insert(index, step);
        reindex_steps(&mut steps);
        let at = format_rfc3339_ms(uow.now().0);
        let change = write_checklist(
            uow.view(),
            &task,
            &steps,
            Ack::Durable,
            Writer::new(uow.view(), &actor.id, &at, &self.params.command_id)?,
            json!({
                "type": "checklist_step_moved",
                "step_id": step_id.as_str(),
                "section": section.as_str(),
                "position": dest,
            }),
        )?;
        Ok((change, task.id))
    }
}
