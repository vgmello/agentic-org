//! `.docx` and `.xlsx` attachments, checked as ZIP containers without unpacking them: the end of
//! central directory record, every central directory entry (bounded count, names, sizes,
//! compression ratio, local header agreement), the parts each format needs, no macros, and the
//! main content type in `[Content_Types].xml` (the one entry inflated, under a small cap).
//!
//! No entry other than `[Content_Types].xml` is decompressed, so a zip bomb costs nothing; its
//! declared sizes are refused instead.

use std::collections::BTreeSet;
use std::io::Read;

use org_proto::ApiError;

use super::{corrupt, unsupported, MediaType};

const EOCD_SIGNATURE: &[u8; 4] = b"PK\x05\x06";
const CENTRAL_SIGNATURE: &[u8; 4] = b"PK\x01\x02";
const LOCAL_SIGNATURE: &[u8; 4] = b"PK\x03\x04";
const EOCD_LEN: usize = 22;
const CENTRAL_LEN: usize = 46;
const LOCAL_LEN: usize = 30;
/// The longest archive comment the end record may be followed by.
const MAX_COMMENT: usize = 0xffff;
/// The most entries an office document may have.
pub const MAX_ENTRIES: usize = 10_000;
/// The most all entries may declare uncompressed.
pub const MAX_TOTAL_UNCOMPRESSED: u64 = 256 * 1024 * 1024;
/// The highest ratio of declared to compressed size for an entry over 1 MiB.
pub const MAX_RATIO: u64 = 200;
/// The largest `[Content_Types].xml` inflated.
const MAX_CONTENT_TYPES: u64 = 1024 * 1024;
const CONTENT_TYPES: &str = "[Content_Types].xml";

#[derive(Debug)]
struct Entry {
    name: String,
    method: u16,
    compressed: u64,
    local_offset: usize,
}

/// Refuses anything but a well-formed `.docx` (or `.xlsx`, as `media` says).
pub fn check(media: MediaType, bytes: &[u8]) -> Result<(), ApiError> {
    let entries = central_directory(bytes)?;
    let names: BTreeSet<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    let (part, main_type) = match media {
        MediaType::Docx => (
            "word/document.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
        ),
        MediaType::Xlsx => (
            "xl/workbook.xml",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml",
        ),
        _ => return Err(unsupported("not an office document type")),
    };
    if !names.contains(CONTENT_TYPES) || !names.contains(part) {
        return Err(unsupported(format!(
            "the file is not a `{}` document: it lacks `{CONTENT_TYPES}` or `{part}`",
            media.mime()
        )));
    }
    if names
        .iter()
        .any(|name| name.to_ascii_lowercase().ends_with("vbaproject.bin"))
    {
        return Err(unsupported("macro-enabled documents are not accepted"));
    }
    let types_entry = entries
        .iter()
        .find(|e| e.name == CONTENT_TYPES)
        .expect("checked above");
    let types = inflate_entry(bytes, types_entry)?;
    let types = String::from_utf8_lossy(&types).to_ascii_lowercase();
    if types.contains("macroenabled") {
        return Err(unsupported("macro-enabled documents are not accepted"));
    }
    if !types.contains(main_type) {
        return Err(unsupported(format!(
            "the file's content does not match its declared type `{}`",
            media.mime()
        )));
    }
    Ok(())
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn broken() -> ApiError {
    corrupt("the document's ZIP structure is damaged or truncated")
}

/// Finds the end record within the last 64 KiB + 22 bytes and checks what it says.
fn end_record(bytes: &[u8]) -> Result<(usize, usize, usize), ApiError> {
    if bytes.len() < EOCD_LEN || !bytes.starts_with(LOCAL_SIGNATURE) {
        return Err(broken());
    }
    let earliest = bytes.len().saturating_sub(EOCD_LEN + MAX_COMMENT);
    let at = (earliest..=bytes.len() - EOCD_LEN)
        .rev()
        .find(|&at| &bytes[at..at + 4] == EOCD_SIGNATURE)
        .ok_or_else(broken)?;
    let field = |offset| u16_at(bytes, at + offset).ok_or_else(broken);
    let (disk, cd_disk) = (field(4)?, field(6)?);
    let (on_disk, total) = (field(8)?, field(10)?);
    let comment_len = usize::from(field(20)?);
    let cd_size = u32_at(bytes, at + 12).ok_or_else(broken)?;
    let cd_offset = u32_at(bytes, at + 16).ok_or_else(broken)?;
    if disk != 0 || cd_disk != 0 || on_disk != total {
        return Err(unsupported("multi-part archives are not accepted"));
    }
    if total == 0xffff || cd_size == u32::MAX || cd_offset == u32::MAX {
        return Err(unsupported("ZIP64 archives are not accepted"));
    }
    if at + EOCD_LEN + comment_len != bytes.len() {
        return Err(broken());
    }
    let (cd_offset, cd_size) = (cd_offset as usize, cd_size as usize);
    if cd_offset.checked_add(cd_size) != Some(at) {
        return Err(broken());
    }
    Ok((cd_offset, cd_size, usize::from(total)))
}

/// Every central directory entry, each checked.
fn central_directory(bytes: &[u8]) -> Result<Vec<Entry>, ApiError> {
    let (mut at, cd_size, total) = end_record(bytes)?;
    if total == 0 || total > MAX_ENTRIES {
        return Err(unsupported(format!(
            "an office document has 1 to {MAX_ENTRIES} entries"
        )));
    }
    let cd_end = at + cd_size;
    let mut entries = Vec::with_capacity(total);
    let mut seen = BTreeSet::new();
    let mut uncompressed_total: u64 = 0;
    for _ in 0..total {
        if at + CENTRAL_LEN > cd_end || &bytes[at..at + 4] != CENTRAL_SIGNATURE {
            return Err(broken());
        }
        let flags = u16_at(bytes, at + 8).ok_or_else(broken)?;
        let method = u16_at(bytes, at + 10).ok_or_else(broken)?;
        let compressed = u64::from(u32_at(bytes, at + 20).ok_or_else(broken)?);
        let uncompressed = u64::from(u32_at(bytes, at + 24).ok_or_else(broken)?);
        let name_len = usize::from(u16_at(bytes, at + 28).ok_or_else(broken)?);
        let extra_len = usize::from(u16_at(bytes, at + 30).ok_or_else(broken)?);
        let comment_len = usize::from(u16_at(bytes, at + 32).ok_or_else(broken)?);
        let local_offset = u32_at(bytes, at + 42).ok_or_else(broken)? as usize;
        let next = at + CENTRAL_LEN + name_len + extra_len + comment_len;
        if next > cd_end {
            return Err(broken());
        }
        if flags & 0x0001 != 0 {
            return Err(unsupported("encrypted documents are not accepted"));
        }
        if !matches!(method, 0 | 8) {
            return Err(unsupported(
                "the document uses an unsupported compression method",
            ));
        }
        let raw_name = &bytes[at + CENTRAL_LEN..at + CENTRAL_LEN + name_len];
        let name = std::str::from_utf8(raw_name).map_err(|_| broken())?;
        check_name(name)?;
        if !seen.insert(name.to_owned()) {
            return Err(unsupported("the document names one entry twice"));
        }
        uncompressed_total = uncompressed_total.saturating_add(uncompressed);
        if uncompressed_total > MAX_TOTAL_UNCOMPRESSED {
            return Err(unsupported("the document unpacks beyond the size limit"));
        }
        if uncompressed > 1024 * 1024 && uncompressed > compressed.saturating_mul(MAX_RATIO) {
            return Err(unsupported(
                "the document's compression ratio is not plausible",
            ));
        }
        check_local_header(bytes, local_offset, name_len, compressed, cd_end - cd_size)?;
        entries.push(Entry {
            name: name.to_owned(),
            method,
            compressed,
            local_offset,
        });
        at = next;
    }
    if at != cd_end {
        return Err(broken());
    }
    Ok(entries)
}

/// A relative name of plain components: no absolute path, no `..`, no backslash.
fn check_name(name: &str) -> Result<(), ApiError> {
    let bad = name.is_empty()
        || name.starts_with('/')
        || name.contains('\\')
        || name.contains('\0')
        || name.split('/').any(|part| part == "..");
    if bad {
        return Err(unsupported("the document holds an unsafe entry name"));
    }
    Ok(())
}

/// The local header an entry points at is before the central directory, says it is one, and
/// its data fits before the directory.
fn check_local_header(
    bytes: &[u8],
    offset: usize,
    name_len: usize,
    compressed: u64,
    cd_start: usize,
) -> Result<(), ApiError> {
    if offset + LOCAL_LEN > cd_start || &bytes[offset..offset + 4] != LOCAL_SIGNATURE {
        return Err(broken());
    }
    let local_name = usize::from(u16_at(bytes, offset + 26).ok_or_else(broken)?);
    let local_extra = usize::from(u16_at(bytes, offset + 28).ok_or_else(broken)?);
    if local_name != name_len {
        return Err(broken());
    }
    let data_start = offset + LOCAL_LEN + local_name + local_extra;
    if (data_start as u64).saturating_add(compressed) > cd_start as u64 {
        return Err(broken());
    }
    Ok(())
}

/// The bytes of one small entry, inflated under [`MAX_CONTENT_TYPES`].
fn inflate_entry(bytes: &[u8], entry: &Entry) -> Result<Vec<u8>, ApiError> {
    let offset = entry.local_offset;
    let local_name = usize::from(u16_at(bytes, offset + 26).ok_or_else(broken)?);
    let local_extra = usize::from(u16_at(bytes, offset + 28).ok_or_else(broken)?);
    let start = offset + LOCAL_LEN + local_name + local_extra;
    let end = start + usize::try_from(entry.compressed).map_err(|_| broken())?;
    let data = bytes.get(start..end).ok_or_else(broken)?;
    let mut out = Vec::new();
    let read = match entry.method {
        0 => data.take(MAX_CONTENT_TYPES + 1).read_to_end(&mut out),
        _ => flate2::read::DeflateDecoder::new(data)
            .take(MAX_CONTENT_TYPES + 1)
            .read_to_end(&mut out),
    };
    read.map_err(|_| broken())?;
    if out.len() as u64 > MAX_CONTENT_TYPES {
        return Err(unsupported(
            "the document's content types part is too large",
        ));
    }
    Ok(out)
}
