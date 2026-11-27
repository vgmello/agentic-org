//! `task note` commands: private notes on a task and their publication.

use org_command::{
    Ack, Alias, Answer, Command, Example, Field, IfMatch, Kind, Refusal, Request, Type,
};
use org_proto::private_notes::{
    TaskNotePublishResult, TaskNoteRemovedResult, TaskNoteResult, TasksNotesAdd,
    TasksNotesAddParams, TasksNotesGet, TasksNotesGetParams, TasksNotesList, TasksNotesListParams,
    TasksNotesListResult, TasksNotesPublish, TasksNotesPublishParams, TasksNotesRemove,
    TasksNotesRemoveParams, TasksNotesUpdate, TasksNotesUpdateParams,
};
use org_proto::Call;
use serde_json::{json, Value};

use super::schema::{COMMENT_TYPE, READ_ERRORS, WRITE_ERRORS};
use super::{cmd_id, unreadable};
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{entity_id, opt_string, require_if_match, require_str, write_answer};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport};

/// `task note <verb>` is `task-note <verb>`.
pub(super) const TASK_NOTE_ALIAS: Alias = Alias {
    words: &["task", "note"],
    command: &["task-note"],
};

const NOTE_OUTPUT_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Private note UUIDv7"),
    Field::required("task_id", Type::String, "Task UUIDv7"),
    Field::required("author_id", Type::String, "Author employee UUIDv7"),
    Field::required("revision", Type::Integer, "Note revision"),
    Field::required("body_markdown", Type::String, "Private Markdown body"),
    Field::required("created_at", Type::String, "Creation timestamp"),
    Field::required("updated_at", Type::String, "Update timestamp"),
    Field::required("editable", Type::Boolean, "True if caller is author"),
];

const NOTE_OUTPUT: Type = Type::Object(NOTE_OUTPUT_FIELDS);

const NOTE_REMOVED_FIELDS: &[Field] = &[
    Field::required("note_id", Type::String, "Deleted private note UUIDv7"),
    Field::required("task_id", Type::String, "Task UUIDv7"),
    Field::required("revision", Type::Integer, "Tombstone revision"),
    Field::required("deleted_at", Type::String, "Deletion timestamp"),
];

const NOTE_PUBLISH_FIELDS: &[Field] = &[
    Field::required("note", NOTE_OUTPUT, "Private note (unchanged)"),
    Field::required("comment", COMMENT_TYPE, "Published shared comment"),
    Field::required("task_id", Type::String, "Task UUIDv7"),
    Field::required("key", Type::String, "Task key"),
];

const NOTE_ADD_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::required("text", Type::String, "Private note Markdown text"),
];

const NOTE_LIST_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task UUIDv7 or T-<CODE><N> key"),
    Field::optional("author_id", Type::String, "Filter by author ID (people)"),
];

const NOTE_ID_INPUT: &[Field] = &[Field::required("id", Type::String, "Private note UUIDv7")];

const NOTE_UPDATE_INPUT: &[Field] = &[
    Field::required("id", Type::String, "Private note UUIDv7"),
    Field::required("text", Type::String, "Updated private note text"),
];

const NOTE_EXAMPLE_JSON: &str = r#"{"id":"0193a5c0-0000-7000-8000-000000000400","task_id":"0193a5c0-0000-7000-8000-000000000100","author_id":"0193a5c0-0000-7000-8000-000000000001","revision":1,"body_markdown":"Check race condition","created_at":"2026-11-22T00:00:00.000Z","updated_at":"2026-11-22T00:00:00.000Z","editable":true}"#;

/// The `task-note` rows.
pub(super) const COMMANDS: &[Command] = &[
    Command {
        resource: "task-note",
        verb: "add",
        summary: "Add private note",
        call: "tasks.notes.add",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: NOTE_ADD_INPUT,
        content_field: Some("text"),
        output: NOTE_OUTPUT_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","text":"Check race condition"}"#,
            output: NOTE_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "task-note",
        verb: "list",
        summary: "List private notes",
        call: "tasks.notes.list",
        kind: Kind::List,
        input: NOTE_LIST_INPUT,
        content_field: None,
        output: NOTE_OUTPUT_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1"}"#,
            output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000400","task_id":"0193a5c0-0000-7000-8000-000000000100","author_id":"0193a5c0-0000-7000-8000-000000000001","revision":1,"body_markdown":"Check race condition","created_at":"2026-11-22T00:00:00.000Z","updated_at":"2026-11-22T00:00:00.000Z","editable":true}]"#,
        },
    },
    Command {
        resource: "task-note",
        verb: "get",
        summary: "Get private note",
        call: "tasks.notes.get",
        kind: Kind::Read,
        input: NOTE_ID_INPUT,
        content_field: None,
        output: NOTE_OUTPUT_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"id":"0193a5c0-0000-7000-8000-000000000400"}"#,
            output: NOTE_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "task-note",
        verb: "update",
        summary: "Edit private note",
        call: "tasks.notes.update",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: NOTE_UPDATE_INPUT,
        content_field: Some("text"),
        output: NOTE_OUTPUT_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"id":"0193a5c0-0000-7000-8000-000000000400","text":"Updated plan"}"#,
            output: NOTE_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "task-note",
        verb: "remove",
        summary: "Delete private note",
        call: "tasks.notes.remove",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: NOTE_ID_INPUT,
        content_field: None,
        output: NOTE_REMOVED_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"id":"0193a5c0-0000-7000-8000-000000000400"}"#,
            output: r#"{"note_id":"0193a5c0-0000-7000-8000-000000000400","task_id":"0193a5c0-0000-7000-8000-000000000100","revision":2,"deleted_at":"2026-11-22T00:05:00.000Z"}"#,
        },
    },
    Command {
        resource: "task-note",
        verb: "publish",
        summary: "Publish note copy",
        call: "tasks.notes.publish",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: NOTE_ID_INPUT,
        content_field: None,
        output: NOTE_PUBLISH_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"id":"0193a5c0-0000-7000-8000-000000000400"}"#,
            output: r#"{"note":{"id":"0193a5c0-0000-7000-8000-000000000400","task_id":"0193a5c0-0000-7000-8000-000000000100","author_id":"0193a5c0-0000-7000-8000-000000000001","revision":1,"body_markdown":"Check race condition","created_at":"2026-11-22T00:00:00.000Z","updated_at":"2026-11-22T00:00:00.000Z","editable":true},"comment":{"id":"0193a5c0-0000-7000-8000-000000000300","position":0,"author_id":"0193a5c0-0000-7000-8000-000000000001","created_at":"2026-11-22T00:01:00.000Z","body_markdown":"Check race condition"},"task_id":"0193a5c0-0000-7000-8000-000000000100","key":"T-A1"}"#,
        },
    },
];

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
