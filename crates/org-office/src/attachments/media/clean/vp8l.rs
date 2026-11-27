//! The lossless bitstream of a WebP `VP8L` chunk (RFC 9649), walked code by code the way a
//! decoder reads it: its header (within the decode limits), its transforms and their
//! sub-images, its prefix codes, and every pixel's codes (a literal, a backward reference or a
//! color cache index). No pixel value is decoded; only where each code ends is followed, so the
//! stream must end in its last byte, its padding bits zero, and nothing rides after it.
//!
//! An entropy image (meta prefix codes), which the office's encoder never writes, is refused:
//! following one would mean decoding its pixel values. So is a two-symbol simple code whose
//! symbols are not in increasing order, which decoders read differently.

use org_proto::ApiError;

use super::super::image::MAX_PIXELS;
use super::super::MediaType;
use super::huffman::Canonical;
use super::refusal::{damaged, not_clean};

const SIGNATURE: u32 = 0x2F;
const LITERALS: u16 = 256;
const LENGTH_PREFIXES: u16 = 24;
const DISTANCE_PREFIXES: u16 = 40;
/// The order the lengths of the code length code come in.
const CODE_LENGTH_ORDER: [usize; 19] = [
    17, 18, 0, 1, 2, 3, 4, 5, 16, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
];
/// The pixel each of the first 120 distance codes names, as `(dy << 4) | (8 - dx)`: `dx` to
/// the right and `dy` rows up.
const NEARBY: [u8; 120] = [
    0x18, 0x07, 0x17, 0x19, 0x28, 0x06, 0x27, 0x29, 0x16, 0x1A, 0x26, 0x2A, 0x38, 0x05, 0x37, 0x39,
    0x15, 0x1B, 0x36, 0x3A, 0x25, 0x2B, 0x48, 0x04, 0x47, 0x49, 0x14, 0x1C, 0x35, 0x3B, 0x46, 0x4A,
    0x24, 0x2C, 0x58, 0x45, 0x4B, 0x34, 0x3C, 0x03, 0x57, 0x59, 0x13, 0x1D, 0x56, 0x5A, 0x23, 0x2D,
    0x44, 0x4C, 0x55, 0x5B, 0x33, 0x3D, 0x68, 0x02, 0x67, 0x69, 0x12, 0x1E, 0x66, 0x6A, 0x22, 0x2E,
    0x54, 0x5C, 0x43, 0x4D, 0x65, 0x6B, 0x32, 0x3E, 0x78, 0x01, 0x77, 0x79, 0x53, 0x5D, 0x11, 0x1F,
    0x64, 0x6C, 0x42, 0x4E, 0x76, 0x7A, 0x21, 0x2F, 0x75, 0x7B, 0x31, 0x3F, 0x63, 0x6D, 0x52, 0x5E,
    0x00, 0x74, 0x7C, 0x41, 0x4F, 0x10, 0x20, 0x62, 0x6E, 0x30, 0x73, 0x7D, 0x51, 0x5F, 0x40, 0x72,
    0x7E, 0x61, 0x6F, 0x50, 0x71, 0x7F, 0x60, 0x70,
];

/// Why a walk stops short.
enum Stop {
    Damaged,
    EntropyImage,
}

type Walk<T> = Result<T, Stop>;

pub(super) fn check(data: &[u8]) -> Result<(), ApiError> {
    let mut bits = Bits { data, at: 0 };
    match stream(&mut bits) {
        Ok(()) if bits.ends_exactly() => Ok(()),
        Ok(()) => Err(not_clean("data hidden after its pixels")),
        Err(Stop::Damaged) => Err(damaged(MediaType::Webp)),
        Err(Stop::EntropyImage) => Err(not_clean("an entropy image the office never writes")),
    }
}

fn stream(bits: &mut Bits<'_>) -> Walk<()> {
    if bits.read(8)? != SIGNATURE {
        return Err(Stop::Damaged);
    }
    let width = bits.read(14)? + 1;
    let height = bits.read(14)? + 1;
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(Stop::Damaged);
    }
    // Whether alpha is used (a hint), then version 0.
    bits.read(1)?;
    if bits.read(3)? != 0 {
        return Err(Stop::Damaged);
    }
    let coded_width = transforms(bits, width, height)?;
    image(bits, coded_width, height, true)
}

/// The stream's bits, least significant first.
struct Bits<'a> {
    data: &'a [u8],
    /// How many bits are read.
    at: usize,
}

impl Bits<'_> {
    fn bit(&mut self) -> Option<u32> {
        let byte = self.data.get(self.at / 8)?;
        let bit = (byte >> (self.at % 8)) & 1;
        self.at += 1;
        Some(u32::from(bit))
    }

    /// The next `count` (at most 32) bits as a number.
    fn read(&mut self, count: u32) -> Walk<u32> {
        let mut value = 0;
        for shift in 0..count {
            value |= self.bit().ok_or(Stop::Damaged)? << shift;
        }
        Ok(value)
    }

    /// Whether every byte was read, and the bits left in the last one (its padding) are zero.
    fn ends_exactly(&self) -> bool {
        self.at.div_ceil(8) == self.data.len()
            && self
                .data
                .get(self.at / 8)
                .is_none_or(|last| last >> (self.at % 8) == 0)
    }
}

/// One prefix code: a lone symbol (read from no bits) or a canonical code.
enum Prefix {
    Single(u16),
    Code(Canonical),
}

impl Prefix {
    fn read(&self, bits: &mut Bits<'_>) -> Walk<u16> {
        match self {
            Self::Single(symbol) => Ok(*symbol),
            Self::Code(code) => code.read(|| bits.bit()).ok_or(Stop::Damaged),
        }
    }

    /// From one length per symbol: one symbol, whatever its length, or a complete code.
    fn from_lengths(lengths: &[u8]) -> Walk<Self> {
        let mut used = lengths
            .iter()
            .enumerate()
            .filter(|(_, length)| **length != 0);
        match (used.next(), used.next()) {
            (None, _) => Err(Stop::Damaged),
            (Some((symbol, _)), None) => u16::try_from(symbol)
                .map(Self::Single)
                .map_err(|_| Stop::Damaged),
            _ => Canonical::complete(lengths)
                .map(Self::Code)
                .ok_or(Stop::Damaged),
        }
    }

    /// Reads a prefix code over `alphabet` symbols: simple (one or two symbols) or by code
    /// lengths, themselves prefix coded.
    fn read_code(bits: &mut Bits<'_>, alphabet: u16) -> Walk<Self> {
        let mut lengths = vec![0u8; usize::from(alphabet)];
        if bits.read(1)? == 1 {
            let two = bits.read(1)? == 1;
            let first_bits = if bits.read(1)? == 1 { 8 } else { 1 };
            let first = bits.read(first_bits)?;
            let second = if two { Some(bits.read(8)?) } else { None };
            for symbol in [Some(first), second].into_iter().flatten() {
                *lengths
                    .get_mut(usize::try_from(symbol).map_err(|_| Stop::Damaged)?)
                    .ok_or(Stop::Damaged)? = 1;
            }
            if second.is_some_and(|second| second <= first) {
                return Err(Stop::Damaged);
            }
            return Self::from_lengths(&lengths);
        }
        let mut length_lengths = [0u8; CODE_LENGTH_ORDER.len()];
        let count = 4 + bits.read(4)?;
        for symbol in CODE_LENGTH_ORDER.into_iter().take(count as usize) {
            length_lengths[symbol] = u8::try_from(bits.read(3)?).map_err(|_| Stop::Damaged)?;
        }
        let length_code = Self::from_lengths(&length_lengths)?;
        // How many length codes follow: all the alphabet's, unless fewer are given.
        let mut tokens = u32::from(alphabet);
        if bits.read(1)? == 1 {
            let width = 2 + 2 * bits.read(3)?;
            tokens = 2 + bits.read(width)?;
            if tokens > u32::from(alphabet) {
                return Err(Stop::Damaged);
            }
        }
        let (mut symbol, mut previous) = (0, 8);
        while symbol < lengths.len() && tokens > 0 {
            tokens -= 1;
            let token = length_code.read(bits)?;
            if let Ok(length @ 0..=15) = u8::try_from(token) {
                lengths[symbol] = length;
                symbol += 1;
                previous = if length == 0 { previous } else { length };
                continue;
            }
            // 16 repeats the last non-zero length 3 to 6 times; 17 and 18 write zeros.
            let (extra, base, length) = match token {
                16 => (2, 3, previous),
                17 => (3, 3, 0),
                _ => (7, 11, 0),
            };
            let run = usize::try_from(base + bits.read(extra)?).map_err(|_| Stop::Damaged)?;
            lengths
                .get_mut(symbol..symbol + run)
                .ok_or(Stop::Damaged)?
                .fill(length);
            symbol += run;
        }
        Self::from_lengths(&lengths)
    }
}

/// The five prefix codes of an image: green (with backward reference lengths and color cache
/// indexes), red, blue, alpha and distance.
struct Group {
    green: Prefix,
    red: Prefix,
    blue: Prefix,
    alpha: Prefix,
    distance: Prefix,
}

impl Group {
    fn read(bits: &mut Bits<'_>, cache: u16) -> Walk<Self> {
        Ok(Self {
            green: Prefix::read_code(bits, LITERALS + LENGTH_PREFIXES + cache)?,
            red: Prefix::read_code(bits, LITERALS)?,
            blue: Prefix::read_code(bits, LITERALS)?,
            alpha: Prefix::read_code(bits, LITERALS)?,
            distance: Prefix::read_code(bits, DISTANCE_PREFIXES)?,
        })
    }
}

/// `size` in blocks of `1 << bits`.
fn subsample(size: u32, bits: u32) -> u32 {
    size.div_ceil(1 << bits)
}

/// Walks the transforms (each kind at most once) and their sub-images; returns the width the
/// pixels are coded at, narrower once color indexing packs several pixels into one.
fn transforms(bits: &mut Bits<'_>, width: u32, height: u32) -> Walk<u32> {
    let mut seen = [false; 4];
    let mut width = width;
    while bits.read(1)? == 1 {
        let kind = bits.read(2)?;
        let seen = seen.get_mut(kind as usize).ok_or(Stop::Damaged)?;
        if std::mem::replace(seen, true) {
            return Err(Stop::Damaged);
        }
        match kind {
            // Predictor or cross-color: a sub-image of one pixel a block.
            0 | 1 => {
                let block = bits.read(3)? + 2;
                image(
                    bits,
                    subsample(width, block),
                    subsample(height, block),
                    false,
                )?;
            }
            // Subtract green: nothing more.
            2 => {}
            // Color indexing: the palette, then pixels packed as tightly as its indexes allow.
            _ => {
                let colors = bits.read(8)? + 1;
                image(bits, colors, 1, false)?;
                let packing = match colors {
                    0..=2 => 3,
                    3..=4 => 2,
                    5..=16 => 1,
                    _ => 0,
                };
                width = subsample(width, packing);
            }
        }
    }
    Ok(width)
}

/// Walks one entropy-coded image of `width` x `height` pixels: its color cache, its prefix
/// codes (the `main` image may announce an entropy image, which is refused), then its pixels.
fn image(bits: &mut Bits<'_>, width: u32, height: u32, main: bool) -> Walk<()> {
    let mut cache = 0;
    if bits.read(1)? == 1 {
        let cache_bits = bits.read(4)?;
        if !(1..=11).contains(&cache_bits) {
            return Err(Stop::Damaged);
        }
        cache = 1 << cache_bits;
    }
    if main && bits.read(1)? == 1 {
        return Err(Stop::EntropyImage);
    }
    let group = Group::read(bits, cache)?;
    pixels(bits, &group, width, u64::from(width) * u64::from(height))
}

/// Walks the codes of `total` pixels, `width` to a row.
fn pixels(bits: &mut Bits<'_>, group: &Group, width: u32, total: u64) -> Walk<()> {
    // A literal or a color cache index read from no bits makes every pixel left the same, also
    // read from no bits (decoders fill them at once).
    if let Prefix::Single(green) = group.green {
        let lone = |prefix: &Prefix| matches!(prefix, Prefix::Single(_));
        let literal = green < LITERALS
            && [&group.red, &group.blue, &group.alpha]
                .into_iter()
                .all(lone);
        if literal || green >= LITERALS + LENGTH_PREFIXES {
            return Ok(());
        }
    }
    let mut at = 0;
    while at < total {
        let green = group.green.read(bits)?;
        if green < LITERALS {
            for prefix in [&group.red, &group.blue, &group.alpha] {
                prefix.read(bits)?;
            }
            at += 1;
        } else if green < LITERALS + LENGTH_PREFIXES {
            let length = copy_value(bits, green - LITERALS)?;
            let prefix = group.distance.read(bits)?;
            let code = copy_value(bits, prefix)?;
            // A copy from before the first pixel or past the last is damaged.
            if distance(width, code) > at || length > total - at {
                return Err(Stop::Damaged);
            }
            at += length;
        } else {
            // A color cache index, the alphabet's last symbols.
            at += 1;
        }
    }
    Ok(())
}

/// The length or distance code a prefix names, with its extra bits.
fn copy_value(bits: &mut Bits<'_>, prefix: u16) -> Walk<u64> {
    let prefix = u32::from(prefix);
    if prefix < 4 {
        return Ok(u64::from(prefix + 1));
    }
    let extra = (prefix - 2) >> 1;
    let offset = (2 + (prefix & 1)) << extra;
    Ok(u64::from(offset + bits.read(extra)? + 1))
}

/// How many pixels back a distance code points, in an image `width` pixels wide: the first 120
/// codes name nearby pixels (at least the one before), the rest count back from 1.
fn distance(width: u32, code: u64) -> u64 {
    let Some(nearby) = code
        .checked_sub(1)
        .and_then(|index| NEARBY.get(usize::try_from(index).ok()?))
    else {
        return code.saturating_sub(120);
    };
    let (dy, dx) = (i64::from(nearby >> 4), 8 - i64::from(nearby & 0x0F));
    u64::try_from(dx + dy * i64::from(width)).map_or(1, |distance| distance.max(1))
}
