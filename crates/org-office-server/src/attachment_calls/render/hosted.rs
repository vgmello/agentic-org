//! Hosted PDF commit pipeline (`attachment.upload_*` + `attachment.attach`) and PDF filename
//! normalization for `attachment.render` (`TASK-98.7`).

use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_office::{Office, OfficeError};
use org_proto::attachments::{
    AttachmentAttachParams, AttachmentRenderParams, AttachmentRenderResult, AttachmentTargetKind,
    AttachmentUploadBeginParams, AttachmentUploadChunkParams, AttachmentUploadRef,
    ATTACHMENT_CHUNK_MAX_BYTES, ATTACHMENT_FILENAME_MAX_BYTES,
};
use org_proto::ApiError;

use super::staging::mint_render_uuid;

pub(super) fn commit_hosted_pdf(
    office: &Office,
    caller: &str,
    params: &AttachmentRenderParams,
    kind: AttachmentTargetKind,
    target_id: Option<String>,
    rendered: org_doc_render::RenderedPdf,
) -> Result<Result<AttachmentRenderResult, ApiError>, OfficeError> {
    let begun = match office.attachment_upload_begin(
        caller,
        AttachmentUploadBeginParams {
            filename: normalize_pdf_filename(params.filename.as_deref()),
            mime_type: "application/pdf".to_owned(),
            byte_count: rendered.byte_count,
        },
    )? {
        Ok(res) => res,
        Err(refusal) => return Ok(Err(refusal)),
    };
    let upload_id = begun.upload.upload_id;
    let chunk_size = begun.chunk_size_bytes.clamp(1, ATTACHMENT_CHUNK_MAX_BYTES) as usize;
    if let Err(refusal) =
        upload_pdf_chunks(office, caller, &upload_id, chunk_size, &rendered.bytes)?
    {
        abort_staged(office, caller, &upload_id);
        return Ok(Err(refusal));
    }
    let commit_ref = AttachmentUploadRef {
        upload_id: upload_id.clone(),
    };
    if let Err(refusal) = office.attachment_upload_commit(caller, commit_ref)? {
        abort_staged(office, caller, &upload_id);
        return Ok(Err(refusal));
    }
    let attached = match office.attachment_attach(
        caller,
        AttachmentAttachParams {
            command_id: params.command_id.clone().unwrap_or_else(mint_render_uuid),
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

fn upload_pdf_chunks(
    office: &Office,
    caller: &str,
    upload_id: &str,
    chunk_size: usize,
    bytes: &[u8],
) -> Result<Result<(), ApiError>, OfficeError> {
    let mut offset = 0u64;
    for chunk in bytes.chunks(chunk_size) {
        let res = office.attachment_upload_chunk(
            caller,
            AttachmentUploadChunkParams {
                upload_id: upload_id.to_owned(),
                offset,
                data_base64: STANDARD.encode(chunk),
            },
        )?;
        if let Err(refusal) = res {
            return Ok(Err(refusal));
        }
        offset = offset.saturating_add(chunk.len() as u64);
    }
    Ok(Ok(()))
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
    let leaf = raw
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("document.pdf")
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
        let cut = clean.floor_char_boundary(ATTACHMENT_FILENAME_MAX_BYTES.saturating_sub(4));
        clean = format!("{}.pdf", &clean[..cut]);
    }
    clean
}
