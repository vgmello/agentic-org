//! Core task mutations: update and assign (`TASK-49`, `TASK-77`; a transition is
//! [`super::transition`]'s). Each rewrites `task.md` at the task's next revision with that
//! revision's history event and is answered `durable` (`doc-79`, `ADR-0014`).

use std::collections::BTreeSet;

use org_proto::tasks::{PatchField, TasksAssignParams, TasksUpdateParams, TASK_MAX_DEPENDENCIES};
use org_proto::ErrorCode;
use org_store::{Ack, Change, Engine, Id, Refusal, UnitOfWork};
use serde_json::{json, Value};

use crate::organization::records::EmployeeKind;
use crate::tasks::ports::TasksRead;
use crate::tasks::records::{normalize_file_paths, normalize_labels, TaskLinkRecord, TaskRecord};
use crate::time::format_rfc3339_ms;

use super::write::{current_links, task_location, Writer};
use super::{
    check_dependency_acyclic, parse_details, parse_uuid_v7, refuse, require_active_employee,
    require_task, storage_refusal, validate_active_employee, validate_active_team, validate_body,
    validate_task_type, validate_title,
};

/// A core mutation of `task`, already advanced to its next revision: `task.md` with the
/// task's links as they stand and that revision's event, answered `durable` (`doc-79`).
pub(crate) fn rewrite_task<R: TasksRead>(
    view: &R,
    task: &TaskRecord,
    actor_id: &Id,
    at: &str,
    command_id: &str,
    event: Value,
) -> Result<Change, Refusal> {
    let location = task_location(view, task)?;
    let links = current_links(view, task)?;
    Writer::new(view, actor_id, at, command_id)?.task_and_event(
        Change::new(Ack::Durable),
        &location,
        task,
        &links,
        event,
    )
}

pub(crate) fn derive_link_id(source: &Id, target: &Id, salt: u32) -> Id {
    let salt_bytes = salt.to_be_bytes();
    derived_id(&[
        b"task-link-v1:",
        source.as_str().as_bytes(),
        b"->",
        target.as_str().as_bytes(),
        &salt_bytes,
    ])
}

/// A UUIDv7-shaped ID derived from `parts` alone, so a retry derives the same one.
pub(crate) fn derived_id(parts: &[&[u8]]) -> Id {
    let digest = org_store::Sha256::of_parts(parts);
    let mut b = [0u8; 16];
    b.copy_from_slice(&digest.as_bytes()[..16]);
    b[6] = (b[6] & 0x0f) | 0x70;
    b[8] = (b[8] & 0x3f) | 0x80;
    let formatted = format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    );
    Id::parse(&formatted).expect("derived UUIDv7 id")
}

/// Updates core task fields, `rank_order`, `labels`, `reference_files`, `modified_files`, and `details`.
pub struct UpdateTask {
    pub actor_member_id: String,
    pub params: TasksUpdateParams,
}

impl UpdateTask {
    pub fn decide<E: Engine>(&self, uow: &mut UnitOfWork<'_, E>) -> Result<(Change, Id), Refusal>
    where
        E::Tx: TasksRead,
    {
        let actor = require_active_employee(uow.view(), &self.actor_member_id)?;
        let _cmd_id = parse_uuid_v7(&self.params.command_id, "command_id")?;
        let mut task = require_task(uow.view(), &self.params.task)?;
        if task.meta.revision != self.params.expected_revision {
            return Err(refuse(
                ErrorCode::Conflict,
                format!(
                    "stale expected_revision {}: task is at revision {}",
                    self.params.expected_revision, task.meta.revision
                ),
            ));
        }

        let mut changed_fields: Vec<&'static str> = Vec::new();
        apply_scalar_updates(uow.view(), &mut task, &self.params, &mut changed_fields)?;
        apply_collection_updates(&mut task, &self.params, &mut changed_fields)?;

        if changed_fields.is_empty() {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                "tasks.update requires at least one field change",
            ));
        }

        let at = format_rfc3339_ms(uow.now().0);
        task.meta.touch(&actor.id, &at);
        let change = rewrite_task(
            uow.view(),
            &task,
            &actor.id,
            &at,
            &self.params.command_id,
            json!({
                "type": "task_updated",
                "fields": changed_fields,
                "rank_order": task.rank_order,
                "labels": task.labels,
            }),
        )?;
        Ok((change, task.id))
    }
}

fn apply_scalar_updates<R: TasksRead>(
    view: &R,
    task: &mut TaskRecord,
    params: &TasksUpdateParams,
    changed: &mut Vec<&'static str>,
) -> Result<(), Refusal> {
    if let Some(ref title) = params.title {
        task.title = validate_title(title)?;
        changed.push("title");
    }
    if let Some(ref body) = params.body_markdown {
        task.body_markdown = validate_body(body)?;
        changed.push("body_markdown");
    }
    if let Some(ref kind) = params.task_type {
        task.task_type = validate_task_type(kind)?;
        changed.push("type");
    }
    if let PatchField::Set(wt) = params.work_type {
        task.work_type = wt;
        changed.push("work_type");
    }
    if let Some(prio) = params.priority {
        task.priority = prio;
        changed.push("priority");
    }
    if let PatchField::Set(effort) = params.effort {
        if let Some(v) = effort {
            super::validate_i64_u64(v, "effort")?;
        }
        task.effort = effort;
        changed.push("effort");
    }
    if let PatchField::Set(ref sev) = params.severity {
        task.severity = sev.clone();
        changed.push("severity");
    }
    if let PatchField::Set(order) = params.rank_order {
        if let Some(v) = order {
            super::validate_i64_u64(v, "rank_order")?;
        }
        task.rank_order = order;
        changed.push("rank_order");
    }
    if let Some(ref details) = params.details {
        let mut next = parse_details(view, &task.id, details)?;
        // Only a transition sets or clears the "done anyway" record (`TASK-88`).
        next.done_anyway = task.details.as_ref().and_then(|d| d.done_anyway.clone());
        task.details = Some(next);
        changed.push("details");
    }
    Ok(())
}

fn apply_collection_updates(
    task: &mut TaskRecord,
    params: &TasksUpdateParams,
    changed: &mut Vec<&'static str>,
) -> Result<(), Refusal> {
    if let Some(ref replace) = params.labels {
        if !params.add_labels.is_empty() || !params.remove_labels.is_empty() {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                "cannot combine `labels` replacement with `add_labels` or `remove_labels`",
            ));
        }
        task.labels = normalize_labels(replace).map_err(|e| refuse(e.code, e.message))?;
        changed.push("labels");
    } else if !params.add_labels.is_empty() || !params.remove_labels.is_empty() {
        let to_remove: BTreeSet<String> = normalize_labels(&params.remove_labels)
            .map_err(|e| refuse(e.code, e.message))?
            .into_iter()
            .collect();
        let mut next: Vec<String> = task
            .labels
            .iter()
            .filter(|l| !to_remove.contains(*l))
            .cloned()
            .collect();
        next.extend(params.add_labels.iter().cloned());
        task.labels = normalize_labels(&next).map_err(|e| refuse(e.code, e.message))?;
        changed.push("labels");
    }

    apply_file_role_update(
        &mut task.reference_files,
        params.reference_files.as_ref(),
        &params.add_reference_files,
        &params.remove_reference_files,
        "reference",
        "reference_files",
        changed,
    )?;
    apply_file_role_update(
        &mut task.modified_files,
        params.modified_files.as_ref(),
        &params.add_modified_files,
        &params.remove_modified_files,
        "modified",
        "modified_files",
        changed,
    )?;
    Ok(())
}

fn apply_file_role_update(
    target: &mut Vec<String>,
    replace: Option<&Vec<String>>,
    add: &[String],
    remove: &[String],
    role: &str,
    field_name: &'static str,
    changed: &mut Vec<&'static str>,
) -> Result<(), Refusal> {
    if let Some(rep) = replace {
        if !add.is_empty() || !remove.is_empty() {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                format!("cannot combine `{field_name}` replacement with incremental add/remove"),
            ));
        }
        *target = normalize_file_paths(rep, role).map_err(|e| refuse(e.code, e.message))?;
        changed.push(field_name);
    } else if !add.is_empty() || !remove.is_empty() {
        let drop_set: BTreeSet<String> = normalize_file_paths(remove, role)
            .map_err(|e| refuse(e.code, e.message))?
            .into_iter()
            .collect();
        let mut next: Vec<String> = target
            .iter()
            .filter(|p| !drop_set.contains(*p))
            .cloned()
            .collect();
        next.extend(add.iter().cloned());
        *target = normalize_file_paths(&next, role).map_err(|e| refuse(e.code, e.message))?;
        changed.push(field_name);
    }
    Ok(())
}

/// Atomically updates `assigned_team_id` and `assignee_id` (`doc-77`, `doc-84`, `AVAIL-2`).
pub struct AssignTask {
    pub actor_member_id: String,
    pub params: TasksAssignParams,
}

impl AssignTask {
    pub fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
    ) -> Result<(Change, Id, Option<Id>), Refusal>
    where
        E::Tx: TasksRead,
    {
        let actor = require_active_employee(uow.view(), &self.actor_member_id)?;
        let _cmd_id = parse_uuid_v7(&self.params.command_id, "command_id")?;
        let mut task = require_task(uow.view(), &self.params.task)?;
        if task.meta.revision != self.params.expected_revision {
            return Err(refuse(
                ErrorCode::Conflict,
                format!(
                    "stale expected_revision {}: task is at revision {}",
                    self.params.expected_revision, task.meta.revision
                ),
            ));
        }

        let from_team = task
            .assigned_team_id
            .as_ref()
            .map(|id| id.as_str().to_owned());
        let from_assignee = task.assignee_id.as_ref().map(|id| id.as_str().to_owned());

        let next_team = match &self.params.assigned_team_id {
            PatchField::Unchanged => task.assigned_team_id.clone(),
            PatchField::Set(None) => None,
            PatchField::Set(Some(tid)) => {
                Some(validate_active_team(uow.view(), tid, "assigned_team_id")?)
            }
        };
        let mut newly_assigned_agent = None;
        let next_assignee = match &self.params.assignee_id {
            PatchField::Unchanged => task.assignee_id.clone(),
            PatchField::Set(None) => None,
            PatchField::Set(Some(eid)) => {
                let emp = validate_active_employee(uow.view(), eid, "assignee_id")?;
                if emp.kind == EmployeeKind::Agent && task.assignee_id.as_ref() != Some(&emp.id) {
                    newly_assigned_agent = Some(emp.id.clone());
                }
                Some(emp.id)
            }
        };

        task.assigned_team_id = next_team;
        task.assignee_id = next_assignee;
        let at = format_rfc3339_ms(uow.now().0);
        task.meta.touch(&actor.id, &at);

        // Exact `assignment_changed` shape required by `schema.sql` CHECK on `task_events` (`doc-84`).
        let change = rewrite_task(
            uow.view(),
            &task,
            &actor.id,
            &at,
            &self.params.command_id,
            json!({
                "type": "assignment_changed",
                "from": {
                    "assigned_team_id": from_team,
                    "assignee_id": from_assignee,
                },
                "to": {
                    "assigned_team_id": task.assigned_team_id.as_ref().map(|id| id.as_str()),
                    "assignee_id": task.assignee_id.as_ref().map(|id| id.as_str()),
                },
            }),
        )?;
        Ok((change, task.id, newly_assigned_agent))
    }
}

/// Validates and adds a direct dependency edge on an existing task (reused by `ops.rs`).
/// Returns `(links, added)` where `added` is `false` when the edge already existed.
pub(crate) fn add_dependency_edge<R: TasksRead>(
    view: &R,
    source_id: &Id,
    target_id: &Id,
) -> Result<(Vec<TaskLinkRecord>, bool), Refusal> {
    check_dependency_acyclic(view, source_id, target_id)?;
    let mut links = view.task_links_from(source_id).map_err(storage_refusal)?;
    if links
        .iter()
        .any(|l| l.kind == "depends_on" && &l.target_id == target_id)
    {
        return Ok((links, false));
    }
    let dep_count = links.iter().filter(|l| l.kind == "depends_on").count();
    if dep_count >= TASK_MAX_DEPENDENCIES {
        return Err(refuse(
            ErrorCode::TooLarge,
            format!("at most {TASK_MAX_DEPENDENCIES} dependencies allowed"),
        ));
    }
    links.push(TaskLinkRecord {
        source_id: source_id.clone(),
        id: derive_link_id(source_id, target_id, links.len() as u32),
        kind: "depends_on".into(),
        target_kind: "task".into(),
        target_id: target_id.clone(),
    });
    Ok((links, true))
}
