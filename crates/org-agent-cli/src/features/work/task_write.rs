//! The `task` writes: `task create|update|transition|assign|comment|watch|unwatch|dep-add|
//! dep-remove`.

use org_command::{Answer, Refusal, Request};
use org_proto::attachments::{TASK_ATTACHMENTS_MAX, TASK_COMMENT_ATTACHMENTS_MAX};
use org_proto::tasks::{
    PatchField, TaskPriority, TaskStatus, TasksAddDependency, TasksAddDependencyParams,
    TasksAssign, TasksAssignParams, TasksComment, TasksCommentParams, TasksCommentResult,
    TasksCreate, TasksCreateParams, TasksMutationResult, TasksRemoveDependency,
    TasksRemoveDependencyParams, TasksTransition, TasksTransitionParams, TasksUnwatch,
    TasksUnwatchParams, TasksUpdate, TasksUpdateParams, TasksWatch, TasksWatchParams, WorkType,
};
use org_proto::{Call, ErrorCode};
use serde_json::json;

use super::{cmd_id, task_to_row, unreadable};
use crate::runtime::domain::CommandCue;
use crate::runtime::file_transfer::resolve_attach_ids;
use crate::runtime::input::{
    entity_id, is_set, opt_str_list, opt_string, opt_u64, require_if_match, require_str,
    write_answer,
};
use crate::runtime::transport::OfficeTransport;

/// `Use when:` guidance of each `task` command here.
pub(super) const CUES: &[CommandCue] = &[
    ("task", "assign", "assigning or clearing a task's responsible team or employee (requires `--if-match`)."),
    ("task", "comment", "posting a public update, review note, or handoff visible to everyone on the task (use `task-note` for private notes)."),
    ("task", "create", "creating a new task, bug, or request with initial priority, labels, files, or dependencies."),
    ("task", "dep-add", "recording that a task depends on an upstream prerequisite task."),
    ("task", "dep-remove", "removing a direct dependency link between two tasks."),
    ("task", "transition", "moving a task's lifecycle status (`todo`, `in_progress`, `blocked`, `done`, `cancelled`; requires `--if-match`)."),
    ("task", "unwatch", "unsubscribing from updates on a task you no longer need to watch."),
    ("task", "update", "editing a task's title, body (or `--append-body`), work type, priority, effort, severity, order, labels, or files (requires `--if-match`)."),
    ("task", "watch", "subscribing to updates on a task you care about."),
];

pub(super) fn write_task_create<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let command_id = cmd_id(request)?;
    let i = &request.input;
    let priority = opt_string(i, "priority")
        .map(|s| {
            TaskPriority::parse(&s).ok_or_else(|| {
                Refusal::new(
                    ErrorCode::InvalidRequest,
                    format!("invalid priority `{s}`"),
                    "Use urgent, high, normal, or low.",
                )
            })
        })
        .transpose()?;
    let work_type = opt_string(i, "work_type")
        .map(|s| {
            WorkType::parse(&s).ok_or_else(|| {
                Refusal::new(
                    ErrorCode::InvalidRequest,
                    format!("invalid work_type `{s}`"),
                    "Use build or run.",
                )
            })
        })
        .transpose()?;
    let title = require_str(i, "title")?.to_owned();
    let task_id = entity_id(request, None, "task-create")?;
    let effort = opt_u64(i, "effort")?;
    let rank_order = opt_u64(i, "order")?;
    let attach = opt_str_list(i, "attach").unwrap_or_default();
    let att_ids = opt_str_list(i, "attachment_id").unwrap_or_default();
    let resolved = resolve_attach_ids(transport, &attach, &att_ids, TASK_ATTACHMENTS_MAX)?;
    let params = TasksCreateParams {
        command_id,
        task_id,
        team_id: opt_string(i, "team_id"),
        project_id: opt_string(i, "project_id"),
        assigned_team_id: opt_string(i, "assigned_team_id"),
        assignee_id: opt_string(i, "assignee_id"),
        task_type: opt_string(i, "type"),
        work_type,
        title,
        body_markdown: opt_string(i, "body").unwrap_or_default(),
        priority,
        effort,
        severity: opt_string(i, "severity"),
        rank_order,
        labels: opt_str_list(i, "labels").unwrap_or_default(),
        reference_files: opt_str_list(i, "reference_files").unwrap_or_default(),
        modified_files: opt_str_list(i, "modified_files").unwrap_or_default(),
        depends_on: opt_str_list(i, "depends_on").unwrap_or_default(),
        attachment_ids: resolved.ids.clone(),
        details: None,
        dod: None,
    };
    let raw = match transport.call_method(TasksCreate::METHOD, json!(params)) {
        Ok(raw) => raw,
        Err(err) => {
            resolved.abort_staged(transport);
            return Err(err);
        }
    };
    let res: TasksMutationResult = serde_json::from_value(raw).map_err(unreadable)?;
    Ok(write_answer(
        request,
        task_to_row(&res.task),
        Some(res.task.revision),
    ))
}

fn parse_priority(raw: &str) -> Result<TaskPriority, Refusal> {
    TaskPriority::parse(raw).ok_or_else(|| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            format!("invalid priority `{raw}`"),
            "Use urgent, high, normal, or low.",
        )
    })
}

fn parse_work_type(raw: &str) -> Result<WorkType, Refusal> {
    WorkType::parse(raw).ok_or_else(|| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            format!("invalid work_type `{raw}`"),
            "Use build or run.",
        )
    })
}

fn reject_pair(both: bool, a: &str, b: &str) -> Result<(), Refusal> {
    if both {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            format!("cannot pass both `{a}` and `{b}`"),
            format!("Pass either `{a}` or `{b}`, not both."),
        ));
    }
    Ok(())
}

fn patch_u64(
    i: &serde_json::Map<String, serde_json::Value>,
    set_key: &str,
    clear_key: &str,
    set_flag: &str,
    clear_flag: &str,
) -> Result<PatchField<u64>, Refusal> {
    let val = opt_u64(i, set_key)?;
    let clear = is_set(i, clear_key);
    reject_pair(val.is_some() && clear, set_flag, clear_flag)?;
    Ok(if clear {
        PatchField::Set(None)
    } else if let Some(n) = val {
        PatchField::Set(Some(n))
    } else {
        PatchField::Unchanged
    })
}

fn patch_opt_str(
    i: &serde_json::Map<String, serde_json::Value>,
    set_key: &str,
    clear_key: &str,
    set_flag: &str,
    clear_flag: &str,
) -> Result<PatchField<String>, Refusal> {
    let val = opt_string(i, set_key);
    let clear = is_set(i, clear_key);
    reject_pair(val.is_some() && clear, set_flag, clear_flag)?;
    Ok(if clear {
        PatchField::Set(None)
    } else if let Some(s) = val {
        PatchField::Set(Some(s))
    } else {
        PatchField::Unchanged
    })
}

pub(super) fn write_task_update<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let command_id = cmd_id(request)?;
    let expected_revision = require_if_match(request)?;
    let i = &request.input;
    let task = require_str(i, "task")?.to_owned();
    let body_markdown = opt_string(i, "body");
    let append_body = opt_string(i, "append_body");
    reject_pair(
        body_markdown.is_some() && append_body.is_some(),
        "--body",
        "--append-body",
    )?;
    let priority = opt_string(i, "priority")
        .map(|s| parse_priority(&s))
        .transpose()?;
    let work_type_val = opt_string(i, "work_type")
        .map(|s| parse_work_type(&s))
        .transpose()?;
    let clear_work_type = is_set(i, "clear_work_type");
    reject_pair(
        work_type_val.is_some() && clear_work_type,
        "--work-type",
        "--clear-work-type",
    )?;
    let work_type = if clear_work_type {
        PatchField::Set(None)
    } else if let Some(wt) = work_type_val {
        PatchField::Set(Some(wt))
    } else {
        PatchField::Unchanged
    };
    let rank_order = patch_u64(i, "order", "clear_order", "--order", "--clear-order")?;
    let effort = patch_u64(i, "effort", "clear_effort", "--effort", "--clear-effort")?;
    let severity = patch_opt_str(
        i,
        "severity",
        "clear_severity",
        "--severity",
        "--clear-severity",
    )?;
    let attach = opt_str_list(i, "attach").unwrap_or_default();
    let add_att = opt_str_list(i, "add_attachment").unwrap_or_default();
    let resolved = resolve_attach_ids(transport, &attach, &add_att, TASK_ATTACHMENTS_MAX)?;
    let params = TasksUpdateParams {
        command_id,
        task,
        expected_revision,
        title: opt_string(i, "title"),
        body_markdown,
        append_body,
        task_type: opt_string(i, "type"),
        work_type,
        priority,
        effort,
        severity,
        rank_order,
        labels: opt_str_list(i, "labels"),
        add_labels: opt_str_list(i, "add_labels").unwrap_or_default(),
        remove_labels: opt_str_list(i, "remove_labels").unwrap_or_default(),
        reference_files: opt_str_list(i, "reference_files"),
        add_reference_files: opt_str_list(i, "add_reference_files").unwrap_or_default(),
        remove_reference_files: opt_str_list(i, "remove_reference_files").unwrap_or_default(),
        modified_files: opt_str_list(i, "modified_files"),
        add_modified_files: opt_str_list(i, "add_modified_files").unwrap_or_default(),
        remove_modified_files: opt_str_list(i, "remove_modified_files").unwrap_or_default(),
        add_attachments: resolved.ids.clone(),
        remove_attachments: opt_str_list(i, "remove_attachment").unwrap_or_default(),
        feature_id: PatchField::Unchanged,
        milestone_id: PatchField::Unchanged,
        details: None,
    };
    let raw = match transport.call_method(TasksUpdate::METHOD, json!(params)) {
        Ok(raw) => raw,
        Err(err) => {
            resolved.abort_staged(transport);
            return Err(err);
        }
    };
    let res: TasksMutationResult = serde_json::from_value(raw).map_err(unreadable)?;
    Ok(write_answer(
        request,
        task_to_row(&res.task),
        Some(res.task.revision),
    ))
}

pub(super) fn write_task_transition<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let i = &request.input;
    let status_str = require_str(i, "status")?;
    let status = TaskStatus::parse(status_str).ok_or_else(|| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            format!("invalid status `{status_str}`"),
            "Use todo, in_progress, blocked, cancelled, or done.",
        )
    })?;
    let params = TasksTransitionParams {
        command_id: cmd_id(request)?,
        task: require_str(i, "task")?.to_owned(),
        expected_revision: require_if_match(request)?,
        status,
        blocked_reason: opt_string(i, "blocked_reason"),
        resolution: opt_string(i, "resolution"),
        note: opt_string(i, "note"),
        done_anyway_reason: None,
        final_summary: None,
    };
    let raw = transport.call_method(TasksTransition::METHOD, json!(params))?;
    let res: TasksMutationResult = serde_json::from_value(raw).map_err(unreadable)?;
    Ok(write_answer(
        request,
        task_to_row(&res.task),
        Some(res.task.revision),
    ))
}

pub(super) fn write_task_assign<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let i = &request.input;
    let assigned_team_id = if is_set(i, "clear_assigned_team") {
        PatchField::Set(None)
    } else if let Some(tid) = opt_string(i, "assigned_team_id") {
        PatchField::Set(Some(tid))
    } else {
        PatchField::Unchanged
    };
    let assignee_id = if is_set(i, "clear_assignee") {
        PatchField::Set(None)
    } else if let Some(eid) = opt_string(i, "assignee_id") {
        PatchField::Set(Some(eid))
    } else {
        PatchField::Unchanged
    };
    let params = TasksAssignParams {
        command_id: cmd_id(request)?,
        task: require_str(i, "task")?.to_owned(),
        expected_revision: require_if_match(request)?,
        assigned_team_id,
        assignee_id,
    };
    let raw = transport.call_method(TasksAssign::METHOD, json!(params))?;
    let res: TasksMutationResult = serde_json::from_value(raw).map_err(unreadable)?;
    Ok(write_answer(
        request,
        task_to_row(&res.task),
        Some(res.task.revision),
    ))
}

pub(super) fn write_task_comment<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let command_id = cmd_id(request)?;
    let i = &request.input;
    let task = require_str(i, "task")?.to_owned();
    let body_markdown = opt_string(i, "body").unwrap_or_default();
    let attach = opt_str_list(i, "attach").unwrap_or_default();
    let att_ids = opt_str_list(i, "attachment_id").unwrap_or_default();
    if body_markdown.trim().is_empty() && attach.is_empty() && att_ids.is_empty() {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "`task comment` requires `--body` or at least one `--attach` / `--attachment-id`.",
            "Pass `--body <text>` (or `--input-file <path>`) or attach a file with `--attach <path>`.",
        ));
    }
    let comment_id = entity_id(request, None, "task-comment")?;
    let resolved = resolve_attach_ids(transport, &attach, &att_ids, TASK_COMMENT_ATTACHMENTS_MAX)?;
    let params = TasksCommentParams {
        command_id,
        task,
        comment_id,
        body_markdown,
        attachment_ids: resolved.ids.clone(),
    };
    let raw = match transport.call_method(TasksComment::METHOD, json!(params)) {
        Ok(raw) => raw,
        Err(err) => {
            resolved.abort_staged(transport);
            return Err(err);
        }
    };
    let res: TasksCommentResult = serde_json::from_value(raw).map_err(unreadable)?;
    Ok(write_answer(
        request,
        json!({
            "task_id": res.task.id,
            "key": res.task.key,
            "revision": res.task.revision,
            "comment": res.comment,
        }),
        Some(res.task.revision),
    ))
}

pub(super) fn write_task_watch<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    watch: bool,
) -> Result<Answer, Refusal> {
    let raw = if watch {
        let params = TasksWatchParams {
            command_id: cmd_id(request)?,
            task: require_str(&request.input, "task")?.to_owned(),
        };
        transport.call_method(TasksWatch::METHOD, json!(params))?
    } else {
        let params = TasksUnwatchParams {
            command_id: cmd_id(request)?,
            task: require_str(&request.input, "task")?.to_owned(),
        };
        transport.call_method(TasksUnwatch::METHOD, json!(params))?
    };
    let res: TasksMutationResult = serde_json::from_value(raw).map_err(unreadable)?;
    Ok(write_answer(
        request,
        task_to_row(&res.task),
        Some(res.task.revision),
    ))
}

pub(super) fn write_task_dep<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    add: bool,
) -> Result<Answer, Refusal> {
    let raw = if add {
        let params = TasksAddDependencyParams {
            command_id: cmd_id(request)?,
            task: require_str(&request.input, "task")?.to_owned(),
            depends_on_task: require_str(&request.input, "depends_on")?.to_owned(),
        };
        transport.call_method(TasksAddDependency::METHOD, json!(params))?
    } else {
        let params = TasksRemoveDependencyParams {
            command_id: cmd_id(request)?,
            task: require_str(&request.input, "task")?.to_owned(),
            depends_on_task: require_str(&request.input, "depends_on")?.to_owned(),
        };
        transport.call_method(TasksRemoveDependency::METHOD, json!(params))?
    };
    let res: TasksMutationResult = serde_json::from_value(raw).map_err(unreadable)?;
    Ok(write_answer(
        request,
        task_to_row(&res.task),
        Some(res.task.revision),
    ))
}
