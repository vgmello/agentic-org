//! What an attachment may be (`TASK-98.1`, ADR-0037): a closed allowlist of media types, each
//! checked against its bytes and its file name, and turned into the bytes the office keeps.
//!
//! - Images (`image/png`, `image/jpeg`, `image/webp`, `image/gif`, at most 10 MiB) are decoded
//!   and encoded again ([`image`]): EXIF, GPS, ICC and every other metadata block is dropped
//!   and the orientation applied, before the content hash is taken.
//! - Documents (at most 25 MiB) are checked structurally, never rendered: PDF ([`pdf`]),
//!   `.docx`/`.xlsx` ([`ooxml`]), and UTF-8 text, CSV, Markdown and JSON ([`text`]).
//! - Everything else is refused: HTML, SVG, scripts, executables, `multipart/form-data`,
//!   `application/octet-stream`, and any file whose magic bytes say something other than its
//!   declared type ([`sniff`]).

pub mod image;
pub mod ooxml;
pub mod pdf;
pub mod sniff;
pub mod text;

use org_proto::attachments::{
    ATTACHMENT_DOCUMENT_MAX_BYTES, ATTACHMENT_FILENAME_MAX_BYTES, ATTACHMENT_IMAGE_MAX_BYTES,
};
use org_proto::{ApiError, ErrorCode};

pub use self::image::{model_derivative, ModelDerivative};

/// One allowed media type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MediaType {
    Png,
    Jpeg,
    Webp,
    Gif,
    Pdf,
    Docx,
    Xlsx,
    Markdown,
    PlainText,
    Csv,
    Json,
}

const DOCX_MIME: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
const XLSX_MIME: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

const ALL: [MediaType; 11] = [
    MediaType::Png,
    MediaType::Jpeg,
    MediaType::Webp,
    MediaType::Gif,
    MediaType::Pdf,
    MediaType::Docx,
    MediaType::Xlsx,
    MediaType::Markdown,
    MediaType::PlainText,
    MediaType::Csv,
    MediaType::Json,
];

/// Types refused by name, with the reason a caller sees.
const BLOCKED: [(&str, &str); 12] = [
    ("multipart/form-data", "multipart bodies are not files"),
    ("text/html", "HTML can run script"),
    ("application/xhtml+xml", "HTML can run script"),
    ("image/svg+xml", "SVG can run script"),
    ("application/javascript", "scripts are not accepted"),
    ("text/javascript", "scripts are not accepted"),
    ("application/ecmascript", "scripts are not accepted"),
    (
        "application/octet-stream",
        "untyped binaries are not accepted",
    ),
    ("application/x-msdownload", "executables are not accepted"),
    ("application/x-executable", "executables are not accepted"),
    ("application/x-elf", "executables are not accepted"),
    ("application/x-mach-binary", "executables are not accepted"),
];

impl MediaType {
    /// The canonical MIME type stored with the file.
    pub const fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
            Self::Gif => "image/gif",
            Self::Pdf => "application/pdf",
            Self::Docx => DOCX_MIME,
            Self::Xlsx => XLSX_MIME,
            Self::Markdown => "text/markdown",
            Self::PlainText => "text/plain",
            Self::Csv => "text/csv",
            Self::Json => "application/json",
        }
    }

    /// The type a stored MIME names, if allowed.
    pub fn from_mime(mime: &str) -> Option<Self> {
        ALL.into_iter().find(|media| media.mime() == mime)
    }

    pub const fn is_image(self) -> bool {
        matches!(self, Self::Png | Self::Jpeg | Self::Webp | Self::Gif)
    }

    const fn is_text(self) -> bool {
        matches!(
            self,
            Self::Markdown | Self::PlainText | Self::Csv | Self::Json
        )
    }

    /// The largest upload of this type.
    pub const fn max_bytes(self) -> u64 {
        if self.is_image() {
            ATTACHMENT_IMAGE_MAX_BYTES
        } else {
            ATTACHMENT_DOCUMENT_MAX_BYTES
        }
    }

    /// The file name extensions this type is uploaded under (lowercase, no dot).
    pub const fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Png => &["png"],
            Self::Jpeg => &["jpg", "jpeg"],
            Self::Webp => &["webp"],
            Self::Gif => &["gif"],
            Self::Pdf => &["pdf"],
            Self::Docx => &["docx"],
            Self::Xlsx => &["xlsx"],
            Self::Markdown => &["md", "markdown"],
            Self::PlainText => &["txt", "text", "log"],
            Self::Csv => &["csv"],
            Self::Json => &["json"],
        }
    }

    /// The type a caller declares: lowercase, parameters dropped except `charset=utf-8` on a
    /// text type. A blocked or unknown type is `unsupported`.
    pub fn parse_declared(declared: &str) -> Result<Self, ApiError> {
        let mut parts = declared.split(';');
        let essence = parts.next().unwrap_or("").trim().to_ascii_lowercase();
        if let Some((_, why)) = BLOCKED.iter().find(|(mime, _)| *mime == essence) {
            return Err(unsupported(format!("`{essence}` is not accepted: {why}")));
        }
        let Some(media) = Self::from_mime(&essence) else {
            return Err(unsupported(format!(
                "`{}` is not an accepted attachment type",
                printable(&essence)
            )));
        };
        for parameter in parts {
            let parameter = parameter.trim().to_ascii_lowercase().replace(' ', "");
            let utf8 = matches!(parameter.as_str(), "charset=utf-8" | "charset=\"utf-8\"");
            if !(media.is_text() && utf8) {
                return Err(unsupported(
                    "only `charset=utf-8` may qualify a text attachment type",
                ));
            }
        }
        Ok(media)
    }
}

/// A file name as a caller gives it: one plain name, at most 255 bytes, no folder, no control
/// character, with an extension of the declared type.
pub fn check_filename(name: &str, media: MediaType) -> Result<String, ApiError> {
    let name = name.trim();
    let invalid = |why: &str| ApiError::new(ErrorCode::InvalidRequest, format!("filename: {why}"));
    if name.is_empty() || name.len() > ATTACHMENT_FILENAME_MAX_BYTES {
        return Err(invalid("must be 1 to 255 bytes"));
    }
    if name
        .chars()
        .any(|c| c.is_control() || matches!(c, '/' | '\\'))
        || matches!(name, "." | "..")
        || name.starts_with('.')
    {
        return Err(invalid("must be one plain file name"));
    }
    let extension = name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    if !media.extensions().contains(&extension.as_str()) {
        return Err(unsupported(format!(
            "a `{}` file is named `.{}`, not `.{}`",
            media.mime(),
            media.extensions()[0],
            printable(&extension)
        )));
    }
    Ok(name.to_owned())
}

/// A file the office accepted, as it keeps it.
#[derive(Clone, PartialEq, Eq)]
pub struct ValidatedFile {
    /// What the bytes are now (an animated WebP is kept as an animated GIF).
    pub media: MediaType,
    pub bytes: Vec<u8>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub page_count: Option<u32>,
}

impl std::fmt::Debug for ValidatedFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ValidatedFile")
            .field("media", &self.media)
            .field("bytes", &self.bytes.len())
            .field("width", &self.width)
            .field("height", &self.height)
            .field("page_count", &self.page_count)
            .finish()
    }
}

/// Checks `raw` against `declared` and returns what the office keeps: re-encoded images,
/// structurally checked documents. Nothing here writes anywhere.
pub fn validate(declared: MediaType, raw: Vec<u8>) -> Result<ValidatedFile, ApiError> {
    if raw.is_empty() {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "the file is empty",
        ));
    }
    if raw.len() as u64 > declared.max_bytes() {
        return Err(too_large(declared));
    }
    sniff::check_matches(declared, &raw)?;
    match declared {
        MediaType::Png | MediaType::Jpeg | MediaType::Webp | MediaType::Gif => {
            image::sanitize(declared, &raw)
        }
        MediaType::Pdf => {
            let pages = pdf::page_count(&raw)?;
            Ok(document(declared, raw, Some(pages)))
        }
        MediaType::Docx | MediaType::Xlsx => {
            ooxml::check(declared, &raw)?;
            Ok(document(declared, raw, None))
        }
        MediaType::Markdown | MediaType::PlainText | MediaType::Csv | MediaType::Json => {
            text::check(declared, &raw)?;
            Ok(document(declared, raw, None))
        }
    }
}

fn document(media: MediaType, bytes: Vec<u8>, page_count: Option<u32>) -> ValidatedFile {
    ValidatedFile {
        media,
        bytes,
        width: None,
        height: None,
        page_count,
    }
}

pub(crate) fn unsupported(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Unsupported, message)
}

pub(crate) fn corrupt(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::InvalidRequest, message)
}

pub(crate) fn too_large(media: MediaType) -> ApiError {
    ApiError::new(
        ErrorCode::TooLarge,
        format!(
            "a `{}` attachment is at most {} MiB",
            media.mime(),
            media.max_bytes() / (1024 * 1024)
        ),
    )
}

/// At most 64 visible characters of caller text, for a refusal message.
fn printable(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(64).collect()
}

#[cfg(test)]
pub(crate) mod tests;
