//! Checklist mutations (`TASK-49`, `TASK-88`, `TASK-90`): child-only (`doc-79`), writing
//! `checklist.yaml` and the next history event, never `task.md`. `check` is `committed`; `add`,
//! `edit`, `remove`, and `move` are `durable` (`ADR-0014`).
//!
//! `check`, `edit`, `remove`, and `move` accept either canonical UUIDv7 `step_id` OR 1-based
//! `(section, index/indices)` addressing; ordinal addressing ALWAYS requires `expected_revision`
//! verified atomically inside the `UnitOfWork` with a structured [`org_proto::ConflictDetail`].

use std::collections::BTreeSet;

use org_proto::tasks::{
    ChecklistAddItem, ChecklistSection, TasksChecklistAddParams, TasksChecklistCheckParams,
    TasksChecklistEditParams, TasksChecklistMoveParams, TasksChecklistRemoveParams,
    TASK_MAX_CHECKLIST_STEPS, TASK_STEP_TEXT_MAX_CHARS,
};
use org_proto::ErrorCode;
use org_store::{Ack, Change, Engine, Id, Refusal, UnitOfWork};
use serde_json::{json, Value};

use crate::tasks::ops::{
    child_bytes, require_local_task_write_authority, task_location, too_deep, TaskWriteAuthority,
    Writer,
};
use crate::tasks::ports::TasksRead;
use crate::tasks::records::{self, ChecklistStepRecord, TaskRecord};
use crate::time::format_rfc3339_ms;

use super::conflict::stale_revision_refusal;
use super::{parse_uuid_v7, refuse, require_active_employee, require_task, storage_refusal};

struct ChecklistCtx {
    actor_id: Id,
    task: TaskRecord,
    steps: Vec<ChecklistStepRecord>,
}

/// Authorizes the actor & office (ADR-0031), parses `command_id`, and loads the task & steps.
impl ChecklistCtx {
    fn load<R: TasksRead>(
        view: &R,
        actor_id: &str,
        command_id: &str,
        task_ref: &str,
        authority: &TaskWriteAuthority,
    ) -> Result<Self, Refusal> {
        let actor = require_active_employee(view, actor_id)?;
        let _ = parse_uuid_v7(command_id, "command_id")?;
        let task = require_task(view, task_ref)?;
        require_local_task_write_authority(view, authority, &task)?;
        let steps = view.checklist_steps(&task.id).map_err(storage_refusal)?;
        Ok(Self {
            actor_id: actor.id,
            task,
            steps,
        })
    }

    fn commit<E: Engine>(
        &self,
        uow: &UnitOfWork<'_, E>,
        command_id: &str,
        ack: Ack,
        event: Value,
        mutated_ids: Vec<String>,
    ) -> Result<(Change, Id, Vec<String>), Refusal>
    where
        E::Tx: TasksRead,
    {
        let at = format_rfc3339_ms(uow.now().0);
        let loc = task_location(uow.view(), &self.task)?;
        let path = records::checklist_path(&loc, &self.task.id).map_err(too_deep)?;
        let bytes = child_bytes(records::encode_checklist(&self.steps))?;
        let writer = Writer::new(uow.view(), &self.actor_id, &at, command_id)?;
        let (change, _) =
            writer.child_event(Change::new(ack).put(path, bytes), &self.task, event)?;
        Ok((change, self.task.id.clone(), mutated_ids))
    }
}

fn batch_or_single_event(kind: &str, entries: &[Value], done: Option<bool>) -> Value {
    let mut obj = if let [single] = entries {
        single.as_object().cloned().unwrap_or_default()
    } else {
        json!({"count": entries.len(), "steps": entries})
            .as_object()
            .cloned()
            .unwrap_or_default()
    };
    obj.insert("type".into(), json!(kind));
    if let Some(done) = done {
        obj.insert("done".into(), json!(done));
    }
    Value::Object(obj)
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
    steps.sort_by_key(|s| s.section);
    let mut next = [0u64; ChecklistSection::ALL.len()];
    for step in steps {
        let slot = &mut next[step.section as usize];
        step.position = *slot;
        *slot += 1;
    }
}

fn check_revision<R: TasksRead>(
    view: &R,
    task: &TaskRecord,
    expected_revision: Option<u64>,
    require_for_ordinal: bool,
) -> Result<(), Refusal> {
    match expected_revision {
        None if require_for_ordinal => Err(refuse(
            ErrorCode::InvalidRequest,
            "1-based checklist ordinal addressing (`section` + `index`) requires `expected_revision` (`--if-match`)",
        )),
        Some(expected) if task.meta.revision != expected => {
            Err(stale_revision_refusal(view, task, expected))
        }
        _ => Ok(()),
    }
}

fn find_by_id(steps: &[ChecklistStepRecord], step_id: &str) -> Result<usize, Refusal> {
    let id = parse_uuid_v7(step_id, "step_id")?;
    steps.iter().position(|s| s.id == id).ok_or_else(|| {
        refuse(
            ErrorCode::NotFound,
            format!("checklist step `{id}` not found"),
        )
    })
}

fn find_ordinal(
    steps: &[ChecklistStepRecord],
    section: ChecklistSection,
    ordinal: u64,
) -> Result<usize, Refusal> {
    let target_pos = ordinal.checked_sub(1).ok_or_else(|| {
        refuse(
            ErrorCode::InvalidRequest,
            "checklist index is 1-based; use 1 or greater",
        )
    })?;
    steps
        .iter()
        .position(|s| s.section == section && s.position == target_pos)
        .ok_or_else(|| {
            let count = steps.iter().filter(|s| s.section == section).count();
            refuse(
                ErrorCode::NotFound,
                format!(
                    "checklist step #{ordinal} in section `{}` not found ({count} steps exist)",
                    section.as_str()
                ),
            )
        })
}

fn resolve_targets<R: TasksRead>(
    view: &R,
    task: &TaskRecord,
    steps: &[ChecklistStepRecord],
    step_id: &str,
    section: Option<ChecklistSection>,
    indices: &[u64],
    expected_revision: Option<u64>,
) -> Result<Vec<usize>, Refusal> {
    let has_id = !step_id.trim().is_empty();
    let has_ordinal = section.is_some() || !indices.is_empty();
    if has_id && has_ordinal {
        return Err(refuse(
            ErrorCode::InvalidRequest,
            "cannot combine `step_id` with `section`/`index` addressing",
        ));
    }
    if has_id {
        check_revision(view, task, expected_revision, false)?;
        return Ok(vec![find_by_id(steps, step_id)?]);
    }
    let (Some(sec), false) = (section, indices.is_empty()) else {
        return Err(refuse(
            ErrorCode::InvalidRequest,
            "specify either `step_id` or both `section` and 1-based `index` (with `expected_revision`)",
        ));
    };
    let mut seen = BTreeSet::new();
    for &ord in indices {
        if ord == 0 {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                "checklist index is 1-based; use 1 or greater",
            ));
        }
        if !seen.insert(ord) {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                format!("duplicate checklist index `{ord}`"),
            ));
        }
    }
    check_revision(view, task, expected_revision, true)?;
    indices
        .iter()
        .map(|&ord| find_ordinal(steps, sec, ord))
        .collect()
}

fn resolve_single<R: TasksRead>(
    view: &R,
    task: &TaskRecord,
    steps: &[ChecklistStepRecord],
    step_id: &str,
    section: Option<ChecklistSection>,
    index: Option<u64>,
    expected_revision: Option<u64>,
) -> Result<usize, Refusal> {
    let slice: Vec<u64> = index.into_iter().collect();
    resolve_targets(
        view,
        task,
        steps,
        step_id,
        section,
        &slice,
        expected_revision,
    )
    .map(|v| v[0])
}

fn collect_add_items(
    params: &TasksChecklistAddParams,
) -> Result<Vec<(Id, ChecklistSection, String, bool)>, Refusal> {
    let batch: Vec<ChecklistAddItem> = if !params.steps.is_empty() {
        if !params.step_id.trim().is_empty() || !params.text.trim().is_empty() {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                "cannot combine single-step `step_id`/`text` with batch `steps`",
            ));
        }
        params.steps.clone()
    } else {
        vec![ChecklistAddItem {
            step_id: params.step_id.clone(),
            text: params.text.clone(),
            required: params.required,
            section: params.section,
        }]
    };
    let mut out = Vec::with_capacity(batch.len());
    let mut seen = BTreeSet::new();
    for item in batch {
        let id = parse_uuid_v7(&item.step_id, "step_id")?;
        if !seen.insert(id.clone()) {
            return Err(refuse(
                ErrorCode::AlreadyExists,
                format!("duplicate checklist step `{id}` in batch"),
            ));
        }
        let text = validate_step_text(&item.text)?;
        let section = item.section.or(params.section).unwrap_or_default();
        let required = item
            .required
            .or(params.required)
            .unwrap_or_else(|| section.required_by_default());
        out.push((id, section, text, required));
    }
    Ok(out)
}

pub struct ChecklistAdd {
    pub actor_member_id: String,
    pub params: TasksChecklistAddParams,
    pub authority: TaskWriteAuthority,
}

impl ChecklistAdd {
    pub fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
    ) -> Result<(Change, Id, Vec<String>), Refusal>
    where
        E::Tx: TasksRead,
    {
        let p = &self.params;
        let mut ctx = ChecklistCtx::load(
            uow.view(),
            &self.actor_member_id,
            &p.command_id,
            &p.task,
            &self.authority,
        )?;
        let items = collect_add_items(p)?;
        if ctx.steps.len().saturating_add(items.len()) > TASK_MAX_CHECKLIST_STEPS {
            return Err(refuse(
                ErrorCode::TooLarge,
                format!("at most {TASK_MAX_CHECKLIST_STEPS} checklist steps allowed"),
            ));
        }
        let mut added_ids = Vec::with_capacity(items.len());
        for (step_id, section, text, required) in items {
            if ctx.steps.iter().any(|s| s.id == step_id) {
                return Err(refuse(
                    ErrorCode::AlreadyExists,
                    format!("checklist step `{step_id}` already exists"),
                ));
            }
            added_ids.push(step_id.as_str().to_owned());
            ctx.steps.push(ChecklistStepRecord {
                id: step_id,
                section,
                position: u64::MAX,
                text,
                required,
                done: false,
                linked_task_id: None,
                checked_by_employee_id: None,
                checked_at: None,
                revision: 1,
            });
        }
        reindex_steps(&mut ctx.steps);
        let entries: Vec<Value> = added_ids
            .iter()
            .filter_map(|id| ctx.steps.iter().find(|s| s.id.as_str() == id))
            .map(|s| json!({"step_id": s.id.as_str(), "section": s.section.as_str(), "index": s.position + 1, "text": s.text, "required": s.required}))
            .collect();
        let event = batch_or_single_event("checklist_step_added", &entries, None);
        ctx.commit(uow, &p.command_id, Ack::Durable, event, added_ids)
    }
}

pub struct ChecklistCheck {
    pub actor_member_id: String,
    pub params: TasksChecklistCheckParams,
    pub authority: TaskWriteAuthority,
}

impl ChecklistCheck {
    pub fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
    ) -> Result<(Change, Id, Vec<String>), Refusal>
    where
        E::Tx: TasksRead,
    {
        let p = &self.params;
        let mut ctx = ChecklistCtx::load(
            uow.view(),
            &self.actor_member_id,
            &p.command_id,
            &p.task,
            &self.authority,
        )?;
        let targets = resolve_targets(
            uow.view(),
            &ctx.task,
            &ctx.steps,
            &p.step_id,
            p.section,
            &p.indices,
            p.expected_revision,
        )?;
        let at = format_rfc3339_ms(uow.now().0);
        let (mut mutated_ids, mut entries) = (
            Vec::with_capacity(targets.len()),
            Vec::with_capacity(targets.len()),
        );
        for idx in targets {
            let step = &mut ctx.steps[idx];
            step.done = p.done;
            step.checked_by_employee_id = p.done.then(|| ctx.actor_id.clone());
            step.checked_at = p.done.then(|| at.clone());
            step.revision = step.revision.saturating_add(1);
            mutated_ids.push(step.id.as_str().to_owned());
            entries.push(json!({"step_id": step.id.as_str(), "section": step.section.as_str(), "index": step.position + 1, "text": step.text}));
        }
        let event = batch_or_single_event("checklist_step_checked", &entries, Some(p.done));
        ctx.commit(uow, &p.command_id, Ack::Committed, event, mutated_ids)
    }
}

pub struct ChecklistEdit {
    pub actor_member_id: String,
    pub params: TasksChecklistEditParams,
    pub authority: TaskWriteAuthority,
}

impl ChecklistEdit {
    pub fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
    ) -> Result<(Change, Id, Vec<String>), Refusal>
    where
        E::Tx: TasksRead,
    {
        let p = &self.params;
        let mut ctx = ChecklistCtx::load(
            uow.view(),
            &self.actor_member_id,
            &p.command_id,
            &p.task,
            &self.authority,
        )?;
        let idx = resolve_single(
            uow.view(),
            &ctx.task,
            &ctx.steps,
            &p.step_id,
            p.section,
            p.index,
            p.expected_revision,
        )?;
        if p.text.is_none() && p.required.is_none() {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                "checklist.edit requires `text` or `required`",
            ));
        }
        let step = &mut ctx.steps[idx];
        let mut fields: Vec<&'static str> = Vec::new();
        if let Some(ref text) = p.text {
            step.text = validate_step_text(text)?;
            fields.push("text");
        }
        if let Some(req) = p.required {
            step.required = req;
            fields.push("required");
        }
        step.revision = step.revision.saturating_add(1);
        let step_id = step.id.as_str().to_owned();
        let event = json!({"type": "checklist_step_edited", "step_id": step_id, "section": step.section.as_str(), "index": step.position + 1, "fields": fields, "text": step.text, "required": step.required});
        ctx.commit(uow, &p.command_id, Ack::Durable, event, vec![step_id])
    }
}

pub struct ChecklistRemove {
    pub actor_member_id: String,
    pub params: TasksChecklistRemoveParams,
    pub authority: TaskWriteAuthority,
}

impl ChecklistRemove {
    pub fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
    ) -> Result<(Change, Id, Vec<String>), Refusal>
    where
        E::Tx: TasksRead,
    {
        let p = &self.params;
        let mut ctx = ChecklistCtx::load(
            uow.view(),
            &self.actor_member_id,
            &p.command_id,
            &p.task,
            &self.authority,
        )?;
        let idx = resolve_single(
            uow.view(),
            &ctx.task,
            &ctx.steps,
            &p.step_id,
            p.section,
            p.index,
            p.expected_revision,
        )?;
        let removed = ctx.steps.remove(idx);
        let step_id = removed.id.as_str().to_owned();
        reindex_steps(&mut ctx.steps);
        let event = json!({"type": "checklist_step_removed", "step_id": step_id, "section": removed.section.as_str(), "index": removed.position + 1, "text": removed.text});
        ctx.commit(uow, &p.command_id, Ack::Durable, event, vec![step_id])
    }
}

pub struct ChecklistMove {
    pub actor_member_id: String,
    pub params: TasksChecklistMoveParams,
    pub authority: TaskWriteAuthority,
}

impl ChecklistMove {
    pub fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
    ) -> Result<(Change, Id, Vec<String>), Refusal>
    where
        E::Tx: TasksRead,
    {
        let p = &self.params;
        let mut ctx = ChecklistCtx::load(
            uow.view(),
            &self.actor_member_id,
            &p.command_id,
            &p.task,
            &self.authority,
        )?;
        let has_id = !p.step_id.trim().is_empty();
        let src_sec = if has_id {
            p.from_section
        } else {
            p.from_section.or(p.section)
        };
        let dest_override = if has_id || p.from_section.is_some() {
            p.section
        } else {
            None
        };
        let pos = resolve_single(
            uow.view(),
            &ctx.task,
            &ctx.steps,
            &p.step_id,
            src_sec,
            p.index,
            p.expected_revision,
        )?;
        let mut step = ctx.steps.remove(pos);
        let (from_section, from_index) = (step.section, step.position + 1);
        let step_id = step.id.as_str().to_owned();
        step.revision = step.revision.saturating_add(1);
        let section = dest_override.unwrap_or(step.section);
        step.section = section;
        let dest = insert_in_section(&mut ctx.steps, section, p.position as usize, step);
        reindex_steps(&mut ctx.steps);
        let event = json!({"type": "checklist_step_moved", "step_id": step_id, "from_section": from_section.as_str(), "from_index": from_index, "section": section.as_str(), "position": dest, "index": dest + 1});
        ctx.commit(uow, &p.command_id, Ack::Durable, event, vec![step_id])
    }
}

fn insert_in_section(
    steps: &mut Vec<ChecklistStepRecord>,
    section: ChecklistSection,
    target_pos: usize,
    step: ChecklistStepRecord,
) -> usize {
    let in_section: Vec<usize> = (0..steps.len())
        .filter(|&i| steps[i].section == section)
        .collect();
    let dest = target_pos.min(in_section.len());
    let index = match in_section.get(dest) {
        Some(&index) => index,
        None => in_section.last().map_or_else(
            || steps.iter().take_while(|s| s.section < section).count(),
            |&last| last + 1,
        ),
    };
    steps.insert(index, step);
    dest
}
