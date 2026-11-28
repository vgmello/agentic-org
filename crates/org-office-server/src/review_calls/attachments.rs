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
    AttachmentView,
};
use org_proto::reviews::{ReviewView, REVIEW_MAX_ATTACHMENTS};
use org_proto::{ApiError, ErrorCode};
use sha2::{Digest, Sha256};

use super::scope::{caller_allowed_teams, is_project_visible};

enum ClassifiedAttachment {
    Existing(String),
    Staged(String),
}

/// Validates and resolves `attachment_ids` for `caller` on `review`, returning canonical bound
/// shared attachment IDs. Every existing attachment across the slice is validated BEFORE any
/// staged upload is bound.
pub(super) fn resolve_review_attachments(
    office: &Office,
    caller: &str,
    command_id: &str,
    review: &ReviewView,
    raw_ids: &[String],
) -> Result<Vec<String>, ApiError> {
    let mut batches =
        resolve_review_attachment_batches(office, caller, command_id, review, &[raw_ids])?;
    Ok(batches.pop().unwrap_or_default())
}

/// Two-phase batch resolver for verdict + findings attachments: validates ALL slices and existing
/// attachment ACL/scope rules BEFORE binding ANY staged upload.
pub(super) fn resolve_review_attachment_batches(
    office: &Office,
    caller: &str,
    command_id: &str,
    review: &ReviewView,
    batches: &[&[String]],
) -> Result<Vec<Vec<String>>, ApiError> {
    if batches.iter().all(|b| b.is_empty()) {
        return Ok(batches.iter().map(|_| Vec::new()).collect());
    }
    let allowed_teams = caller_allowed_teams(office, caller)?;
    let mut classified_batches = Vec::with_capacity(batches.len());
    for raw_ids in batches {
        if raw_ids.len() > REVIEW_MAX_ATTACHMENTS {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                format!("at most {REVIEW_MAX_ATTACHMENTS} attachments are allowed"),
            ));
        }
        let mut seen = BTreeSet::new();
        let mut classified = Vec::with_capacity(raw_ids.len());
        for raw in *raw_ids {
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
            classified.push(classify_one_attachment(
                office,
                caller,
                allowed_teams.as_ref(),
                review,
                trimmed,
            )?);
        }
        classified_batches.push(classified);
    }

    let mut out = Vec::with_capacity(classified_batches.len());
    for batch in classified_batches {
        let mut resolved = Vec::with_capacity(batch.len());
        for item in batch {
            match item {
                ClassifiedAttachment::Existing(id) => resolved.push(id),
                ClassifiedAttachment::Staged(upload_id) => {
                    resolved.push(bind_staged_upload(
                        office, caller, command_id, review, &upload_id,
                    )?);
                }
            }
        }
        out.push(resolved);
    }
    Ok(out)
}

fn classify_one_attachment(
    office: &Office,
    caller: &str,
    allowed_teams: Option<&BTreeSet<String>>,
    review: &ReviewView,
    id: &str,
) -> Result<ClassifiedAttachment, ApiError> {
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
            verify_existing_attachment_scope(office, allowed_teams, review, &res.attachment)?;
            Ok(ClassifiedAttachment::Existing(res.attachment.id))
        }
        Err(refusal) if refusal.code == ErrorCode::NotFound => {
            Ok(ClassifiedAttachment::Staged(id.to_owned()))
        }
        Err(refusal) => Err(refusal),
    }
}

fn verify_existing_attachment_scope(
    office: &Office,
    allowed_teams: Option<&BTreeSet<String>>,
    review: &ReviewView,
    att: &AttachmentView,
) -> Result<(), ApiError> {
    if att.scope != AttachmentScope::Shared {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "private attachments cannot be linked to shared reviews",
        ));
    }
    match att.target_kind {
        AttachmentTargetKind::Organization => {
            if att.target_id.is_some() {
                return Err(ApiError::new(ErrorCode::NotFound, "attachment not found"));
            }
            Ok(())
        }
        AttachmentTargetKind::Project => {
            let Some(att_pid) = att.target_id.as_deref() else {
                return Err(ApiError::new(ErrorCode::NotFound, "attachment not found"));
            };
            verify_attachment_project_scope(office, allowed_teams, review, att_pid)
        }
        AttachmentTargetKind::Task => {
            let Some(tid) = att.target_id.as_deref() else {
                return Err(ApiError::new(ErrorCode::NotFound, "attachment not found"));
            };
            let task = office
                .task_inspect(tid)
                .map_err(|_| ApiError::new(ErrorCode::StorageError, "the office could not answer"))?
                .map_err(|_| ApiError::new(ErrorCode::NotFound, "attachment not found"))?;
            if let Some(att_pid) = task.task.project_id.as_deref() {
                verify_attachment_project_scope(office, allowed_teams, review, att_pid)?;
            }
            Ok(())
        }
        AttachmentTargetKind::TaskComment => {
            let Some(cid) = att.target_id.as_deref() else {
                return Err(ApiError::new(ErrorCode::NotFound, "attachment not found"));
            };
            let Some(rtid) = review.task_id.as_deref() else {
                return Err(ApiError::new(
                    ErrorCode::NotAuthorized,
                    "attachment belongs to a different project scope than this review",
                ));
            };
            let comments = office
                .task_comments(rtid)
                .map_err(|_| ApiError::new(ErrorCode::StorageError, "the office could not answer"))?
                .map_err(|_| ApiError::new(ErrorCode::NotFound, "attachment not found"))?;
            if comments.comments.iter().any(|c| c.id == cid) {
                Ok(())
            } else {
                Err(ApiError::new(
                    ErrorCode::NotAuthorized,
                    "attachment belongs to a different project scope than this review",
                ))
            }
        }
        AttachmentTargetKind::Message | AttachmentTargetKind::AskAnswer => Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "conversation attachments cannot be linked to shared reviews",
        )),
        _ => Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "only shared organization, project, or task attachments may be linked to reviews",
        )),
    }
}

fn verify_attachment_project_scope(
    office: &Office,
    allowed_teams: Option<&BTreeSet<String>>,
    review: &ReviewView,
    att_pid: &str,
) -> Result<(), ApiError> {
    let proj = office
        .inspect_project(att_pid)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "the office could not answer"))?
        .map_err(|_| ApiError::new(ErrorCode::NotFound, "attachment not found"))?;
    if !is_project_visible(&proj, allowed_teams) {
        return Err(ApiError::new(ErrorCode::NotFound, "attachment not found"));
    }
    if review.project_id.as_deref() != Some(att_pid) {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "attachment belongs to a different project scope than this review",
        ));
    }
    Ok(())
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
