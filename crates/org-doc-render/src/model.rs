//! Core port types, limits, and errors for pure-Rust document-to-PDF rendering (`TASK-98.7`).

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use thiserror::Error;

/// Hard combined input cap (source + optional CSS): 2 MiB.
pub const MAX_SOURCE_BYTES: usize = 2 * 1024 * 1024;
/// Default maximum rendered PDF page count.
pub const DEFAULT_MAX_PAGES: u32 = 100;
/// Default maximum concurrent renders per office process.
pub const DEFAULT_MAX_CONCURRENCY: usize = 2;
/// Default per-render wall-clock timeout.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
/// Default maximum rendered PDF size (matches `ATTACHMENT_DOCUMENT_MAX_BYTES`: 25 MiB).
pub const MAX_PDF_BYTES: usize = 25 * 1024 * 1024;
/// Maximum number of embedded raster images allowed in one document.
pub const MAX_EMBEDDED_IMAGES: usize = 32;
/// Maximum decoded byte size of one embedded raster image (4 MiB).
pub const MAX_IMAGE_DECODED_BYTES: usize = 4 * 1024 * 1024;
/// Maximum width or height in pixels of one embedded raster image.
pub const MAX_IMAGE_DIMENSION: u32 = 4_096;

/// Source markup format accepted by [`DocumentRenderer`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Markdown,
    Html,
}

/// Pre-authorized raster image supplied by the caller for an `attachment:<id>` reference.
#[derive(Clone, PartialEq, Eq)]
pub struct ResolvedImage {
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

impl fmt::Debug for ResolvedImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResolvedImage")
            .field("mime_type", &self.mime_type)
            .field("bytes", &format_args!("<{} bytes>", self.bytes.len()))
            .finish()
    }
}

/// One in-memory document rendering request.
#[derive(Clone, PartialEq, Eq)]
pub struct RenderRequest {
    pub source_kind: SourceKind,
    pub source: String,
    pub css: Option<String>,
    pub title: Option<String>,
    /// Pre-authorized images keyed by canonical lowercase attachment UUIDv7.
    pub attachments: BTreeMap<String, ResolvedImage>,
}

impl RenderRequest {
    /// Creates a Markdown rendering request.
    pub fn markdown(source: impl Into<String>) -> Self {
        Self {
            source_kind: SourceKind::Markdown,
            source: source.into(),
            css: None,
            title: None,
            attachments: BTreeMap::new(),
        }
    }

    /// Creates an HTML rendering request.
    pub fn html(source: impl Into<String>) -> Self {
        Self {
            source_kind: SourceKind::Html,
            source: source.into(),
            css: None,
            title: None,
            attachments: BTreeMap::new(),
        }
    }

    /// Attaches an optional stylesheet string.
    pub fn with_css(mut self, css: impl Into<String>) -> Self {
        self.css = Some(css.into());
        self
    }

    /// Sets an optional document title for PDF metadata.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Combined UTF-8 byte length of `source` and optional `css`.
    pub fn total_input_bytes(&self) -> usize {
        self.source
            .len()
            .saturating_add(self.css.as_deref().map_or(0, str::len))
    }
}

impl fmt::Debug for RenderRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderRequest")
            .field("source_kind", &self.source_kind)
            .field("source", &format_args!("<{} bytes>", self.source.len()))
            .field(
                "css",
                &self.css.as_ref().map(|c| format!("<{} bytes>", c.len())),
            )
            .field("title", &self.title)
            .field("attachments", &self.attachments.len())
            .finish()
    }
}

/// Resource and security boundaries enforced by [`crate::BoundedDocumentRenderer`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderLimits {
    pub max_input_bytes: usize,
    pub max_pages: u32,
    pub max_concurrency: usize,
    pub timeout: Duration,
    pub max_output_bytes: usize,
}

impl Default for RenderLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: MAX_SOURCE_BYTES,
            max_pages: DEFAULT_MAX_PAGES,
            max_concurrency: DEFAULT_MAX_CONCURRENCY,
            timeout: DEFAULT_TIMEOUT,
            max_output_bytes: MAX_PDF_BYTES,
        }
    }
}

/// Verified PDF output returned by a [`DocumentRenderer`].
#[derive(Clone, PartialEq, Eq)]
pub struct RenderedPdf {
    pub bytes: Vec<u8>,
    pub page_count: u32,
    pub byte_count: u64,
}

impl fmt::Debug for RenderedPdf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderedPdf")
            .field("page_count", &self.page_count)
            .field("byte_count", &self.byte_count)
            .finish()
    }
}

/// Fail-closed errors from document sanitization, resource bounds, or PDF rendering.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum RenderError {
    #[error("render input is {actual} bytes; maximum allowed is {max} bytes")]
    InputTooLarge { actual: usize, max: usize },
    #[error("render source must not be empty")]
    EmptySource,
    #[error("forbidden resource URL ({reason}): `{url}`")]
    ForbiddenResourceUrl { url: String, reason: &'static str },
    #[error("forbidden active or unsafe markup ({reason})")]
    ForbiddenContent { reason: &'static str },
    #[error("rendered PDF has {actual} pages; maximum allowed is {max}")]
    PageLimitExceeded { actual: u32, max: u32 },
    #[error("rendered PDF is {actual} bytes; maximum allowed is {max} bytes")]
    OutputTooLarge { actual: usize, max: usize },
    #[error("document render timed out after {timeout_ms} ms")]
    Timeout { timeout_ms: u64 },
    #[error("render engine is busy; all concurrency slots are occupied")]
    Busy,
    #[error("renderer produced an invalid PDF: {0}")]
    InvalidPdf(&'static str),
    #[error("document rendering failed: {0}")]
    RenderFailed(String),
}

/// Consumer-owned port for pure in-memory Markdown and HTML+CSS to PDF rendering.
pub trait DocumentRenderer: Send + Sync {
    /// Renders a Markdown document (with optional CSS) to PDF bytes.
    fn render_markdown_to_pdf(&self, request: &RenderRequest) -> Result<RenderedPdf, RenderError>;

    /// Renders an HTML+CSS document to PDF bytes.
    fn render_html_to_pdf(&self, request: &RenderRequest) -> Result<RenderedPdf, RenderError>;

    /// Renders `request` according to [`RenderRequest::source_kind`].
    fn render(&self, request: &RenderRequest) -> Result<RenderedPdf, RenderError> {
        match request.source_kind {
            SourceKind::Markdown => self.render_markdown_to_pdf(request),
            SourceKind::Html => self.render_html_to_pdf(request),
        }
    }
}
