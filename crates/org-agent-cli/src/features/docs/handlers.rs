//! Handlers for `agent-os doc index|ls|show|new|edit|move|delete|put` (`TASK-55`).

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use org_command::{Answer, Refusal, Request};
use org_proto::docs::{
    DocMediaKindWire, DocStatusWire, DocsCreate, DocsCreateParams, DocsDelete, DocsDeleteParams,
    DocsDeleteResult, DocsIndex, DocsIndexParams, DocsIndexResult, DocsInspect, DocsInspectParams,
    DocsInspectResult, DocsList, DocsListParams, DocsListResult, DocsMove, DocsMoveParams,
    DocsPutMedia, DocsPutMediaParams, DocsPutMediaResult, DocsUpdate, DocsUpdateParams,
    DocsWriteResult, DOC_MEDIA_MAX_BYTES,
};
use org_proto::{Call, ErrorCode};
use serde_json::json;

use crate::runtime::domain::CommandCue;
use crate::runtime::file_guard::{read_upload_file, FileRoots};
use crate::runtime::input::{
    command_id, decode_err, opt_string, reject_dry_run, require_if_match, require_str, write_answer,
};
use crate::runtime::transport::OfficeTransport;

/// `Use when:` guidance for every `doc` verb.
pub(super) const CUES: &[CommandCue] = &[
    (
        "doc",
        "delete",
        "removing a document from a project's docs/ tree and regenerating index.yaml (requires `--if-match`).",
    ),
    (
        "doc",
        "edit",
        "updating a project document's title, kind, status, summary, owner, or Markdown body (requires `--if-match`).",
    ),
    (
        "doc",
        "index",
        "reading a project's full office-maintained docs/index.yaml tree (sections, documents, media).",
    ),
    (
        "doc",
        "ls",
        "listing documents in a project's docs/ tree filtered by section, kind, or status.",
    ),
    (
        "doc",
        "move",
        "moving a project document to a new path or section while regenerating index.yaml (requires `--if-match`).",
    ),
    (
        "doc",
        "new",
        "creating a new Markdown document in a project's docs/ tree (accepts `--body` or `--input-file`).",
    ),
    (
        "doc",
        "put",
        "uploading or replacing a validated PNG/JPEG/WebP/GIF asset under a project's docs/media/.",
    ),
    (
        "doc",
        "show",
        "reading one project document's metadata, revision, and full Markdown body by UUIDv7 or path.",
    ),
];

fn parse_status(raw: &str) -> Result<DocStatusWire, Refusal> {
    match raw {
        "draft" => Ok(DocStatusWire::Draft),
        "current" => Ok(DocStatusWire::Current),
        "deprecated" => Ok(DocStatusWire::Deprecated),
        _ => Err(Refusal::new(
            ErrorCode::InvalidRequest,
            format!("Invalid document status `{raw}`."),
            "Use draft, current, or deprecated.",
        )),
    }
}

fn parse_media_kind(raw: &str) -> Result<DocMediaKindWire, Refusal> {
    match raw {
        "image" => Ok(DocMediaKindWire::Image),
        "diagram" => Ok(DocMediaKindWire::Diagram),
        _ => Err(Refusal::new(
            ErrorCode::InvalidRequest,
            format!("Invalid media kind `{raw}`."),
            "Use image or diagram.",
        )),
    }
}

fn resolve_doc_id<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    project: &str,
    document: &str,
) -> Result<String, Refusal> {
    let trimmed = document.trim();
    if org_proto::is_uuid_v7(trimmed) {
        return Ok(trimmed.to_owned());
    }
    let raw = transport.call_method(
        DocsInspect::METHOD,
        json!(DocsInspectParams {
            project: project.to_owned(),
            document: trimmed.to_owned(),
        }),
    )?;
    let inspected: DocsInspectResult =
        serde_json::from_value(raw).map_err(|_| decode_err(DocsInspect::METHOD))?;
    Ok(inspected.document.id)
}

pub(super) fn read_index<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let project = require_str(&request.input, "project")?.to_owned();
    let raw = transport.call_method(DocsIndex::METHOD, json!(DocsIndexParams { project }))?;
    let res: DocsIndexResult =
        serde_json::from_value(raw).map_err(|_| decode_err(DocsIndex::METHOD))?;
    Ok(Answer::completed(json!(res)))
}

pub(super) fn read_ls<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let i = &request.input;
    let project = require_str(i, "project")?.to_owned();
    let status = opt_string(i, "status")
        .map(|s| parse_status(&s))
        .transpose()?;
    let limit = request.limit;
    let params = DocsListParams {
        project,
        section: opt_string(i, "section"),
        kind: opt_string(i, "kind"),
        status,
        limit,
        cursor: request.cursor.clone(),
    };
    let raw = transport.call_method(DocsList::METHOD, json!(params))?;
    let res: DocsListResult =
        serde_json::from_value(raw).map_err(|_| decode_err(DocsList::METHOD))?;
    let mut answer = Answer::completed(json!(res.documents));
    answer.next_cursor = res.next_cursor;
    Ok(answer)
}

pub(super) fn read_show<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let i = &request.input;
    let params = DocsInspectParams {
        project: require_str(i, "project")?.to_owned(),
        document: require_str(i, "document")?.to_owned(),
    };
    let raw = transport.call_method(DocsInspect::METHOD, json!(params))?;
    let res: DocsInspectResult =
        serde_json::from_value(raw).map_err(|_| decode_err(DocsInspect::METHOD))?;
    Ok(Answer::completed(json!(res)))
}

pub(super) fn write_new<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    reject_dry_run(request)?;
    let cmd_id = command_id(request, "doc-new")?;
    let i = &request.input;
    let status = opt_string(i, "status")
        .map(|s| parse_status(&s))
        .transpose()?;
    let params = DocsCreateParams {
        command_id: cmd_id,
        project: require_str(i, "project")?.to_owned(),
        kind: require_str(i, "kind")?.to_owned(),
        title: require_str(i, "title")?.to_owned(),
        path: opt_string(i, "path"),
        section: opt_string(i, "section"),
        summary: opt_string(i, "summary"),
        owner_employee_id: opt_string(i, "owner"),
        status,
        body_markdown: opt_string(i, "body").unwrap_or_default(),
    };
    let raw = transport.call_method(DocsCreate::METHOD, json!(params))?;
    let res: DocsWriteResult =
        serde_json::from_value(raw).map_err(|_| decode_err(DocsCreate::METHOD))?;
    let rev = res.document.revision;
    Ok(write_answer(request, json!(res.document), Some(rev)))
}

pub(super) fn write_edit<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    reject_dry_run(request)?;
    let cmd_id = command_id(request, "doc-edit")?;
    let expected_revision = require_if_match(request)?;
    let i = &request.input;
    let project = require_str(i, "project")?.to_owned();
    let doc_ref = require_str(i, "document")?;
    let document_id = resolve_doc_id(transport, &project, doc_ref)?;
    let status = opt_string(i, "status")
        .map(|s| parse_status(&s))
        .transpose()?;
    let params = DocsUpdateParams {
        command_id: cmd_id,
        project,
        document_id,
        expected_revision,
        title: opt_string(i, "title"),
        kind: opt_string(i, "kind"),
        status,
        summary: opt_string(i, "summary"),
        owner_employee_id: opt_string(i, "owner"),
        body_markdown: opt_string(i, "body"),
    };
    let raw = transport.call_method(DocsUpdate::METHOD, json!(params))?;
    let res: DocsWriteResult =
        serde_json::from_value(raw).map_err(|_| decode_err(DocsUpdate::METHOD))?;
    let rev = res.document.revision;
    Ok(write_answer(request, json!(res.document), Some(rev)))
}

pub(super) fn write_move<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    reject_dry_run(request)?;
    let cmd_id = command_id(request, "doc-move")?;
    let expected_revision = require_if_match(request)?;
    let i = &request.input;
    let project = require_str(i, "project")?.to_owned();
    let doc_ref = require_str(i, "document")?;
    let document_id = resolve_doc_id(transport, &project, doc_ref)?;
    let params = DocsMoveParams {
        command_id: cmd_id,
        project,
        document_id,
        expected_revision,
        new_path: opt_string(i, "new_path"),
        new_section: opt_string(i, "new_section"),
    };
    let raw = transport.call_method(DocsMove::METHOD, json!(params))?;
    let res: DocsWriteResult =
        serde_json::from_value(raw).map_err(|_| decode_err(DocsMove::METHOD))?;
    let rev = res.document.revision;
    Ok(write_answer(request, json!(res.document), Some(rev)))
}

pub(super) fn write_delete<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    reject_dry_run(request)?;
    let cmd_id = command_id(request, "doc-delete")?;
    let expected_revision = require_if_match(request)?;
    let i = &request.input;
    let project = require_str(i, "project")?.to_owned();
    let doc_ref = require_str(i, "document")?;
    let document_id = resolve_doc_id(transport, &project, doc_ref)?;
    let params = DocsDeleteParams {
        command_id: cmd_id,
        project,
        document_id,
        expected_revision,
    };
    let raw = transport.call_method(DocsDelete::METHOD, json!(params))?;
    let res: DocsDeleteResult =
        serde_json::from_value(raw).map_err(|_| decode_err(DocsDelete::METHOD))?;
    Ok(write_answer(request, json!(res), None))
}

pub(super) fn write_put<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    reject_dry_run(request)?;
    let cmd_id = command_id(request, "doc-put")?;
    let i = &request.input;
    let project = require_str(i, "project")?.to_owned();
    let path = require_str(i, "path")?;
    let title = require_str(i, "title")?.to_owned();
    let kind = match opt_string(i, "kind") {
        Some(raw) => parse_media_kind(&raw)?,
        None => DocMediaKindWire::Image,
    };
    let local = read_upload_file(path, &FileRoots::from_env())?;
    if !matches!(
        local.mime_type,
        "image/png" | "image/jpeg" | "image/webp" | "image/gif"
    ) {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            format!("Unsupported docs media MIME type `{}`.", local.mime_type),
            "Pass a PNG, JPEG, WebP, or GIF image file.",
        ));
    }
    if local.bytes.len() > DOC_MEDIA_MAX_BYTES {
        return Err(Refusal::new(
            ErrorCode::TooLarge,
            format!(
                "Image asset exceeds the {} byte documentation media limit.",
                DOC_MEDIA_MAX_BYTES
            ),
            "Resize or compress the image before uploading.",
        ));
    }
    let file_name =
        opt_string(i, "file_name").unwrap_or_else(|| local.filename.to_ascii_lowercase());
    let params = DocsPutMediaParams {
        command_id: cmd_id,
        project,
        file_name,
        media_type: local.mime_type.to_owned(),
        kind,
        title,
        summary: opt_string(i, "summary"),
        content_base64: STANDARD.encode(&local.bytes),
        expected_revision: request.if_match,
    };
    let raw = transport.call_method(DocsPutMedia::METHOD, json!(params))?;
    let res: DocsPutMediaResult =
        serde_json::from_value(raw).map_err(|_| decode_err(DocsPutMedia::METHOD))?;
    let rev = res.media.revision;
    Ok(write_answer(request, json!(res.media), Some(rev)))
}
