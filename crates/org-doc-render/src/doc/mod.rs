//! Intermediate document model shared by Markdown and HTML+CSS inputs.

mod html;
mod markdown;

use std::collections::BTreeMap;

use crate::css::{PageStyle, Rgb, StyleProps};
use crate::model::{RenderError, ResolvedImage};
use crate::sanitize::{validate_image_url, SafeImageRef};

pub(crate) use html::html_to_document;
pub(crate) use markdown::markdown_to_document;

/// Normalized printable document ready for pagination into PDF pages.
#[derive(Clone, Debug)]
pub(crate) struct Document {
    pub title: Option<String>,
    pub page: PageStyle,
    pub blocks: Vec<Block>,
}

/// One inline text run with typography attributes.
#[derive(Clone, Debug)]
pub(crate) struct InlineSpan {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub color: Option<Rgb>,
}

impl InlineSpan {
    pub(crate) fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            bold: false,
            italic: false,
            code: false,
            color: None,
        }
    }
}

/// One table cell containing inline runs.
#[derive(Clone, Debug)]
pub(crate) struct TableCell {
    pub spans: Vec<InlineSpan>,
    pub header: bool,
    pub style: StyleProps,
}

/// One table row.
#[derive(Clone, Debug)]
pub(crate) struct TableRow {
    pub cells: Vec<TableCell>,
}

/// Authorized raster image source referenced by a [`Block::Image`].
#[derive(Clone, Debug)]
pub(crate) enum ImageSource {
    /// Pre-authorized attachment UUIDv7 resolved zero-copy from `RenderRequest::attachments`.
    Attachment(String),
    /// Validated inline `data:image/...;base64,...` image with canonical deduplication key.
    Inline { key: String, bytes: Vec<u8> },
}

impl ImageSource {
    pub(crate) fn cache_key(&self) -> String {
        match self {
            Self::Attachment(id) => format!("attachment:{id}"),
            Self::Inline { key, .. } => key.clone(),
        }
    }
}

/// Block-level document element.
#[derive(Clone, Debug)]
pub(crate) enum Block {
    Heading {
        level: u8,
        spans: Vec<InlineSpan>,
        style: StyleProps,
    },
    Paragraph {
        spans: Vec<InlineSpan>,
        style: StyleProps,
    },
    Code {
        text: String,
        style: StyleProps,
    },
    Quote {
        blocks: Vec<Block>,
    },
    List {
        ordered: bool,
        start: u64,
        items: Vec<Vec<Block>>,
    },
    Table {
        headers: Vec<TableRow>,
        rows: Vec<TableRow>,
    },
    Image {
        source: ImageSource,
        alt: String,
        width_pt: Option<f32>,
        height_pt: Option<f32>,
    },
    HorizontalRule,
    PageBreak,
}

pub(super) fn resolve_image_source(
    raw_url: &str,
    attachments: &BTreeMap<String, ResolvedImage>,
) -> Result<ImageSource, RenderError> {
    match validate_image_url(raw_url, attachments)? {
        SafeImageRef::Inline { mime_type, bytes } => Ok(ImageSource::Inline {
            key: format!("inline:{mime_type}:{}", raw_url.trim()),
            bytes,
        }),
        SafeImageRef::Attachment { id } => {
            if attachments.contains_key(&id) {
                Ok(ImageSource::Attachment(id))
            } else {
                Err(RenderError::ForbiddenResourceUrl {
                    url: format!("attachment:{id}"),
                    reason: "referenced attachment is not authorized or not found",
                })
            }
        }
    }
}
