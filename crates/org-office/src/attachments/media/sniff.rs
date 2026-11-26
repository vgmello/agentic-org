//! What a file's first bytes say it is, compared with what its uploader declared. A file whose
//! magic bytes name another type (an executable called `notes.txt`, a ZIP called `photo.png`,
//! HTML called `readme.md`) is refused before any decoder sees it.

use org_proto::ApiError;

use super::{unsupported, MediaType};

/// What the leading bytes look like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sniffed {
    Png,
    Jpeg,
    Gif,
    Webp,
    Pdf,
    Zip,
    Elf,
    MachO,
    Pe,
    Html,
    Svg,
    /// None of the above: plain text, or something no rule recognizes.
    Unknown,
}

impl Sniffed {
    fn is_executable(self) -> bool {
        matches!(self, Self::Elf | Self::MachO | Self::Pe)
    }
}

/// The type `bytes` begin like.
pub fn detect(bytes: &[u8]) -> Sniffed {
    let starts = |magic: &[u8]| bytes.starts_with(magic);
    if starts(b"\x89PNG\r\n\x1a\n") {
        Sniffed::Png
    } else if starts(&[0xff, 0xd8, 0xff]) {
        Sniffed::Jpeg
    } else if starts(b"GIF87a") || starts(b"GIF89a") {
        Sniffed::Gif
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Sniffed::Webp
    } else if starts(b"%PDF-") {
        Sniffed::Pdf
    } else if starts(b"PK\x03\x04") || starts(b"PK\x05\x06") || starts(b"PK\x07\x08") {
        Sniffed::Zip
    } else if starts(b"\x7fELF") {
        Sniffed::Elf
    } else if [
        [0xfe, 0xed, 0xfa, 0xce],
        [0xfe, 0xed, 0xfa, 0xcf],
        [0xce, 0xfa, 0xed, 0xfe],
        [0xcf, 0xfa, 0xed, 0xfe],
        [0xca, 0xfe, 0xba, 0xbe],
    ]
    .iter()
    .any(|magic| starts(magic))
    {
        Sniffed::MachO
    } else if is_pe(bytes) {
        Sniffed::Pe
    } else {
        markup(bytes)
    }
}

/// A Windows executable: `MZ` with the usual DOS stub, or `MZ` whose header points at a
/// `PE\0\0` signature (text that merely begins with "MZ" is not one).
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

/// HTML or SVG at the start of a text file (after a byte-order mark, white space and
/// comments or an XML declaration).
fn markup(bytes: &[u8]) -> Sniffed {
    let head = &bytes[..bytes.len().min(1024)];
    let text = String::from_utf8_lossy(head).to_ascii_lowercase();
    let text = text.trim_start_matches('\u{feff}').trim_start();
    let html = [
        "<!doctype html",
        "<html",
        "<head",
        "<body",
        "<script",
        "<iframe",
    ];
    if html.iter().any(|tag| text.starts_with(tag)) {
        return Sniffed::Html;
    }
    if text.starts_with("<svg")
        || ((text.starts_with("<?xml") || text.starts_with("<!--")) && text.contains("<svg"))
    {
        return Sniffed::Svg;
    }
    Sniffed::Unknown
}

/// Refuses `bytes` unless they begin as `declared` says.
pub fn check_matches(declared: MediaType, bytes: &[u8]) -> Result<(), ApiError> {
    let found = detect(bytes);
    if found.is_executable() {
        return Err(unsupported("executables are not accepted as attachments"));
    }
    let expected = match declared {
        MediaType::Png => Some(Sniffed::Png),
        MediaType::Jpeg => Some(Sniffed::Jpeg),
        MediaType::Gif => Some(Sniffed::Gif),
        MediaType::Webp => Some(Sniffed::Webp),
        MediaType::Pdf => Some(Sniffed::Pdf),
        MediaType::Docx | MediaType::Xlsx => Some(Sniffed::Zip),
        MediaType::Markdown | MediaType::PlainText | MediaType::Csv | MediaType::Json => None,
    };
    let matches = match expected {
        Some(expected) => found == expected,
        // Text has no magic: it must not look like any binary or markup type.
        None => found == Sniffed::Unknown,
    };
    if matches {
        Ok(())
    } else {
        Err(unsupported(format!(
            "the file's content does not match its declared type `{}`",
            declared.mime()
        )))
    }
}
