//! `checklist` commands: adding, checking, editing, removing and moving steps.

use org_command::{Ack, Answer, Command, Example, Field, IfMatch, Kind, Refusal, Request, Type};
use org_proto::tasks::{
    TasksChecklistAdd, TasksChecklistAddParams, TasksChecklistCheck, TasksChecklistCheckParams,
    TasksChecklistEdit, TasksChecklistEditParams, TasksChecklistMove, TasksChecklistMoveParams,
    TasksChecklistRemove, TasksChecklistRemoveParams, TasksChecklistResult,
};
use org_proto::{Call, ErrorCode};
use serde_json::{json, Value};

use super::schema::{CHECKLIST_STEP_TYPE, WRITE_ERRORS};
use super::{cmd_id, unreadable};
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{entity_id, is_set, opt_string, opt_u64, require_str, write_answer};
use crate::runtime::transport::OfficeTransport;

const CHECKLIST_ADD_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::required("text", Type::String, "Checklist step text"),
    Field::optional("required", Type::Boolean, "Mark step as required"),
];

const CHECKLIST_CHECK_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::required("step_id", Type::String, "Checklist step UUIDv7"),
    Field::optional("done", Type::Boolean, "Mark step done (default)"),
    Field::optional("undone", Type::Boolean, "Mark step not done"),
];

const CHECKLIST_EDIT_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::required("step_id", Type::String, "Checklist step UUIDv7"),
    Field::optional("text", Type::String, "Updated step text"),
    Field::optional("required", Type::Boolean, "Updated required flag"),
];

const CHECKLIST_REMOVE_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::required("step_id", Type::String, "Checklist step UUIDv7"),
];

const CHECKLIST_MOVE_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::required("step_id", Type::String, "Checklist step UUIDv7"),
    Field::required("position", Type::Integer, "Zero-based target position"),
];

const CHECKLIST_OUTPUT: &[Field] = &[
    Field::required("task_id", Type::String, "Task UUIDv7"),
    Field::required("key", Type::String, "Task key"),
    Field::required("revision", Type::Integer, "New task revision"),
    Field::required(
        "checklist",
        Type::List(&CHECKLIST_STEP_TYPE),
        "Updated checklist steps",
    ),
];

const CHECKLIST_EXAMPLE_JSON: &str = r#"{"task_id":"0193a5c0-0000-7000-8000-000000000100","key":"T-A1","revision":2,"checklist":[{"id":"0193a5c0-0000-7000-8000-000000000200","position":0,"text":"Add tests","required":true,"done":false,"revision":1}]}"#;

/// The `checklist` rows.
pub(super) const COMMANDS: &[Command] = &[
    Command {
        resource: "checklist",
        verb: "add",
        summary: "Add checklist step",
        call: "tasks.checklist.add",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: CHECKLIST_ADD_INPUT,
        content_field: None,
        output: CHECKLIST_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","text":"Add tests","required":true}"#,
            output: CHECKLIST_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "checklist",
        verb: "check",
        summary: "Check or uncheck step",
        call: "tasks.checklist.check",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: CHECKLIST_CHECK_INPUT,
        content_field: None,
        output: CHECKLIST_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","step_id":"0193a5c0-0000-7000-8000-000000000200","done":true}"#,
            output: CHECKLIST_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "checklist",
        verb: "edit",
        summary: "Edit checklist step",
        call: "tasks.checklist.edit",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: CHECKLIST_EDIT_INPUT,
        content_field: None,
        output: CHECKLIST_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","step_id":"0193a5c0-0000-7000-8000-000000000200","text":"Add integration tests"}"#,
            output: CHECKLIST_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "checklist",
        verb: "remove",
        summary: "Remove checklist step",
        call: "tasks.checklist.remove",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: CHECKLIST_REMOVE_INPUT,
        content_field: None,
        output: CHECKLIST_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","step_id":"0193a5c0-0000-7000-8000-000000000200"}"#,
            output: CHECKLIST_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "checklist",
        verb: "move",
        summary: "Move checklist step",
        call: "tasks.checklist.move",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: CHECKLIST_MOVE_INPUT,
        content_field: None,
        output: CHECKLIST_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","step_id":"0193a5c0-0000-7000-8000-000000000200","position":0}"#,
            output: CHECKLIST_EXAMPLE_JSON,
        },
    },
];

/// `Use when:` guidance of each `checklist` command.
pub(super) const CUES: &[CommandCue] = &[
    (
        "checklist",
        "add",
        "appending a new acceptance or execution step to a task's checklist.",
    ),
    (
        "checklist",
        "check",
        "marking a checklist step done (or `--undone`) as you verify task progress.",
    ),
    (
        "checklist",
        "edit",
        "updating a checklist step's wording or `--required` flag.",
    ),
    (
        "checklist",
        "move",
        "reordering a checklist step to a new zero-based position.",
    ),
    (
        "checklist",
        "remove",
        "deleting an obsolete checklist step from a task.",
    ),
];

pub(super) fn try_checklist<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    match request.command.verb {
        "add" => Some(write_checklist_add(transport, request)),
        "check" => Some(write_checklist_check(transport, request)),
        "edit" => Some(write_checklist_edit(transport, request)),
        "remove" => Some(write_checklist_remove(transport, request)),
        "move" => Some(write_checklist_move(transport, request)),
        _ => None,
    }
}

pub(super) fn checklist_answer(request: &Request, raw: Value) -> Result<Answer, Refusal> {
    let res: TasksChecklistResult = serde_json::from_value(raw).map_err(unreadable)?;
    Ok(write_answer(
        request,
        json!({
            "task_id": res.task.id,
            "key": res.task.key,
            "revision": res.task.revision,
            "checklist": res.checklist,
        }),
        Some(res.task.revision),
    ))
}

pub(super) fn write_checklist_add<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = TasksChecklistAddParams {
        command_id: cmd_id(request)?,
        task: require_str(&request.input, "task")?.to_owned(),
        step_id: entity_id(request, None, "checklist-step")?,
        text: require_str(&request.input, "text")?.to_owned(),
        required: Some(is_set(&request.input, "required")),
        section: None,
    };
    checklist_answer(
        request,
        transport.call_method(TasksChecklistAdd::METHOD, json!(params))?,
    )
}

pub(super) fn write_checklist_check<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let done_flag = request.input.get("done").and_then(Value::as_bool);
    let undone = is_set(&request.input, "undone");
    if undone && done_flag == Some(true) {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "cannot pass both `--done` and `--undone`",
            "Use `--done` (default) to check or `--undone` to uncheck.",
        ));
    }
    let done = if undone {
        false
    } else {
        done_flag.unwrap_or(true)
    };
    let params = TasksChecklistCheckParams {
        command_id: cmd_id(request)?,
        task: require_str(&request.input, "task")?.to_owned(),
        step_id: require_str(&request.input, "step_id")?.to_owned(),
        done,
    };
    checklist_answer(
        request,
        transport.call_method(TasksChecklistCheck::METHOD, json!(params))?,
    )
}

pub(super) fn write_checklist_edit<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = TasksChecklistEditParams {
        command_id: cmd_id(request)?,
        task: require_str(&request.input, "task")?.to_owned(),
        step_id: require_str(&request.input, "step_id")?.to_owned(),
        text: opt_string(&request.input, "text"),
        required: request.input.get("required").and_then(Value::as_bool),
    };
    checklist_answer(
        request,
        transport.call_method(TasksChecklistEdit::METHOD, json!(params))?,
    )
}

pub(super) fn write_checklist_remove<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = TasksChecklistRemoveParams {
        command_id: cmd_id(request)?,
        task: require_str(&request.input, "task")?.to_owned(),
        step_id: require_str(&request.input, "step_id")?.to_owned(),
    };
    checklist_answer(
        request,
        transport.call_method(TasksChecklistRemove::METHOD, json!(params))?,
    )
}

pub(super) fn write_checklist_move<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let position = opt_u64(&request.input, "position")?.ok_or_else(|| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            "`position` is required",
            "Provide a zero-based position.",
        )
    })?;
    let params = TasksChecklistMoveParams {
        command_id: cmd_id(request)?,
        task: require_str(&request.input, "task")?.to_owned(),
        step_id: require_str(&request.input, "step_id")?.to_owned(),
        position,
        section: None,
    };
    checklist_answer(
        request,
        transport.call_method(TasksChecklistMove::METHOD, json!(params))?,
    )
}
