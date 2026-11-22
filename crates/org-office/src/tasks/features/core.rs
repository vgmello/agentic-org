//! Core task lifecycle mutations: create, update, transition, and assign (`TASK-49`, `TASK-77`).

use std::collections::BTreeSet;

use org_proto::tasks::{
    PatchField, TaskPriority, TaskStatus, TasksAssignParams, TasksCreateParams,
    TasksTransitionParams, TasksUpdateParams, TASK_MAX_DEPENDENCIES,
};
use org_proto::ErrorCode;
use org_store::{Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};
use serde_json::json;

use crate::organization::records::EmployeeKind;
use crate::tasks::ports::TasksRead;
use crate::tasks::records::{
    encode_json, links_path, normalize_file_paths, normalize_labels, office_task_counter_path,
    OfficeTaskCounterRecord, TaskLinkRecord, TaskMeta, TaskRecord,
};
use crate::time::format_rfc3339_ms;

use super::{
    check_dependency_acyclic, parse_details, parse_uuid_v7, refuse, require_active_employee,
    require_office_code, require_task, storage_refusal, validate_active_employee,
    validate_active_team, validate_body, validate_task_type, validate_title, write_task_and_event,
};

/// Creates a unified task (`task`, `bug`, `request`, ...), minting its office-coded key locally.
pub struct CreateTask {
    pub actor_member_id: String,
    pub params: TasksCreateParams,
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
        let prev_counter = uow
            .view()
            .office_task_counter(&office_code)
            .map_err(storage_refusal)?;
        let next_number = prev_counter.as_ref().map_or(1, |c| c.last_number + 1);
        let counter = OfficeTaskCounterRecord {
            office_code: office_code.clone(),
            last_number: next_number,
            reserved_through: prev_counter
                .as_ref()
                .map_or(next_number, |c| c.reserved_through.max(next_number)),
        };

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
            details,
            meta: TaskMeta::created(&actor.id, &at),
        };

        let counter_bytes = encode_json(&counter)
            .map_err(|e| refuse(ErrorCode::StorageError, format!("encode counter: {e}")))?;
        let mut change = Change::new(org_store::Ack::Durable)
            .put(office_task_counter_path(&office_code), counter_bytes);
        if !links.is_empty() {
            let link_bytes = encode_json(&links)
                .map_err(|e| refuse(ErrorCode::StorageError, format!("encode links: {e}")))?;
            change = change.put(links_path(&task_id), link_bytes);
        }
        let change = write_task_and_event(
            change,
            &task,
            &actor.id,
            &at,
            &self.params.command_id,
            json!({
                "type": "task_created",
                "key": task.key(),
                "task_type": task_type,
                "title": title,
                "rank_order": task.rank_order,
                "labels": task.labels,
            }),
        )?;
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

pub(crate) fn derive_link_id(source: &Id, target: &Id, salt: u32) -> Id {
    let salt_bytes = salt.to_be_bytes();
    let digest = org_store::Sha256::of_parts(&[
        b"task-link-v1:",
        source.as_str().as_bytes(),
        b"->",
        target.as_str().as_bytes(),
        &salt_bytes,
    ]);
    let mut b = [0u8; 16];
    b.copy_from_slice(&digest.as_bytes()[..16]);
    b[6] = (b[6] & 0x0f) | 0x70;
    b[8] = (b[8] & 0x3f) | 0x80;
    let formatted = format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    );
    Id::parse(&formatted).expect("derived UUIDv7 link id")
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
        let change = write_task_and_event(
            Change::new(org_store::Ack::Durable),
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
        task.details = Some(parse_details(view, &task.id, details)?);
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

/// Transitions a task's status (`todo`, `in_progress`, `blocked`, `cancelled`, `done`).
pub struct TransitionTask {
    pub actor_member_id: String,
    pub params: TasksTransitionParams,
}

impl TransitionTask {
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
        if self.params.status == TaskStatus::Blocked
            && self
                .params
                .blocked_reason
                .as_deref()
                .map(str::trim)
                .is_none_or(str::is_empty)
        {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                "transitioning to `blocked` requires a non-empty `blocked_reason`",
            ));
        }
        let from_status = task.status;
        task.status = self.params.status;
        task.blocked_reason = if self.params.status == TaskStatus::Blocked {
            self.params
                .blocked_reason
                .as_deref()
                .map(|s| s.trim().to_owned())
        } else {
            None
        };
        task.resolution = match self.params.status {
            TaskStatus::Done => Some(
                self.params
                    .resolution
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or("completed")
                    .to_owned(),
            ),
            TaskStatus::Cancelled => Some(
                self.params
                    .resolution
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or("cancelled")
                    .to_owned(),
            ),
            _ => None,
        };
        if matches!(from_status, TaskStatus::Done | TaskStatus::Cancelled)
            && !matches!(task.status, TaskStatus::Done | TaskStatus::Cancelled)
        {
            let det = task.details.get_or_insert_with(Default::default);
            det.reopen_count =
                super::validate_i64_u64(det.reopen_count.saturating_add(1), "reopen_count")?;
        }

        let at = format_rfc3339_ms(uow.now().0);
        task.meta.touch(&actor.id, &at);
        let change = write_task_and_event(
            Change::new(org_store::Ack::Durable),
            &task,
            &actor.id,
            &at,
            &self.params.command_id,
            json!({
                "type": "status_changed",
                "from": from_status.as_str(),
                "to": task.status.as_str(),
                "blocked_reason": task.blocked_reason,
                "resolution": task.resolution,
                "note": self.params.note,
            }),
        )?;
        Ok((change, task.id))
    }
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
        let change = write_task_and_event(
            Change::new(org_store::Ack::Durable),
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
