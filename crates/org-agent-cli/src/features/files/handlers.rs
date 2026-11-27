//! Handlers and command rows for `agent-os file upload|list|inspect|fetch|update` (`TASK-98.4`).

use org_command::{Ack, Answer, Command, Example, IfMatch, Kind, Refusal, Request};
use org_proto::attachments::{
    ArtifactStatus, AttachmentAttach, AttachmentAttachParams, AttachmentInspect,
    AttachmentInspectParams, AttachmentList, AttachmentListParams, AttachmentListResult,
    AttachmentResult, AttachmentScope, AttachmentTargetKind, AttachmentUpdateMeta,
    AttachmentUpdateMetaParams,
};
use org_proto::projects::{ProjectInspect, ProjectInspectParams, ProjectRecord};
use org_proto::tasks::{TaskInspectResult, TasksInspect, TasksInspectParams};
use org_proto::{Call, ErrorCode};
use serde_json::{json, Value};

use super::schema::{
    FILE_EXAMPLE_JSON, FILE_FETCH_INPUT, FILE_FETCH_OUTPUT, FILE_FIELDS, FILE_ID_INPUT,
    FILE_LIST_INPUT, FILE_RENDER_EXAMPLE_JSON, FILE_RENDER_INPUT, FILE_RENDER_OUTPUT,
    FILE_UPDATE_INPUT, FILE_UPLOAD_INPUT, READ_ERRORS, WRITE_ERRORS,
};
use crate::runtime::domain::CommandCue;
use crate::runtime::file_guard::FileRoots;
use crate::runtime::file_transfer::{
    abort_upload_best_effort, fetch_attachment_to_path, stage_local_file,
};
use crate::runtime::input::{
    command_id, decode_err, opt_string, reject_dry_run, require_str, write_answer,
};
use crate::runtime::transport::OfficeTransport;

/// The five `file` commands (zero positional arguments, concise summaries for catalogue budget).
pub(super) const COMMANDS: &[Command] = &[
    Command {
        resource: "file",
        verb: "upload",
        summary: "Upload and attach a file",
        call: "attachment.attach",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: FILE_UPLOAD_INPUT,
        content_field: None,
        output: FILE_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"path":"spec.md","scope":"org","title":"Spec"}"#,
            output: FILE_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "file",
        verb: "list",
        summary: "List files in a scope",
        call: "attachment.list",
        kind: Kind::List,
        input: FILE_LIST_INPUT,
        content_field: None,
        output: FILE_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"scope":"org"}"#,
            output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000500","scope":"shared","target_kind":"organization","filename":"spec.md","mime_type":"text/markdown","byte_count":42,"title":"Spec","artifact_status":"final","uploaded_by_employee_id":"0193a5c0-0000-7000-8000-000000000020","created_at":"2026-11-27T00:00:00.000Z","updated_at":"2026-11-27T00:00:00.000Z","content_sha256":"0000000000000000000000000000000000000000000000000000000000000000"}]"#,
        },
    },
    Command {
        resource: "file",
        verb: "inspect",
        summary: "Inspect file metadata",
        call: "attachment.inspect",
        kind: Kind::Read,
        input: FILE_ID_INPUT,
        content_field: None,
        output: FILE_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"id":"0193a5c0-0000-7000-8000-000000000500"}"#,
            output: FILE_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "file",
        verb: "fetch",
        summary: "Download a file locally",
        call: "attachment.read_chunk",
        kind: Kind::Read,
        input: FILE_FETCH_INPUT,
        content_field: None,
        output: FILE_FETCH_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"id":"0193a5c0-0000-7000-8000-000000000500","output":"spec.md"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000500","output":"spec.md","byte_count":42,"content_sha256":"0000000000000000000000000000000000000000000000000000000000000000"}"#,
        },
    },
    Command {
        resource: "file",
        verb: "update",
        summary: "Update file metadata",
        call: "attachment.update_meta",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: FILE_UPDATE_INPUT,
        content_field: None,
        output: FILE_FIELDS,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"id":"0193a5c0-0000-7000-8000-000000000500","title":"Final spec","status":"final"}"#,
            output: FILE_EXAMPLE_JSON,
        },
    },
    Command {
        resource: "file",
        verb: "render",
        summary: "Render Markdown/HTML to PDF",
        call: "attachment.render",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: FILE_RENDER_INPUT,
        content_field: None,
        output: FILE_RENDER_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"from":"report.md","to":"pdf","scope":"org","title":"Report"}"#,
            output: FILE_RENDER_EXAMPLE_JSON,
        },
    },
];

/// `Use when:` guidance for every `file` verb.
pub(super) const CUES: &[CommandCue] = &[
    (
        "file",
        "fetch",
        "downloading a readable attachment to a local path with SHA-256 verification.",
    ),
    (
        "file",
        "inspect",
        "reading one attachment's metadata, MIME type, size, and SHA-256 digest by `--id`.",
    ),
    (
        "file",
        "list",
        "listing files hosted in an org, project, task, or private scope.",
    ),
    (
        "file",
        "render",
        "rendering a local Markdown or HTML+CSS file (`--from`, optional `--css`) to PDF via the office and either attaching it (`--scope`) or saving locally (`--output`).",
    ),
    (
        "file",
        "update",
        "updating a hosted or task file's `--title`, `--description`, or `--status`.",
    ),
    (
        "file",
        "upload",
        "uploading a local file and attaching it to an org, project, task, or private scope.",
    ),
];

fn agent_private_refusal() -> Refusal {
    Refusal::new(
        ErrorCode::NotAuthorized,
        "Agents have no private file space.",
        "To hand a file to one person, send it in a 1:1 direct message with --attach.",
    )
}

pub(super) fn parse_status(raw: &str) -> Result<ArtifactStatus, Refusal> {
    ArtifactStatus::parse(raw).ok_or_else(|| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            format!("Invalid artifact status `{raw}`."),
            "Use draft, review, final, superseded, or archived.",
        )
    })
}

fn resolve_project_id<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    reference: &str,
) -> Result<String, Refusal> {
    let trimmed = reference.trim();
    if org_proto::is_uuid_v7(trimmed) {
        return Ok(trimmed.to_owned());
    }
    let raw = transport.call_method(
        ProjectInspect::METHOD,
        json!(ProjectInspectParams {
            project: trimmed.to_owned(),
        }),
    )?;
    let res: ProjectRecord =
        serde_json::from_value(raw).map_err(|_| decode_err(ProjectInspect::METHOD))?;
    Ok(res.id)
}

fn resolve_task_id<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    reference: &str,
) -> Result<String, Refusal> {
    let trimmed = reference.trim();
    if org_proto::is_uuid_v7(trimmed) {
        return Ok(trimmed.to_owned());
    }
    let raw = transport.call_method(
        TasksInspect::METHOD,
        json!(TasksInspectParams {
            task: trimmed.to_owned(),
            fields: Some(vec!["task".to_owned()]),
        }),
    )?;
    let res: TaskInspectResult =
        serde_json::from_value(raw).map_err(|_| decode_err(TasksInspect::METHOD))?;
    Ok(res.task.id)
}

pub(super) struct TargetResolution {
    pub kind: AttachmentTargetKind,
    pub id: Option<String>,
    pub scope: AttachmentScope,
}

pub(super) fn resolve_scope_target<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    scope: &str,
    project: Option<&str>,
    task: Option<&str>,
) -> Result<TargetResolution, Refusal> {
    match scope {
        "org" => {
            if project.is_some() || task.is_some() {
                return Err(Refusal::new(
                    ErrorCode::InvalidRequest,
                    "`--scope org` takes neither `--project` nor `--task`.",
                    "Omit `--project` and `--task`, or use `--scope project` / `--scope task`.",
                ));
            }
            Ok(TargetResolution {
                kind: AttachmentTargetKind::Organization,
                id: None,
                scope: AttachmentScope::Shared,
            })
        }
        "project" => {
            if task.is_some() {
                return Err(Refusal::new(
                    ErrorCode::InvalidRequest,
                    "`--scope project` cannot be combined with `--task`.",
                    "Pass `--project <slug|id>` alone for project files.",
                ));
            }
            let proj = project.ok_or_else(|| {
                Refusal::new(
                    ErrorCode::InvalidRequest,
                    "`--scope project` requires `--project <slug|id>`.",
                    "Pass `--project <slug|P-n|uuid>`.",
                )
            })?;
            Ok(TargetResolution {
                kind: AttachmentTargetKind::Project,
                id: Some(resolve_project_id(transport, proj)?),
                scope: AttachmentScope::Shared,
            })
        }
        "task" => {
            if project.is_some() {
                return Err(Refusal::new(
                    ErrorCode::InvalidRequest,
                    "`--scope task` cannot be combined with `--project`.",
                    "Pass `--task <T-CODE-N|id>` alone for task files.",
                ));
            }
            let task_ref = task.ok_or_else(|| {
                Refusal::new(
                    ErrorCode::InvalidRequest,
                    "`--scope task` requires `--task <ref>`.",
                    "Pass `--task <T-A1|uuid>`.",
                )
            })?;
            Ok(TargetResolution {
                kind: AttachmentTargetKind::Task,
                id: Some(resolve_task_id(transport, task_ref)?),
                scope: AttachmentScope::Shared,
            })
        }
        "private" => {
            if project.is_some() || task.is_some() {
                return Err(Refusal::new(
                    ErrorCode::InvalidRequest,
                    "`--scope private` takes neither `--project` nor `--task`.",
                    "Omit `--project` and `--task` for private files.",
                ));
            }
            // Managed agent sessions always bind via `AGENT_OS_AGENT_ID`; fail fast before
            // reading or uploading bytes. The office's `AuthorizationPolicy` also enforces this.
            if transport.caller_member_id().is_some() {
                return Err(agent_private_refusal());
            }
            Ok(TargetResolution {
                kind: AttachmentTargetKind::UserPrivate,
                id: None,
                scope: AttachmentScope::UserPrivate,
            })
        }
        other => Err(Refusal::new(
            ErrorCode::InvalidRequest,
            format!("Invalid scope `{other}`."),
            "Use `--scope org`, `project`, `task`, or `private`.",
        )),
    }
}

pub(super) fn write_file_upload<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    reject_dry_run(request)?;
    let cmd_id = command_id(request, "file-upload")?;
    let i = &request.input;
    let path = require_str(i, "path")?;
    let scope = require_str(i, "scope")?;
    let project = opt_string(i, "project");
    let task = opt_string(i, "task");
    let title = opt_string(i, "title");
    let description = opt_string(i, "description");
    let artifact_status = opt_string(i, "status")
        .map(|s| parse_status(&s))
        .transpose()?;

    let target = resolve_scope_target(transport, scope, project.as_deref(), task.as_deref())?;
    let staged = stage_local_file(transport, path, &FileRoots::from_env())?;
    let params = AttachmentAttachParams {
        command_id: cmd_id,
        upload_id: Some(staged.upload_id.clone()),
        source_attachment_id: None,
        target_kind: target.kind,
        target_id: target.id,
        title,
        description,
        artifact_status,
    };
    let raw = match transport.call_method(AttachmentAttach::METHOD, json!(params)) {
        Ok(raw) => raw,
        Err(err) => {
            abort_upload_best_effort(transport, &staged.upload_id);
            return Err(err);
        }
    };
    let res: AttachmentResult =
        serde_json::from_value(raw).map_err(|_| decode_err(AttachmentAttach::METHOD))?;
    Ok(write_answer(request, json!(res.attachment), None))
}

pub(super) fn read_file_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let i = &request.input;
    let project = opt_string(i, "project");
    let task = opt_string(i, "task");
    let scope = opt_string(i, "scope").unwrap_or_else(|| {
        if project.is_some() {
            "project".to_owned()
        } else if task.is_some() {
            "task".to_owned()
        } else {
            "org".to_owned()
        }
    });
    let target = resolve_scope_target(transport, &scope, project.as_deref(), task.as_deref())?;
    let params = AttachmentListParams {
        target_kind: Some(target.kind),
        target_id: target.id,
        scope: Some(target.scope),
        limit: request.limit,
        cursor: request.cursor.clone(),
    };
    let raw = transport.call_method(AttachmentList::METHOD, json!(params))?;
    let res: AttachmentListResult =
        serde_json::from_value(raw).map_err(|_| decode_err(AttachmentList::METHOD))?;
    let items: Vec<Value> = res.attachments.into_iter().map(|a| json!(a)).collect();
    let mut answer = Answer::completed(Value::Array(items));
    answer.next_cursor = res.next_cursor;
    Ok(answer)
}

pub(super) fn read_file_inspect<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let id = require_str(&request.input, "id")?;
    if !org_proto::is_uuid_v7(id) {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "File `--id` must be a canonical lowercase UUIDv7.",
            "Pass an attachment ID returned by `file upload` or `file list`.",
        ));
    }
    let raw = transport.call_method(
        AttachmentInspect::METHOD,
        json!(AttachmentInspectParams {
            attachment_id: id.to_owned(),
        }),
    )?;
    let res: AttachmentResult =
        serde_json::from_value(raw).map_err(|_| decode_err(AttachmentInspect::METHOD))?;
    Ok(Answer::completed(json!(res.attachment)))
}

pub(super) fn read_file_fetch<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let id = require_str(&request.input, "id")?;
    let output = require_str(&request.input, "output")?;
    let fetched = fetch_attachment_to_path(transport, id, output, &FileRoots::from_env())?;
    Ok(Answer::completed(json!({
        "id": fetched.id,
        "output": fetched.output,
        "byte_count": fetched.byte_count,
        "content_sha256": fetched.content_sha256,
    })))
}

pub(super) fn write_file_update<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    reject_dry_run(request)?;
    let cmd_id = command_id(request, "file-update")?;
    let i = &request.input;
    let id = require_str(i, "id")?;
    if !org_proto::is_uuid_v7(id) {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "File `--id` must be a canonical lowercase UUIDv7.",
            "Pass an attachment ID returned by `file upload` or `file list`.",
        ));
    }
    let title = opt_string(i, "title");
    let description = opt_string(i, "description");
    let artifact_status = opt_string(i, "status")
        .map(|s| parse_status(&s))
        .transpose()?;
    if title.is_none() && description.is_none() && artifact_status.is_none() {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "`file update` requires at least one of `--title`, `--description`, or `--status`.",
            "Pass one or more metadata flags to update.",
        ));
    }
    let params = AttachmentUpdateMetaParams {
        command_id: cmd_id,
        attachment_id: id.to_owned(),
        title,
        description,
        artifact_status,
    };
    let raw = transport.call_method(AttachmentUpdateMeta::METHOD, json!(params))?;
    let res: AttachmentResult =
        serde_json::from_value(raw).map_err(|_| decode_err(AttachmentUpdateMeta::METHOD))?;
    Ok(write_answer(request, json!(res.attachment), None))
}
