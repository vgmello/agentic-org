//! Canonical prefix codes, as JPEG (`DHT`) and WebP lossless (`VP8L`) define them: codes are
//! handed out shortest first and, within one length, in symbol order. A code is read one bit
//! at a time, most significant bit first, which both formats share however they pack bits into
//! bytes.

/// The longest code either format allows (JPEG 16 bits, WebP lossless 15).
const MAX_LENGTH: usize = 16;

/// A canonical prefix code: how many codes each length has, and the symbols in code order.
pub(super) struct Canonical {
    /// `counts[n]`: how many codes are `n` bits long (`counts[0]` is unused).
    counts: [u16; MAX_LENGTH + 1],
    symbols: Vec<u16>,
}

impl Canonical {
    /// A JPEG table: `counts[n - 1]` codes `n` bits long, for `symbols` in code order. `None`
    /// when the counts and the symbols disagree, or when a length runs out of codes or would
    /// use its all-ones code, which JPEG reserves.
    pub(super) fn jpeg(counts: &[u8; MAX_LENGTH], symbols: Vec<u16>) -> Option<Self> {
        let mut table = [0u16; MAX_LENGTH + 1];
        let mut next = 0u32;
        for (length, count) in (1..=MAX_LENGTH).zip(counts) {
            table[length] = u16::from(*count);
            // One past the last code of this length, which must still fit short of all ones.
            next += u32::from(*count);
            if next >= 1 << length {
                return None;
            }
            next <<= 1;
        }
        let total: usize = table.iter().map(|count| usize::from(*count)).sum();
        (total == symbols.len()).then_some(Self {
            counts: table,
            symbols,
        })
    }

    /// A WebP lossless code from one length per symbol (0: the symbol is unused), when every
    /// code is used (the code is complete): `None` for an incomplete or oversubscribed code,
    /// and so for fewer than two symbols, which the caller handles first.
    pub(super) fn complete(lengths: &[u8]) -> Option<Self> {
        let mut counts = [0u16; MAX_LENGTH + 1];
        for length in lengths.iter().filter(|length| **length != 0) {
            *counts.get_mut(usize::from(*length))? += 1;
        }
        // How many codes of the current length are still free; a complete code leaves none.
        let mut free: i64 = 1;
        for count in &counts[1..] {
            free = free * 2 - i64::from(*count);
            if free < 0 {
                return None;
            }
        }
        if free != 0 {
            return None;
        }
        let mut next = [0usize; MAX_LENGTH + 1];
        for length in 1..MAX_LENGTH {
            next[length + 1] = next[length] + usize::from(counts[length]);
        }
        let mut symbols = vec![0u16; next[MAX_LENGTH] + usize::from(counts[MAX_LENGTH])];
        for (symbol, length) in lengths.iter().enumerate() {
            if *length != 0 {
                let slot = &mut next[usize::from(*length)];
                *symbols.get_mut(*slot)? = u16::try_from(symbol).ok()?;
                *slot += 1;
            }
        }
        Some(Self { counts, symbols })
    }

    /// Reads one symbol, `bit` giving the code's bits in order: `None` when the bits run out
    /// or spell no code.
    pub(super) fn read(&self, mut bit: impl FnMut() -> Option<u32>) -> Option<u16> {
        // The code read so far, the first code of its length, and that code's symbol index.
        let (mut code, mut first, mut index) = (0u32, 0u32, 0usize);
        for count in &self.counts[1..] {
            let count = u32::from(*count);
            code |= bit()?;
            if let Some(offset) = code.checked_sub(first).filter(|offset| *offset < count) {
                return self
                    .symbols
                    .get(index + usize::try_from(offset).ok()?)
                    .copied();
            }
            index += usize::try_from(count).ok()?;
            first = (first + count) << 1;
            code <<= 1;
        }
        None
    }
}
