//! Wire types and typed RPC contract for `attachment.render` (`TASK-98.7`).
//!
//! Renders Markdown or HTML+CSS to PDF strictly inside `agentic-org-office`. Oversized inputs
//! (up to [`ATTACHMENT_RENDER_SOURCE_MAX_BYTES`] = 2 MiB) and large local PDF responses (> 512 KiB)
//! stream in `<= 512 KiB` base64 chunks so every frame stays safely inside [`crate::MAX_FRAME_BYTES`].

use std::fmt;

use serde::{Deserialize, Serialize};

use super::{ArtifactStatus, AttachmentTargetKind, AttachmentView};
use crate::Call;

/// Hard combined source + CSS byte limit accepted by `attachment.render` (2 MiB).
pub const ATTACHMENT_RENDER_SOURCE_MAX_BYTES: usize = 2 * 1024 * 1024;
/// Maximum page count accepted from a single render.
pub const ATTACHMENT_RENDER_MAX_PAGES: u32 = 100;

/// Output document format accepted by `attachment.render`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderFormat {
    #[default]
    Pdf,
}

impl RenderFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "pdf" => Some(Self::Pdf),
            _ => None,
        }
    }
}

/// Input markup kind accepted by `attachment.render`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderSourceKind {
    #[default]
    Markdown,
    Html,
}

impl RenderSourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Html => "html",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "markdown" | "md" => Some(Self::Markdown),
            "html" | "htm" => Some(Self::Html),
            _ => None,
        }
    }
}

/// `attachment.render`: renders Markdown or HTML+CSS to PDF inside `agentic-org-office`,
/// either committing the rendered PDF to a hosted attachment scope or returning bounded PDF bytes.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentRenderParams {
    /// Optional canonical lowercase UUIDv7 for idempotent hosted attach retries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_id: Option<String>,
    #[serde(default)]
    pub format: RenderFormat,
    #[serde(default)]
    pub source_kind: RenderSourceKind,
    #[serde(default)]
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub css: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_kind: Option<AttachmentTargetKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_status: Option<ArtifactStatus>,
    #[serde(default)]
    pub return_bytes: bool,
    /// Continuation handle for chunked PDF output reads or chunked >1 MiB input staging.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_id: Option<String>,
    /// Byte offset when reading a subsequent chunk of a rendered PDF.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk_offset: Option<u64>,
    /// Optional base64-encoded chunk of `source` (used when `source + css` exceeds one frame).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_chunk_base64: Option<String>,
    /// Optional base64-encoded chunk of `css` (used when `source + css` exceeds one frame).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub css_chunk_base64: Option<String>,
    /// When staging input chunks, `true` finalizes the staged input and executes rendering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_eof: Option<bool>,
}

impl fmt::Debug for AttachmentRenderParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AttachmentRenderParams")
            .field("command_id", &self.command_id)
            .field("format", &self.format)
            .field("source_kind", &self.source_kind)
            .field("source", &format_args!("<{} bytes>", self.source.len()))
            .field(
                "css",
                &self.css.as_ref().map(|c| format!("<{} bytes>", c.len())),
            )
            .field("filename", &self.filename)
            .field("target_kind", &self.target_kind)
            .field("target_id", &self.target_id)
            .field("project_slug", &self.project_slug)
            .field("task_ref", &self.task_ref)
            .field("title", &self.title)
            .field("artifact_status", &self.artifact_status)
            .field("return_bytes", &self.return_bytes)
            .field("render_id", &self.render_id)
            .field("chunk_offset", &self.chunk_offset)
            .field(
                "source_chunk_base64",
                &self
                    .source_chunk_base64
                    .as_ref()
                    .map(|c| format!("<{} chars>", c.len())),
            )
            .field(
                "css_chunk_base64",
                &self
                    .css_chunk_base64
                    .as_ref()
                    .map(|c| format!("<{} chars>", c.len())),
            )
            .field("input_eof", &self.input_eof)
            .finish()
    }
}

/// Reply of `attachment.render`.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentRenderResult {
    /// Set when committed to a hosted scope (`organization`, `project`, `task`, `user_private`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment_id: Option<String>,
    /// Canonical download URL (`/api/v1/organizations/<org_id>/attachments/<id>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_url: Option<String>,
    /// Full hosted attachment metadata when committed to a hosted scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment: Option<AttachmentView>,
    /// Base64-encoded PDF bytes (or current `<= 512 KiB` chunk) when `return_bytes == true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdf_base64: Option<String>,
    /// Continuation handle when more output chunks remain (`eof == Some(false)`) or while
    /// staging input chunks (`input_eof == Some(false)`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_id: Option<String>,
    /// Byte offset of `pdf_base64` within the full rendered PDF.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
    /// Whether `pdf_base64` reaches the end of the rendered PDF.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eof: Option<bool>,
    pub page_count: u32,
    pub byte_count: u64,
    /// Lowercase hex SHA-256 of the complete rendered PDF bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_sha256: Option<String>,
}

impl fmt::Debug for AttachmentRenderResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AttachmentRenderResult")
            .field("attachment_id", &self.attachment_id)
            .field("canonical_url", &self.canonical_url)
            .field("attachment", &self.attachment)
            .field(
                "pdf_base64",
                &self
                    .pdf_base64
                    .as_ref()
                    .map(|c| format!("<{} chars>", c.len())),
            )
            .field("render_id", &self.render_id)
            .field("offset", &self.offset)
            .field("eof", &self.eof)
            .field("page_count", &self.page_count)
            .field("byte_count", &self.byte_count)
            .field("content_sha256", &self.content_sha256)
            .finish()
    }
}

/// `attachment.render`
pub struct AttachmentRender;
impl Call for AttachmentRender {
    const METHOD: &'static str = "attachment.render";
    type Params = AttachmentRenderParams;
    type Result = AttachmentRenderResult;
}
