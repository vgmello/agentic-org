//! Thin `attachment.render` RPC bridge over `org-doc-render` and `Office::attachment_*`
//! (`TASK-98.7`).
//!
//! All rendering, sanitization, semaphore bounding, timeouts, and page counting live inside
//! `org-doc-render`; this module handles caller/target pre-authorization, `attachment:<id>`
//! image loading via `AuthorizationPolicy::can_read`, bounded frame chunking for large inputs
//! and outputs, and hosted commit through the existing `Office` attachment pipeline.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_doc_render::{
    extract_attachment_refs, BoundedDocumentRenderer, DocumentRenderer, RenderError, RenderRequest,
    ResolvedImage, SourceKind, MAX_IMAGE_DECODED_BYTES, MAX_SOURCE_BYTES,
};
use org_office::{Office, OfficeError};
use org_proto::attachments::{
    is_image_mime, AttachmentAttachParams, AttachmentInspectParams, AttachmentListParams,
    AttachmentReadChunkParams, AttachmentRenderParams, AttachmentRenderResult, AttachmentScope,
    AttachmentTargetKind, AttachmentUploadBeginParams, AttachmentUploadChunkParams,
    AttachmentUploadRef, RenderSourceKind, ATTACHMENT_CHUNK_MAX_BASE64_CHARS,
    ATTACHMENT_CHUNK_MAX_BYTES, ATTACHMENT_FILENAME_MAX_BYTES,
};
use org_proto::{ApiError, ErrorCode};
use sha2::{Digest, Sha256};

use super::read_chunk;

const BUFFER_TTL: Duration = Duration::from_secs(300);
const MAX_STAGED_INPUTS: usize = 8;
const MAX_BUFFERED_OUTPUTS: usize = 8;
const MAX_BUFFERED_OUTPUT_BYTES: usize = 64 * 1024 * 1024;

static RENDER_ID_SEQ: AtomicU64 = AtomicU64::new(1);

/// Shared render engine and bounded chunk buffers for one office worker.
pub(crate) struct RenderRuntime {
    renderer: Arc<dyn DocumentRenderer>,
    inputs: Mutex<BTreeMap<String, StagedInput>>,
    outputs: Mutex<BTreeMap<String, BufferedPdf>>,
}

struct StagedInput {
    caller: String,
    source: Vec<u8>,
    css: Vec<u8>,
    expires_at: Instant,
}

struct BufferedPdf {
    caller: String,
    bytes: Vec<u8>,
    page_count: u32,
    sha256: String,
    expires_at: Instant,
}

impl RenderRuntime {
    pub(crate) fn production() -> Self {
        Self::with_renderer(Arc::new(BoundedDocumentRenderer::production()))
    }

    pub(crate) fn with_renderer(renderer: Arc<dyn DocumentRenderer>) -> Self {
        Self {
            renderer,
            inputs: Mutex::new(BTreeMap::new()),
            outputs: Mutex::new(BTreeMap::new()),
        }
    }
}

pub(crate) fn default_runtime() -> &'static RenderRuntime {
    static RUNTIME: OnceLock<RenderRuntime> = OnceLock::new();
    RUNTIME.get_or_init(RenderRuntime::production)
}

fn mint_render_uuid(office: &Office) -> String {
    let seq = RENDER_ID_SEQ.fetch_add(1, Ordering::Relaxed);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    let seed = format!("render:{}:{now}:{seq}", office.info().office_id);
    let digest = Sha256::digest(seed.as_bytes());
    let mut b = [0u8; 16];
    b[0..6].copy_from_slice(&now.to_be_bytes()[2..8]);
    b[6..16].copy_from_slice(&digest[0..10]);
    b[6] = 0x70 | (b[6] & 0x0f);
    b[8] = 0x80 | (b[8] & 0x3f);
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Executes `attachment.render` for `caller`.
pub(crate) fn handle_render(
    office: &Arc<Office>,
    runtime: &RenderRuntime,
    caller: &str,
    params: AttachmentRenderParams,
) -> Result<Result<AttachmentRenderResult, ApiError>, OfficeError> {
    if is_output_chunk_read(&params) {
        return handle_output_chunk_read(office, runtime, caller, &params);
    }

    let destination = match resolve_destination(office, caller, &params)? {
        Ok(dest) => dest,
        Err(refusal) => return Ok(Err(refusal)),
    };

    let Some((source, css)) = (match assemble_input(office, runtime, caller, &params)? {
        Ok(InputStep::NeedMore(render_id)) => {
            return Ok(Ok(AttachmentRenderResult {
                render_id: Some(render_id),
                page_count: 0,
                byte_count: 0,
                ..Default::default()
            }));
        }
        Ok(InputStep::Ready(pair)) => Some(pair),
        Err(refusal) => return Ok(Err(refusal)),
    }) else {
        unreachable!()
    };

    let source_kind = match params.source_kind {
        RenderSourceKind::Markdown => SourceKind::Markdown,
        RenderSourceKind::Html => SourceKind::Html,
    };
    let attachments =
        match resolve_authorized_images(office, caller, source_kind, &source, css.as_deref())? {
            Ok(map) => map,
            Err(refusal) => return Ok(Err(refusal)),
        };

    let request = RenderRequest {
        source_kind,
        source,
        css,
        title: params.title.clone(),
        attachments,
    };
    let rendered = match runtime.renderer.render(&request) {
        Ok(pdf) => pdf,
        Err(err) => return Ok(Err(map_render_error(err))),
    };

    match destination {
        RenderDestination::LocalBytes => {
            Ok(Ok(deliver_local_pdf(office, runtime, caller, rendered)))
        }
        RenderDestination::Hosted { kind, target_id } => {
            commit_hosted_pdf(office, caller, &params, kind, target_id, rendered)
        }
    }
}

enum RenderDestination {
    LocalBytes,
    Hosted {
        kind: AttachmentTargetKind,
        target_id: Option<String>,
    },
}

fn is_output_chunk_read(params: &AttachmentRenderParams) -> bool {
    params.return_bytes
        && params.render_id.is_some()
        && params.source.is_empty()
        && params.source_chunk_base64.is_none()
        && params.css_chunk_base64.is_none()
        && params.input_eof.is_none()
}

fn handle_output_chunk_read(
    office: &Arc<Office>,
    runtime: &RenderRuntime,
    caller: &str,
    params: &AttachmentRenderParams,
) -> Result<Result<AttachmentRenderResult, ApiError>, OfficeError> {
    if let Err(refusal) = verify_active_caller(office, caller)? {
        return Ok(Err(refusal));
    }
    let Some(render_id) = params.render_id.as_deref() else {
        return Ok(Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "missing render_id",
        )));
    };
    let offset = params.chunk_offset.unwrap_or(0) as usize;
    let mut guard = runtime
        .outputs
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let now = Instant::now();
    guard.retain(|_, v| v.expires_at > now);
    let Some(entry) = guard.get(render_id) else {
        return Ok(Err(ApiError::new(
            ErrorCode::NotFound,
            "rendered PDF buffer not found or expired",
        )));
    };
    if entry.caller != caller {
        return Ok(Err(ApiError::new(
            ErrorCode::NotFound,
            "rendered PDF buffer not found or expired",
        )));
    }
    if offset > entry.bytes.len() {
        return Ok(Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "chunk_offset is past the end of the rendered PDF",
        )));
    }
    let end = offset
        .saturating_add(ATTACHMENT_CHUNK_MAX_BYTES as usize)
        .min(entry.bytes.len());
    let eof = end >= entry.bytes.len();
    let result = AttachmentRenderResult {
        pdf_base64: Some(STANDARD.encode(&entry.bytes[offset..end])),
        render_id: (!eof).then(|| render_id.to_owned()),
        offset: Some(offset as u64),
        eof: Some(eof),
        page_count: entry.page_count,
        byte_count: entry.bytes.len() as u64,
        content_sha256: Some(entry.sha256.clone()),
        ..Default::default()
    };
    if eof {
        guard.remove(render_id);
    }
    Ok(Ok(result))
}

fn verify_active_caller(
    office: &Office,
    caller: &str,
) -> Result<Result<(), ApiError>, OfficeError> {
    // `attachment_list` on `Organization` runs `Office::attachment_caller` + policy read check
    // without mutating any state, failing closed for unknown or inactive/paused members.
    let probe = AttachmentListParams {
        target_kind: Some(AttachmentTargetKind::Organization),
        target_id: None,
        scope: Some(AttachmentScope::Shared),
        limit: Some(1),
        cursor: None,
    };
    Ok(office.attachment_list(caller, probe)?.map(|_| ()))
}

fn resolve_destination(
    office: &Office,
    caller: &str,
    params: &AttachmentRenderParams,
) -> Result<Result<RenderDestination, ApiError>, OfficeError> {
    let has_hosted =
        params.target_kind.is_some() || params.project_slug.is_some() || params.task_ref.is_some();
    if has_hosted && params.return_bytes {
        return Ok(Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "choose either a hosted target scope or return_bytes, never both",
        )));
    }
    if !has_hosted && !params.return_bytes {
        return Ok(Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "specify either a hosted target scope or return_bytes=true",
        )));
    }
    if let Err(refusal) = verify_active_caller(office, caller)? {
        return Ok(Err(refusal));
    }
    if params.return_bytes {
        return Ok(Ok(RenderDestination::LocalBytes));
    }
    resolve_hosted_destination(office, caller, params)
}

fn resolve_hosted_destination(
    office: &Office,
    caller: &str,
    params: &AttachmentRenderParams,
) -> Result<Result<RenderDestination, ApiError>, OfficeError> {
    let kind = match (
        params.target_kind,
        params.project_slug.as_deref(),
        params.task_ref.as_deref(),
    ) {
        (Some(k), _, _) => k,
        (None, Some(_), None) => AttachmentTargetKind::Project,
        (None, None, Some(_)) => AttachmentTargetKind::Task,
        _ => {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "ambiguous hosted destination: set target_kind or one of project_slug/task_ref",
            )));
        }
    };
    if kind == AttachmentTargetKind::UserPrivate {
        let probe = AttachmentListParams {
            target_kind: Some(AttachmentTargetKind::UserPrivate),
            target_id: None,
            scope: Some(AttachmentScope::UserPrivate),
            limit: Some(1),
            cursor: None,
        };
        if let Err(refusal) = office.attachment_list(caller, probe)? {
            return Ok(Err(refusal));
        }
        return Ok(Ok(RenderDestination::Hosted {
            kind,
            target_id: None,
        }));
    }
    let target_id = match resolve_shared_target_id(office, kind, params)? {
        Ok(id) => id,
        Err(refusal) => return Ok(Err(refusal)),
    };
    Ok(Ok(RenderDestination::Hosted { kind, target_id }))
}

fn resolve_shared_target_id(
    office: &Office,
    kind: AttachmentTargetKind,
    params: &AttachmentRenderParams,
) -> Result<Result<Option<String>, ApiError>, OfficeError> {
    match kind {
        AttachmentTargetKind::Organization => {
            if params.target_id.is_some()
                || params.project_slug.is_some()
                || params.task_ref.is_some()
            {
                return Ok(Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "organization target takes no target_id, project_slug, or task_ref",
                )));
            }
            Ok(Ok(None))
        }
        AttachmentTargetKind::Project => {
            if let Some(id) = params.target_id.as_deref() {
                Ok(Ok(Some(id.to_owned())))
            } else if let Some(slug) = params.project_slug.as_deref() {
                Ok(office.inspect_project(slug)?.map(|p| Some(p.id)))
            } else {
                Ok(Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "project target requires target_id or project_slug",
                )))
            }
        }
        AttachmentTargetKind::Task => {
            if let Some(id) = params.target_id.as_deref() {
                Ok(Ok(Some(id.to_owned())))
            } else if let Some(task_ref) = params.task_ref.as_deref() {
                Ok(office.task_inspect(task_ref)?.map(|r| Some(r.task.id)))
            } else {
                Ok(Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "task target requires target_id or task_ref",
                )))
            }
        }
        other => Ok(Err(ApiError::new(
            ErrorCode::Unsupported,
            format!(
                "attachment.render supports organization, project, task, or user_private targets (got `{}`)",
                other.as_str()
            ),
        ))),
    }
}

enum InputStep {
    NeedMore(String),
    Ready((String, Option<String>)),
}

fn assemble_input(
    office: &Office,
    runtime: &RenderRuntime,
    caller: &str,
    params: &AttachmentRenderParams,
) -> Result<Result<InputStep, ApiError>, OfficeError> {
    let chunked = params.source_chunk_base64.is_some()
        || params.css_chunk_base64.is_some()
        || params.input_eof.is_some();
    if !chunked {
        return Ok(Ok(InputStep::Ready((
            params.source.clone(),
            params.css.clone(),
        ))));
    }
    let src_chunk = match decode_chunk_field(params.source_chunk_base64.as_deref()) {
        Ok(b) => b,
        Err(refusal) => return Ok(Err(refusal)),
    };
    let css_chunk = match decode_chunk_field(params.css_chunk_base64.as_deref()) {
        Ok(b) => b,
        Err(refusal) => return Ok(Err(refusal)),
    };

    let mut guard = runtime
        .inputs
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let now = Instant::now();
    guard.retain(|_, v| v.expires_at > now);

    let id = match params.render_id.as_deref() {
        Some(existing) => existing.to_owned(),
        None => {
            if guard.len() >= MAX_STAGED_INPUTS {
                return Ok(Err(ApiError::new(
                    ErrorCode::Unavailable,
                    "too many concurrent staged render inputs",
                )));
            }
            let fresh = mint_render_uuid(office);
            guard.insert(
                fresh.clone(),
                StagedInput {
                    caller: caller.to_owned(),
                    source: Vec::new(),
                    css: Vec::new(),
                    expires_at: now + BUFFER_TTL,
                },
            );
            fresh
        }
    };

    let Some(entry) = guard.get_mut(&id) else {
        return Ok(Err(ApiError::new(
            ErrorCode::NotFound,
            "staged render input not found or expired",
        )));
    };
    if entry.caller != caller {
        return Ok(Err(ApiError::new(
            ErrorCode::NotFound,
            "staged render input not found or expired",
        )));
    }
    let next_total = entry
        .source
        .len()
        .saturating_add(entry.css.len())
        .saturating_add(src_chunk.len())
        .saturating_add(css_chunk.len());
    if next_total > MAX_SOURCE_BYTES {
        guard.remove(&id);
        return Ok(Err(ApiError::new(
            ErrorCode::TooLarge,
            format!("render input exceeds {MAX_SOURCE_BYTES} bytes"),
        )));
    }
    entry.source.extend_from_slice(&src_chunk);
    entry.css.extend_from_slice(&css_chunk);
    entry.expires_at = now + BUFFER_TTL;

    if params.input_eof != Some(true) {
        return Ok(Ok(InputStep::NeedMore(id)));
    }
    let finished = guard.remove(&id).expect("staged entry present");
    let Ok(source) = String::from_utf8(finished.source) else {
        return Ok(Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "render source must be valid UTF-8",
        )));
    };
    let css = if finished.css.is_empty() {
        None
    } else {
        match String::from_utf8(finished.css) {
            Ok(c) => Some(c),
            Err(_) => {
                return Ok(Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "render CSS must be valid UTF-8",
                )));
            }
        }
    };
    Ok(Ok(InputStep::Ready((source, css))))
}

fn decode_chunk_field(raw: Option<&str>) -> Result<Vec<u8>, ApiError> {
    let Some(text) = raw else {
        return Ok(Vec::new());
    };
    if text.len() > ATTACHMENT_CHUNK_MAX_BASE64_CHARS {
        return Err(ApiError::new(
            ErrorCode::TooLarge,
            "chunk exceeds 512 KiB limit",
        ));
    }
    STANDARD
        .decode(text.as_bytes())
        .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "chunk is not valid base64"))
}

fn resolve_authorized_images(
    office: &Arc<Office>,
    caller: &str,
    source_kind: SourceKind,
    source: &str,
    css: Option<&str>,
) -> Result<Result<BTreeMap<String, ResolvedImage>, ApiError>, OfficeError> {
    let refs = extract_attachment_refs(source_kind, source, css);
    let mut map = BTreeMap::new();
    for att_id in refs {
        let inspected = match office.attachment_inspect(
            caller,
            AttachmentInspectParams {
                attachment_id: att_id.clone(),
            },
        )? {
            Ok(res) => res.attachment,
            Err(_) => continue, // Fail closed inside BoundedDocumentRenderer as ForbiddenResourceUrl.
        };
        if !is_image_mime(&inspected.mime_type)
            || inspected.byte_count == 0
            || inspected.byte_count > MAX_IMAGE_DECODED_BYTES as u64
        {
            continue;
        }
        let mut bytes = Vec::with_capacity(inspected.byte_count as usize);
        let mut ok = true;
        loop {
            let chunk = match read_chunk(
                office,
                caller,
                AttachmentReadChunkParams {
                    attachment_id: att_id.clone(),
                    offset: bytes.len() as u64,
                    max_bytes: Some(ATTACHMENT_CHUNK_MAX_BYTES),
                },
            )? {
                Ok(c) => c,
                Err(_) => {
                    ok = false;
                    break;
                }
            };
            let Ok(decoded) = STANDARD.decode(chunk.data_base64.as_bytes()) else {
                ok = false;
                break;
            };
            bytes.extend_from_slice(&decoded);
            if bytes.len() > MAX_IMAGE_DECODED_BYTES {
                ok = false;
                break;
            }
            if chunk.eof {
                break;
            }
        }
        if ok && bytes.len() as u64 == inspected.byte_count {
            map.insert(
                att_id,
                ResolvedImage {
                    mime_type: inspected.mime_type,
                    bytes,
                },
            );
        }
    }
    Ok(Ok(map))
}

fn deliver_local_pdf(
    office: &Office,
    runtime: &RenderRuntime,
    caller: &str,
    rendered: org_doc_render::RenderedPdf,
) -> AttachmentRenderResult {
    let sha256 = sha256_hex(&rendered.bytes);
    let chunk_max = ATTACHMENT_CHUNK_MAX_BYTES as usize;
    if rendered.bytes.len() <= chunk_max {
        return AttachmentRenderResult {
            pdf_base64: Some(STANDARD.encode(&rendered.bytes)),
            offset: Some(0),
            eof: Some(true),
            page_count: rendered.page_count,
            byte_count: rendered.byte_count,
            content_sha256: Some(sha256),
            ..Default::default()
        };
    }
    let first_chunk = STANDARD.encode(&rendered.bytes[..chunk_max]);
    let render_id = mint_render_uuid(office);
    let mut guard = runtime
        .outputs
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let now = Instant::now();
    guard.retain(|_, v| v.expires_at > now);
    while guard.len() >= MAX_BUFFERED_OUTPUTS
        || guard.values().map(|v| v.bytes.len()).sum::<usize>() + rendered.bytes.len()
            > MAX_BUFFERED_OUTPUT_BYTES
    {
        let Some(oldest) = guard
            .iter()
            .min_by_key(|(_, v)| v.expires_at)
            .map(|(k, _)| k.clone())
        else {
            break;
        };
        guard.remove(&oldest);
    }
    guard.insert(
        render_id.clone(),
        BufferedPdf {
            caller: caller.to_owned(),
            bytes: rendered.bytes,
            page_count: rendered.page_count,
            sha256: sha256.clone(),
            expires_at: now + BUFFER_TTL,
        },
    );
    AttachmentRenderResult {
        pdf_base64: Some(first_chunk),
        render_id: Some(render_id),
        offset: Some(0),
        eof: Some(false),
        page_count: rendered.page_count,
        byte_count: rendered.byte_count,
        content_sha256: Some(sha256),
        ..Default::default()
    }
}

fn commit_hosted_pdf(
    office: &Office,
    caller: &str,
    params: &AttachmentRenderParams,
    kind: AttachmentTargetKind,
    target_id: Option<String>,
    rendered: org_doc_render::RenderedPdf,
) -> Result<Result<AttachmentRenderResult, ApiError>, OfficeError> {
    let filename = normalize_pdf_filename(params.filename.as_deref());
    let begun = match office.attachment_upload_begin(
        caller,
        AttachmentUploadBeginParams {
            filename,
            mime_type: "application/pdf".to_owned(),
            byte_count: rendered.byte_count,
        },
    )? {
        Ok(res) => res,
        Err(refusal) => return Ok(Err(refusal)),
    };
    let upload_id = begun.upload.upload_id;
    let chunk_size = begun.chunk_size_bytes.clamp(1, ATTACHMENT_CHUNK_MAX_BYTES) as usize;
    let mut offset = 0u64;
    for chunk in rendered.bytes.chunks(chunk_size) {
        let res = office.attachment_upload_chunk(
            caller,
            AttachmentUploadChunkParams {
                upload_id: upload_id.clone(),
                offset,
                data_base64: STANDARD.encode(chunk),
            },
        )?;
        if let Err(refusal) = res {
            abort_staged(office, caller, &upload_id);
            return Ok(Err(refusal));
        }
        offset = offset.saturating_add(chunk.len() as u64);
    }
    if let Err(refusal) = office.attachment_upload_commit(
        caller,
        AttachmentUploadRef {
            upload_id: upload_id.clone(),
        },
    )? {
        abort_staged(office, caller, &upload_id);
        return Ok(Err(refusal));
    }
    let command_id = params
        .command_id
        .clone()
        .unwrap_or_else(|| mint_render_uuid(office));
    let attached = match office.attachment_attach(
        caller,
        AttachmentAttachParams {
            command_id,
            upload_id: Some(upload_id.clone()),
            source_attachment_id: None,
            target_kind: kind,
            target_id,
            title: params.title.clone(),
            description: params.description.clone(),
            artifact_status: params.artifact_status,
        },
    )? {
        Ok(res) => res.attachment,
        Err(refusal) => {
            abort_staged(office, caller, &upload_id);
            return Ok(Err(refusal));
        }
    };
    let canonical_url = format!(
        "/api/v1/organizations/{}/attachments/{}",
        office.info().org_id,
        attached.id
    );
    Ok(Ok(AttachmentRenderResult {
        attachment_id: Some(attached.id.clone()),
        canonical_url: Some(canonical_url),
        page_count: attached.page_count.unwrap_or(rendered.page_count),
        byte_count: attached.byte_count,
        content_sha256: Some(attached.content_sha256.clone()),
        attachment: Some(attached),
        ..Default::default()
    }))
}

fn abort_staged(office: &Office, caller: &str, upload_id: &str) {
    let _ = office.attachment_upload_abort(
        caller,
        AttachmentUploadRef {
            upload_id: upload_id.to_owned(),
        },
    );
}

fn normalize_pdf_filename(raw: Option<&str>) -> String {
    let candidate = raw
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("document.pdf");
    let leaf = candidate
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("document.pdf")
        .trim_start_matches('.');
    let mut clean: String = leaf
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '/' | '\\'))
        .collect();
    if clean.is_empty() {
        clean = "document.pdf".to_owned();
    }
    if !clean.to_ascii_lowercase().ends_with(".pdf") {
        let stem = clean
            .rsplit_once('.')
            .map(|(s, _)| s)
            .filter(|s| !s.is_empty())
            .unwrap_or(&clean);
        clean = format!("{stem}.pdf");
    }
    if clean.len() > ATTACHMENT_FILENAME_MAX_BYTES {
        let keep = ATTACHMENT_FILENAME_MAX_BYTES.saturating_sub(4);
        let cut = clean.floor_char_boundary(keep);
        clean = format!("{}.pdf", &clean[..cut]);
    }
    clean
}

fn map_render_error(err: RenderError) -> ApiError {
    let code = match &err {
        RenderError::InputTooLarge { .. }
        | RenderError::OutputTooLarge { .. }
        | RenderError::PageLimitExceeded { .. } => ErrorCode::TooLarge,
        RenderError::ForbiddenResourceUrl { .. } | RenderError::ForbiddenContent { .. } => {
            ErrorCode::NotAuthorized
        }
        RenderError::Timeout { .. } | RenderError::Busy => ErrorCode::Unavailable,
        RenderError::EmptySource | RenderError::InvalidPdf(_) | RenderError::RenderFailed(_) => {
            ErrorCode::InvalidRequest
        }
    };
    ApiError::new(code, err.to_string())
}
