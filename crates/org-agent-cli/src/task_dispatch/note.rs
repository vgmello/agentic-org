//! `task note` commands: private notes on a task and their publication.

use super::*;

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
        task: req_str(&request.input, "task")?.to_owned(),
        note_id: entity_id(request, "task-note-add")?,
        body_markdown: req_str(&request.input, "text")?.to_owned(),
    };
    let raw = transport.call_method(TasksNotesAdd::METHOD, json!(params))?;
    let res: TaskNoteResult = serde_json::from_value(raw).map_err(decode_err)?;
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
    let params = TasksNotesListParams {
        task: req_str(&request.input, "task")?.to_owned(),
        author_id: opt_str(&request.input, "author_id"),
    };
    let raw = transport.call_method(TasksNotesList::METHOD, json!(params))?;
    let res: TasksNotesListResult = serde_json::from_value(raw).map_err(decode_err)?;
    let items: Vec<Value> = res.notes.into_iter().map(|n| json!(n)).collect();
    Ok(list_answer(items))
}

pub(super) fn read_note_get<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = TasksNotesGetParams {
        note_id: req_str(&request.input, "id")?.to_owned(),
    };
    let raw = transport.call_method(TasksNotesGet::METHOD, json!(params))?;
    let res: TaskNoteResult = serde_json::from_value(raw).map_err(decode_err)?;
    Ok(Answer::completed(json!(res.note)))
}

pub(super) fn write_note_update<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = TasksNotesUpdateParams {
        command_id: cmd_id(request)?,
        note_id: req_str(&request.input, "id")?.to_owned(),
        expected_revision: require_revision(request)?,
        body_markdown: req_str(&request.input, "text")?.to_owned(),
    };
    let raw = transport.call_method(TasksNotesUpdate::METHOD, json!(params))?;
    let res: TaskNoteResult = serde_json::from_value(raw).map_err(decode_err)?;
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
        note_id: req_str(&request.input, "id")?.to_owned(),
        expected_revision: require_revision(request)?,
    };
    let raw = transport.call_method(TasksNotesRemove::METHOD, json!(params))?;
    let res: TaskNoteRemovedResult = serde_json::from_value(raw).map_err(decode_err)?;
    Ok(write_answer(request, json!(res), Some(res.revision)))
}

pub(super) fn write_note_publish<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = TasksNotesPublishParams {
        command_id: cmd_id(request)?,
        note_id: req_str(&request.input, "id")?.to_owned(),
        expected_revision: require_revision(request)?,
        comment_id: entity_id(request, "task-note-publish-comment")?,
    };
    let raw = transport.call_method(TasksNotesPublish::METHOD, json!(params))?;
    let res: TaskNotePublishResult = serde_json::from_value(raw).map_err(decode_err)?;
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
