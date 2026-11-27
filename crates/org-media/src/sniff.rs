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

/// HTML or SVG at the start of a text file: after a byte-order mark, white space, and any
/// comments, XML declarations or other `<!`/`<?` constructs before it, each skipped where an
/// HTML parser ends it (so `<!-- x --><html><script>` is HTML, not text).
fn markup(bytes: &[u8]) -> Sniffed {
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
        rest = skip_white(rest);
        let head = lowered(rest, 16);
        if HTML.iter().any(|tag| head.starts_with(tag)) {
            return Sniffed::Html;
        }
        if head.starts_with(b"<svg") {
            return Sniffed::Svg;
        }
        match past_prolog_item(rest) {
            Some(after) => rest = after,
            None => break,
        }
    }
    // An XML file with an `<svg` element anywhere near its start is SVG.
    let head = lowered(skip_white(lead), 1024);
    if (head.starts_with(b"<?xml") || head.starts_with(b"<!--")) && contains(&head, b"<svg") {
        return Sniffed::Svg;
    }
    Sniffed::Unknown
}

fn skip_white(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    &bytes[start..]
}

/// At most `n` bytes of `bytes`, ASCII-lowercased.
fn lowered(bytes: &[u8], n: usize) -> Vec<u8> {
    bytes[..bytes.len().min(n)].to_ascii_lowercase()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    find(haystack, needle).is_some()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// What follows the comment (`<!-- -->`) or other `<!`/`<?` construct `bytes` starts with,
/// ended where an HTML parser ends it; `None` when it starts with none, or one that runs to
/// the end of the file (all of the rest is then inside it).
fn past_prolog_item(bytes: &[u8]) -> Option<&[u8]> {
    if let Some(comment) = bytes.strip_prefix(b"<!--") {
        // `<!-->` and `<!--->` end at once; otherwise the first `-->` or `--!>` ends it.
        if let Some(after) = comment.strip_prefix(b">") {
            return Some(after);
        }
        if let Some(after) = comment.strip_prefix(b"->") {
            return Some(after);
        }
        // One pass to the first `--` that closes: the scan stays linear in the file.
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
        // A doctype, a bogus comment or a processing instruction: the first `>` ends it.
        let end = bytes.iter().position(|&b| b == b'>')?;
        return Some(&bytes[end + 1..]);
    }
    None
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
