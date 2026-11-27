//! The `task` writes: `task create|update|transition|assign|comment|watch|unwatch|dep-add|
//! dep-remove`.

use org_command::{Ack, Answer, Command, Example, IfMatch, Kind, Refusal, Request};
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

use super::schema::{
    TASK_ASSIGN_INPUT, TASK_COMMENT_INPUT, TASK_COMMENT_OUTPUT, TASK_CREATE_INPUT,
    TASK_DEP_MUTATION_INPUT, TASK_EXAMPLE_JSON, TASK_FIELDS, TASK_REF_INPUT, TASK_TRANSITION_INPUT,
    TASK_UPDATE_INPUT, WRITE_ERRORS,
};
use super::{cmd_id, task_to_row, unreadable};
use crate::runtime::domain::CommandCue;
use crate::runtime::file_transfer::resolve_attach_ids;
use crate::runtime::input::{
    entity_id, is_set, opt_str_list, opt_string, opt_u64, require_if_match, require_str,
    write_answer,
};
use crate::runtime::transport::OfficeTransport;

/// The `task` write rows.
pub(super) const COMMANDS: &[Command] = &[
    Command {
        resource: "task",
        verb: "create",
        summary: "Create a task, bug, or request",
        call: "tasks.create",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: TASK_CREATE_INPUT,
        content_field: Some("body"),
        output: TASK_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"title":"Implement auth","team_id":"0193a5c0-0000-7000-8000-000000000010","order":10,"labels":["backend","security"]}"#,
            output: TASK_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "task",
        verb: "update",
        summary: "Update task fields, labels, files, or order",
        call: "tasks.update",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: TASK_UPDATE_INPUT,
        content_field: Some("body"),
        output: TASK_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","order":1,"add_labels":["p0"]}"#,
            output: TASK_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "task",
        verb: "transition",
        summary: "Change task status",
        call: "tasks.transition",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: TASK_TRANSITION_INPUT,
        content_field: None,
        output: TASK_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","status":"in_progress"}"#,
            output: TASK_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "task",
        verb: "assign",
        summary: "Assign task team or employee",
        call: "tasks.assign",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: TASK_ASSIGN_INPUT,
        content_field: None,
        output: TASK_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","assignee_id":"0193a5c0-0000-7000-8000-000000000020"}"#,
            output: TASK_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "task",
        verb: "comment",
        summary: "Add a public task comment",
        call: "tasks.comment",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: TASK_COMMENT_INPUT,
        content_field: Some("body"),
        output: TASK_COMMENT_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","body":"Ready for review."}"#,
            output: r#"{"task_id":"0193a5c0-0000-7000-8000-000000000100","key":"T-A1","revision":2,"comment":{"id":"0193a5c0-0000-7000-8000-000000000300","position":0,"author_id":"0193a5c0-0000-7000-8000-000000000020","created_at":"2026-11-22T00:00:00.000Z","body_markdown":"Ready for review."}}"#,
        },
    },
    Command {
        resource: "task",
        verb: "watch",
        summary: "Watch a task",
        call: "tasks.watch",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: TASK_REF_INPUT,
        content_field: None,
        output: TASK_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1"}"#,
            output: TASK_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "task",
        verb: "unwatch",
        summary: "Stop watching a task",
        call: "tasks.unwatch",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: TASK_REF_INPUT,
        content_field: None,
        output: TASK_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1"}"#,
            output: TASK_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "task",
        verb: "dep-add",
        summary: "Add task dependency",
        call: "tasks.add_dependency",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: TASK_DEP_MUTATION_INPUT,
        content_field: None,
        output: TASK_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A2","depends_on":"T-A1"}"#,
            output: TASK_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "task",
        verb: "dep-remove",
        summary: "Remove task dependency",
        call: "tasks.remove_dependency",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: TASK_DEP_MUTATION_INPUT,
        content_field: None,
        output: TASK_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A2","depends_on":"T-A1"}"#,
            output: TASK_EXAMPLE_JSON,
        },
    },
];

/// `Use when:` guidance of each `task` command here.
pub(super) const CUES: &[CommandCue] = &[
    ("task", "assign", "assigning or clearing a task's responsible team or employee (requires `--if-match`)."),
    ("task", "comment", "posting a public update, review note, or handoff visible to everyone on the task (use `task-note` for private notes)."),
    ("task", "create", "creating a new task, bug, or request with initial priority, labels, files, or dependencies."),
    ("task", "dep-add", "recording that a task depends on an upstream prerequisite task."),
    ("task", "dep-remove", "removing a direct dependency link between two tasks."),
    ("task", "transition", "moving a task's lifecycle status (`todo`, `in_progress`, `blocked`, `done`, `cancelled`; requires `--if-match`)."),
    ("task", "unwatch", "unsubscribing from updates on a task you no longer need to watch."),
    ("task", "update", "editing a task's title, body, priority, order, labels, or reference/modified files (requires `--if-match`)."),
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
    let attach = opt_str_list(i, "attach").unwrap_or_default();
    let att_ids = opt_str_list(i, "attachment_id").unwrap_or_default();
    let attachment_ids = resolve_attach_ids(transport, &attach, &att_ids, TASK_ATTACHMENTS_MAX)?;
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
        effort: opt_u64(i, "effort")?,
        severity: opt_string(i, "severity"),
        rank_order: opt_u64(i, "order")?,
        labels: opt_str_list(i, "labels").unwrap_or_default(),
        reference_files: opt_str_list(i, "reference_files").unwrap_or_default(),
        modified_files: opt_str_list(i, "modified_files").unwrap_or_default(),
        depends_on: opt_str_list(i, "depends_on").unwrap_or_default(),
        attachment_ids,
        details: None,
        dod: None,
    };
    let raw = transport.call_method(TasksCreate::METHOD, json!(params))?;
    let res: TasksMutationResult = serde_json::from_value(raw).map_err(unreadable)?;
    Ok(write_answer(
        request,
        task_to_row(&res.task),
        Some(res.task.revision),
    ))
}

pub(super) fn write_task_update<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let command_id = cmd_id(request)?;
    let expected_revision = require_if_match(request)?;
    let i = &request.input;
    let task = require_str(i, "task")?.to_owned();
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
    let rank_order = if is_set(i, "clear_order") {
        PatchField::Set(None)
    } else if let Some(n) = opt_u64(i, "order")? {
        PatchField::Set(Some(n))
    } else {
        PatchField::Unchanged
    };
    let effort = match opt_u64(i, "effort")? {
        Some(n) => PatchField::Set(Some(n)),
        None => PatchField::Unchanged,
    };
    let severity = match opt_string(i, "severity") {
        Some(s) => PatchField::Set(Some(s)),
        None => PatchField::Unchanged,
    };
    let attach = opt_str_list(i, "attach").unwrap_or_default();
    let add_att = opt_str_list(i, "add_attachment").unwrap_or_default();
    let add_attachments = resolve_attach_ids(transport, &attach, &add_att, TASK_ATTACHMENTS_MAX)?;
    let params = TasksUpdateParams {
        command_id,
        task,
        expected_revision,
        title: opt_string(i, "title"),
        body_markdown: opt_string(i, "body"),
        task_type: opt_string(i, "type"),
        work_type: PatchField::Unchanged,
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
        add_attachments,
        remove_attachments: opt_str_list(i, "remove_attachment").unwrap_or_default(),
        details: None,
    };
    let raw = transport.call_method(TasksUpdate::METHOD, json!(params))?;
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
    let attachment_ids =
        resolve_attach_ids(transport, &attach, &att_ids, TASK_COMMENT_ATTACHMENTS_MAX)?;
    let params = TasksCommentParams {
        command_id,
        task,
        comment_id,
        body_markdown,
        attachment_ids,
    };
    let raw = transport.call_method(TasksComment::METHOD, json!(params))?;
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
