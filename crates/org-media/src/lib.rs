//! Canonical attachment media allowlist, magic-byte sniffer, and optional image/document
//! sanitizer (`TASK-98.1`, `TASK-252`, ADR-0037).
//!
//! Default features pull **zero** codec or compression crates (depends strictly on `org-proto`),
//! providing the single security choke-point for filename classification, MIME parsing, and
//! magic-byte / HTML / SVG / executable sniffing shared by `agent-os` and `org-office`.
//!
//! Enabling the `sanitize` feature adds:
//! - Image decoding and re-encoding ([`image`]) that strips EXIF/GPS/XMP/ICC metadata, applies
//!   orientation, enforces pixel/frame/allocation bounds, and produces bounded model derivatives.
//! - Clean-image verification ([`clean`]) for bytes shipped across offices.
//! - Structural document checks ([`pdf`], [`ooxml`], [`text`]).

#![forbid(unsafe_code)]

#[cfg(feature = "sanitize")]
pub mod clean;
#[cfg(feature = "sanitize")]
pub mod image;
#[cfg(feature = "sanitize")]
pub mod ooxml;
#[cfg(feature = "sanitize")]
pub mod pdf;
pub mod sniff;
#[cfg(all(feature = "sanitize", any(test, feature = "testkit")))]
pub mod testkit;
#[cfg(feature = "sanitize")]
pub mod text;

use std::path::Path;

use org_proto::attachments::{
    ATTACHMENT_DOCUMENT_MAX_BYTES, ATTACHMENT_FILENAME_MAX_BYTES, ATTACHMENT_IMAGE_MAX_BYTES,
};
use org_proto::{ApiError, ErrorCode};

#[cfg(feature = "sanitize")]
pub use self::clean::check_clean;
#[cfg(feature = "sanitize")]
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

/// Alias for [`MediaType`] representing the closed attachment media allowlist.
pub type AllowedMedia = MediaType;

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

    /// The type a lowercase file extension (without leading dot) names, if allowed.
    pub fn from_extension(ext: &str) -> Option<Self> {
        let lower = ext.to_ascii_lowercase();
        ALL.into_iter()
            .find(|media| media.extensions().contains(&lower.as_str()))
    }

    pub const fn is_image(self) -> bool {
        matches!(self, Self::Png | Self::Jpeg | Self::Webp | Self::Gif)
    }

    pub const fn is_text(self) -> bool {
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

    /// The types an upload may be declared as and still be kept as `self`: an animated WebP
    /// is kept as GIF, and a still PNG or WebP too large once re-encoded is kept as JPEG.
    const fn kept_from(self) -> &'static [MediaType] {
        match self {
            Self::Gif => &[Self::Webp],
            Self::Jpeg => &[Self::Png, Self::Webp],
            _ => &[],
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

/// Validates a local file path's leaf filename and classifies it into an allowlisted
/// [`MediaType`] by extension (plain non-hidden UTF-8 name, 1..=255 bytes, no path separators or
/// control characters).
pub fn classify_filename(path: &Path) -> Result<(String, MediaType), ApiError> {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "File name must be valid UTF-8.",
        ));
    };
    if name.is_empty()
        || name.len() > ATTACHMENT_FILENAME_MAX_BYTES
        || matches!(name, "." | "..")
        || name.starts_with('.')
        || name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\'))
    {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "Filename must be a single plain non-hidden filename.",
        ));
    }
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    let media = MediaType::from_extension(&ext).ok_or_else(|| {
        unsupported(format!(
            "File extension `.{}` is not in the attachment allowlist.",
            printable(&ext)
        ))
    })?;
    Ok((name.to_owned(), media))
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

/// The name of a file an office keeps as `media` (another office's record): exactly one plain
/// name, as [`check_filename`] leaves it, with an extension of `media` or of a type an upload
/// is kept as `media` from (`photo.png` kept as JPEG). `evil.html` or `dropper.exe` named
/// `text/plain` is refused.
pub fn check_kept_filename(name: &str, media: MediaType) -> Result<(), ApiError> {
    let mut checked = check_filename(name, media);
    for source in media.kept_from() {
        if checked.is_err() {
            if let Ok(name) = check_filename(name, *source) {
                checked = Ok(name);
            }
        }
    }
    if checked? != name {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "filename: must be one plain file name",
        ));
    }
    Ok(())
}

/// A file the office accepted, as it keeps it.
#[cfg(feature = "sanitize")]
#[derive(Clone, PartialEq, Eq)]
pub struct ValidatedFile {
    /// What the bytes are now (an animated WebP is kept as an animated GIF).
    pub media: MediaType,
    pub bytes: Vec<u8>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub page_count: Option<u32>,
}

#[cfg(feature = "sanitize")]
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
#[cfg(feature = "sanitize")]
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

#[cfg(feature = "sanitize")]
fn document(media: MediaType, bytes: Vec<u8>, page_count: Option<u32>) -> ValidatedFile {
    ValidatedFile {
        media,
        bytes,
        width: None,
        height: None,
        page_count,
    }
}

pub fn unsupported(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Unsupported, message)
}

pub fn corrupt(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::InvalidRequest, message)
}

pub fn too_large(media: MediaType) -> ApiError {
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
mod tests;
