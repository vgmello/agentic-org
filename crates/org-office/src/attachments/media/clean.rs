//! Whether an image is already in the form an office keeps it (`TASK-98.6`, ADR-0037). HQ keeps
//! a satellite's shared image byte for byte as the satellite's own sanitizer left it (its
//! SHA-256 names it everywhere), so before HQ stores it, it checks that the bytes carry pixels
//! and nothing else: every metadata block (EXIF and its GPS, XMP, ICC profiles, comments, text
//! chunks, thumbnails), every block the office's encoders never write, and any byte after the
//! format's end marker is refused, and a still image asks for no orientation.
//!
//! - PNG: `IHDR` first (not interlaced, within the decode limits), `PLTE` and `tRNS` at most
//!   once each and only before the pixels, one run of `IDAT` whose compressed stream ends
//!   exactly at its last byte and inflates to exactly the rows `IHDR` declares, and an empty
//!   `IEND` straight after it.
//! - JPEG: `SOI`, one bare `JFIF` `APP0` (no thumbnail), tables, one baseline frame (`SOF0`),
//!   one scan, and `EOI` straight after it: no `APP1`..`APP15`, no `COM`.
//! - WebP: a `RIFF` holding exactly one lossless `VP8L` chunk.
//! - GIF: at most one looping (`NETSCAPE2.0`) block, frames each with at most one graphic
//!   control block right before it, and the trailer last: no comment, plain-text or other
//!   application block.

use flate2::{Decompress, FlushDecompress, Status};
use image::metadata::Orientation;
use org_proto::ApiError;

use super::image::{MAX_DIMENSION, MAX_PIXELS};
use super::{corrupt, MediaType};

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
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

/// Where the `IDAT` run stands while the chunks are walked: once it starts, only more `IDAT`
/// and then `IEND` may follow.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Idat {
    #[default]
    Before,
    In,
}

/// A PNG color type, by the samples each pixel holds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PngColor {
    Grey,
    Rgb,
    Palette,
    GreyAlpha,
    Rgba,
}

/// What a PNG's `IHDR` declares, when it is a header the office's decoder accepts: not
/// interlaced, a color type with a bit depth it allows, and within the decode limits.
#[derive(Clone, Copy)]
struct PngHeader {
    width: u32,
    height: u32,
    bit_depth: u8,
    color: PngColor,
}

impl PngHeader {
    fn parse(data: &[u8]) -> Option<Self> {
        // Compression, filter and interlace methods all 0: deflate, adaptive, not interlaced.
        let [w0, w1, w2, w3, h0, h1, h2, h3, bit_depth, color_type, 0, 0, 0] =
            <[u8; 13]>::try_from(data).ok()?
        else {
            return None;
        };
        let color = match (color_type, bit_depth) {
            (0, 1 | 2 | 4 | 8 | 16) => PngColor::Grey,
            (2, 8 | 16) => PngColor::Rgb,
            (3, 1 | 2 | 4 | 8) => PngColor::Palette,
            (4, 8 | 16) => PngColor::GreyAlpha,
            (6, 8 | 16) => PngColor::Rgba,
            _ => return None,
        };
        let (width, height) = (
            u32::from_be_bytes([w0, w1, w2, w3]),
            u32::from_be_bytes([h0, h1, h2, h3]),
        );
        let side = 1..=MAX_DIMENSION;
        let within = side.contains(&width)
            && side.contains(&height)
            && u64::from(width) * u64::from(height) <= MAX_PIXELS;
        within.then_some(Self {
            width,
            height,
            bit_depth,
            color,
        })
    }

    /// How many bytes the pixel stream inflates to: each row's filter byte and its packed
    /// samples (exact, as nothing is interlaced; at most about 512 MB within the limits).
    fn inflated_len(self) -> u64 {
        let samples: u64 = match self.color {
            PngColor::Grey | PngColor::Palette => 1,
            PngColor::GreyAlpha => 2,
            PngColor::Rgb => 3,
            PngColor::Rgba => 4,
        };
        let row = 1 + (u64::from(self.width) * samples * u64::from(self.bit_depth)).div_ceil(8);
        u64::from(self.height) * row
    }

    /// Whether a `PLTE` of `len` bytes fits: whole entries, no more than the bit depth indexes.
    fn fits_palette(self, len: usize) -> bool {
        let entries = len / 3;
        self.color == PngColor::Palette
            && len.is_multiple_of(3)
            && (1..=(1usize << self.bit_depth)).contains(&entries)
    }

    /// Whether a `tRNS` of `len` bytes fits, after a palette of `palette` entries if any.
    fn fits_transparency(self, len: usize, palette: Option<usize>) -> bool {
        match self.color {
            PngColor::Grey => len == 2,
            PngColor::Rgb => len == 6,
            PngColor::Palette => palette.is_some_and(|entries| (1..=entries).contains(&len)),
            PngColor::GreyAlpha | PngColor::Rgba => false,
        }
    }
}

/// The chunk at the start of `rest`: its type, its data, and what follows its CRC.
fn png_chunk(rest: &[u8]) -> Option<([u8; 4], &[u8], &[u8])> {
    let len = be32(rest, 0)?;
    let kind = rest.get(4..8)?.try_into().ok()?;
    let data = rest.get(8..len.checked_add(8)?)?;
    let after = rest.get(len.checked_add(12)?..)?;
    Some((kind, data, after))
}

/// The chunks after `IHDR`, as they are walked in order.
#[derive(Default)]
struct PngWalk {
    /// How many entries the `PLTE` holds, once it is seen.
    palette: Option<usize>,
    transparency: bool,
    idat_state: Idat,
    /// The pixel stream, every `IDAT` of the run joined.
    idat: Vec<u8>,
}

impl PngWalk {
    /// Takes the next chunk, `kind` holding `data`; `true` when it is the (empty) end.
    fn take(&mut self, header: PngHeader, kind: [u8; 4], data: &[u8]) -> Result<bool, ApiError> {
        match (&kind, self.idat_state) {
            (b"IDAT", _) => {
                self.idat_state = Idat::In;
                self.idat.extend_from_slice(data);
                Ok(false)
            }
            // Straight after the last `IDAT`, and empty.
            (b"IEND", Idat::In) if data.is_empty() => Ok(true),
            (b"IEND", Idat::In) => Err(not_clean("data inside its end block")),
            (b"IHDR" | b"PLTE" | b"tRNS", Idat::In) => Err(not_clean("a block after its pixels")),
            (b"PLTE" | b"tRNS", Idat::Before) if self.take_table(header, &kind, data.len()) => {
                Ok(false)
            }
            (b"IHDR" | b"PLTE" | b"tRNS" | b"IEND", _) => Err(damaged(MediaType::Png)),
            _ => Err(not_clean(&format!(
                "a `{}` block",
                String::from_utf8_lossy(&kind)
            ))),
        }
    }

    /// Takes a `PLTE` or `tRNS` of `len` bytes before the pixels: whether it fits, once each
    /// and the palette first.
    fn take_table(&mut self, header: PngHeader, kind: &[u8; 4], len: usize) -> bool {
        if kind == b"PLTE" && self.palette.is_none() && !self.transparency {
            self.palette = header.fits_palette(len).then_some(len / 3);
            return self.palette.is_some();
        }
        let fits =
            kind == b"tRNS" && !self.transparency && header.fits_transparency(len, self.palette);
        self.transparency |= fits;
        fits
    }

    /// Refuses pixels that are missing, lack their palette, or are not exactly the one zlib
    /// stream of the rows `header` declares.
    fn finish(self, header: PngHeader) -> Result<(), ApiError> {
        if self.idat.is_empty() || (header.color == PngColor::Palette && self.palette.is_none()) {
            return Err(damaged(MediaType::Png));
        }
        if !zlib_ends_exactly(&self.idat, header.inflated_len()) {
            return Err(not_clean("data hidden after its pixels"));
        }
        Ok(())
    }
}

fn png(raw: &[u8]) -> Result<(), ApiError> {
    let damaged = || damaged(MediaType::Png);
    let rest = raw
        .strip_prefix(PNG_SIGNATURE.as_slice())
        .ok_or_else(damaged)?;
    let (kind, data, mut rest) = png_chunk(rest).ok_or_else(damaged)?;
    if kind != *b"IHDR" {
        return Err(damaged());
    }
    let header = PngHeader::parse(data).ok_or_else(damaged)?;
    let mut walk = PngWalk::default();
    loop {
        let (kind, data, after) = png_chunk(rest).ok_or_else(damaged)?;
        rest = after;
        if walk.take(header, kind, data)? {
            break;
        }
    }
    if !rest.is_empty() {
        return Err(not_clean("data after its end"));
    }
    walk.finish(header)
}

/// Whether `stream` is one zlib stream that ends at its very last byte and inflates to exactly
/// `expected` bytes. It gives up as soon as the output passes `expected`, so a stream that
/// would inflate far past it (a zlib bomb) costs at most one window more than `expected`.
fn zlib_ends_exactly(stream: &[u8], expected: u64) -> bool {
    let mut inflate = Decompress::new(true);
    let mut out = vec![0u8; INFLATE_WINDOW];
    loop {
        let (read, written) = (inflate.total_in(), inflate.total_out());
        let Some(input) = usize::try_from(read).ok().and_then(|at| stream.get(at..)) else {
            return false;
        };
        let status = inflate.decompress(input, &mut out, FlushDecompress::None);
        let (now_read, now_written) = (inflate.total_in(), inflate.total_out());
        match status {
            _ if now_written > expected => return false,
            Ok(Status::StreamEnd) => {
                return now_read == stream.len() as u64 && now_written == expected
            }
            // No progress: the stream stops short of its end.
            Ok(_) if now_read == read && now_written == written => return false,
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
    let mut frame = false;
    loop {
        if raw.get(at) != Some(&0xFF) {
            return Err(damaged());
        }
        let marker = *raw.get(at + 1).ok_or_else(damaged)?;
        at += 2;
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
            // The one baseline frame, before its scan.
            0xC0 if !frame => frame = true,
            // Huffman and quantization tables, restart interval.
            0xC4 | 0xDB | 0xDD => {}
            // The one scan, and the end straight after it: nothing between them.
            0xDA if frame => {
                let end = after_scan(raw, at + len).ok_or_else(damaged)?;
                return match raw.get(end..) {
                    Some([0xFF, 0xD9]) => Ok(()),
                    Some([0xFF, 0xD9, ..]) => Err(not_clean("data after its end")),
                    _ => Err(not_clean("a block after its pixels")),
                };
            }
            0xDA => return Err(damaged()),
            0xC0..=0xCF => return Err(not_clean("a second or non-baseline frame")),
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
    // Whether a graphic control block waits for the frame right after it.
    let mut control = false;
    let orphan = || not_clean("a graphic control block without its frame");
    loop {
        match raw.get(at) {
            Some(0x3B) if control => return Err(orphan()),
            Some(0x3B) => {
                return if at + 1 == raw.len() {
                    Ok(())
                } else {
                    Err(not_clean("data after its end"))
                }
            }
            Some(0x2C) => {
                control = false;
                let flags = *raw.get(at + 9).ok_or_else(damaged)?;
                // The descriptor, its color table and the LZW code size, then the data.
                at = after_sub_blocks(raw, at + 11 + color_table_len(flags)).ok_or_else(damaged)?;
            }
            Some(0x21) if control => return Err(orphan()),
            Some(0x21) => {
                control = raw.get(at + 1) == Some(&0xF9);
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
