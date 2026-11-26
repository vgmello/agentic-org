//! PDF attachments, checked structurally and never rendered: the `%PDF-` header, a
//! `startxref` that points inside the file, a final `%%EOF`, and a page count.
//!
//! The scan is linear and bounded: it never follows an object reference (so a cyclic page
//! tree cannot loop it), and it inflates compressed object streams (`/Type /ObjStm`) only up to
//! a fixed total budget, so a hostile file cannot make it allocate more than that.

use std::io::Read;

use org_proto::ApiError;

use super::corrupt;

/// The most bytes all object streams of one file may inflate to.
const INFLATE_BUDGET: u64 = 64 * 1024 * 1024;
/// The most object streams one file may have inflated.
const MAX_OBJECT_STREAMS: usize = 4_096;
/// The largest page count accepted.
pub const MAX_PAGES: u32 = 100_000;
/// How far from the end `startxref` and `%%EOF` may be.
const TAIL: usize = 2_048;

/// Checks a PDF and returns its page count.
pub fn page_count(bytes: &[u8]) -> Result<u32, ApiError> {
    if !bytes.starts_with(b"%PDF-") {
        return Err(corrupt("a PDF must begin with `%PDF-`"));
    }
    let version = bytes.get(5..8).unwrap_or_default();
    if !(version.len() == 3
        && version[0].is_ascii_digit()
        && version[1] == b'.'
        && version[2].is_ascii_digit())
    {
        return Err(corrupt("a PDF must name its version after `%PDF-`"));
    }
    check_trailer(bytes)?;
    let mut counts = Counts::default();
    counts.scan(bytes);
    if counts.pages_nodes == 0 && counts.leaves == 0 {
        counts.scan_object_streams(bytes)?;
    }
    let pages = counts.page_count();
    if pages == 0 {
        return Err(corrupt("the PDF has no readable page tree"));
    }
    if pages > MAX_PAGES {
        return Err(corrupt(format!("a PDF has at most {MAX_PAGES} pages")));
    }
    Ok(pages)
}

/// `%%EOF` at the end (white space after it allowed) and a `startxref` before it whose offset
/// lies inside the file.
fn check_trailer(bytes: &[u8]) -> Result<(), ApiError> {
    let tail_start = bytes.len().saturating_sub(TAIL);
    let tail = &bytes[tail_start..];
    let trimmed_len = tail
        .iter()
        .rposition(|b| !b.is_ascii_whitespace() && *b != 0)
        .map_or(0, |at| at + 1);
    let trimmed = &tail[..trimmed_len];
    if !trimmed.ends_with(b"%%EOF") {
        return Err(corrupt(
            "the PDF is truncated: it does not end with `%%EOF`",
        ));
    }
    let Some(at) = find_last(trimmed, b"startxref") else {
        return Err(corrupt("the PDF is truncated: it has no `startxref`"));
    };
    let after = &trimmed[at + b"startxref".len()..];
    let digits: Vec<u8> = after
        .iter()
        .copied()
        .skip_while(u8::is_ascii_whitespace)
        .take_while(u8::is_ascii_digit)
        .take(20)
        .collect();
    let offset = std::str::from_utf8(&digits)
        .ok()
        .and_then(|text| text.parse::<u64>().ok())
        .ok_or_else(|| corrupt("the PDF's `startxref` names no offset"))?;
    if offset == 0 || offset >= bytes.len() as u64 {
        return Err(corrupt("the PDF's `startxref` points outside the file"));
    }
    Ok(())
}

#[derive(Default)]
struct Counts {
    /// The largest `/Count` of any `/Type /Pages` node: the root's.
    max_pages_count: u64,
    pages_nodes: usize,
    /// `/Type /Page` leaves.
    leaves: u64,
}

impl Counts {
    fn page_count(&self) -> u32 {
        let pages = if self.pages_nodes > 0 {
            self.max_pages_count
        } else {
            self.leaves
        };
        u32::try_from(pages).unwrap_or(u32::MAX)
    }

    /// Counts every `/Type /Page` and `/Type /Pages` in `data`.
    fn scan(&mut self, data: &[u8]) {
        let mut from = 0;
        while let Some(found) = find(&data[from..], b"/Type") {
            let at = from + found + b"/Type".len();
            from = at;
            let rest = skip_space(&data[at..]);
            if !rest.starts_with(b"/Page") {
                continue;
            }
            let after = &rest[b"/Page".len()..];
            if after.first() == Some(&b's') && !after.get(1).is_some_and(|b| is_name_char(*b)) {
                self.pages_nodes += 1;
                if let Some(count) = count_near(data, at) {
                    self.max_pages_count = self.max_pages_count.max(count);
                }
            } else if !after.first().is_some_and(|b| is_name_char(*b)) {
                self.leaves += 1;
            }
        }
    }

    /// Inflates each `/Type /ObjStm` stream (bounded) and scans what it holds.
    fn scan_object_streams(&mut self, data: &[u8]) -> Result<(), ApiError> {
        // Every `endstream` once, so finding a stream's end never rescans the file.
        let ends = positions(data, b"endstream");
        let mut budget = INFLATE_BUDGET;
        let mut streams = 0;
        let mut from = 0;
        while let Some(found) = find(&data[from..], b"/ObjStm") {
            let at = from + found;
            from = at + b"/ObjStm".len();
            streams += 1;
            if streams > MAX_OBJECT_STREAMS {
                return Err(corrupt("the PDF has too many object streams"));
            }
            let Some(body) = stream_body(data, from, &ends) else {
                continue;
            };
            let mut inflated = Vec::new();
            let mut decoder = flate2::read::ZlibDecoder::new(body).take(budget + 1);
            // A damaged stream inflates to what it could; the scan reads what is there.
            let _ = decoder.read_to_end(&mut inflated);
            if inflated.len() as u64 > budget {
                return Err(corrupt(
                    "the PDF's compressed objects inflate beyond the limit",
                ));
            }
            budget -= inflated.len() as u64;
            self.scan(&inflated);
        }
        Ok(())
    }
}

/// The bytes between the next `stream` keyword after `from` and the first `endstream` after
/// it (`ends` holds every `endstream` offset, ascending).
fn stream_body<'a>(data: &'a [u8], from: usize, ends: &[usize]) -> Option<&'a [u8]> {
    let window_end = data.len().min(from.saturating_add(4_096));
    let start = from + find(&data[from..window_end], b"stream")? + b"stream".len();
    let start = match data.get(start..start + 2) {
        Some(b"\r\n") => start + 2,
        _ if matches!(data.get(start), Some(b'\n' | b'\r')) => start + 1,
        _ => start,
    };
    let end = *ends.get(ends.partition_point(|&end| end < start))?;
    Some(&data[start..end])
}

/// Every offset of `needle` in `data`, ascending.
fn positions(data: &[u8], needle: &[u8]) -> Vec<usize> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = find(&data[from..], needle) {
        found.push(from + at);
        from += at + needle.len();
    }
    found
}

/// The `/Count N` of the dictionary around `at` (searched within its nearest `<<` `>>`).
fn count_near(data: &[u8], at: usize) -> Option<u64> {
    let open = find_last(&data[at.saturating_sub(4_096)..at], b"<<")
        .map(|found| at.saturating_sub(4_096) + found)?;
    let close = at + find(&data[at..data.len().min(at + 4_096)], b">>")?;
    let dict = &data[open..close];
    let count_at = find(dict, b"/Count")? + b"/Count".len();
    let digits: Vec<u8> = skip_space(&dict[count_at..])
        .iter()
        .copied()
        .take_while(u8::is_ascii_digit)
        .take(12)
        .collect();
    std::str::from_utf8(&digits).ok()?.parse().ok()
}

fn is_name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')
}

fn skip_space(data: &[u8]) -> &[u8] {
    let start = data
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(data.len());
    &data[start..]
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn find_last(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).rposition(|w| w == needle)
}
