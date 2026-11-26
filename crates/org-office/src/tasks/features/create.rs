//! Creating a task (`TASK-49`, `TASK-77`, `doc-77`, `doc-80`, `ADR-0019`): its office-coded
//! key is minted locally from the office's own counter, `local/offices/<office-id>/
//! task-counter.yaml`, written in the same entry as the task's `task.md` and its revision-1
//! `created` history event.
//!
//! The counter reserves numbers a block at a time (`ADR-0019`): `last_number <=
//! reserved_through`, and a number above `reserved_through` is only issued with a new
//! reservation answered `durable`. A create inside the reserved block is answered
//! `committed`. After a stop that was not clean the next number is past the reservation, so
//! a number an answer named but a crash lost is never issued again.

use std::collections::BTreeSet;

use org_proto::tasks::{TaskPriority, TaskStatus, TasksCreateParams, TASK_MAX_DEPENDENCIES};
use org_proto::ErrorCode;
use org_store::{Ack, Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};

use crate::organization::records::EmployeeKind;
use crate::tasks::ports::TasksRead;
use crate::tasks::records::{
    self, normalize_file_paths, normalize_labels, OfficeTaskCounterRecord, TaskLinkRecord,
    TaskMeta, TaskRecord,
};
use crate::time::format_rfc3339_ms;

use super::core::derive_link_id;
use super::write::{task_location, Writer};
use super::{
    parse_details, parse_uuid_v7, refuse, require_active_employee, require_office_code,
    require_task, storage_refusal, validate_active_employee, validate_active_team, validate_body,
    validate_task_type, validate_title,
};

/// How many task numbers one durable reservation covers (`ADR-0019`): a create inside the
/// reserved block is answered `committed`; the create that needs the next block waits for
/// its reservation to be `durable`.
pub const TASK_NUMBER_BLOCK: u64 = 32;

/// Creates a unified task (`task`, `bug`, `request`, ...), minting its office-coded key locally.
pub struct CreateTask {
    pub actor_member_id: String,
    pub params: TasksCreateParams,
    /// Whether numbers up to the counter's `reserved_through` may have been issued by a run
    /// that did not stop cleanly (`ADR-0019`): the next number is then past the reservation,
    /// so no `T-<CODE><N>` an earlier answer named is ever issued again.
    pub jump_reserved: bool,
}

/// The office's counter after issuing one more number, and the class the create is
/// answered in: `durable` when it reserves the next block, `committed` inside one.
fn issue_number<R: TasksRead>(
    view: &R,
    office_code: &str,
    jump_reserved: bool,
) -> Result<(OfficeTaskCounterRecord, Ack), Refusal> {
    let prev = view
        .office_task_counter(office_code)
        .map_err(storage_refusal)?;
    let floor = match &prev {
        Some(c) if jump_reserved => c.last_number.max(c.reserved_through),
        Some(c) => c.last_number,
        // No counter (a new office, or one whose local files are gone): past every number
        // this office code already has.
        None => view
            .tasks()
            .map_err(storage_refusal)?
            .iter()
            .filter(|t| t.office_code == office_code)
            .map(|t| t.number)
            .max()
            .unwrap_or(0),
    };
    let next = super::validate_i64_u64(floor + 1, "task number")?;
    let reserved = prev.as_ref().map_or(0, |c| c.reserved_through);
    let (reserved_through, ack) = if jump_reserved || next > reserved {
        let through = next
            .saturating_add(TASK_NUMBER_BLOCK - 1)
            .min(i64::MAX as u64);
        (through, Ack::Durable)
    } else {
        (reserved, Ack::Committed)
    };
    let counter = OfficeTaskCounterRecord {
        office_code: office_code.to_owned(),
        last_number: next,
        reserved_through,
    };
    Ok((counter, ack))
}

impl CreateTask {
    pub fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
        here: &OfficeIdentity,
    ) -> Result<(Change, Id, Option<Id>), Refusal>
    where
        E::Tx: TasksRead,
    {
        let actor = require_active_employee(uow.view(), &self.actor_member_id)?;
        let _cmd_id = parse_uuid_v7(&self.params.command_id, "command_id")?;
        let task_id = parse_uuid_v7(&self.params.task_id, "task_id")?;
        if uow
            .view()
            .task(&task_id)
            .map_err(storage_refusal)?
            .is_some()
        {
            return Err(refuse(
                ErrorCode::AlreadyExists,
                format!("task `{task_id}` already exists"),
            ));
        }
        let (team_id, project_id) = resolve_scope(uow.view(), &self.params)?;
        let assigned_team_id = match self.params.assigned_team_id.as_deref() {
            Some(tid) => Some(validate_active_team(uow.view(), tid, "assigned_team_id")?),
            None => None,
        };
        let (assignee_id, assigned_agent) = match self.params.assignee_id.as_deref() {
            Some(eid) => {
                let emp = validate_active_employee(uow.view(), eid, "assignee_id")?;
                let agent = (emp.kind == EmployeeKind::Agent).then(|| emp.id.clone());
                (Some(emp.id), agent)
            }
            None => (None, None),
        };
        let title = validate_title(&self.params.title)?;
        let body_markdown = validate_body(&self.params.body_markdown)?;
        let task_type = match self.params.task_type.as_deref() {
            Some(t) => validate_task_type(t)?,
            None => "task".to_owned(),
        };
        let labels =
            normalize_labels(&self.params.labels).map_err(|e| refuse(e.code, e.message))?;
        let reference_files = normalize_file_paths(&self.params.reference_files, "reference")
            .map_err(|e| refuse(e.code, e.message))?;
        let modified_files = normalize_file_paths(&self.params.modified_files, "modified")
            .map_err(|e| refuse(e.code, e.message))?;
        let details = match self.params.details.as_ref() {
            Some(w) => Some(parse_details(uow.view(), &task_id, w)?),
            None => None,
        };
        let links = build_initial_depends_on(uow.view(), &task_id, &self.params.depends_on)?;

        // Mint final `T-<CODE><N>` reference locally using this office's active office_code.
        let office_code = require_office_code(uow.view(), &here.office_id)?;
        let (counter, ack) = issue_number(uow.view(), &office_code, self.jump_reserved)?;
        let next_number = counter.last_number;

        if let Some(effort) = self.params.effort {
            super::validate_i64_u64(effort, "effort")?;
        }
        if let Some(rank_order) = self.params.rank_order {
            super::validate_i64_u64(rank_order, "rank_order")?;
        }
        let at = format_rfc3339_ms(uow.now().0);
        let task = TaskRecord {
            id: task_id.clone(),
            office_code: office_code.clone(),
            number: next_number,
            team_id,
            assigned_team_id,
            project_id,
            task_type: task_type.clone(),
            work_type: self.params.work_type,
            title: title.clone(),
            status: TaskStatus::Todo,
            assignee_id,
            body_markdown,
            feature_id: None,
            milestone_id: None,
            priority: self.params.priority.unwrap_or(TaskPriority::Normal),
            effort: self.params.effort,
            severity: self.params.severity.clone(),
            resolution: None,
            blocked_reason: None,
            rank_order: self.params.rank_order,
            labels,
            reference_files,
            modified_files,
            repositories: Vec::new(),
            details,
            meta: TaskMeta::created(&actor.id, &at),
        };

        let location = task_location(uow.view(), &task)?;
        let writer = Writer::new(uow.view(), &actor.id, &at, &self.params.command_id)?;
        let known_facts = records::task_md::task_fields(&task, &links);
        let created = records::created_change(known_facts, &task.body_markdown);
        let change = Change::new(ack).put(
            records::counter_path(&here.office_id),
            records::encode_counter(&here.office_id, &counter),
        );
        let change = writer.task_and_event(change, &location, &task, &links, created)?;
        Ok((change, task_id, assigned_agent))
    }
}

fn resolve_scope<R: TasksRead>(
    view: &R,
    params: &TasksCreateParams,
) -> Result<(Id, Option<Id>), Refusal> {
    if let Some(proj_ref) = params.project_id.as_deref() {
        let project = view
            .resolve_project(proj_ref)
            .map_err(storage_refusal)?
            .ok_or_else(|| {
                refuse(
                    ErrorCode::NotFound,
                    format!("project `{proj_ref}` not found"),
                )
            })?;
        if project.archived {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                format!("project `{proj_ref}` is archived"),
            ));
        }
        if let Some(team_str) = params.team_id.as_deref() {
            let explicit_team = validate_active_team(view, team_str, "team_id")?;
            if explicit_team != project.owner_team_id {
                return Err(refuse(
                    ErrorCode::InvalidRequest,
                    "a project task's owning team_id must match the project's owner_team_id",
                ));
            }
        }
        return Ok((project.owner_team_id, Some(project.id)));
    }
    let Some(team_str) = params.team_id.as_deref() else {
        return Err(refuse(
            ErrorCode::InvalidRequest,
            "either `team_id` or `project_id` is required",
        ));
    };
    let team_id = validate_active_team(view, team_str, "team_id")?;
    Ok((team_id, None))
}

fn build_initial_depends_on<R: TasksRead>(
    view: &R,
    task_id: &Id,
    refs: &[String],
) -> Result<Vec<TaskLinkRecord>, Refusal> {
    if refs.len() > TASK_MAX_DEPENDENCIES {
        return Err(refuse(
            ErrorCode::TooLarge,
            format!("at most {TASK_MAX_DEPENDENCIES} dependencies allowed"),
        ));
    }
    let mut seen = BTreeSet::new();
    let mut links = Vec::new();
    for (idx, dep_ref) in refs.iter().enumerate() {
        let target = require_task(view, dep_ref)?;
        if &target.id == task_id {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                "a task cannot depend on itself",
            ));
        }
        if seen.insert(target.id.clone()) {
            links.push(TaskLinkRecord {
                source_id: task_id.clone(),
                id: derive_link_id(task_id, &target.id, idx as u32),
                kind: "depends_on".into(),
                target_kind: "task".into(),
                target_id: target.id,
            });
        }
    }
    Ok(links)
}
