//! Pure-Rust, zero-Chrome document-to-PDF rendering behind a consumer-owned
//! [`DocumentRenderer`] port (`TASK-98.7`).
//!
//! Linked strictly into `crates/org-office-server` / `apps/agentic-org-office` — never into
//! `apps/agent-os` or `crates/org-agent-cli`.

#![forbid(unsafe_code)]

mod bounded;
mod css;
mod doc;
mod layout;
mod model;
mod pdf;
mod sanitize;
#[cfg(test)]
mod tests;

pub use bounded::{BoundedDocumentRenderer, PdfWriterRenderer};
pub use model::{
    DocumentRenderer, RenderError, RenderLimits, RenderRequest, RenderedPdf, ResolvedImage,
    SourceKind, DEFAULT_MAX_CONCURRENCY, DEFAULT_MAX_PAGES, DEFAULT_TIMEOUT, MAX_EMBEDDED_IMAGES,
    MAX_IMAGE_DECODED_BYTES, MAX_IMAGE_DIMENSION, MAX_PDF_BYTES, MAX_SOURCE_BYTES,
};
pub use pdf::count_pdf_pages;
pub use sanitize::extract_attachment_refs;
