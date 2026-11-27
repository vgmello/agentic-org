//! Files on chat messages, structured asks and ask answers (`TASK-98.2`, ADR-0037).
//!
//! A message, an ask or an answer names up to [`MESSAGE_ATTACHMENTS_MAX`] of the caller's
//! committed staged uploads in `attachment_ids`. The office binds each to the new record under
//! a fresh attachment ID, and every member of the conversation then sees it as an
//! [`AttachmentMeta`]. A message may carry files and no text; it may not carry neither.

use serde::{Deserialize, Serialize};

use super::ArtifactStatus;
use crate::{ApiError, ErrorCode};

/// Most files one message, ask or answer carries.
pub const MESSAGE_ATTACHMENTS_MAX: usize = 10;
/// Most files one task carries (`TASK-98.4`).
pub const TASK_ATTACHMENTS_MAX: usize = 20;
/// Most files one task comment carries (`TASK-98.4`).
pub const TASK_COMMENT_ATTACHMENTS_MAX: usize = 10;

/// One file bound to a message, an ask or an ask answer, as a member of its conversation sees
/// it. Read the bytes with `attachment.read_chunk` and its `id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentMeta {
    pub id: String,
    pub filename: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub artifact_status: ArtifactStatus,
    pub mime_type: String,
    pub byte_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_count: Option<u32>,
    pub uploaded_by_employee_id: String,
    pub created_at: String,
}

impl AttachmentMeta {
    /// Whether the file is an image (`has:image`); anything else is a document (`has:file`).
    pub fn is_image(&self) -> bool {
        is_image_mime(&self.mime_type)
    }
}

/// Whether a stored MIME type names an image.
pub fn is_image_mime(mime: &str) -> bool {
    mime.starts_with("image/")
}

/// The `has:image` / `has:file` filter of a message or inbox read. `has_image: Some(true)`
/// keeps what carries at least one image, `Some(false)` what carries none; `has_file` does the
/// same for documents (every attachment that is not an image). `None` does not filter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AttachmentFilter {
    pub has_image: Option<bool>,
    pub has_file: Option<bool>,
}

impl AttachmentFilter {
    pub fn new(has_image: Option<bool>, has_file: Option<bool>) -> Self {
        Self {
            has_image,
            has_file,
        }
    }

    /// Whether this filter keeps everything.
    pub fn is_none(&self) -> bool {
        self.has_image.is_none() && self.has_file.is_none()
    }

    /// Whether a record whose files have these MIME types passes.
    pub fn admits<'a>(&self, mimes: impl IntoIterator<Item = &'a str>) -> bool {
        let (mut image, mut file) = (false, false);
        for mime in mimes {
            if is_image_mime(mime) {
                image = true;
            } else {
                file = true;
            }
        }
        self.has_image.is_none_or(|want| want == image)
            && self.has_file.is_none_or(|want| want == file)
    }
}

/// Checks `ids` against `max`: each a canonical lowercase UUIDv7, none named twice.
pub fn check_attachment_ids_limit(ids: &[String], max: usize, owner: &str) -> Result<(), ApiError> {
    if ids.len() > max {
        return Err(ApiError::new(
            ErrorCode::TooLarge,
            format!("{owner} carries at most {max} attachments"),
        ));
    }
    for (index, id) in ids.iter().enumerate() {
        if !crate::is_uuid_v7(id) {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "attachment_ids are canonical lowercase UUIDv7 IDs",
            ));
        }
        if ids[..index].contains(id) {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "attachment_ids names an ID twice",
            ));
        }
    }
    Ok(())
}

/// Checks the `attachment_ids` of a message, ask or answer: at most
/// [`MESSAGE_ATTACHMENTS_MAX`], each a canonical lowercase UUIDv7, none named twice.
pub fn check_attachment_ids(ids: &[String]) -> Result<(), ApiError> {
    check_attachment_ids_limit(ids, MESSAGE_ATTACHMENTS_MAX, "a message, ask or answer")
}

/// Checks the `attachment_ids` of a task (`TASK-98.4`): at most [`TASK_ATTACHMENTS_MAX`].
pub fn check_task_attachment_ids(ids: &[String]) -> Result<(), ApiError> {
    check_attachment_ids_limit(ids, TASK_ATTACHMENTS_MAX, "a task")
}

/// Checks the `attachment_ids` of a task comment (`TASK-98.4`): at most
/// [`TASK_COMMENT_ATTACHMENTS_MAX`].
pub fn check_task_comment_attachment_ids(ids: &[String]) -> Result<(), ApiError> {
    check_attachment_ids_limit(ids, TASK_COMMENT_ATTACHMENTS_MAX, "a task comment")
}

/// Whether a message body with these files says something: text, files, or both.
pub fn has_content(body: &str, attachment_ids: &[String]) -> bool {
    !body.trim().is_empty() || !attachment_ids.is_empty()
}
