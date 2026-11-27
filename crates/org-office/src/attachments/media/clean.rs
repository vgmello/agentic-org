//! Whether an image is already in the form an office keeps it (`TASK-98.6`, ADR-0037). HQ keeps
//! a satellite's shared image byte for byte as the satellite's own sanitizer left it (its
//! SHA-256 names it everywhere), so before HQ stores it, it checks that the bytes carry pixels
//! and nothing else: every metadata block (EXIF and its GPS, XMP, ICC profiles, comments, text
//! chunks, thumbnails), every block the office's encoders never write, and any byte after the
//! format's end marker is refused, and a still image asks for no orientation.
//!
//! - PNG: `IHDR`, `PLTE`, `tRNS`, one run of `IDAT` whose compressed stream ends exactly at its
//!   last byte, and `IEND` last.
//! - JPEG: `SOI`, one bare `JFIF` `APP0` (no thumbnail), tables, frames and scans, and `EOI`
//!   last: no `APP1`..`APP15`, no `COM`.
//! - WebP: a `RIFF` holding exactly one lossless `VP8L` chunk.
//! - GIF: graphic control blocks, at most one looping (`NETSCAPE2.0`) block, frames, and the
//!   trailer last: no comment, plain-text or other application block.

use flate2::{Decompress, FlushDecompress, Status};
use image::metadata::Orientation;
use org_proto::ApiError;

use super::{corrupt, MediaType};

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
/// The only PNG chunks the office's encoder writes.
const PNG_KEPT: [[u8; 4]; 5] = [*b"IHDR", *b"PLTE", *b"tRNS", *b"IDAT", *b"IEND"];
/// How much one step of the `IDAT` stream check inflates at a time (output is discarded).
const INFLATE_WINDOW: usize = 64 * 1024;

/// The refusal for an image that is not as an office keeps it.
fn not_clean(what: &str) -> ApiError {
    corrupt(format!(
        "the image carries {what}: only an image as an office keeps it (pixels alone) is accepted"
    ))
}

fn damaged(media: MediaType) -> ApiError {
    corrupt(format!("the `{}` image is damaged", media.mime()))
}

/// Refuses an image (`media`, sniffed as such) whose bytes carry anything but pixels; any
/// other type passes.
pub fn check_clean(media: MediaType, raw: &[u8]) -> Result<(), ApiError> {
    match media {
        MediaType::Png => png(raw)?,
        MediaType::Jpeg => jpeg(raw)?,
        MediaType::Webp => webp(raw)?,
        MediaType::Gif => gif(raw)?,
        _ => return Ok(()),
    }
    if media != MediaType::Gif
        && super::image::orientation(media, raw)? != Orientation::NoTransforms
    {
        return Err(not_clean("an orientation"));
    }
    Ok(())
}

fn be32(bytes: &[u8], at: usize) -> Option<usize> {
    let word: [u8; 4] = bytes.get(at..at.checked_add(4)?)?.try_into().ok()?;
    usize::try_from(u32::from_be_bytes(word)).ok()
}

fn le32(bytes: &[u8], at: usize) -> Option<usize> {
    let word: [u8; 4] = bytes.get(at..at.checked_add(4)?)?.try_into().ok()?;
    usize::try_from(u32::from_le_bytes(word)).ok()
}

fn be16(bytes: &[u8], at: usize) -> Option<usize> {
    let word: [u8; 2] = bytes.get(at..at.checked_add(2)?)?.try_into().ok()?;
    Some(usize::from(u16::from_be_bytes(word)))
}

/// Where the `IDAT` run stands while the chunks are walked.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Idat {
    Before,
    In,
    After,
}

fn png(raw: &[u8]) -> Result<(), ApiError> {
    let damaged = || damaged(MediaType::Png);
    let mut rest = raw
        .strip_prefix(PNG_SIGNATURE.as_slice())
        .ok_or_else(damaged)?;
    let mut first = true;
    let mut idat_state = Idat::Before;
    let mut idat = Vec::new();
    loop {
        let len = be32(rest, 0).ok_or_else(damaged)?;
        let kind: [u8; 4] = rest
            .get(4..8)
            .and_then(|kind| kind.try_into().ok())
            .ok_or_else(damaged)?;
        let end = len.checked_add(12).ok_or_else(damaged)?;
        let data = rest.get(8..8 + len).ok_or_else(damaged)?;
        if rest.len() < end {
            return Err(damaged());
        }
        if !PNG_KEPT.contains(&kind) {
            return Err(not_clean(&format!(
                "a `{}` block",
                String::from_utf8_lossy(&kind)
            )));
        }
        if first != (kind == *b"IHDR") {
            return Err(damaged());
        }
        first = false;
        idat_state = match (idat_state, kind == *b"IDAT") {
            (Idat::After, true) => return Err(damaged()),
            (_, true) => Idat::In,
            (Idat::In, false) => Idat::After,
            (state, false) => state,
        };
        if kind == *b"IDAT" {
            idat.extend_from_slice(data);
        }
        rest = &rest[end..];
        if kind == *b"IEND" {
            break;
        }
    }
    if !rest.is_empty() {
        return Err(not_clean("data after its end"));
    }
    if idat.is_empty() {
        return Err(damaged());
    }
    if !zlib_ends_exactly(&idat) {
        return Err(not_clean("data hidden after its pixels"));
    }
    Ok(())
}

/// Whether `stream` is one zlib stream that ends at its very last byte.
fn zlib_ends_exactly(stream: &[u8]) -> bool {
    let mut inflate = Decompress::new(true);
    let mut out = vec![0u8; INFLATE_WINDOW];
    loop {
        let (read, written) = (inflate.total_in(), inflate.total_out());
        let Some(input) = usize::try_from(read).ok().and_then(|at| stream.get(at..)) else {
            return false;
        };
        match inflate.decompress(input, &mut out, FlushDecompress::None) {
            Ok(Status::StreamEnd) => return inflate.total_in() == stream.len() as u64,
            // No progress: the stream stops short of its end.
            Ok(_) if inflate.total_in() == read && inflate.total_out() == written => return false,
            Ok(_) => {}
            Err(_) => return false,
        }
    }
}

fn jpeg(raw: &[u8]) -> Result<(), ApiError> {
    let damaged = || damaged(MediaType::Jpeg);
    if !raw.starts_with(&[0xFF, 0xD8]) {
        return Err(damaged());
    }
    let mut at = 2;
    let mut jfif = false;
    loop {
        if raw.get(at) != Some(&0xFF) {
            return Err(damaged());
        }
        let marker = *raw.get(at + 1).ok_or_else(damaged)?;
        at += 2;
        if marker == 0xD9 {
            return if at == raw.len() {
                Ok(())
            } else {
                Err(not_clean("data after its end"))
            };
        }
        let len = be16(raw, at).filter(|len| *len >= 2).ok_or_else(damaged)?;
        let segment = raw.get(at + 2..at + len).ok_or_else(damaged)?;
        match marker {
            // One bare JFIF header: exactly its 14 bytes, no thumbnail.
            0xE0 if !jfif => {
                if len != 16 || !segment.starts_with(b"JFIF\0") || segment[12..14] != [0, 0] {
                    return Err(not_clean("a JFIF thumbnail or extension"));
                }
                jfif = true;
            }
            // Frames, Huffman and quantization tables, restart interval.
            0xC0..=0xC2 | 0xC4 | 0xDB | 0xDD => {}
            0xDA => {
                at = after_scan(raw, at + len).ok_or_else(damaged)?;
                continue;
            }
            0xE0..=0xEF => return Err(not_clean("a metadata block (EXIF, XMP, ICC or other)")),
            0xFE => return Err(not_clean("a comment")),
            _ => return Err(not_clean("a block the office never writes")),
        }
        at += len;
    }
}

/// Where the entropy-coded data starting at `at` ends: the next marker that is neither a
/// stuffed `FF 00` nor a restart marker.
fn after_scan(raw: &[u8], mut at: usize) -> Option<usize> {
    loop {
        at += raw.get(at..)?.iter().position(|byte| *byte == 0xFF)?;
        match raw.get(at + 1)? {
            0x00 | 0xD0..=0xD7 => at += 2,
            _ => return Some(at),
        }
    }
}

fn webp(raw: &[u8]) -> Result<(), ApiError> {
    let damaged = || damaged(MediaType::Webp);
    if raw.get(0..4) != Some(b"RIFF".as_slice()) || raw.get(8..12) != Some(b"WEBP".as_slice()) {
        return Err(damaged());
    }
    let riff_end = le32(raw, 4)
        .and_then(|len| len.checked_add(8))
        .ok_or_else(damaged)?;
    if riff_end > raw.len() {
        return Err(damaged());
    }
    if riff_end < raw.len() {
        return Err(not_clean("data after its end"));
    }
    let kind = raw.get(12..16).ok_or_else(damaged)?;
    if kind != b"VP8L" {
        return Err(not_clean(&format!(
            "a `{}` block",
            String::from_utf8_lossy(kind)
        )));
    }
    let len = le32(raw, 16).ok_or_else(damaged)?;
    let end = len
        .checked_add(20 + (len & 1))
        .filter(|end| *end <= raw.len())
        .ok_or_else(damaged)?;
    if end != raw.len() {
        return Err(not_clean("a block after its pixels"));
    }
    Ok(())
}

/// The bytes of the color table a GIF descriptor's `flags` announce.
fn color_table_len(flags: u8) -> usize {
    if flags & 0x80 == 0 {
        0
    } else {
        3 << ((flags & 0x07) + 1)
    }
}

/// Where the data sub-blocks starting at `at` end (past their terminator).
fn after_sub_blocks(raw: &[u8], mut at: usize) -> Option<usize> {
    loop {
        let len = usize::from(*raw.get(at)?);
        at += 1;
        if len == 0 {
            return Some(at);
        }
        at += len;
    }
}

fn gif(raw: &[u8]) -> Result<(), ApiError> {
    let damaged = || damaged(MediaType::Gif);
    if !(raw.starts_with(b"GIF89a") || raw.starts_with(b"GIF87a")) {
        return Err(damaged());
    }
    let mut at = 13 + color_table_len(*raw.get(10).ok_or_else(damaged)?);
    let mut looped = false;
    loop {
        match raw.get(at) {
            Some(0x3B) => {
                return if at + 1 == raw.len() {
                    Ok(())
                } else {
                    Err(not_clean("data after its end"))
                }
            }
            Some(0x2C) => {
                let flags = *raw.get(at + 9).ok_or_else(damaged)?;
                // The descriptor, its color table and the LZW code size, then the data.
                at = after_sub_blocks(raw, at + 11 + color_table_len(flags)).ok_or_else(damaged)?;
            }
            Some(0x21) => {
                at = gif_extension(raw, at + 1, &mut looped)?;
            }
            _ => return Err(damaged()),
        }
    }
}

/// Checks the extension block whose label is at `at`; returns where it ends.
fn gif_extension(raw: &[u8], at: usize, looped: &mut bool) -> Result<usize, ApiError> {
    let damaged = || damaged(MediaType::Gif);
    match *raw.get(at).ok_or_else(damaged)? {
        // A graphic control block: one 4-byte block and its terminator.
        0xF9 => {
            if raw.get(at + 1) != Some(&4) || raw.get(at + 6) != Some(&0) {
                return Err(damaged());
            }
            Ok(at + 7)
        }
        // The looping block, once: `NETSCAPE2.0` and one 3-byte loop count.
        0xFF if !*looped => {
            let id = raw.get(at + 1..at + 13).ok_or_else(damaged)?;
            if id[0] != 11 || !matches!(&id[1..], b"NETSCAPE2.0" | b"ANIMEXTS1.0") {
                return Err(not_clean("an application block (XMP or other)"));
            }
            let body = raw.get(at + 13..at + 18).ok_or_else(damaged)?;
            if body[0] != 3 || body[1] != 1 || body[4] != 0 {
                return Err(not_clean("an application block (XMP or other)"));
            }
            *looped = true;
            Ok(at + 18)
        }
        0xFE => Err(not_clean("a comment")),
        _ => Err(not_clean("an extension block the office never writes")),
    }
}
