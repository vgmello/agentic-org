//! Core task mutations: update and assign (`TASK-49`, `TASK-77`; a transition is
//! [`super::transition`]'s). Each rewrites `task.md` at the task's next revision with that
//! revision's history event and is answered `durable` (`doc-79`, `ADR-0014`).

use std::collections::BTreeSet;

use org_proto::attachments::{
    check_task_attachment_ids, AttachmentTargetKind, TASK_ATTACHMENTS_MAX,
};
use org_proto::tasks::{PatchField, TasksAssignParams, TasksUpdateParams, TASK_MAX_DEPENDENCIES};
use org_proto::ErrorCode;
use org_store::{Ack, Change, Engine, Id, Refusal, UnitOfWork};
use serde_json::{json, Map, Value};

use crate::attachments::records::AttachmentRecord;
use crate::organization::records::EmployeeKind;
use crate::tasks::ops::{
    current_attachments, current_links, derive_link_id, require_local_task_write_authority,
    rewrite_task, task_location, TaskWriteAuthority, Writer,
};
use crate::tasks::ports::TasksRead;
use crate::tasks::records::{normalize_file_paths, normalize_labels, TaskLinkRecord, TaskRecord};
use crate::time::format_rfc3339_ms;

use super::attachments::PreparedTaskFiles;
use super::conflict::stale_revision_refusal;
use super::{
    check_dependency_acyclic, parse_details, parse_uuid_v7, refuse, require_active_employee,
    require_task, storage_refusal, validate_active_employee, validate_active_team, validate_body,
    validate_task_type, validate_title,
};

/// Updates core task fields, `rank_order`, `labels`, `reference_files`, `modified_files`,
/// `details`, and task attachments (`TASK-98.4`).
pub struct UpdateTask {
    pub actor_member_id: String,
    pub params: TasksUpdateParams,
    /// Validated, blob-promoted files for `params.add_attachments` (`TASK-98.4`).
    pub prepared_files: PreparedTaskFiles,
    /// Single-office task write authority verified atomically inside the `UnitOfWork` (`TASK-218`).
    pub authority: TaskWriteAuthority,
}

impl UpdateTask {
    pub fn decide<E: Engine>(&self, uow: &mut UnitOfWork<'_, E>) -> Result<(Change, Id), Refusal>
    where
        E::Tx: TasksRead,
    {
        let actor = require_active_employee(uow.view(), &self.actor_member_id)?;
        let _cmd_id = parse_uuid_v7(&self.params.command_id, "command_id")?;
        let mut task = require_task(uow.view(), &self.params.task)?;
        require_local_task_write_authority(uow.view(), &self.authority, &task)?;
        if task.meta.revision != self.params.expected_revision {
            return Err(stale_revision_refusal(
                uow.view(),
                &task,
                self.params.expected_revision,
            ));
        }

        let mut changed_fields: Vec<&'static str> = Vec::new();
        let mut deltas = Map::new();
        apply_scalar_updates(
            uow.view(),
            &mut task,
            &self.params,
            &mut changed_fields,
            &mut deltas,
        )?;
        apply_collection_updates(&mut task, &self.params, &mut changed_fields, &mut deltas)?;

        let at = format_rfc3339_ms(uow.now().0);
        let UpdatedAttachments {
            records: attachments,
            added_ids,
            removed_ids,
        } = apply_attachment_updates(
            uow.view(),
            &task,
            &self.params,
            &self.prepared_files,
            &at,
            &mut changed_fields,
        )?;

        if changed_fields.is_empty() {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                "tasks.update requires at least one field change",
            ));
        }

        task.meta.touch(&actor.id, &at);
        let location = task_location(uow.view(), &task)?;
        let links = current_links(uow.view(), &task)?;
        let mut event = json!({
            "type": "task_updated",
            "fields": changed_fields,
            "rank_order": task.rank_order,
            "labels": task.labels,
        });
        if !deltas.is_empty() {
            event["changes"] = Value::Object(deltas);
        }
        if !added_ids.is_empty() {
            event["added_attachment_ids"] = json!(added_ids);
        }
        if !removed_ids.is_empty() {
            event["removed_attachment_ids"] = json!(removed_ids);
        }
        let change = Writer::new(uow.view(), &actor.id, &at, &self.params.command_id)?
            .task_and_event(
                Change::new(Ack::Durable),
                &location,
                &task,
                &links,
                &attachments,
                event,
            )?;
        Ok((change, task.id))
    }
}

struct UpdatedAttachments {
    records: Vec<AttachmentRecord>,
    added_ids: Vec<String>,
    removed_ids: Vec<String>,
}

fn apply_attachment_updates<R: TasksRead>(
    view: &R,
    task: &TaskRecord,
    params: &TasksUpdateParams,
    prepared_files: &PreparedTaskFiles,
    at: &str,
    changed: &mut Vec<&'static str>,
) -> Result<UpdatedAttachments, Refusal> {
    let mut attachments = current_attachments(view, task)?;
    if params.add_attachments.is_empty() && params.remove_attachments.is_empty() {
        return Ok(UpdatedAttachments {
            records: attachments,
            added_ids: Vec::new(),
            removed_ids: Vec::new(),
        });
    }
    if let Err(err) = check_task_attachment_ids(&params.add_attachments) {
        return Err(refuse(err.code, err.message));
    }
    if let Err(err) = check_task_attachment_ids(&params.remove_attachments) {
        return Err(refuse(err.code, err.message));
    }
    let remove_set: BTreeSet<&str> = params
        .remove_attachments
        .iter()
        .map(String::as_str)
        .collect();
    if params
        .add_attachments
        .iter()
        .any(|id| remove_set.contains(id.as_str()))
    {
        return Err(refuse(
            ErrorCode::InvalidRequest,
            "cannot add and remove the same attachment in one update",
        ));
    }
    for remove_id in &params.remove_attachments {
        if !attachments.iter().any(|a| a.id.as_str() == remove_id) {
            return Err(refuse(
                ErrorCode::NotFound,
                format!(
                    "attachment `{remove_id}` is not attached to task `{}`",
                    task.key()
                ),
            ));
        }
    }
    let removed_ids = params.remove_attachments.clone();
    attachments.retain(|a| !remove_set.contains(a.id.as_str()));
    let added = if params.add_attachments.is_empty() {
        Vec::new()
    } else {
        prepared_files.to_records(AttachmentTargetKind::Task, &task.id, &params.command_id, at)?
    };
    if attachments.len() + added.len() > TASK_ATTACHMENTS_MAX {
        return Err(refuse(
            ErrorCode::TooLarge,
            format!("a task carries at most {TASK_ATTACHMENTS_MAX} attachments"),
        ));
    }
    let added_ids: Vec<String> = added.iter().map(|r| r.id.to_string()).collect();
    attachments.extend(added);
    changed.push("attachments");
    Ok(UpdatedAttachments {
        records: attachments,
        added_ids,
        removed_ids,
    })
}

fn record_delta<T: serde::Serialize>(
    changed: &mut Vec<&'static str>,
    deltas: &mut Map<String, Value>,
    field: &'static str,
    from: &T,
    to: &T,
) {
    changed.push(field);
    deltas.insert(field.into(), json!({ "from": from, "to": to }));
}

fn append_separator(existing: &str) -> &'static str {
    if existing.is_empty() || existing.ends_with("\n\n") {
        ""
    } else if existing.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    }
}

fn apply_body_update(
    task: &mut TaskRecord,
    params: &TasksUpdateParams,
    changed: &mut Vec<&'static str>,
    deltas: &mut Map<String, Value>,
) -> Result<(), Refusal> {
    if params.body_markdown.is_some() && params.append_body.is_some() {
        return Err(refuse(
            ErrorCode::InvalidRequest,
            "cannot combine `body_markdown` and `append_body` in one update",
        ));
    }
    let before_chars = task.body_markdown.chars().count();
    let mode = if let Some(ref body) = params.body_markdown {
        task.body_markdown = validate_body(body)?;
        "replaced"
    } else if let Some(ref fragment) = params.append_body {
        if fragment.trim().is_empty() {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                "append_body cannot be empty",
            ));
        }
        let sep = append_separator(&task.body_markdown);
        task.body_markdown = validate_body(&format!("{}{sep}{fragment}", task.body_markdown))?;
        "appended"
    } else {
        return Ok(());
    };
    changed.push("body_markdown");
    deltas.insert(
        "body_markdown".into(),
        json!({ "mode": mode, "before_chars": before_chars, "after_chars": task.body_markdown.chars().count() }),
    );
    Ok(())
}

fn apply_scalar_updates<R: TasksRead>(
    view: &R,
    task: &mut TaskRecord,
    params: &TasksUpdateParams,
    changed: &mut Vec<&'static str>,
    deltas: &mut Map<String, Value>,
) -> Result<(), Refusal> {
    if let Some(ref title) = params.title {
        let prev = std::mem::replace(&mut task.title, validate_title(title)?);
        record_delta(changed, deltas, "title", &prev, &task.title);
    }
    apply_body_update(task, params, changed, deltas)?;
    if let Some(ref kind) = params.task_type {
        let prev = std::mem::replace(&mut task.task_type, validate_task_type(kind)?);
        record_delta(changed, deltas, "type", &prev, &task.task_type);
    }
    if let PatchField::Set(wt) = params.work_type {
        let prev = std::mem::replace(&mut task.work_type, wt);
        record_delta(changed, deltas, "work_type", &prev, &task.work_type);
    }
    if let Some(prio) = params.priority {
        let prev = std::mem::replace(&mut task.priority, prio);
        record_delta(changed, deltas, "priority", &prev, &task.priority);
    }
    apply_patch_scalar_updates(view, task, params, changed, deltas)
}

fn resolve_task_project_ref(
    task_project: Option<&Id>,
    raw: Option<&str>,
    field: &'static str,
    kind: &'static str,
    exists: impl FnOnce(&Id, &Id) -> Result<bool, org_store::EngineError>,
) -> Result<Option<Id>, Refusal> {
    let Some(v) = raw else {
        return Ok(None);
    };
    let target = parse_uuid_v7(v, field)?;
    let Some(pid) = task_project else {
        return Err(refuse(
            ErrorCode::InvalidRequest,
            format!("{field} requires the task to belong to a project"),
        ));
    };
    if !exists(pid, &target).map_err(storage_refusal)? {
        return Err(refuse(
            ErrorCode::NotFound,
            format!("referenced {kind} `{target}` not found in project"),
        ));
    }
    Ok(Some(target))
}

fn apply_delivery_patch_fields<R: TasksRead>(
    view: &R,
    task: &mut TaskRecord,
    params: &TasksUpdateParams,
    changed: &mut Vec<&'static str>,
    deltas: &mut Map<String, Value>,
) -> Result<(), Refusal> {
    if let PatchField::Set(ref raw) = params.feature_id {
        let next = resolve_task_project_ref(
            task.project_id.as_ref(),
            raw.as_deref(),
            "feature_id",
            "feature",
            |pid, fid| view.feature_in_project(pid, fid),
        )?;
        let prev = std::mem::replace(&mut task.feature_id, next);
        let prev_s = prev.as_ref().map(Id::to_string);
        let next_s = task.feature_id.as_ref().map(Id::to_string);
        record_delta(changed, deltas, "feature_id", &prev_s, &next_s);
    }
    if let PatchField::Set(ref raw) = params.milestone_id {
        let next = resolve_task_project_ref(
            task.project_id.as_ref(),
            raw.as_deref(),
            "milestone_id",
            "milestone",
            |pid, mid| view.milestone_in_project(pid, mid),
        )?;
        let prev = std::mem::replace(&mut task.milestone_id, next);
        let prev_s = prev.as_ref().map(Id::to_string);
        let next_s = task.milestone_id.as_ref().map(Id::to_string);
        record_delta(changed, deltas, "milestone_id", &prev_s, &next_s);
    }
    Ok(())
}

fn apply_patch_scalar_updates<R: TasksRead>(
    view: &R,
    task: &mut TaskRecord,
    params: &TasksUpdateParams,
    changed: &mut Vec<&'static str>,
    deltas: &mut Map<String, Value>,
) -> Result<(), Refusal> {
    if let PatchField::Set(effort) = params.effort {
        if let Some(v) = effort {
            super::validate_i64_u64(v, "effort")?;
        }
        let prev = std::mem::replace(&mut task.effort, effort);
        record_delta(changed, deltas, "effort", &prev, &task.effort);
    }
    if let PatchField::Set(ref sev) = params.severity {
        let prev = std::mem::replace(&mut task.severity, sev.clone());
        record_delta(changed, deltas, "severity", &prev, &task.severity);
    }
    if let PatchField::Set(order) = params.rank_order {
        if let Some(v) = order {
            super::validate_i64_u64(v, "rank_order")?;
        }
        let prev = std::mem::replace(&mut task.rank_order, order);
        record_delta(changed, deltas, "rank_order", &prev, &task.rank_order);
    }
    apply_delivery_patch_fields(view, task, params, changed, deltas)?;
    if let Some(ref details) = params.details {
        let mut next = parse_details(view, &task.id, task.project_id.as_ref(), details)?;
        // Only a transition sets or clears the "done anyway" record (`TASK-88`).
        next.done_anyway = task.details.as_ref().and_then(|d| d.done_anyway.clone());
        task.details = Some(next);
        changed.push("details");
        deltas.insert("details".into(), json!({ "updated": true }));
    }
    Ok(())
}

fn diff_set(before: &[String], after: &[String]) -> Value {
    let old: BTreeSet<&str> = before.iter().map(String::as_str).collect();
    let new: BTreeSet<&str> = after.iter().map(String::as_str).collect();
    let added: Vec<&str> = new.difference(&old).copied().collect();
    let removed: Vec<&str> = old.difference(&new).copied().collect();
    json!({ "added": added, "removed": removed })
}

struct SetPatch<'a> {
    replace: Option<&'a Vec<String>>,
    add: &'a [String],
    remove: &'a [String],
    field_name: &'static str,
}

fn apply_string_set_update<F>(
    target: &mut Vec<String>,
    patch: SetPatch<'_>,
    normalize: F,
    changed: &mut Vec<&'static str>,
    deltas: &mut Map<String, Value>,
) -> Result<(), Refusal>
where
    F: Fn(&[String]) -> Result<Vec<String>, org_proto::ApiError>,
{
    let norm = |items: &[String]| normalize(items).map_err(|e| refuse(e.code, e.message));
    if let Some(rep) = patch.replace {
        if !patch.add.is_empty() || !patch.remove.is_empty() {
            return Err(refuse(
                ErrorCode::InvalidRequest,
                format!(
                    "cannot combine `{}` replacement with incremental add/remove",
                    patch.field_name
                ),
            ));
        }
        let prev = std::mem::replace(target, norm(rep)?);
        changed.push(patch.field_name);
        deltas.insert(patch.field_name.into(), diff_set(&prev, target));
    } else if !patch.add.is_empty() || !patch.remove.is_empty() {
        let drop_set: BTreeSet<String> = norm(patch.remove)?.into_iter().collect();
        let mut next: Vec<String> = target
            .iter()
            .filter(|v| !drop_set.contains(*v))
            .cloned()
            .collect();
        next.extend(patch.add.iter().cloned());
        let prev = std::mem::replace(target, norm(&next)?);
        changed.push(patch.field_name);
        deltas.insert(patch.field_name.into(), diff_set(&prev, target));
    }
    Ok(())
}

fn apply_collection_updates(
    task: &mut TaskRecord,
    params: &TasksUpdateParams,
    changed: &mut Vec<&'static str>,
    deltas: &mut Map<String, Value>,
) -> Result<(), Refusal> {
    apply_string_set_update(
        &mut task.labels,
        SetPatch {
            replace: params.labels.as_ref(),
            add: &params.add_labels,
            remove: &params.remove_labels,
            field_name: "labels",
        },
        normalize_labels,
        changed,
        deltas,
    )?;
    apply_string_set_update(
        &mut task.reference_files,
        SetPatch {
            replace: params.reference_files.as_ref(),
            add: &params.add_reference_files,
            remove: &params.remove_reference_files,
            field_name: "reference_files",
        },
        |v| normalize_file_paths(v, "reference"),
        changed,
        deltas,
    )?;
    apply_string_set_update(
        &mut task.modified_files,
        SetPatch {
            replace: params.modified_files.as_ref(),
            add: &params.add_modified_files,
            remove: &params.remove_modified_files,
            field_name: "modified_files",
        },
        |v| normalize_file_paths(v, "modified"),
        changed,
        deltas,
    )
}

/// Atomically updates `assigned_team_id` and `assignee_id` (`doc-77`, `doc-84`, `AVAIL-2`).
pub struct AssignTask {
    pub actor_member_id: String,
    pub params: TasksAssignParams,
    /// Single-office task write authority verified atomically inside the `UnitOfWork` (`TASK-218`).
    pub authority: TaskWriteAuthority,
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
        require_local_task_write_authority(uow.view(), &self.authority, &task)?;
        if task.meta.revision != self.params.expected_revision {
            return Err(stale_revision_refusal(
                uow.view(),
                &task,
                self.params.expected_revision,
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
