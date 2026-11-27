//! What the browser may send and what a download is served as (`TASK-98.5`, ADR-0037).
//!
//! Upload request headers are checked here before any body byte is read or any office call
//! is made: a closed `Content-Type` allowlist (everything else, including multipart, HTML,
//! SVG, scripts and untyped binaries, is `415`), one percent-encoded file name, and one exact
//! `Content-Length` within the type's cap. The office still validates every byte and the
//! file name's extension; this is the gateway's own refusal, never its replacement.
//!
//! Downloads are served under fixed headers: a canonical type (raster images `inline`,
//! everything else `attachment`), a `Content-Disposition` that never carries a raw quote,
//! backslash or control character, and a sandboxing policy.

use hyper::header::{HeaderMap, HeaderValue, CONTENT_LENGTH, CONTENT_TYPE, TRANSFER_ENCODING};
use hyper::StatusCode;
use org_proto::attachments::{
    ATTACHMENT_DOCUMENT_MAX_BYTES, ATTACHMENT_FILENAME_MAX_BYTES, ATTACHMENT_IMAGE_MAX_BYTES,
};
use org_proto::web::WebErrorCode;

use crate::service::ApiReply;

/// The request header that names the uploaded file: its UTF-8 name, percent-encoded
/// (`encodeURIComponent`), so any name fits a header value.
pub(crate) const FILENAME_HEADER: &str = "x-attachment-filename";

const RASTER_IMAGES: [&str; 4] = ["image/png", "image/jpeg", "image/webp", "image/gif"];
const DOCUMENTS: [&str; 3] = [
    "application/pdf",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
];
/// Text documents: the only types a `charset=utf-8` parameter may qualify.
const TEXT_DOCUMENTS: [&str; 4] = [
    "text/markdown",
    "text/plain",
    "text/csv",
    "application/json",
];

/// One accepted upload type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UploadMedia {
    /// The canonical MIME type, as the office stores it.
    pub(crate) mime: &'static str,
    /// The largest upload of this type.
    pub(crate) max_bytes: u64,
}

fn unsupported() -> ApiReply {
    ApiReply::error(
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        WebErrorCode::InvalidRequest,
        "This file type is not accepted.",
    )
}

fn invalid(message: &'static str) -> ApiReply {
    ApiReply::error(
        StatusCode::BAD_REQUEST,
        WebErrorCode::InvalidRequest,
        message,
    )
}

/// The upload's type from its one `Content-Type`: an allowlisted image or document, with
/// at most one `charset=utf-8` parameter on a text type. Anything else is `415`.
pub(crate) fn upload_media(headers: &HeaderMap) -> Result<UploadMedia, ApiReply> {
    let mut values = headers.get_all(CONTENT_TYPE).iter();
    let (Some(value), None) = (values.next(), values.next()) else {
        return Err(unsupported());
    };
    let value = value.to_str().map_err(|_| unsupported())?;
    let mut parts = value.split(';');
    let essence = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
    let (mime, max_bytes, text) = if let Some(mime) = RASTER_IMAGES.iter().find(|m| **m == essence)
    {
        (*mime, ATTACHMENT_IMAGE_MAX_BYTES, false)
    } else if let Some(mime) = DOCUMENTS.iter().find(|m| **m == essence) {
        (*mime, ATTACHMENT_DOCUMENT_MAX_BYTES, false)
    } else if let Some(mime) = TEXT_DOCUMENTS.iter().find(|m| **m == essence) {
        (*mime, ATTACHMENT_DOCUMENT_MAX_BYTES, true)
    } else {
        return Err(unsupported());
    };
    match (parts.next(), parts.next()) {
        (None, _) => Ok(UploadMedia { mime, max_bytes }),
        (Some(parameter), None) if text && utf8_charset(parameter) => {
            Ok(UploadMedia { mime, max_bytes })
        }
        _ => Err(unsupported()),
    }
}

/// `charset=utf-8`, its value bare or in exactly one pair of quotes.
fn utf8_charset(parameter: &str) -> bool {
    parameter.split_once('=').is_some_and(|(name, value)| {
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|quoted| quoted.strip_suffix('"'))
            .unwrap_or(value);
        name.trim().eq_ignore_ascii_case("charset") && value.eq_ignore_ascii_case("utf-8")
    })
}

/// The file name from its one percent-encoded header: 1 to 255 bytes of UTF-8, one plain
/// name (no folder, no leading dot, no control or bidirectional override character, which
/// could show a name other than the stored one).
pub(crate) fn upload_filename(headers: &HeaderMap) -> Result<String, ApiReply> {
    const RULE: &str = "X-Attachment-Filename must be one percent-encoded file name.";
    let mut values = headers.get_all(FILENAME_HEADER).iter();
    let (Some(value), None) = (values.next(), values.next()) else {
        return Err(invalid(RULE));
    };
    let raw = value.to_str().map_err(|_| invalid(RULE))?;
    let name = percent_decode(raw).ok_or_else(|| invalid(RULE))?;
    if name.is_empty()
        || name.len() > ATTACHMENT_FILENAME_MAX_BYTES
        || name.trim() != name
        || name.starts_with('.')
        || name.chars().any(|c| {
            c.is_control()
                || matches!(c, '/' | '\\' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
        })
    {
        return Err(invalid(RULE));
    }
    Ok(name)
}

/// Decodes `%XX` escapes exactly once; any other byte must be visible ASCII.
fn percent_decode(raw: &str) -> Option<String> {
    let mut bytes = raw.bytes();
    let mut out = Vec::with_capacity(raw.len());
    while let Some(byte) = bytes.next() {
        match byte {
            b'%' => {
                let high = (bytes.next()? as char).to_digit(16)?;
                let low = (bytes.next()? as char).to_digit(16)?;
                out.push(u8::try_from(high * 16 + low).ok()?);
            }
            b'!'..=b'~' => out.push(byte),
            _ => return None,
        }
    }
    String::from_utf8(out).ok()
}

/// The body's exact length: one `Content-Length` (no `Transfer-Encoding`), 1 byte to the
/// type's cap. Missing is `411`; over the cap is `413`, before any byte is read.
pub(crate) fn upload_length(headers: &HeaderMap, media: UploadMedia) -> Result<u64, ApiReply> {
    let length_required = || {
        ApiReply::error(
            StatusCode::LENGTH_REQUIRED,
            WebErrorCode::InvalidRequest,
            "Content-Length is required.",
        )
    };
    if headers.contains_key(TRANSFER_ENCODING) {
        return Err(length_required());
    }
    let mut values = headers.get_all(CONTENT_LENGTH).iter();
    let (Some(value), None) = (values.next(), values.next()) else {
        return Err(length_required());
    };
    let text = value.to_str().map_err(|_| length_required())?;
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(length_required());
    }
    let length: u64 = text.parse().map_err(|_| too_large())?;
    if length == 0 {
        return Err(invalid("An attachment holds at least 1 byte."));
    }
    if length > media.max_bytes {
        return Err(too_large());
    }
    Ok(length)
}

fn too_large() -> ApiReply {
    ApiReply::error(
        StatusCode::PAYLOAD_TOO_LARGE,
        WebErrorCode::TooLarge,
        "This file is larger than its type allows.",
    )
}

/// How a stored file is served: its canonical type (text with an explicit UTF-8 charset),
/// and whether it may display `inline` (raster images only). A type outside the allowlist
/// is served as an untyped download.
pub(crate) fn served_type(mime: &str) -> (HeaderValue, bool) {
    if let Some(image) = RASTER_IMAGES.iter().find(|m| **m == mime) {
        return (HeaderValue::from_static(image), true);
    }
    if let Some(document) = DOCUMENTS.iter().find(|m| **m == mime) {
        return (HeaderValue::from_static(document), false);
    }
    let text = match mime {
        "text/markdown" => "text/markdown; charset=utf-8",
        "text/plain" => "text/plain; charset=utf-8",
        "text/csv" => "text/csv; charset=utf-8",
        "application/json" => "application/json; charset=utf-8",
        _ => "application/octet-stream",
    };
    (HeaderValue::from_static(text), false)
}

/// `inline` or `attachment`, with an ASCII-only quoted fallback name (anything outside a
/// small safe set becomes `_`) and the exact name as RFC 5987 `filename*`.
pub(crate) fn content_disposition(filename: &str, inline: bool) -> HeaderValue {
    let kind = if inline { "inline" } else { "attachment" };
    let fallback: String = filename
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ' | '(' | ')' | '+') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let fallback = match fallback.trim() {
        "" => "download",
        name => name,
    };
    let mut encoded = String::with_capacity(filename.len() * 3);
    for byte in filename.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'!' | b'#' | b'$' | b'&' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~'
            )
        {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    HeaderValue::from_str(&format!(
        "{kind}; filename=\"{fallback}\"; filename*=UTF-8''{encoded}"
    ))
    .unwrap_or_else(|_| HeaderValue::from_static("attachment"))
}
