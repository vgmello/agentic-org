//! `checklist` commands: adding, checking, editing, removing and moving steps.

use org_command::{Answer, Refusal, Request};
use org_proto::tasks::{
    TasksChecklistAdd, TasksChecklistAddParams, TasksChecklistCheck, TasksChecklistCheckParams,
    TasksChecklistEdit, TasksChecklistEditParams, TasksChecklistMove, TasksChecklistMoveParams,
    TasksChecklistRemove, TasksChecklistRemoveParams, TasksChecklistResult,
};
use org_proto::{Call, ErrorCode};
use serde_json::{json, Value};

use super::{cmd_id, decode_err, entity_id, opt_bool, opt_str, opt_u64, req_str, write_answer};
use crate::transport::OfficeTransport;

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
    let res: TasksChecklistResult = serde_json::from_value(raw).map_err(decode_err)?;
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
        task: req_str(&request.input, "task")?.to_owned(),
        step_id: entity_id(request, "checklist-step")?,
        text: req_str(&request.input, "text")?.to_owned(),
        required: Some(opt_bool(&request.input, "required")),
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
    let undone = opt_bool(&request.input, "undone");
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
        task: req_str(&request.input, "task")?.to_owned(),
        step_id: req_str(&request.input, "step_id")?.to_owned(),
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
        task: req_str(&request.input, "task")?.to_owned(),
        step_id: req_str(&request.input, "step_id")?.to_owned(),
        text: opt_str(&request.input, "text"),
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
        task: req_str(&request.input, "task")?.to_owned(),
        step_id: req_str(&request.input, "step_id")?.to_owned(),
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
        task: req_str(&request.input, "task")?.to_owned(),
        step_id: req_str(&request.input, "step_id")?.to_owned(),
        position,
        section: None,
    };
    checklist_answer(
        request,
        transport.call_method(TasksChecklistMove::METHOD, json!(params))?,
    )
}
