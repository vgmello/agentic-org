//! `task note` commands: private notes on a task and their publication.

use org_command::{Answer, Refusal, Request};
use org_proto::private_notes::{
    TaskNotePublishResult, TaskNoteRemovedResult, TaskNoteResult, TasksNotesAdd,
    TasksNotesAddParams, TasksNotesGet, TasksNotesGetParams, TasksNotesList, TasksNotesListParams,
    TasksNotesListResult, TasksNotesPublish, TasksNotesPublishParams, TasksNotesRemove,
    TasksNotesRemoveParams, TasksNotesUpdate, TasksNotesUpdateParams,
};
use org_proto::Call;
use serde_json::{json, Value};

use super::{cmd_id, unreadable};
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{entity_id, opt_string, require_if_match, require_str, write_answer};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport};

/// `Use when:` guidance of each `task-note` command.
pub(super) const CUES: &[CommandCue] = &[
    ("task-note", "add", "recording a private working note on a task that is not visible to other agents until published."),
    ("task-note", "get", "reading one private task note and its current revision by `--id`."),
    ("task-note", "list", "listing your private working notes on a task."),
    ("task-note", "publish", "copying a private task note into a public task comment while keeping the private note intact (requires `--if-match`)."),
    ("task-note", "remove", "deleting a private task note you no longer need (requires `--if-match`)."),
    ("task-note", "update", "editing an existing private task note's text (requires `--if-match`)."),
];

pub(super) fn try_task_note<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    match request.command.verb {
        "add" => Some(write_note_add(transport, request)),
        "list" => Some(read_note_list(transport, request)),
        "get" => Some(read_note_get(transport, request)),
        "update" => Some(write_note_update(transport, request)),
        "remove" => Some(write_note_remove(transport, request)),
        "publish" => Some(write_note_publish(transport, request)),
        _ => None,
    }
}

pub(super) fn write_note_add<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = TasksNotesAddParams {
        command_id: cmd_id(request)?,
        task: require_str(&request.input, "task")?.to_owned(),
        note_id: entity_id(request, None, "task-note-add")?,
        body_markdown: require_str(&request.input, "text")?.to_owned(),
    };
    let raw = transport.call_method(TasksNotesAdd::METHOD, json!(params))?;
    let res: TaskNoteResult = serde_json::from_value(raw).map_err(unreadable)?;
    Ok(write_answer(
        request,
        json!(res.note),
        Some(res.note.revision),
    ))
}

pub(super) fn read_note_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    parse_cursor(request.cursor.as_deref())?;
    let params = TasksNotesListParams {
        task: require_str(&request.input, "task")?.to_owned(),
        author_id: opt_string(&request.input, "author_id"),
    };
    let raw = transport.call_method(TasksNotesList::METHOD, json!(params))?;
    let res: TasksNotesListResult = serde_json::from_value(raw).map_err(unreadable)?;
    let items: Vec<Value> = res.notes.into_iter().map(|n| json!(n)).collect();
    paginate(request, items)
}

pub(super) fn read_note_get<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = TasksNotesGetParams {
        note_id: require_str(&request.input, "id")?.to_owned(),
    };
    let raw = transport.call_method(TasksNotesGet::METHOD, json!(params))?;
    let res: TaskNoteResult = serde_json::from_value(raw).map_err(unreadable)?;
    Ok(Answer::completed(json!(res.note)))
}

pub(super) fn write_note_update<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = TasksNotesUpdateParams {
        command_id: cmd_id(request)?,
        note_id: require_str(&request.input, "id")?.to_owned(),
        expected_revision: require_if_match(request)?,
        body_markdown: require_str(&request.input, "text")?.to_owned(),
    };
    let raw = transport.call_method(TasksNotesUpdate::METHOD, json!(params))?;
    let res: TaskNoteResult = serde_json::from_value(raw).map_err(unreadable)?;
    Ok(write_answer(
        request,
        json!(res.note),
        Some(res.note.revision),
    ))
}

pub(super) fn write_note_remove<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = TasksNotesRemoveParams {
        command_id: cmd_id(request)?,
        note_id: require_str(&request.input, "id")?.to_owned(),
        expected_revision: require_if_match(request)?,
    };
    let raw = transport.call_method(TasksNotesRemove::METHOD, json!(params))?;
    let res: TaskNoteRemovedResult = serde_json::from_value(raw).map_err(unreadable)?;
    Ok(write_answer(request, json!(res), Some(res.revision)))
}

pub(super) fn write_note_publish<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = TasksNotesPublishParams {
        command_id: cmd_id(request)?,
        note_id: require_str(&request.input, "id")?.to_owned(),
        expected_revision: require_if_match(request)?,
        comment_id: entity_id(request, None, "task-note-publish-comment")?,
    };
    let raw = transport.call_method(TasksNotesPublish::METHOD, json!(params))?;
    let res: TaskNotePublishResult = serde_json::from_value(raw).map_err(unreadable)?;
    Ok(write_answer(
        request,
        json!({
            "note": res.note,
            "comment": res.comment,
            "task_id": res.task.id,
            "key": res.task.key,
        }),
        Some(res.task.revision),
    ))
}
