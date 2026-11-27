//! Shared-attachment validation and idempotent staged-upload binding for reviews (`TASK-92.1`).
//!
//! Review findings, replies, and verdicts may reference either:
//! - an already-bound `shared` attachment that the caller is authorized to read (never
//!   `user_private` — ADR-0031), OR
//! - a committed staged upload belonging to the caller (`--attach`), bound idempotently onto the
//!   review's owning `task`, `project`, or `organization` using a deterministic UUIDv7 command ID.

use std::collections::BTreeSet;

use org_office::Office;
use org_proto::attachments::{
    AttachmentAttachParams, AttachmentInspectParams, AttachmentScope, AttachmentTargetKind,
};
use org_proto::reviews::{ReviewView, REVIEW_MAX_ATTACHMENTS};
use org_proto::{ApiError, ErrorCode};
use sha2::{Digest, Sha256};

/// Validates and resolves `attachment_ids` for `caller` on `review`, returning canonical bound
/// shared attachment IDs.
pub(super) fn resolve_review_attachments(
    office: &Office,
    caller: &str,
    command_id: &str,
    review: &ReviewView,
    raw_ids: &[String],
) -> Result<Vec<String>, ApiError> {
    if raw_ids.is_empty() {
        return Ok(Vec::new());
    }
    if raw_ids.len() > REVIEW_MAX_ATTACHMENTS {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!("at most {REVIEW_MAX_ATTACHMENTS} attachments are allowed"),
        ));
    }
    let mut seen = BTreeSet::new();
    let mut resolved = Vec::with_capacity(raw_ids.len());
    for raw in raw_ids {
        let trimmed = raw.trim();
        if !org_proto::is_uuid_v7(trimmed) {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "attachment_ids must contain canonical lowercase UUIDv7 values",
            ));
        }
        if !seen.insert(trimmed.to_owned()) {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "attachment_ids must not contain duplicates",
            ));
        }
        let bound_id = resolve_one_attachment(office, caller, command_id, review, trimmed)?;
        resolved.push(bound_id);
    }
    Ok(resolved)
}

fn resolve_one_attachment(
    office: &Office,
    caller: &str,
    command_id: &str,
    review: &ReviewView,
    id: &str,
) -> Result<String, ApiError> {
    match office
        .attachment_inspect(
            caller,
            AttachmentInspectParams {
                attachment_id: id.to_owned(),
            },
        )
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "the office could not answer"))?
    {
        Ok(res) => {
            if res.attachment.scope != AttachmentScope::Shared {
                return Err(ApiError::new(
                    ErrorCode::NotAuthorized,
                    "private attachments cannot be linked to shared reviews",
                ));
            }
            Ok(res.attachment.id)
        }
        Err(refusal) if refusal.code == ErrorCode::NotFound => {
            bind_staged_upload(office, caller, command_id, review, id)
        }
        Err(refusal) => Err(refusal),
    }
}

fn bind_staged_upload(
    office: &Office,
    caller: &str,
    command_id: &str,
    review: &ReviewView,
    upload_id: &str,
) -> Result<String, ApiError> {
    let (target_kind, target_id) = if let Some(tid) = review.task_id.clone() {
        (AttachmentTargetKind::Task, Some(tid))
    } else if let Some(pid) = review.project_id.clone() {
        (AttachmentTargetKind::Project, Some(pid))
    } else {
        (AttachmentTargetKind::Organization, None)
    };
    let params = AttachmentAttachParams {
        command_id: derive_attach_command_id(command_id, upload_id),
        upload_id: Some(upload_id.to_owned()),
        source_attachment_id: None,
        target_kind,
        target_id,
        title: None,
        description: None,
        artifact_status: None,
    };
    let bound = office
        .attachment_attach(caller, params)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "the office could not answer"))??;
    Ok(bound.attachment.id)
}

/// Derives a deterministic canonical lowercase UUIDv7 for idempotent staged-upload binding.
fn derive_attach_command_id(command_id: &str, upload_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"review-attach-v1:");
    hasher.update(command_id.as_bytes());
    hasher.update(b":");
    hasher.update(upload_id.as_bytes());
    let digest = hasher.finalize();
    let mut b = [0u8; 16];
    b.copy_from_slice(&digest[..16]);
    // Keep version 7 + RFC 4122 variant bits with a non-zero timestamp prefix.
    if b[0..6] == [0; 6] {
        b[5] = 1;
    }
    b[6] = (b[6] & 0x0f) | 0x70;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}
