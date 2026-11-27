//! PNG: `IHDR` first (not interlaced, within the decode limits), `PLTE` and `tRNS` at most once
//! each and only before the pixels, one run of `IDAT` whose compressed stream ends exactly at
//! its last byte and inflates to exactly the rows `IHDR` declares, and an empty `IEND` straight
//! after it. Every chunk's CRC-32 (over its type and data) is checked.

use flate2::{Crc, Decompress, FlushDecompress, Status};
use org_proto::ApiError;

use super::super::image::{MAX_DIMENSION, MAX_PIXELS};
use super::super::MediaType;
use super::refusal::{damaged, not_clean};

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
/// How much one step of the `IDAT` stream check inflates at a time (output is discarded).
const INFLATE_WINDOW: usize = 64 * 1024;

fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    let word: [u8; 4] = bytes.get(at..at.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_be_bytes(word))
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

/// The chunk at the start of `rest`: its type, its data, and what follows its CRC. `None` when
/// it is cut short or its CRC-32 (over its type and data) is not the one it carries.
fn chunk(rest: &[u8]) -> Option<([u8; 4], &[u8], &[u8])> {
    let len = usize::try_from(be32(rest, 0)?).ok()?;
    let kind: [u8; 4] = rest.get(4..8)?.try_into().ok()?;
    let data = rest.get(8..len.checked_add(8)?)?;
    let carried = be32(rest, len + 8)?;
    let mut crc = Crc::new();
    crc.update(&kind);
    crc.update(data);
    if crc.sum() != carried {
        return None;
    }
    let after = rest.get(len + 12..)?;
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

pub(super) fn check(raw: &[u8]) -> Result<(), ApiError> {
    let damaged = || damaged(MediaType::Png);
    let rest = raw.strip_prefix(SIGNATURE.as_slice()).ok_or_else(damaged)?;
    let (kind, data, mut rest) = chunk(rest).ok_or_else(damaged)?;
    if kind != *b"IHDR" {
        return Err(damaged());
    }
    let header = PngHeader::parse(data).ok_or_else(damaged)?;
    let mut walk = PngWalk::default();
    loop {
        let (kind, data, after) = chunk(rest).ok_or_else(damaged)?;
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
