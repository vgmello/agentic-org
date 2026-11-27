//! WebP: a `RIFF` holding exactly one lossless `VP8L` chunk (never lossy `VP8 `, `VP8X` or
//! metadata), its odd length padded with a zero byte, whose bitstream ends in its last byte
//! (`vp8l`).

use org_proto::ApiError;

use super::super::MediaType;
use super::refusal::{damaged, not_clean};
use super::vp8l;

fn le32(bytes: &[u8], at: usize) -> Option<usize> {
    let word: [u8; 4] = bytes.get(at..at.checked_add(4)?)?.try_into().ok()?;
    usize::try_from(u32::from_le_bytes(word)).ok()
}

pub(super) fn check(raw: &[u8]) -> Result<(), ApiError> {
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
    // A chunk of odd length is padded with one zero byte.
    if len & 1 == 1 && raw[end - 1] != 0 {
        return Err(not_clean("data in the padding of its pixels"));
    }
    vp8l::check(&raw[20..20 + len])
}
