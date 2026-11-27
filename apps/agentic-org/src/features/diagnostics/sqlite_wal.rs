//! Read-only SQLite 3 WAL index reader and panic-free big-endian binary helpers for
//! `sqlite_read.rs`.
//!
//! Fails closed on any symlink, non-regular file, unreadable WAL, corrupt header, corrupt frame,
//! or WAL exceeding [`MAX_WAL_FRAMES`]; only `ErrorKind::NotFound` (or a cleanly 0-byte truncated
//! WAL after checkpoint) indicates an absent/empty WAL.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use super::paths::normalize_lstat_path;

const WAL_MAGIC_LE: u32 = 0x377f_0682;
const WAL_MAGIC_BE: u32 = 0x377f_0683;
pub(super) const MAX_WAL_FRAMES: usize = 65_536;

/// Reads a big-endian `u32` from an exact 4-byte slice without panicking.
pub(super) fn read_be_u32(slice: &[u8]) -> Result<u32, String> {
    let arr: [u8; 4] = slice
        .try_into()
        .map_err(|_| "unexpected short slice reading SQLite u32".to_owned())?;
    Ok(u32::from_be_bytes(arr))
}

/// Reads a big-endian `u32` at `off..off+4` with checked bounds.
pub(super) fn read_be_u32_at(buf: &[u8], off: usize) -> Result<u32, String> {
    let end = off
        .checked_add(4)
        .ok_or_else(|| "SQLite u32 offset overflow".to_owned())?;
    let slice = buf
        .get(off..end)
        .ok_or_else(|| "SQLite u32 out of bounds".to_owned())?;
    read_be_u32(slice)
}

/// Reads a big-endian `u16` at `off..off+2` with checked bounds.
pub(super) fn read_be_u16_at(buf: &[u8], off: usize) -> Result<u16, String> {
    let end = off
        .checked_add(2)
        .ok_or_else(|| "SQLite u16 offset overflow".to_owned())?;
    let slice = buf
        .get(off..end)
        .ok_or_else(|| "SQLite u16 out of bounds".to_owned())?;
    let arr: [u8; 2] = slice
        .try_into()
        .map_err(|_| "unexpected short slice reading SQLite u16".to_owned())?;
    Ok(u16::from_be_bytes(arr))
}

/// Opens and indexes an optional `-wal` sidecar read-only with `O_NOFOLLOW`, failing closed on
/// any error other than `NotFound`.
pub(super) fn load_wal_index(
    wal_path: &Path,
    expected_page_size: usize,
) -> Result<(Option<File>, BTreeMap<u32, u64>), String> {
    load_wal_index_with_limit(wal_path, expected_page_size, MAX_WAL_FRAMES)
}

pub(super) fn load_wal_index_with_limit(
    wal_path: &Path,
    expected_page_size: usize,
    max_frames: usize,
) -> Result<(Option<File>, BTreeMap<u32, u64>), String> {
    let mut wal = match org_platform::fs::open_nofollow_ro(&normalize_lstat_path(wal_path)) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((None, BTreeMap::new())),
        Err(e) => {
            return Err(format!("opening SQLite WAL `{}`: {e}", wal_path.display()));
        }
    };
    let meta = wal
        .metadata()
        .map_err(|e| format!("reading SQLite WAL metadata `{}`: {e}", wal_path.display()))?;
    if !meta.is_file() {
        return Err(format!(
            "SQLite WAL `{}` is not a regular file",
            wal_path.display()
        ));
    }
    if meta.len() == 0 {
        return Ok((None, BTreeMap::new()));
    }
    if meta.len() < 32 {
        return Err(format!(
            "SQLite WAL `{}` header is truncated (< 32 bytes)",
            wal_path.display()
        ));
    }
    let mut hdr = [0u8; 32];
    wal.read_exact(&mut hdr)
        .map_err(|e| format!("reading SQLite WAL header: {e}"))?;
    let magic = read_be_u32_at(&hdr, 0)?;
    let big_endian = match magic {
        WAL_MAGIC_LE => false,
        WAL_MAGIC_BE => true,
        other => return Err(format!("invalid SQLite WAL magic {other:#x}")),
    };
    let wal_ps = usize::try_from(read_be_u32_at(&hdr, 8)?)
        .map_err(|_| "SQLite WAL page size exceeds platform usize".to_owned())?;
    if wal_ps != expected_page_size {
        return Err(format!(
            "SQLite WAL page size {wal_ps} does not match database page size {expected_page_size}"
        ));
    }
    let (mut s0, mut s1) = wal_checksum(&hdr[..24], 0, 0, big_endian);
    let hdr_c0 = read_be_u32_at(&hdr, 24)?;
    let hdr_c1 = read_be_u32_at(&hdr, 28)?;
    if (s0, s1) != (hdr_c0, hdr_c1) {
        return Err("SQLite WAL header checksum mismatch".to_owned());
    }
    let salt = &hdr[16..24];
    let committed = scan_wal_frames(
        &mut wal, wal_ps, salt, &mut s0, &mut s1, big_endian, max_frames,
    )?;
    Ok((Some(wal), committed))
}

fn scan_wal_frames(
    wal: &mut File,
    page_size: usize,
    salt: &[u8],
    s0: &mut u32,
    s1: &mut u32,
    big_endian: bool,
    max_frames: usize,
) -> Result<BTreeMap<u32, u64>, String> {
    let mut committed = BTreeMap::new();
    let mut pending = Vec::new();
    let mut frame_hdr = [0u8; 24];
    let mut page_buf = vec![0u8; page_size];
    let page_size_u64 =
        u64::try_from(page_size).map_err(|_| "SQLite WAL page size overflow".to_owned())?;
    let frame_bytes = 24u64
        .checked_add(page_size_u64)
        .ok_or_else(|| "SQLite WAL frame size overflow".to_owned())?;
    let mut offset = 32u64;
    let mut frames = 0usize;

    loop {
        if !read_frame_header(wal, &mut frame_hdr)? {
            break;
        }
        if &frame_hdr[8..16] != salt {
            break;
        }
        frames += 1;
        if frames > max_frames {
            return Err(format!(
                "SQLite WAL exceeds maximum frame limit of {max_frames}"
            ));
        }
        wal.read_exact(&mut page_buf)
            .map_err(|e| format!("reading SQLite WAL frame page: {e}"))?;
        let (n0, n1) = wal_checksum(&frame_hdr[..8], *s0, *s1, big_endian);
        let (n0, n1) = wal_checksum(&page_buf, n0, n1, big_endian);
        let exp0 = read_be_u32_at(&frame_hdr, 16)?;
        let exp1 = read_be_u32_at(&frame_hdr, 20)?;
        if (n0, n1) != (exp0, exp1) {
            return Err("SQLite WAL frame checksum mismatch".to_owned());
        }
        *s0 = n0;
        *s1 = n1;
        let page_no = read_be_u32_at(&frame_hdr, 0)?;
        let db_pages = read_be_u32_at(&frame_hdr, 4)?;
        let page_off = offset
            .checked_add(24)
            .ok_or_else(|| "SQLite WAL offset overflow".to_owned())?;
        pending.push((page_no, page_off));
        if db_pages > 0 {
            for (p, off) in pending.drain(..) {
                committed.insert(p, off);
            }
        }
        offset = offset
            .checked_add(frame_bytes)
            .ok_or_else(|| "SQLite WAL offset overflow".to_owned())?;
    }
    Ok(committed)
}

fn read_frame_header(wal: &mut File, buf: &mut [u8; 24]) -> Result<bool, String> {
    let mut filled = 0usize;
    while filled < buf.len() {
        match wal.read(&mut buf[filled..]) {
            Ok(0) => {
                return if filled == 0 {
                    Ok(false)
                } else {
                    Err("SQLite WAL frame header is truncated".to_owned())
                };
            }
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(format!("reading SQLite WAL frame header: {e}")),
        }
    }
    Ok(true)
}

pub(super) fn wal_checksum(data: &[u8], mut s0: u32, mut s1: u32, big_endian: bool) -> (u32, u32) {
    let (chunks, _) = data.as_chunks::<8>();
    for &[a0, a1, a2, a3, b0, b1, b2, b3] in chunks {
        let x0 = if big_endian {
            u32::from_be_bytes([a0, a1, a2, a3])
        } else {
            u32::from_le_bytes([a0, a1, a2, a3])
        };
        let x1 = if big_endian {
            u32::from_be_bytes([b0, b1, b2, b3])
        } else {
            u32::from_le_bytes([b0, b1, b2, b3])
        };
        s0 = s0.wrapping_add(x0).wrapping_add(s1);
        s1 = s1.wrapping_add(x1).wrapping_add(s0);
    }
    (s0, s1)
}
