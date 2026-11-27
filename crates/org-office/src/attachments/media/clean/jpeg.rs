//! JPEG: `SOI`, one bare `JFIF` `APP0` (no thumbnail), its tables, one baseline frame (`SOF0`)
//! within the decode limits, one scan of all its components, and `EOI` straight after the scan:
//! no `APP1`..`APP15`, no `COM`.
//!
//! - Each quantization table (`DQT`: 8-bit, ids 0..=3, no zero entry) and Huffman table (`DHT`:
//!   DC or AC, ids 0..=1, a prefix code of values a baseline scan can code) is defined once,
//!   whole (no byte of its segment left over), and used by the frame or the scan. At most one
//!   restart interval (`DRI`), exactly its two bytes.
//! - The scan's entropy-coded data is walked code by code: it holds exactly the frame's MCUs,
//!   a restart marker after every interval but the last, and one-bit padding to each marker, so
//!   no byte rides after the last MCU (stuffed `FF 00` or not).

use org_proto::ApiError;

use super::super::image::{MAX_DIMENSION, MAX_PIXELS};
use super::super::MediaType;
use super::huffman::Canonical;
use super::refusal::{damaged, not_clean};

const SOF0: u8 = 0xC0;
const DHT: u8 = 0xC4;
const RST0: u8 = 0xD0;
const EOI: u8 = 0xD9;
const SOS: u8 = 0xDA;
const DQT: u8 = 0xDB;
const DRI: u8 = 0xDD;
const APP0: u8 = 0xE0;
const COM: u8 = 0xFE;
/// The most blocks one MCU of an interleaved scan may hold (ITU T.81, B.2.3).
const MAX_MCU_BLOCKS: u32 = 10;

fn damaged_jpeg() -> ApiError {
    damaged(MediaType::Jpeg)
}

fn be16(bytes: &[u8], at: usize) -> Option<usize> {
    let word: [u8; 2] = bytes.get(at..at.checked_add(2)?)?.try_into().ok()?;
    Some(usize::from(u16::from_be_bytes(word)))
}

pub(super) fn check(raw: &[u8]) -> Result<(), ApiError> {
    if !raw.starts_with(&[0xFF, 0xD8]) {
        return Err(damaged_jpeg());
    }
    let mut at = 2;
    let mut jfif = false;
    let mut frame = None;
    let mut tables = Tables::default();
    loop {
        if raw.get(at) != Some(&0xFF) {
            return Err(damaged_jpeg());
        }
        let marker = *raw.get(at + 1).ok_or_else(damaged_jpeg)?;
        at += 2;
        let len = be16(raw, at)
            .filter(|len| *len >= 2)
            .ok_or_else(damaged_jpeg)?;
        let segment = raw.get(at + 2..at + len).ok_or_else(damaged_jpeg)?;
        match marker {
            // One bare JFIF header: exactly its 14 bytes, no thumbnail.
            APP0 if !jfif => {
                if len != 16 || !segment.starts_with(b"JFIF\0") || segment[12..14] != [0, 0] {
                    return Err(not_clean("a JFIF thumbnail or extension"));
                }
                jfif = true;
            }
            // The one baseline frame, before its scan.
            SOF0 if frame.is_none() => {
                frame = Some(Frame::parse(segment).ok_or_else(damaged_jpeg)?);
            }
            DQT => tables.take_quantization(segment)?,
            DHT => tables.take_huffman(segment)?,
            DRI => tables.take_restart(segment)?,
            // The one scan, and the end straight after its last MCU: nothing between them.
            SOS => {
                let frame = frame.as_ref().ok_or_else(damaged_jpeg)?;
                let parts = tables.scan(segment, frame)?;
                let restart = tables.restart.unwrap_or(0);
                let end = walk(raw, at + len, &parts, frame.mcus(), restart)?;
                return match raw.get(end..) {
                    Some([0xFF, EOI]) => Ok(()),
                    Some([0xFF, EOI, ..]) => Err(not_clean("data after its end")),
                    _ => Err(not_clean("data or a block after its pixels")),
                };
            }
            0xC0..=0xCF => return Err(not_clean("a second or non-baseline frame")),
            0xE0..=0xEF => return Err(not_clean("a metadata block (EXIF, XMP, ICC or other)")),
            COM => return Err(not_clean("a comment")),
            _ => return Err(not_clean("a block the office never writes")),
        }
        at += len;
    }
}

/// One component of the frame: its id, sampling factors and quantization table.
struct Component {
    id: u8,
    h: u8,
    v: u8,
    quant: u8,
}

/// The baseline frame: its size (within the decode limits) and its one to four components.
struct Frame {
    width: u32,
    height: u32,
    components: Vec<Component>,
}

impl Frame {
    fn parse(segment: &[u8]) -> Option<Self> {
        // 8-bit samples, a height (never 0, which a later `DNL` would set), the width, and the
        // components.
        let [8, h1, h0, w1, w0, count, specs @ ..] = segment else {
            return None;
        };
        let height = u32::from(u16::from_be_bytes([*h1, *h0]));
        let width = u32::from(u16::from_be_bytes([*w1, *w0]));
        let side = 1..=MAX_DIMENSION;
        if !side.contains(&width)
            || !side.contains(&height)
            || u64::from(width) * u64::from(height) > MAX_PIXELS
            || !(1..=4).contains(count)
            || specs.len() != 3 * usize::from(*count)
        {
            return None;
        }
        let mut components: Vec<Component> = Vec::new();
        for &[id, sampling, quant] in specs.as_chunks::<3>().0 {
            let (h, v) = (sampling >> 4, sampling & 0x0F);
            let factors = 1..=4;
            if !factors.contains(&h)
                || !factors.contains(&v)
                || quant > 3
                || components.iter().any(|component| component.id == id)
            {
                return None;
            }
            components.push(Component { id, h, v, quant });
        }
        Some(Self {
            width,
            height,
            components,
        })
    }

    /// How many MCUs the scan of every component holds: one block each when there is one
    /// component, else the frame in MCUs of the largest sampling factors.
    fn mcus(&self) -> u64 {
        let (mut h_max, mut v_max) = (1, 1);
        for component in &self.components {
            h_max = h_max.max(u32::from(component.h));
            v_max = v_max.max(u32::from(component.v));
        }
        if self.components.len() == 1 {
            (h_max, v_max) = (1, 1);
        }
        u64::from(self.width.div_ceil(8 * h_max)) * u64::from(self.height.div_ceil(8 * v_max))
    }
}

/// The tables the segments before the scan define.
#[derive(Default)]
struct Tables {
    /// Which quantization tables (ids 0..=3) are defined.
    quant: [bool; 4],
    /// The Huffman tables, by class (DC 0, AC 1) and id (0..=1).
    huffman: [[Option<Canonical>; 2]; 2],
    /// The restart interval, once a `DRI` sets it.
    restart: Option<u16>,
}

/// One component of the scan: its blocks in each MCU, and its DC and AC tables.
struct ScanPart<'a> {
    blocks: u32,
    dc: &'a Canonical,
    ac: &'a Canonical,
}

impl Tables {
    /// Takes a `DQT`: whole 8-bit tables of 64 non-zero entries, each id defined once.
    fn take_quantization(&mut self, mut segment: &[u8]) -> Result<(), ApiError> {
        if segment.is_empty() {
            return Err(damaged_jpeg());
        }
        while let [spec, rest @ ..] = segment {
            // `Pq` (the high nibble) is 0 for 8-bit entries, so the byte is the id itself.
            let defined = self
                .quant
                .get_mut(usize::from(*spec))
                .ok_or_else(damaged_jpeg)?;
            let (table, after) = rest.split_at_checked(64).ok_or_else(damaged_jpeg)?;
            if table.contains(&0) {
                return Err(damaged_jpeg());
            }
            if *defined {
                return Err(not_clean("a table defined twice"));
            }
            *defined = true;
            segment = after;
        }
        Ok(())
    }

    /// Takes a `DHT`: whole baseline tables (DC or AC, id 0 or 1), each defined once.
    fn take_huffman(&mut self, mut segment: &[u8]) -> Result<(), ApiError> {
        if segment.is_empty() {
            return Err(damaged_jpeg());
        }
        while let [spec, rest @ ..] = segment {
            let class = usize::from(spec >> 4);
            let slot = self
                .huffman
                .get_mut(class)
                .and_then(|ids| ids.get_mut(usize::from(spec & 0x0F)))
                .ok_or_else(damaged_jpeg)?;
            let (counts, rest) = rest.split_first_chunk::<16>().ok_or_else(damaged_jpeg)?;
            let total = counts.iter().map(|count| usize::from(*count)).sum();
            let (values, after) = rest.split_at_checked(total).ok_or_else(damaged_jpeg)?;
            if !baseline_values(class, values) {
                return Err(damaged_jpeg());
            }
            let symbols = values.iter().map(|value| u16::from(*value)).collect();
            let code = Canonical::jpeg(counts, symbols).ok_or_else(damaged_jpeg)?;
            if slot.is_some() {
                return Err(not_clean("a table defined twice"));
            }
            *slot = Some(code);
            segment = after;
        }
        Ok(())
    }

    /// Takes a `DRI`: exactly its two bytes (a length of 4), once.
    fn take_restart(&mut self, segment: &[u8]) -> Result<(), ApiError> {
        let &[high, low] = segment else {
            return Err(damaged_jpeg());
        };
        if self.restart.is_some() {
            return Err(not_clean("a second restart interval"));
        }
        self.restart = Some(u16::from_be_bytes([high, low]));
        Ok(())
    }

    /// Checks the scan header `segment` against `frame`: every component in frame order, the
    /// spectral selection and approximation of a baseline scan, every table it names defined
    /// and every table defined named. Returns its components in MCU order.
    fn scan(&self, segment: &[u8], frame: &Frame) -> Result<Vec<ScanPart<'_>>, ApiError> {
        let [count, specs @ ..] = segment else {
            return Err(damaged_jpeg());
        };
        let count = usize::from(*count);
        if count != frame.components.len()
            || specs.len() != 2 * count + 3
            || specs[2 * count..] != [0, 63, 0]
        {
            return Err(damaged_jpeg());
        }
        let mut quant_used = [false; 4];
        let mut huffman_used = [[false; 2]; 2];
        let mut parts = Vec::new();
        for (component, &[id, ids]) in frame.components.iter().zip(specs.as_chunks::<2>().0) {
            let (dc, ac) = (usize::from(ids >> 4), usize::from(ids & 0x0F));
            let quant = usize::from(component.quant);
            let code = |class: usize, id: usize| self.huffman[class].get(id)?.as_ref();
            let (Some(dc_code), Some(ac_code)) = (code(0, dc), code(1, ac)) else {
                return Err(damaged_jpeg());
            };
            if id != component.id || !self.quant[quant] {
                return Err(damaged_jpeg());
            }
            quant_used[quant] = true;
            huffman_used[0][dc] = true;
            huffman_used[1][ac] = true;
            // An interleaved MCU holds each component's sampling factors in blocks.
            let blocks = if count == 1 {
                1
            } else {
                u32::from(component.h) * u32::from(component.v)
            };
            parts.push(ScanPart {
                blocks,
                dc: dc_code,
                ac: ac_code,
            });
        }
        if parts.iter().map(|part| part.blocks).sum::<u32>() > MAX_MCU_BLOCKS {
            return Err(damaged_jpeg());
        }
        let huffman_unused = self
            .huffman
            .iter()
            .flatten()
            .zip(huffman_used.iter().flatten())
            .any(|(table, used)| table.is_some() && !used);
        if huffman_unused || quant_used != self.quant {
            return Err(not_clean("a table its pixels never use"));
        }
        Ok(parts)
    }
}

/// Whether Huffman `values` of `class` are distinct and each one a baseline scan can code: a
/// DC difference of at most 11 bits, or an AC run and size of at most 10 bits (size 0 only for
/// the end of the block and a run of sixteen zeros).
fn baseline_values(class: usize, values: &[u8]) -> bool {
    let mut seen = [false; 256];
    values.iter().all(|value| {
        let fresh = !std::mem::replace(&mut seen[usize::from(*value)], true);
        let codable = match class {
            0 => *value <= 11,
            _ => matches!(value & 0x0F, 1..=10) || matches!(value, 0x00 | 0xF0),
        };
        fresh && codable
    })
}

/// Entropy-coded data from `at`, read one bit at a time (most significant first), a stuffed
/// `FF 00` read as `FF`: any other marker ends it.
struct Entropy<'a> {
    raw: &'a [u8],
    at: usize,
    byte: u8,
    /// The bits of `byte` not read yet.
    left: u32,
}

impl Entropy<'_> {
    fn bit(&mut self) -> Option<u32> {
        if self.left == 0 {
            let byte = *self.raw.get(self.at)?;
            if byte == 0xFF && *self.raw.get(self.at + 1)? != 0 {
                return None;
            }
            self.at += if byte == 0xFF { 2 } else { 1 };
            (self.byte, self.left) = (byte, 8);
        }
        self.left -= 1;
        Some(u32::from(self.byte >> self.left) & 1)
    }

    fn skip(&mut self, bits: u16) -> Option<()> {
        for _ in 0..bits {
            self.bit()?;
        }
        Some(())
    }

    /// Ends the current byte: whether the bits left in it are all ones, the padding a JPEG
    /// encoder writes before a marker.
    fn pad(&mut self) -> bool {
        let mask = (1u32 << self.left) - 1;
        self.left = 0;
        u32::from(self.byte) & mask == mask
    }
}

/// Walks the entropy-coded data from `at`: exactly `mcus` MCUs of `parts`, a restart marker
/// (`RST0`..`RST7` in turn) after every `restart` MCUs but the last, and one-bit padding to each
/// marker. Returns where the data ends.
fn walk(
    raw: &[u8],
    at: usize,
    parts: &[ScanPart<'_>],
    mcus: u64,
    restart: u16,
) -> Result<usize, ApiError> {
    let padding = || not_clean("data hidden in its padding");
    let mut data = Entropy {
        raw,
        at,
        byte: 0,
        left: 0,
    };
    let restart = u64::from(restart);
    for mcu in 0..mcus {
        if restart != 0 && mcu != 0 && mcu % restart == 0 {
            if !data.pad() {
                return Err(padding());
            }
            let turn = u8::try_from((mcu / restart - 1) % 8).map_err(|_| damaged_jpeg())?;
            if raw.get(data.at..data.at + 2) != Some(&[0xFF, RST0 + turn][..]) {
                return Err(damaged_jpeg());
            }
            data.at += 2;
        }
        for part in parts {
            for _ in 0..part.blocks {
                block(&mut data, part).ok_or_else(damaged_jpeg)?;
            }
        }
    }
    if !data.pad() {
        return Err(padding());
    }
    Ok(data.at)
}

/// Walks one block's codes: its DC difference, then AC coefficients up to the end of the block
/// or the 63rd. `None` when a code is not in its table, runs past the block, or the data ends.
fn block(data: &mut Entropy<'_>, part: &ScanPart<'_>) -> Option<()> {
    let size = part.dc.read(|| data.bit())?;
    data.skip(size)?;
    // The next coefficient, in zigzag order.
    let mut k = 1;
    while k < 64 {
        let symbol = part.ac.read(|| data.bit())?;
        let (run, size) = (symbol >> 4, symbol & 0x0F);
        if symbol == 0x00 {
            break;
        }
        // Sixteen zeros (`F0`, the only other size 0), or a run of zeros before a coefficient:
        // either way a coefficient must still fit.
        k += if size == 0 { 16 } else { run };
        if k > 63 {
            return None;
        }
        if size != 0 {
            data.skip(size)?;
            k += 1;
        }
    }
    Some(())
}
