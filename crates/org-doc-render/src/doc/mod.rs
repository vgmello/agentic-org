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
        bytes: Vec<u8>,
        alt: String,
        width_pt: Option<f32>,
        height_pt: Option<f32>,
    },
    HorizontalRule,
    PageBreak,
}

pub(super) fn resolve_image_bytes(
    raw_url: &str,
    attachments: &BTreeMap<String, ResolvedImage>,
) -> Result<Vec<u8>, RenderError> {
    match validate_image_url(raw_url, attachments)? {
        SafeImageRef::Inline { bytes, .. } => Ok(bytes),
        SafeImageRef::Attachment { id } => attachments
            .get(&id)
            .map(|img| img.bytes.clone())
            .ok_or_else(|| RenderError::ForbiddenResourceUrl {
                url: format!("attachment:{id}"),
                reason: "referenced attachment is not authorized or not found",
            }),
    }
}
