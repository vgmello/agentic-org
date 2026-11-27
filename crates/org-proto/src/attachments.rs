//! Wire types and typed RPCs for attachments (`TASK-98.1`, ADR-0037): chunked uploads into an
//! uploader-only staging area, binding a validated upload (or re-attaching an existing file) to
//! one target, and chunked reads.
//!
//! External APIs address files only by `attachment_id` (a UUIDv7 minted per binding) or by
//! `upload_id` (a staged upload); a file's `content_sha256` is never a lookup key and is
//! returned only after the caller passed the read check, for use as an ETag.
//!
//! Every byte payload travels base64-encoded in chunks of at most
//! [`ATTACHMENT_CHUNK_MAX_BYTES`], so a request or reply stays well inside
//! [`crate::MAX_FRAME_BYTES`]. `Debug` never prints chunk bytes.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::Call;

mod meta;

pub use meta::{
    check_attachment_ids, check_attachment_ids_limit, check_task_attachment_ids,
    check_task_comment_attachment_ids, has_content, is_image_mime, AttachmentFilter,
    AttachmentMeta, MESSAGE_ATTACHMENTS_MAX, TASK_ATTACHMENTS_MAX, TASK_COMMENT_ATTACHMENTS_MAX,
};

/// Largest decoded chunk an upload or a read moves in one call (512 KiB).
pub const ATTACHMENT_CHUNK_MAX_BYTES: u32 = 512 * 1024;
/// Largest base64 text a chunk may take on the wire: `4 * ceil(512 KiB / 3)`.
pub const ATTACHMENT_CHUNK_MAX_BASE64_CHARS: usize = 699_052;
/// Largest image upload (`image/png`, `image/jpeg`, `image/webp`, `image/gif`).
pub const ATTACHMENT_IMAGE_MAX_BYTES: u64 = 10 * 1024 * 1024;
/// Largest document upload (PDF, `.docx`, `.xlsx`, Markdown, text, CSV, JSON).
pub const ATTACHMENT_DOCUMENT_MAX_BYTES: u64 = 25 * 1024 * 1024;
/// Longest attachment title, in Unicode scalar values.
pub const ATTACHMENT_TITLE_MAX_CHARS: usize = 160;
/// Longest attachment description, in Unicode scalar values.
pub const ATTACHMENT_DESCRIPTION_MAX_CHARS: usize = 280;
/// Longest file name, in bytes.
pub const ATTACHMENT_FILENAME_MAX_BYTES: usize = 255;
/// Most attachments one `attachment.list` page returns.
pub const ATTACHMENT_LIST_MAX_LIMIT: u32 = 200;

/// What an attachment is bound to. Each binding has its own `attachment_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentTargetKind {
    Task,
    TaskComment,
    ReviewFinding,
    ReviewFindingReply,
    Project,
    Organization,
    Message,
    AskAnswer,
    /// A person's own HQ-only private space; never visible to any agent.
    UserPrivate,
}

impl AttachmentTargetKind {
    /// The stable lowercase name stored in records and tables.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::TaskComment => "task_comment",
            Self::ReviewFinding => "review_finding",
            Self::ReviewFindingReply => "review_finding_reply",
            Self::Project => "project",
            Self::Organization => "organization",
            Self::Message => "message",
            Self::AskAnswer => "ask_answer",
            Self::UserPrivate => "user_private",
        }
    }

    /// The kind a stored name stands for.
    pub fn parse(text: &str) -> Option<Self> {
        [
            Self::Task,
            Self::TaskComment,
            Self::ReviewFinding,
            Self::ReviewFindingReply,
            Self::Project,
            Self::Organization,
            Self::Message,
            Self::AskAnswer,
            Self::UserPrivate,
        ]
        .into_iter()
        .find(|kind| kind.as_str() == text)
    }
}

/// Where an attachment's bytes and metadata live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentScope {
    /// Shared organization storage (`durable/blobs/`, canonical files under `durable/state/`).
    Shared,
    /// The caller's HQ-only private space (`durable/private/`).
    UserPrivate,
}

/// Where a file stands in its review life.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactStatus {
    Draft,
    Review,
    #[default]
    Final,
    Superseded,
    Archived,
}

impl ArtifactStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Review => "review",
            Self::Final => "final",
            Self::Superseded => "superseded",
            Self::Archived => "archived",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        [
            Self::Draft,
            Self::Review,
            Self::Final,
            Self::Superseded,
            Self::Archived,
        ]
        .into_iter()
        .find(|status| status.as_str() == text)
    }
}

/// One bound attachment as a caller who may read it sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentView {
    pub id: String,
    pub scope: AttachmentScope,
    pub target_kind: AttachmentTargetKind,
    /// `None` for an organization-wide file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    pub filename: String,
    pub mime_type: String,
    pub byte_count: u64,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub artifact_status: ArtifactStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_count: Option<u32>,
    pub uploaded_by_employee_id: String,
    pub created_at: String,
    pub updated_at: String,
    /// Lowercase hex SHA-256 of the stored (sanitized) bytes: an ETag, never a lookup key.
    pub content_sha256: String,
}

/// A staged upload as its uploader sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedUploadView {
    pub upload_id: String,
    pub filename: String,
    pub mime_type: String,
    /// The declared size until committed; the sanitized size after.
    pub byte_count: u64,
    pub received_bytes: u64,
    pub committed: bool,
    pub expires_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_count: Option<u32>,
    /// Set once committed: the sanitized bytes' SHA-256 (uploader-only ETag).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_sha256: Option<String>,
}

/// `attachment.upload_begin`: declares the file before any byte moves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentUploadBeginParams {
    pub filename: String,
    pub mime_type: String,
    pub byte_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentUploadBeginResult {
    pub upload: StagedUploadView,
    /// The largest decoded chunk `attachment.upload_chunk` takes.
    pub chunk_size_bytes: u32,
}

/// `attachment.upload_chunk`: appends bytes at `offset` (the bytes received so far).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentUploadChunkParams {
    pub upload_id: String,
    pub offset: u64,
    pub data_base64: String,
}

impl fmt::Debug for AttachmentUploadChunkParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AttachmentUploadChunkParams")
            .field("upload_id", &self.upload_id)
            .field("offset", &self.offset)
            .field(
                "data_base64",
                &format_args!("<{} chars>", self.data_base64.len()),
            )
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentUploadChunkResult {
    pub upload_id: String,
    pub received_bytes: u64,
    /// Whether every declared byte has arrived.
    pub complete: bool,
}

/// `attachment.upload_commit` and `attachment.upload_abort` name one staged upload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentUploadRef {
    pub upload_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentUploadCommitResult {
    pub upload: StagedUploadView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentUploadAbortResult {
    pub upload_id: String,
}

/// `attachment.attach`: binds a committed upload, or re-attaches an existing attachment the
/// caller may read, to one target. Exactly one of `upload_id` and `source_attachment_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentAttachParams {
    /// Canonical lowercase UUIDv7: retrying with the same id and request is answered once.
    pub command_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upload_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_attachment_id: Option<String>,
    pub target_kind: AttachmentTargetKind,
    /// The target's id; omitted for `organization` (and, for `user_private`, the caller).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    /// Defaults to the file name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Defaults to `final`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_status: Option<ArtifactStatus>,
}

/// The reply of `attachment.attach`, `attachment.inspect` and `attachment.update_meta`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentResult {
    pub attachment: AttachmentView,
}

/// `attachment.inspect`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentInspectParams {
    pub attachment_id: String,
}

/// `attachment.list`: one target's attachments (or the caller's private space), oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_kind: Option<AttachmentTargetKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<AttachmentScope>,
    /// At most [`ATTACHMENT_LIST_MAX_LIMIT`]; defaults to 50.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// The `next_cursor` of the previous page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentListResult {
    pub attachments: Vec<AttachmentView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// `attachment.update_meta`: changes the title, description or status of a standalone
/// `project`, `organization` or `user_private` file. An empty `description` clears it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentUpdateMetaParams {
    pub command_id: String,
    pub attachment_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_status: Option<ArtifactStatus>,
}

/// `attachment.read_chunk`: up to `max_bytes` (default and cap
/// [`ATTACHMENT_CHUNK_MAX_BYTES`]) from `offset`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentReadChunkParams {
    pub attachment_id: String,
    pub offset: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<u32>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentReadChunkResult {
    pub attachment_id: String,
    pub offset: u64,
    pub data_base64: String,
    pub eof: bool,
    pub total_bytes: u64,
    pub content_sha256: String,
}

impl fmt::Debug for AttachmentReadChunkResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AttachmentReadChunkResult")
            .field("attachment_id", &self.attachment_id)
            .field("offset", &self.offset)
            .field(
                "data_base64",
                &format_args!("<{} chars>", self.data_base64.len()),
            )
            .field("eof", &self.eof)
            .field("total_bytes", &self.total_bytes)
            .field("content_sha256", &self.content_sha256)
            .finish()
    }
}

/// `attachment.upload_begin`
pub struct AttachmentUploadBegin;
impl Call for AttachmentUploadBegin {
    const METHOD: &'static str = "attachment.upload_begin";
    type Params = AttachmentUploadBeginParams;
    type Result = AttachmentUploadBeginResult;
}

/// `attachment.upload_chunk`
pub struct AttachmentUploadChunk;
impl Call for AttachmentUploadChunk {
    const METHOD: &'static str = "attachment.upload_chunk";
    type Params = AttachmentUploadChunkParams;
    type Result = AttachmentUploadChunkResult;
}

/// `attachment.upload_commit`
pub struct AttachmentUploadCommit;
impl Call for AttachmentUploadCommit {
    const METHOD: &'static str = "attachment.upload_commit";
    type Params = AttachmentUploadRef;
    type Result = AttachmentUploadCommitResult;
}

/// `attachment.upload_abort`
pub struct AttachmentUploadAbort;
impl Call for AttachmentUploadAbort {
    const METHOD: &'static str = "attachment.upload_abort";
    type Params = AttachmentUploadRef;
    type Result = AttachmentUploadAbortResult;
}

/// `attachment.attach`
pub struct AttachmentAttach;
impl Call for AttachmentAttach {
    const METHOD: &'static str = "attachment.attach";
    type Params = AttachmentAttachParams;
    type Result = AttachmentResult;
}

/// `attachment.inspect`
pub struct AttachmentInspect;
impl Call for AttachmentInspect {
    const METHOD: &'static str = "attachment.inspect";
    type Params = AttachmentInspectParams;
    type Result = AttachmentResult;
}

/// `attachment.list`
pub struct AttachmentList;
impl Call for AttachmentList {
    const METHOD: &'static str = "attachment.list";
    type Params = AttachmentListParams;
    type Result = AttachmentListResult;
}

/// `attachment.update_meta`
pub struct AttachmentUpdateMeta;
impl Call for AttachmentUpdateMeta {
    const METHOD: &'static str = "attachment.update_meta";
    type Params = AttachmentUpdateMetaParams;
    type Result = AttachmentResult;
}

/// `attachment.read_chunk`
pub struct AttachmentReadChunk;
impl Call for AttachmentReadChunk {
    const METHOD: &'static str = "attachment.read_chunk";
    type Params = AttachmentReadChunkParams;
    type Result = AttachmentReadChunkResult;
}

/// Every attachment method, in the order of the contract.
pub const ATTACHMENT_METHODS: [&str; 9] = [
    AttachmentUploadBegin::METHOD,
    AttachmentUploadChunk::METHOD,
    AttachmentUploadCommit::METHOD,
    AttachmentUploadAbort::METHOD,
    AttachmentAttach::METHOD,
    AttachmentInspect::METHOD,
    AttachmentList::METHOD,
    AttachmentUpdateMeta::METHOD,
    AttachmentReadChunk::METHOD,
];

#[cfg(test)]
mod tests;
