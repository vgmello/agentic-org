//! `note` commands (`TASK-50`, `TASK-111` ADEC-3): private working notes, plans, and
//! decisions, with auto-bound task/project context, server-side `--fields` pruning for compact
//! Turn-40 recall, and explicit publication to a shared task comment.

use org_command::{Answer, Refusal, Request};
use org_proto::directory::{Who, WhoParams, WhoResult};
use org_proto::private_notes::{
    PrivateNoteAddParams, PrivateNoteField, PrivateNoteKind, PrivateNoteListParams,
    PrivateNoteUpdateParams, TaskNotePublishResult, TaskNoteRemovedResult, TaskNoteResult,
    TasksNotesAdd, TasksNotesGet, TasksNotesGetParams, TasksNotesList, TasksNotesListResult,
    TasksNotesPublish, TasksNotesPublishParams, TasksNotesRemove, TasksNotesRemoveParams,
    TasksNotesUpdate, PRIVATE_NOTE_CHECKPOINT_TAG,
};
use org_proto::projects::{ProjectInspect, ProjectInspectParams, ProjectRecord};
use org_proto::{Call, ErrorCode};
use serde_json::{json, Value};

use super::{cmd_id, unreadable};
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{
    entity_id, is_set, opt_str_list, opt_string, require_if_match, require_str, write_answer,
};
use crate::runtime::transport::OfficeTransport;

/// Default server-side `fields` sent on `note list` when `--fields` is omitted: omits full
/// `body_markdown` so Turn-40 recall stays compact (`<1 KiB`).
const DEFAULT_LIST_FIELDS: &[PrivateNoteField] = &[
    PrivateNoteField::Title,
    PrivateNoteField::Why,
    PrivateNoteField::Alternatives,
    PrivateNoteField::Tags,
];

/// `Use when:` guidance of each `note` command.
pub(super) const CUES: &[CommandCue] = &[
    ("note", "add", "recording a private working note, plan, or decision (auto-binds active task unless `--standalone`; never visible to other agents until published)."),
    ("note", "inspect", "reading one private note's full Markdown body, rationale, tags, and current revision by `--id`."),
    ("note", "list", "recalling private notes, plans, or decisions across tasks/projects (omits `body_markdown` by default for compact recall; `--fields` selects fields server-side)."),
    ("note", "publish", "copying a task-bound private note into a public task comment while keeping the private note intact (requires `--if-match`)."),
    ("note", "remove", "deleting a private note you no longer need (requires `--if-match`)."),
    ("note", "update", "editing an existing private note's text, kind, title, rationale, alternatives, or tags (requires `--if-match`)."),
];

pub(super) fn try_task_note<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    match request.command.verb {
        "add" => Some(write_note_add(transport, request)),
        "list" => Some(read_note_list(transport, request)),
        "inspect" => Some(read_note_inspect(transport, request)),
        "update" => Some(write_note_update(transport, request)),
        "remove" => Some(write_note_remove(transport, request)),
        "publish" => Some(write_note_publish(transport, request)),
        _ => None,
    }
}

fn invalid(message: impl Into<String>, hint: &str) -> Refusal {
    Refusal::new(ErrorCode::InvalidRequest, message, hint)
}

fn parse_kind(raw: &str) -> Result<PrivateNoteKind, Refusal> {
    PrivateNoteKind::parse(raw).ok_or_else(|| {
        invalid(
            format!("invalid note kind `{raw}`"),
            "Use `note`, `plan`, or `decision`.",
        )
    })
}

/// Removes `Value::Null` entries recursively from objects so optional `Option<String>` wire
/// fields (`task_id`, `project_id`, `title`, ...) conform to `org-command` `Field::optional`.
fn strip_nulls(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let cleaned = map
                .into_iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (k, strip_nulls(v)))
                .collect();
            Value::Object(cleaned)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(strip_nulls).collect()),
        other => other,
    }
}

fn resolve_project_id<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    raw: &str,
) -> Result<String, Refusal> {
    let trimmed = raw.trim();
    if org_proto::is_uuid_v7(trimmed) {
        return Ok(trimmed.to_owned());
    }
    let params = ProjectInspectParams {
        project: trimmed.to_owned(),
    };
    let reply = transport.call_method(ProjectInspect::METHOD, json!(params))?;
    let inspected: ProjectRecord = serde_json::from_value(reply).map_err(|_| {
        Refusal::new(
            ErrorCode::Unavailable,
            "office returned an unreadable `project.inspect` reply",
            "Retry the command or pass the project UUIDv7 directly.",
        )
    })?;
    Ok(inspected.id)
}

fn resolve_author_id<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    raw: &str,
) -> Result<String, Refusal> {
    let trimmed = raw.trim();
    if org_proto::is_uuid_v7(trimmed) {
        return Ok(trimmed.to_owned());
    }
    let handle = trimmed.trim_start_matches('@').to_ascii_lowercase();
    if handle.is_empty() {
        return Err(invalid(
            "--author cannot be empty.",
            "Pass a member @handle or UUIDv7.",
        ));
    }
    let reply = transport.call_method(Who::METHOD, json!(WhoParams::default()))?;
    let directory: WhoResult = serde_json::from_value(reply).map_err(|_| {
        Refusal::new(
            ErrorCode::Unavailable,
            "office returned an unreadable `who` reply",
            "Retry the command or pass the author's UUIDv7 directly.",
        )
    })?;
    directory
        .members
        .into_iter()
        .find(|m| {
            m.handle
                .as_deref()
                .is_some_and(|h| h.eq_ignore_ascii_case(&handle))
        })
        .map(|m| m.id)
        .ok_or_else(|| {
            Refusal::new(
                ErrorCode::NotFound,
                format!("No member found matching `{trimmed}`."),
                "Check active handles with `agent-os who` or pass a member UUIDv7.",
            )
        })
}

fn server_list_fields(requested: Option<&[String]>) -> Vec<PrivateNoteField> {
    let Some(roots) = requested else {
        return DEFAULT_LIST_FIELDS.to_vec();
    };
    let mapped: Vec<PrivateNoteField> = roots
        .iter()
        .filter_map(|root| PrivateNoteField::parse(root))
        .collect();
    if mapped.is_empty() {
        // Avoid sending empty `fields` (which fetches all fields including `body_markdown`).
        vec![PrivateNoteField::Title]
    } else {
        mapped
    }
}

fn resolve_note_task<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    input: &serde_json::Map<String, Value>,
    checkpoint: bool,
) -> Result<Option<String>, Refusal> {
    let standalone = is_set(input, "standalone");
    let explicit_task = opt_string(input, "task");
    if standalone {
        if explicit_task.is_some() {
            return Err(invalid(
                "--standalone and --task cannot be given together.",
                "Pass --task to attach the note to a task, or --standalone for an unbound note.",
            ));
        }
        if checkpoint {
            return Err(invalid(
                "--checkpoint cannot be combined with --standalone.",
                "Working checkpoints are pinned to a task; pass --task or omit --standalone.",
            ));
        }
        return Ok(None);
    }
    let task = explicit_task.or_else(|| transport.active_task());
    if checkpoint && task.is_none() {
        return Err(invalid(
            "--checkpoint requires a task.",
            "Pass --task <task> or run inside an active task session.",
        ));
    }
    Ok(task)
}

pub(super) fn write_note_add<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let i = &request.input;
    let checkpoint = is_set(i, "checkpoint");
    let task = resolve_note_task(transport, i, checkpoint)?;
    let project_ref = opt_string(i, "project")
        .or_else(|| task.is_none().then(|| transport.active_project()).flatten());
    let project_id = project_ref
        .map(|raw| resolve_project_id(transport, &raw))
        .transpose()?;
    let kind = opt_string(i, "kind")
        .map(|raw| parse_kind(&raw))
        .transpose()?
        .unwrap_or(PrivateNoteKind::Note);
    let params = PrivateNoteAddParams {
        command_id: cmd_id(request)?,
        task,
        project_id,
        note_id: entity_id(request, None, "task-note-add")?,
        kind,
        title: opt_string(i, "title"),
        why: opt_string(i, "why"),
        alternatives: opt_string(i, "alternatives"),
        tags: opt_str_list(i, "tag").unwrap_or_default(),
        body_markdown: require_str(i, "text")?.to_owned(),
        checkpoint,
    };
    let raw = transport.call_method(TasksNotesAdd::METHOD, json!(params))?;
    let res: TaskNoteResult = serde_json::from_value(raw).map_err(unreadable)?;
    Ok(write_answer(
        request,
        strip_nulls(json!(res.note)),
        Some(res.note.revision),
    ))
}

fn resolve_list_tag(request: &Request) -> Result<Option<String>, Refusal> {
    let checkpoint = is_set(&request.input, "checkpoint");
    let tag = opt_string(&request.input, "tag");
    match (checkpoint, tag) {
        (true, Some(t)) if !t.eq_ignore_ascii_case(PRIVATE_NOTE_CHECKPOINT_TAG) => Err(invalid(
            "--checkpoint cannot be combined with a different --tag.",
            "Pass either --checkpoint or --tag <tag>.",
        )),
        (true, _) => Ok(Some(PRIVATE_NOTE_CHECKPOINT_TAG.to_owned())),
        (false, tag) => Ok(tag),
    }
}

pub(super) fn read_note_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let i = &request.input;
    let count_only = is_set(i, "count");
    if count_only && request.cursor.is_some() {
        return Err(invalid(
            "--count answers no page, so it takes no --cursor.",
            "Run `note list --count` with the filters alone.",
        ));
    }
    let project_id = opt_string(i, "project")
        .map(|raw| resolve_project_id(transport, &raw))
        .transpose()?;
    let author_id = opt_string(i, "author")
        .map(|raw| resolve_author_id(transport, &raw))
        .transpose()?;
    let kind = opt_string(i, "kind").map(|k| parse_kind(&k)).transpose()?;
    let tag = resolve_list_tag(request)?;
    let fields = if count_only {
        vec![PrivateNoteField::Title]
    } else {
        server_list_fields(request.fields.as_deref())
    };
    let params = PrivateNoteListParams {
        task: opt_string(i, "task"),
        project_id,
        kind,
        tag,
        since: opt_string(i, "since"),
        author_id,
        limit: if count_only { Some(1) } else { request.limit },
        cursor: if count_only {
            None
        } else {
            request.cursor.clone()
        },
        fields,
    };
    let raw = transport.call_method(TasksNotesList::METHOD, json!(params))?;
    let res: TasksNotesListResult = serde_json::from_value(raw).map_err(unreadable)?;
    let items: Vec<Value> = if count_only {
        Vec::new()
    } else {
        res.notes
            .into_iter()
            .map(|n| strip_nulls(json!(n)))
            .collect()
    };
    let mut answer = Answer::completed(Value::Array(items));
    answer.total = Some(res.total);
    if !count_only {
        answer.next_cursor = res.next_cursor;
    }
    Ok(answer)
}

pub(super) fn read_note_inspect<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = TasksNotesGetParams {
        note_id: require_str(&request.input, "id")?.to_owned(),
    };
    let raw = transport.call_method(TasksNotesGet::METHOD, json!(params))?;
    let res: TaskNoteResult = serde_json::from_value(raw).map_err(unreadable)?;
    Ok(Answer::completed(strip_nulls(json!(res.note))))
}

pub(super) fn write_note_update<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let i = &request.input;
    let kind = opt_string(i, "kind").map(|k| parse_kind(&k)).transpose()?;
    let title = opt_string(i, "title");
    let why = opt_string(i, "why");
    let alternatives = opt_string(i, "alternatives");
    let tags = opt_str_list(i, "tag");
    let body_markdown = opt_string(i, "text");
    if kind.is_none()
        && title.is_none()
        && why.is_none()
        && alternatives.is_none()
        && tags.is_none()
        && body_markdown.is_none()
    {
        return Err(invalid(
            "`note update` needs at least one field to change.",
            "Pass --text, --kind, --title, --why, --alternatives, or --tag.",
        ));
    }
    let params = PrivateNoteUpdateParams {
        command_id: cmd_id(request)?,
        note_id: require_str(i, "id")?.to_owned(),
        expected_revision: require_if_match(request)?,
        kind,
        title,
        why,
        alternatives,
        tags,
        body_markdown,
    };
    let raw = transport.call_method(TasksNotesUpdate::METHOD, json!(params))?;
    let res: TaskNoteResult = serde_json::from_value(raw).map_err(unreadable)?;
    Ok(write_answer(
        request,
        strip_nulls(json!(res.note)),
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
    Ok(write_answer(
        request,
        strip_nulls(json!(res)),
        Some(res.revision),
    ))
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
        strip_nulls(json!({
            "note": res.note,
            "comment": res.comment,
            "task_id": res.task.id,
            "key": res.task.key,
        })),
        Some(res.task.revision),
    ))
}
