//! GIF: at most one looping (`NETSCAPE2.0`) block, frames each with at most one graphic
//! control block right before it, and the trailer last: no comment, plain-text or other
//! application block.
//!
//! Each frame's LZW data is walked code by code, its code width growing as its table does: it
//! decodes to exactly the frame's pixels and ends with the end-of-information code in its last
//! byte (zero padding bits), so no sub-block rides between that code and the terminator.

use org_proto::ApiError;

use super::super::MediaType;
use super::refusal::{damaged, not_clean};

/// The widest LZW code, and so the largest table.
const MAX_CODE_WIDTH: u32 = 12;
const MAX_CODES: usize = 1 << MAX_CODE_WIDTH;

fn damaged_gif() -> ApiError {
    damaged(MediaType::Gif)
}

/// The bytes of the color table a GIF descriptor's `flags` announce.
fn color_table_len(flags: u8) -> usize {
    if flags & 0x80 == 0 {
        0
    } else {
        3 << ((flags & 0x07) + 1)
    }
}

pub(super) fn check(raw: &[u8]) -> Result<(), ApiError> {
    if !(raw.starts_with(b"GIF89a") || raw.starts_with(b"GIF87a")) {
        return Err(damaged_gif());
    }
    let mut at = 13 + color_table_len(*raw.get(10).ok_or_else(damaged_gif)?);
    let mut looped = false;
    // Whether a graphic control block waits for the frame right after it.
    let mut control = false;
    // How long each LZW code's string is, shared by every frame's walk.
    let mut lengths = vec![0u32; MAX_CODES];
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
                at = frame(raw, at, &mut lengths)?;
            }
            Some(0x21) if control => return Err(orphan()),
            Some(0x21) => {
                control = raw.get(at + 1) == Some(&0xF9);
                at = extension(raw, at + 1, &mut looped)?;
            }
            _ => return Err(damaged_gif()),
        }
    }
}

/// Checks the extension block whose label is at `at`; returns where it ends.
fn extension(raw: &[u8], at: usize, looped: &mut bool) -> Result<usize, ApiError> {
    match *raw.get(at).ok_or_else(damaged_gif)? {
        // A graphic control block: one 4-byte block and its terminator.
        0xF9 => {
            if raw.get(at + 1) != Some(&4) || raw.get(at + 6) != Some(&0) {
                return Err(damaged_gif());
            }
            Ok(at + 7)
        }
        // The looping block, once: `NETSCAPE2.0` and one 3-byte loop count.
        0xFF if !*looped => {
            let id = raw.get(at + 1..at + 13).ok_or_else(damaged_gif)?;
            if id[0] != 11 || !matches!(&id[1..], b"NETSCAPE2.0" | b"ANIMEXTS1.0") {
                return Err(not_clean("an application block (XMP or other)"));
            }
            let body = raw.get(at + 13..at + 18).ok_or_else(damaged_gif)?;
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

/// Checks the frame whose image descriptor (`0x2C`) is at `at`: its LZW data decodes to exactly
/// its `width` x `height` pixels and its end code closes it. Returns where the frame ends.
fn frame(raw: &[u8], at: usize, lengths: &mut [u32]) -> Result<usize, ApiError> {
    let descriptor = raw.get(at..at + 10).ok_or_else(damaged_gif)?;
    let width = u16::from_le_bytes([descriptor[5], descriptor[6]]);
    let height = u16::from_le_bytes([descriptor[7], descriptor[8]]);
    let at = at + 10 + color_table_len(descriptor[9]);
    // The LZW minimum code size: 2 to 8 bits a pixel.
    let min_size = *raw.get(at).ok_or_else(damaged_gif)?;
    if !(2..=8).contains(&min_size) {
        return Err(damaged_gif());
    }
    let mut codes = Codes {
        blocks: SubBlocks {
            raw,
            at: at + 1,
            left: 0,
        },
        bits: 0,
        count: 0,
    };
    let pixels = lzw_pixels(&mut codes, min_size, lengths).ok_or_else(damaged_gif)?;
    let expected = u64::from(width) * u64::from(height);
    if pixels < expected {
        return Err(damaged_gif());
    }
    let hidden = || not_clean("data hidden after its pixels");
    // Past its pixels, in the padding of the end code's byte, or in a sub-block after it.
    if pixels > expected || codes.bits != 0 || codes.blocks.left != 0 {
        return Err(hidden());
    }
    match raw.get(codes.blocks.at) {
        Some(0) => Ok(codes.blocks.at + 1),
        Some(_) => Err(hidden()),
        None => Err(damaged_gif()),
    }
}

/// The data sub-blocks from `at` (a length byte), byte by byte.
struct SubBlocks<'a> {
    raw: &'a [u8],
    at: usize,
    /// The bytes of the current sub-block not read yet.
    left: usize,
}

impl SubBlocks<'_> {
    /// The next data byte: `None` at the terminator or the end of the file.
    fn byte(&mut self) -> Option<u8> {
        if self.left == 0 {
            self.left = usize::from(*self.raw.get(self.at)?);
            if self.left == 0 {
                return None;
            }
            self.at += 1;
        }
        let byte = *self.raw.get(self.at)?;
        self.at += 1;
        self.left -= 1;
        Some(byte)
    }
}

/// LZW codes read least significant bit first from the sub-blocks.
struct Codes<'a> {
    blocks: SubBlocks<'a>,
    /// Bits read from the sub-blocks and not yet taken by a code (`count` of them).
    bits: u32,
    count: u32,
}

impl Codes<'_> {
    fn next(&mut self, width: u32) -> Option<usize> {
        while self.count < width {
            self.bits |= u32::from(self.blocks.byte()?) << self.count;
            self.count += 8;
        }
        let code = self.bits & ((1 << width) - 1);
        self.bits >>= width;
        self.count -= width;
        usize::try_from(code).ok()
    }
}

/// How many pixels a frame's LZW codes decode to, up to its end code. The table grows by one
/// string per code after the first since a clear code, the code width by one bit each time the
/// next string would not fit, until a full table stops growing (as decoders read GIF, with no
/// early change). `None` when a code names no string yet or the data ends before the end code.
/// `lengths` is scratch space for each string's length.
fn lzw_pixels(codes: &mut Codes<'_>, min_size: u8, lengths: &mut [u32]) -> Option<u64> {
    let clear = 1usize << min_size;
    let end = clear + 1;
    let first_width = u32::from(min_size) + 1;
    let mut width = first_width;
    // The string the next code defines, and the code before it (none after a clear code).
    let mut next = end;
    let mut previous: Option<usize> = None;
    let mut pixels = 0u64;
    loop {
        let code = codes.next(width)?;
        if code == clear {
            (width, next, previous) = (first_width, end, None);
            continue;
        }
        if code == end {
            return Some(pixels);
        }
        if code > next {
            return None;
        }
        let length = |code: usize, lengths: &[u32]| {
            if code < clear {
                Some(1)
            } else {
                lengths.get(code).copied()
            }
        };
        // The new string is the previous one and one more pixel (`code` itself may be the
        // string being defined). Right after a clear code `next` is the end code, so only a
        // one-pixel code passes the check above.
        if let Some(previous) = previous {
            *lengths.get_mut(next)? = length(previous, lengths)? + 1;
        }
        pixels += u64::from(length(code, lengths)?);
        previous = Some(code);
        next += 1;
        if next >= 1 << width {
            if width == MAX_CODE_WIDTH {
                // A full table: no string is added until the next clear code.
                (next, previous) = (MAX_CODES - 1, None);
            } else {
                width += 1;
            }
        }
    }
}
