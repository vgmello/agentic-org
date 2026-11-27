//! Handler for `agent-os file render` (`TASK-98.7`).
//!
//! Zero positional arguments, no `content_field`, strict two-layer `file_guard` confinement on
//! `--from`, `--css`, and `--output`, and automatic frame chunking for large inputs (up to 2 MiB)
//! and large local PDF outputs (> 512 KiB).

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use org_command::{Answer, Refusal, Request};
use org_proto::attachments::{
    ArtifactStatus, AttachmentRender, AttachmentRenderParams, AttachmentRenderResult, RenderFormat,
    ATTACHMENT_CHUNK_MAX_BYTES, ATTACHMENT_DOCUMENT_MAX_BYTES,
};
use org_proto::{Call, ErrorCode};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::handlers::{parse_status, resolve_scope_target, TargetResolution};
use crate::runtime::file_guard::{
    prepare_output_path, write_prepared_output, FileRoots, PreparedOutputPath,
};
use crate::runtime::file_transfer::{read_render_inputs, LocalRenderInputs};
use crate::runtime::input::{
    command_id, decode_err, opt_string, reject_dry_run, require_str, write_answer,
};
use crate::runtime::transport::OfficeTransport;

/// Maximum combined raw UTF-8 source + CSS bytes sent inline in one JSON RPC frame (512 KiB),
/// staying safely below `MAX_FRAME_BYTES` (1 MiB) even with JSON escaping overhead.
const INLINE_SOURCE_FRAME_CAP: usize = 512 * 1024;

enum RenderDest {
    Hosted(TargetResolution),
    Local(PreparedOutputPath),
}

/// Rewrites `--format <doc-format>` / `--format=<doc-format>` on `file render` into `--to`
/// whenever `<doc-format>` is not one of `org-command`'s global stdout envelope formats
/// (`json`, `yaml`, `compact`).
pub(crate) fn rewrite_render_args(args: &[String]) -> Option<Vec<String>> {
    let [resource, verb, rest @ ..] = args else {
        return None;
    };
    if resource != "file" || verb != "render" {
        return None;
    }
    let mut out = Vec::with_capacity(args.len());
    out.push(resource.clone());
    out.push(verb.clone());
    let mut changed = false;
    let mut i = 0usize;
    while i < rest.len() {
        let arg = &rest[i];
        if arg == "--" {
            out.extend_from_slice(&rest[i..]);
            break;
        }
        if arg == "--format" {
            if let Some(val) = rest.get(i + 1) {
                if !is_envelope_format(val) {
                    out.push("--to".to_owned());
                    out.push(val.clone());
                    changed = true;
                    i += 2;
                    continue;
                }
            }
        } else if let Some(val) = arg.strip_prefix("--format=") {
            if !is_envelope_format(val) {
                out.push(format!("--to={val}"));
                changed = true;
                i += 1;
                continue;
            }
        }
        out.push(arg.clone());
        i += 1;
    }
    changed.then_some(out)
}

fn is_envelope_format(val: &str) -> bool {
    matches!(val, "json" | "yaml" | "compact")
}

pub(super) fn write_file_render<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    reject_dry_run(request)?;
    let cmd_id = command_id(request, "file-render")?;
    let i = &request.input;
    let from_path = require_str(i, "from")?;
    let format_raw = opt_string(i, "to").unwrap_or_else(|| "pdf".to_owned());
    let format = RenderFormat::parse(&format_raw).ok_or_else(|| {
        Refusal::new(
            ErrorCode::Unsupported,
            format!("Unsupported render format `{format_raw}`."),
            "Pass `--format pdf` (or `--to pdf`).",
        )
    })?;
    let css_path = opt_string(i, "css");
    let scope = opt_string(i, "scope");
    let output_path = opt_string(i, "output");
    let project = opt_string(i, "project");
    let task = opt_string(i, "task");
    let title = opt_string(i, "title");
    let description = opt_string(i, "description");
    let artifact_status = parse_render_status(opt_string(i, "status").as_deref())?;

    let roots = FileRoots::from_env();
    let dest = validate_destination(
        transport,
        &roots,
        scope.as_deref(),
        output_path.as_deref(),
        project.as_deref(),
        task.as_deref(),
        description.as_deref(),
        artifact_status,
    )?;

    let local = read_render_inputs(from_path, css_path.as_deref(), &roots)?;
    let base_params = build_base_params(
        cmd_id,
        format,
        &local,
        &dest,
        title,
        description,
        artifact_status,
    );
    let reply = send_render_rpc(transport, base_params, &local)?;

    match dest {
        RenderDest::Hosted(_) => Ok(write_answer(request, hosted_answer_json(&reply), None)),
        RenderDest::Local(prepared) => {
            let saved = collect_and_write_pdf(transport, &prepared, reply)?;
            Ok(write_answer(request, saved, None))
        }
    }
}

fn parse_render_status(raw: Option<&str>) -> Result<Option<ArtifactStatus>, Refusal> {
    let Some(text) = raw else {
        return Ok(None);
    };
    let status = parse_status(text)?;
    if !matches!(
        status,
        ArtifactStatus::Draft | ArtifactStatus::Review | ArtifactStatus::Final
    ) {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            format!("`file render --status` does not accept `{text}`."),
            "Use `draft`, `review`, or `final`.",
        ));
    }
    Ok(Some(status))
}

#[expect(
    clippy::too_many_arguments,
    reason = "validates mutually exclusive hosted vs local CLI flags before file I/O"
)]
fn validate_destination<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    roots: &FileRoots,
    scope: Option<&str>,
    output: Option<&str>,
    project: Option<&str>,
    task: Option<&str>,
    description: Option<&str>,
    status: Option<ArtifactStatus>,
) -> Result<RenderDest, Refusal> {
    match (scope, output) {
        (Some(_), Some(_)) => Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "`--scope` and `--output` are mutually exclusive.",
            "Pass either `--scope <org|project|task|private>` or `--output <path.pdf>`, never both.",
        )),
        (None, None) => Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "`file render` requires either `--scope` or `--output`.",
            "Pass `--scope <org|project|task|private>` to attach the PDF, or `--output <path.pdf>` to save it locally.",
        )),
        (Some(scope_str), None) => {
            let target = resolve_scope_target(transport, scope_str, project, task)?;
            Ok(RenderDest::Hosted(target))
        }
        (None, Some(out_path)) => {
            if project.is_some() || task.is_some() || description.is_some() || status.is_some() {
                return Err(Refusal::new(
                    ErrorCode::InvalidRequest,
                    "`--output` cannot be combined with `--project`, `--task`, `--description`, or `--status`.",
                    "Omit hosted scope flags when rendering to a local `--output` path.",
                ));
            }
            let prepared = prepare_output_path(out_path, roots)?;
            Ok(RenderDest::Local(prepared))
        }
    }
}

fn build_base_params(
    cmd_id: String,
    format: RenderFormat,
    local: &LocalRenderInputs,
    dest: &RenderDest,
    title: Option<String>,
    description: Option<String>,
    artifact_status: Option<ArtifactStatus>,
) -> AttachmentRenderParams {
    let (target_kind, target_id, return_bytes) = match dest {
        RenderDest::Hosted(t) => (Some(t.kind), t.id.clone(), false),
        RenderDest::Local(_) => (None, None, true),
    };
    AttachmentRenderParams {
        command_id: (!return_bytes).then_some(cmd_id),
        format,
        source_kind: local.source_kind,
        source: String::new(),
        css: None,
        filename: Some(local.filename.clone()),
        target_kind,
        target_id,
        project_slug: None,
        task_ref: None,
        title,
        description,
        artifact_status,
        return_bytes,
        render_id: None,
        chunk_offset: None,
        source_chunk_base64: None,
        css_chunk_base64: None,
        input_eof: None,
    }
}

fn send_render_rpc<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    mut base: AttachmentRenderParams,
    local: &LocalRenderInputs,
) -> Result<AttachmentRenderResult, Refusal> {
    let total_raw = local
        .source
        .len()
        .saturating_add(local.css.as_deref().map_or(0, str::len));
    if total_raw <= INLINE_SOURCE_FRAME_CAP {
        base.source = local.source.clone();
        base.css = local.css.clone();
        let raw = transport.call_method(AttachmentRender::METHOD, json!(base))?;
        return serde_json::from_value(raw).map_err(|_| decode_err(AttachmentRender::METHOD));
    }

    // Stream oversized (<= 2 MiB) source + CSS in <= 512 KiB base64 chunks so every request
    // fits comfortably inside `MAX_FRAME_BYTES` (1 MiB).
    let chunk_max = ATTACHMENT_CHUNK_MAX_BYTES as usize;
    let src_chunks: Vec<&[u8]> = local.source.as_bytes().chunks(chunk_max).collect();
    let css_chunks: Vec<&[u8]> = local
        .css
        .as_deref()
        .map(|c| c.as_bytes().chunks(chunk_max).collect())
        .unwrap_or_default();
    let total_steps = src_chunks.len().max(css_chunks.len()).max(1);
    let mut render_id: Option<String> = None;

    for step in 0..total_steps {
        let mut req = base.clone();
        req.render_id = render_id.clone();
        req.source_chunk_base64 = src_chunks.get(step).map(|c| STANDARD.encode(c));
        req.css_chunk_base64 = css_chunks.get(step).map(|c| STANDARD.encode(c));
        let is_last = step + 1 == total_steps;
        req.input_eof = Some(is_last);
        let raw = transport.call_method(AttachmentRender::METHOD, json!(req))?;
        let reply: AttachmentRenderResult =
            serde_json::from_value(raw).map_err(|_| decode_err(AttachmentRender::METHOD))?;
        if is_last {
            return Ok(reply);
        }
        render_id = Some(
            reply
                .render_id
                .ok_or_else(|| decode_err(AttachmentRender::METHOD))?,
        );
    }
    Err(decode_err(AttachmentRender::METHOD))
}

fn hosted_answer_json(reply: &AttachmentRenderResult) -> Value {
    let mut out = Map::new();
    if let Some(id) = &reply.attachment_id {
        out.insert("attachment_id".into(), json!(id));
    }
    if let Some(url) = &reply.canonical_url {
        out.insert("canonical_url".into(), json!(url));
    }
    out.insert("page_count".into(), json!(reply.page_count));
    out.insert("byte_count".into(), json!(reply.byte_count));
    if let Some(sha) = &reply.content_sha256 {
        out.insert("content_sha256".into(), json!(sha));
    }
    Value::Object(out)
}

fn collect_and_write_pdf<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    prepared: &PreparedOutputPath,
    first: AttachmentRenderResult,
) -> Result<Value, Refusal> {
    if first.byte_count > ATTACHMENT_DOCUMENT_MAX_BYTES {
        return Err(Refusal::new(
            ErrorCode::TooLarge,
            "Rendered PDF exceeds the 25 MiB local download bound.",
            "Reduce document size or embedded images and retry.",
        ));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(first.byte_count).unwrap_or(0));
    let expected_total = first.byte_count;
    let expected_sha = first.content_sha256.clone();
    let page_count = first.page_count;

    let mut cur = first;
    loop {
        let b64 = cur
            .pdf_base64
            .as_deref()
            .ok_or_else(|| decode_err(AttachmentRender::METHOD))?;
        let decoded = STANDARD.decode(b64.as_bytes()).map_err(|_| {
            Refusal::new(
                ErrorCode::Unavailable,
                "Office returned invalid base64 PDF data.",
                "Retry `file render`.",
            )
        })?;
        if (bytes.len() as u64).saturating_add(decoded.len() as u64) > ATTACHMENT_DOCUMENT_MAX_BYTES
        {
            return Err(Refusal::new(
                ErrorCode::TooLarge,
                "Rendered PDF exceeded the 25 MiB local download bound.",
                "Reduce document size and retry.",
            ));
        }
        bytes.extend_from_slice(&decoded);
        if cur.eof.unwrap_or(true) {
            break;
        }
        if decoded.is_empty() {
            return Err(Refusal::new(
                ErrorCode::Unavailable,
                "Office returned an empty non-EOF PDF chunk.",
                "Retry `file render`.",
            ));
        }
        let next_id = cur
            .render_id
            .clone()
            .ok_or_else(|| decode_err(AttachmentRender::METHOD))?;
        let next_params = AttachmentRenderParams {
            return_bytes: true,
            render_id: Some(next_id),
            chunk_offset: Some(bytes.len() as u64),
            ..Default::default()
        };
        let raw = transport.call_method(AttachmentRender::METHOD, json!(next_params))?;
        cur = serde_json::from_value(raw).map_err(|_| decode_err(AttachmentRender::METHOD))?;
    }

    if bytes.len() as u64 != expected_total {
        return Err(Refusal::new(
            ErrorCode::Unavailable,
            "Downloaded PDF byte count did not match office byte_count.",
            "Retry `file render`.",
        ));
    }
    let actual_sha: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if let Some(expected) = expected_sha.as_deref() {
        if expected != actual_sha {
            return Err(Refusal::new(
                ErrorCode::Unavailable,
                "Downloaded PDF bytes did not match content_sha256.",
                "Retry `file render`.",
            ));
        }
    }
    write_prepared_output(prepared, &bytes)?;
    Ok(json!({
        "output": prepared.display_path.display().to_string(),
        "page_count": page_count,
        "byte_count": bytes.len() as u64,
        "content_sha256": actual_sha,
    }))
}
