//! Thin `attachment.render` RPC bridge over `org-doc-render` and `Office::attachment_*`
//! (`TASK-98.7`).
//!
//! All rendering, sanitization, semaphore bounding, timeouts, and page counting live inside
//! `org-doc-render`; this module handles caller/target pre-authorization, Chief-of-Staff
//! `caller ∩ tess` post-assembly attachment checks, `attachment:<id>` image loading via
//! `AuthorizationPolicy::can_read`, and hosted commit through the existing `Office` pipeline.

mod hosted;
mod staging;

use std::collections::BTreeMap;
use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_doc_render::{
    extract_attachment_refs, RenderError, RenderRequest, ResolvedImage, SourceKind,
    MAX_IMAGE_DECODED_BYTES,
};
use org_office::{Office, OfficeError};
use org_proto::attachments::{
    is_image_mime, AttachmentInspectParams, AttachmentListParams, AttachmentReadChunkParams,
    AttachmentRenderParams, AttachmentRenderResult, AttachmentScope, AttachmentTargetKind,
    RenderSourceKind, ATTACHMENT_CHUNK_MAX_BYTES,
};
use org_proto::{ApiError, ErrorCode};

use super::read_chunk;
use hosted::commit_hosted_pdf;
use staging::{
    assemble_input, deliver_local_pdf, handle_output_chunk_read, is_output_chunk_read, InputStep,
    RenderCallerKey,
};

pub(crate) use staging::{default_runtime, RenderRuntime};

/// Chief-of-Staff delegation context enforced inside `attachment.render` (`TASK-240`).
///
/// Keyed into staged input and buffered output ownership (`(member_id, delegated_for)`) and
/// invoked post-assembly on every `attachment:<id>` reference before loading image bytes.
pub(crate) struct RenderDelegation<'a> {
    /// `Some(caller_id)` when Tess acts for a human caller; `None` during a wake.
    pub(crate) caller_id: Option<&'a str>,
    /// Single choke-point enforcing `caller ∩ tess` for one referenced `attachment:<id>`.
    pub(crate) check_attachment: &'a dyn Fn(&str) -> Result<(), ApiError>,
}

/// Executes `attachment.render` for `caller`, enforcing `delegation` when active.
pub(crate) fn handle_render(
    office: &Arc<Office>,
    runtime: &RenderRuntime,
    caller: &str,
    delegation: Option<RenderDelegation<'_>>,
    params: AttachmentRenderParams,
) -> Result<Result<AttachmentRenderResult, ApiError>, OfficeError> {
    let caller_key = RenderCallerKey::from_context(caller, delegation.as_ref());
    if is_output_chunk_read(&params) {
        return handle_output_chunk_read(office, runtime, caller, &caller_key, &params);
    }

    let destination = match resolve_destination(office, caller, &params)? {
        Ok(dest) => dest,
        Err(refusal) => return Ok(Err(refusal)),
    };

    let (source, css) = match assemble_input(runtime, &caller_key, &params) {
        Ok(InputStep::NeedMore(render_id)) => {
            return Ok(Ok(AttachmentRenderResult {
                render_id: Some(render_id),
                ..Default::default()
            }));
        }
        Ok(InputStep::Ready(pair)) => pair,
        Err(refusal) => return Ok(Err(refusal)),
    };

    let source_kind = match params.source_kind {
        RenderSourceKind::Markdown => SourceKind::Markdown,
        RenderSourceKind::Html => SourceKind::Html,
    };
    let refs = extract_attachment_refs(source_kind, &source, css.as_deref());
    if let Some(del) = delegation.as_ref() {
        for att_id in &refs {
            if let Err(refusal) = (del.check_attachment)(att_id) {
                return Ok(Err(refusal));
            }
        }
    }
    let attachments = resolve_authorized_images(office, caller, &refs)?;
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
        RenderDestination::LocalBytes => Ok(Ok(deliver_local_pdf(runtime, &caller_key, rendered))),
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

fn probe_scope(
    office: &Office,
    caller: &str,
    target_kind: AttachmentTargetKind,
    scope: AttachmentScope,
) -> Result<Result<(), ApiError>, OfficeError> {
    let probe = AttachmentListParams {
        target_kind: Some(target_kind),
        target_id: None,
        scope: Some(scope),
        limit: Some(1),
        cursor: None,
    };
    Ok(office.attachment_list(caller, probe)?.map(|_| ()))
}

pub(super) fn verify_active_caller(
    office: &Office,
    caller: &str,
) -> Result<Result<(), ApiError>, OfficeError> {
    probe_scope(
        office,
        caller,
        AttachmentTargetKind::Organization,
        AttachmentScope::Shared,
    )
}

fn resolve_destination(
    office: &Office,
    caller: &str,
    params: &AttachmentRenderParams,
) -> Result<Result<RenderDestination, ApiError>, OfficeError> {
    let has_hosted =
        params.target_kind.is_some() || params.project_slug.is_some() || params.task_ref.is_some();
    if has_hosted == params.return_bytes {
        let msg = if has_hosted {
            "choose either a hosted target scope or return_bytes, never both"
        } else {
            "specify either a hosted target scope or return_bytes=true"
        };
        return Ok(Err(ApiError::new(ErrorCode::InvalidRequest, msg)));
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
        if let Err(refusal) = probe_scope(office, caller, kind, AttachmentScope::UserPrivate)? {
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

fn resolve_authorized_images(
    office: &Arc<Office>,
    caller: &str,
    refs: &[String],
) -> Result<BTreeMap<String, ResolvedImage>, OfficeError> {
    let mut map = BTreeMap::new();
    for att_id in refs {
        if let Some(image) = load_one_authorized_image(office, caller, att_id)? {
            map.insert(att_id.clone(), image);
        }
    }
    Ok(map)
}

fn load_one_authorized_image(
    office: &Arc<Office>,
    caller: &str,
    att_id: &str,
) -> Result<Option<ResolvedImage>, OfficeError> {
    let Ok(res) = office.attachment_inspect(
        caller,
        AttachmentInspectParams {
            attachment_id: att_id.to_owned(),
        },
    )?
    else {
        return Ok(None); // Fail closed inside BoundedDocumentRenderer as ForbiddenResourceUrl.
    };
    let inspected = res.attachment;
    if !is_image_mime(&inspected.mime_type)
        || inspected.byte_count == 0
        || inspected.byte_count > MAX_IMAGE_DECODED_BYTES as u64
    {
        return Ok(None);
    }
    let Ok(expected_len) = usize::try_from(inspected.byte_count) else {
        return Ok(None);
    };
    let Some(bytes) = read_attachment_image_bytes(office, caller, att_id, expected_len)? else {
        return Ok(None);
    };
    Ok(Some(ResolvedImage {
        mime_type: inspected.mime_type,
        bytes,
    }))
}

fn read_attachment_image_bytes(
    office: &Arc<Office>,
    caller: &str,
    att_id: &str,
    expected_len: usize,
) -> Result<Option<Vec<u8>>, OfficeError> {
    let mut bytes = Vec::with_capacity(expected_len);
    loop {
        let Ok(chunk) = read_chunk(
            office,
            caller,
            AttachmentReadChunkParams {
                attachment_id: att_id.to_owned(),
                offset: bytes.len() as u64,
                max_bytes: Some(ATTACHMENT_CHUNK_MAX_BYTES),
            },
        )?
        else {
            return Ok(None);
        };
        let Ok(decoded) = STANDARD.decode(chunk.data_base64.as_bytes()) else {
            return Ok(None);
        };
        if decoded.is_empty() && !chunk.eof {
            return Ok(None);
        }
        bytes.extend_from_slice(&decoded);
        if bytes.len() > MAX_IMAGE_DECODED_BYTES {
            return Ok(None);
        }
        if chunk.eof {
            return Ok((bytes.len() == expected_len).then_some(bytes));
        }
    }
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
