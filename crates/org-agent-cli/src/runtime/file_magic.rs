//! Extension allowlist and magic-byte pre-check for local attachment uploads (`TASK-98.4`,
//! ADR-0037). Runs BEFORE opening an `attachment.upload_begin` RPC so disallowed types,
//! executables, HTML, SVG, and spoofed binaries fail closed locally.

use std::path::Path;

use org_command::Refusal;
use org_proto::attachments::{
    ATTACHMENT_DOCUMENT_MAX_BYTES, ATTACHMENT_FILENAME_MAX_BYTES, ATTACHMENT_IMAGE_MAX_BYTES,
};
use org_proto::ErrorCode;

const DOCX_MIME: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
const XLSX_MIME: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AllowedMedia {
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

impl AllowedMedia {
    fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "webp" => Some(Self::Webp),
            "gif" => Some(Self::Gif),
            "pdf" => Some(Self::Pdf),
            "docx" => Some(Self::Docx),
            "xlsx" => Some(Self::Xlsx),
            "md" | "markdown" => Some(Self::Markdown),
            "txt" | "text" | "log" => Some(Self::PlainText),
            "csv" => Some(Self::Csv),
            "json" => Some(Self::Json),
            _ => None,
        }
    }

    pub(super) const fn mime(self) -> &'static str {
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

    const fn is_image(self) -> bool {
        matches!(self, Self::Png | Self::Jpeg | Self::Webp | Self::Gif)
    }

    pub(super) const fn max_bytes(self) -> u64 {
        if self.is_image() {
            ATTACHMENT_IMAGE_MAX_BYTES
        } else {
            ATTACHMENT_DOCUMENT_MAX_BYTES
        }
    }
}

pub(super) fn classify_filename(path: &Path) -> Result<(String, AllowedMedia), Refusal> {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "File name must be valid UTF-8.",
            "Rename the file to a plain UTF-8 filename with an allowed extension.",
        ));
    };
    if name.is_empty()
        || name.len() > ATTACHMENT_FILENAME_MAX_BYTES
        || name.starts_with('.')
        || name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\'))
    {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "Filename must be a single plain non-hidden filename.",
            "Rename the file to a plain filename (1..=255 bytes, no leading dot).",
        ));
    }
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    let media = AllowedMedia::from_extension(&ext).ok_or_else(|| {
        Refusal::new(
            ErrorCode::Unsupported,
            format!("File extension `.{ext}` is not in the attachment allowlist."),
            "Allowed types: png, jpg/jpeg, webp, gif, pdf, docx, xlsx, md, txt/log, csv, json.",
        )
    })?;
    Ok((name.to_owned(), media))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sniffed {
    Png,
    Jpeg,
    Gif,
    Webp,
    Pdf,
    Zip,
    Executable,
    Html,
    Svg,
    Unknown,
}

fn sniff(bytes: &[u8]) -> Sniffed {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Sniffed::Png
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Sniffed::Jpeg
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Sniffed::Gif
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Sniffed::Webp
    } else if bytes.starts_with(b"%PDF-") {
        Sniffed::Pdf
    } else if bytes.starts_with(b"PK\x03\x04")
        || bytes.starts_with(b"PK\x05\x06")
        || bytes.starts_with(b"PK\x07\x08")
    {
        Sniffed::Zip
    } else if bytes.starts_with(b"\x7fELF")
        || [
            [0xfe, 0xed, 0xfa, 0xce],
            [0xfe, 0xed, 0xfa, 0xcf],
            [0xce, 0xfa, 0xed, 0xfe],
            [0xcf, 0xfa, 0xed, 0xfe],
            [0xca, 0xfe, 0xba, 0xbe],
        ]
        .iter()
        .any(|m| bytes.starts_with(m))
        || is_pe(bytes)
    {
        Sniffed::Executable
    } else {
        sniff_markup(bytes)
    }
}

fn is_pe(bytes: &[u8]) -> bool {
    if !bytes.starts_with(b"MZ") {
        return false;
    }
    if bytes.starts_with(b"MZ\x90\x00") {
        return true;
    }
    let Some(offset) = bytes
        .get(0x3c..0x40)
        .map(|raw| u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize)
    else {
        return false;
    };
    bytes.get(offset..offset.saturating_add(4)) == Some(b"PE\0\0".as_slice())
}

fn sniff_markup(bytes: &[u8]) -> Sniffed {
    const HTML: [&[u8]; 6] = [
        b"<!doctype html",
        b"<html",
        b"<head",
        b"<body",
        b"<script",
        b"<iframe",
    ];
    let lead = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let mut rest = lead;
    loop {
        let start = rest
            .iter()
            .position(|b| !b.is_ascii_whitespace())
            .unwrap_or(rest.len());
        rest = &rest[start..];
        let head = rest[..rest.len().min(16)].to_ascii_lowercase();
        if HTML.iter().any(|tag| head.starts_with(tag)) {
            return Sniffed::Html;
        }
        if head.starts_with(b"<svg") {
            return Sniffed::Svg;
        }
        match past_prolog(rest) {
            Some(after) => rest = after,
            None => break,
        }
    }
    let trimmed = {
        let start = lead
            .iter()
            .position(|b| !b.is_ascii_whitespace())
            .unwrap_or(lead.len());
        &lead[start..]
    };
    let head = trimmed[..trimmed.len().min(1024)].to_ascii_lowercase();
    if (head.starts_with(b"<?xml") || head.starts_with(b"<!--"))
        && head.windows(4).any(|w| w == b"<svg")
    {
        return Sniffed::Svg;
    }
    Sniffed::Unknown
}

fn past_prolog(bytes: &[u8]) -> Option<&[u8]> {
    if let Some(comment) = bytes.strip_prefix(b"<!--") {
        if let Some(after) = comment.strip_prefix(b">") {
            return Some(after);
        }
        if let Some(after) = comment.strip_prefix(b"->") {
            return Some(after);
        }
        let end = comment
            .windows(2)
            .enumerate()
            .filter(|(_, pair)| *pair == b"--")
            .find_map(|(at, _)| match &comment[at + 2..] {
                [b'>', ..] => Some(at + 3),
                [b'!', b'>', ..] => Some(at + 4),
                _ => None,
            })?;
        return Some(&comment[end..]);
    }
    if bytes.starts_with(b"<!") || bytes.starts_with(b"<?") {
        let end = bytes.iter().position(|&b| b == b'>')?;
        return Some(&bytes[end + 1..]);
    }
    None
}

pub(super) fn check_magic_matches(media: AllowedMedia, bytes: &[u8]) -> Result<(), Refusal> {
    let found = sniff(bytes);
    if found == Sniffed::Executable {
        return Err(Refusal::new(
            ErrorCode::Unsupported,
            "Executable binaries are not accepted as attachments.",
            "Upload an allowed image or document type instead.",
        ));
    }
    let expected = match media {
        AllowedMedia::Png => Some(Sniffed::Png),
        AllowedMedia::Jpeg => Some(Sniffed::Jpeg),
        AllowedMedia::Gif => Some(Sniffed::Gif),
        AllowedMedia::Webp => Some(Sniffed::Webp),
        AllowedMedia::Pdf => Some(Sniffed::Pdf),
        AllowedMedia::Docx | AllowedMedia::Xlsx => Some(Sniffed::Zip),
        AllowedMedia::Markdown
        | AllowedMedia::PlainText
        | AllowedMedia::Csv
        | AllowedMedia::Json => None,
    };
    let ok = match expected {
        Some(want) => found == want,
        None => found == Sniffed::Unknown,
    };
    if ok {
        Ok(())
    } else {
        Err(Refusal::new(
            ErrorCode::Unsupported,
            format!(
                "File magic bytes do not match declared type `{}`.",
                media.mime()
            ),
            "Ensure the file contents match its extension and are not HTML, SVG, or a mismatched binary.",
        ))
    }
}
