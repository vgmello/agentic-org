//! PEM private-key block and orphan-tail redaction helpers for `features::diagnostics::redact`.

pub(super) const REDACTED_PRIVATE_KEY: &str = "[REDACTED PRIVATE KEY]";

const PEM_BEGIN: &str = "-----BEGIN ";
const PEM_END: &str = "-----END ";
const PEM_DASHES: &str = "-----";
const PEM_MAX_LABEL: usize = 64;

fn is_private_key_label(after_marker: &str) -> bool {
    let label = match after_marker.find(PEM_DASHES) {
        Some(end) if end <= PEM_MAX_LABEL => &after_marker[..end],
        _ => &after_marker[..after_marker.floor_char_boundary(PEM_MAX_LABEL)],
    };
    label.to_ascii_uppercase().contains("PRIVATE KEY")
}

fn find_private_key_begin(input: &str) -> Option<usize> {
    let mut offset = 0;
    while let Some(rel) = input[offset..].find(PEM_BEGIN) {
        let idx = offset + rel;
        let after = &input[idx + PEM_BEGIN.len()..];
        if is_private_key_label(after) {
            return Some(idx);
        }
        offset = idx + PEM_BEGIN.len();
    }
    None
}

fn find_private_key_end(input: &str) -> Option<(usize, usize)> {
    let mut offset = 0;
    while let Some(rel) = input[offset..].find(PEM_END) {
        let idx = offset + rel;
        let after = &input[idx + PEM_END.len()..];
        if is_private_key_label(after) {
            let end_after = after
                .find(PEM_DASHES)
                .map_or(input.len(), |n| idx + PEM_END.len() + n + PEM_DASHES.len());
            return Some((idx, end_after));
        }
        offset = idx + PEM_END.len();
    }
    None
}

pub(super) fn has_leading_orphan_pem_end(line: &str) -> bool {
    let Some((end_start, _)) = find_private_key_end(line) else {
        return false;
    };
    find_private_key_begin(line).is_none_or(|begin_start| end_start < begin_start)
}

pub(super) fn has_unclosed_pem_begin(line: &str) -> bool {
    let Some(start) = line.rfind(PEM_BEGIN) else {
        return false;
    };
    let after = &line[start + PEM_BEGIN.len()..];
    is_private_key_label(after) && find_private_key_end(after).is_none()
}

pub(super) fn strip_pem_end_line(line: &str) -> Option<&str> {
    let (_, end_after) = find_private_key_end(line)?;
    Some(&line[end_after..])
}

fn is_b64_bytes(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
}

fn is_long_pem_line(s: &str) -> bool {
    s.len() >= 16 && is_b64_bytes(s)
}

fn has_b64_signal(s: &str) -> bool {
    s.ends_with('=')
        || s.bytes()
            .any(|b| b.is_ascii_digit() || matches!(b, b'+' | b'/'))
        || s.bytes().skip(1).any(|b| b.is_ascii_uppercase())
}

fn is_short_pem_tail(s: &str, preceded_by_long_b64: bool) -> bool {
    (4..=15).contains(&s.len()) && is_b64_bytes(s) && (preceded_by_long_b64 || has_b64_signal(s))
}

pub(super) fn scrub_preceding_pem_body(out: &mut [String], total: &mut usize) {
    let mut idx = out.len();
    let mut first_line = true;
    while idx > 0 {
        idx -= 1;
        let trimmed = out[idx].trim();
        let prev_is_long = idx > 0 && is_long_pem_line(out[idx - 1].trim());
        let matches =
            is_long_pem_line(trimmed) || (first_line && is_short_pem_tail(trimmed, prev_is_long));
        first_line = false;
        if !matches {
            break;
        }
        out[idx] = REDACTED_PRIVATE_KEY.to_owned();
        *total += 1;
    }
}

pub(super) fn redact_pem_blocks(input: &str, total: &mut usize) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find(PEM_BEGIN) {
        let after_begin = &rest[start + PEM_BEGIN.len()..];
        if !is_private_key_label(after_begin) {
            out.push_str(&rest[..start + PEM_BEGIN.len()]);
            rest = after_begin;
            continue;
        }
        out.push_str(&rest[..start]);
        out.push_str(REDACTED_PRIVATE_KEY);
        *total += 1;
        rest = skip_past_pem_end(after_begin);
    }
    out.push_str(rest);
    redact_orphan_pem_ends(&out, total)
}

fn redact_orphan_pem_ends(input: &str, total: &mut usize) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some((end_start, end_after)) = find_private_key_end(rest) {
        let cleaned = scrub_orphan_pem_prefix(&rest[..end_start], total);
        out.push_str(&cleaned);
        out.push_str(REDACTED_PRIVATE_KEY);
        *total += 1;
        rest = &rest[end_after..];
    }
    out.push_str(rest);
    out
}

fn scrub_orphan_pem_prefix(before: &str, total: &mut usize) -> String {
    if before.is_empty() {
        return String::new();
    }
    let mut lines: Vec<String> = before.split('\n').map(str::to_owned).collect();
    let Some(last) = lines.pop() else {
        return String::new();
    };
    let (cleaned_last, last_all_scrubbed) =
        scrub_inline_orphan_tail(&last, !lines.is_empty(), total);
    if last.trim().is_empty() || last_all_scrubbed {
        scrub_preceding_pem_body(&mut lines, total);
    }
    if lines.is_empty() {
        cleaned_last
    } else {
        format!("{}\n{cleaned_last}", lines.join("\n"))
    }
}

fn scrub_inline_orphan_tail(
    segment: &str,
    has_prev_line: bool,
    total: &mut usize,
) -> (String, bool) {
    let mut end = segment.len();
    let mut first = true;
    let mut scrubbed_any = false;
    loop {
        let trimmed_end = trim_trailing_pem_seps(&segment[..end]);
        let tok_start = trimmed_end
            - segment[..trimmed_end]
                .bytes()
                .rev()
                .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
                .count();
        let token = &segment[tok_start..trimmed_end];
        if token.is_empty() {
            break;
        }
        let preceded_by_b64 = has_prev_line && tok_start == 0;
        let is_pem_chunk =
            is_long_pem_line(token) || (first && is_short_pem_tail(token, preceded_by_b64));
        first = false;
        if !is_pem_chunk {
            break;
        }
        end = tok_start;
        scrubbed_any = true;
        *total += 1;
    }
    let prefix = &segment[..end];
    (prefix.to_owned(), scrubbed_any && prefix.trim().is_empty())
}

fn trim_trailing_pem_seps(s: &str) -> usize {
    let mut end = s.len();
    loop {
        if let Some(prev) = s[..end]
            .strip_suffix("\\n")
            .or_else(|| s[..end].strip_suffix("\\r"))
        {
            end = prev.len();
        } else if s[..end].ends_with([' ', '\t', '\r']) {
            end -= 1;
        } else {
            return end;
        }
    }
}

fn skip_past_pem_end(after_begin: &str) -> &str {
    let Some(end_idx) = after_begin.find(PEM_END) else {
        return "";
    };
    let tail = &after_begin[end_idx + PEM_END.len()..];
    tail.find(PEM_DASHES)
        .map_or("", |off| &tail[off + PEM_DASHES.len()..])
}
