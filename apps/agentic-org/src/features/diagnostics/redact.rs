//! Single secret-redaction choke-point for `agentic-org` diagnostics (`logs`, `doctor`, `support`).
//!
//! Orchestrates PEM private keys (single-line, multi-line, and truncated), provider/service token
//! prefixes, auth schemes, JWTs, URL userinfo, invite/reset/registration URL tokens, and delegates
//! header, quoted-field, env/query, unquoted-field, and recovery-code passes to `redact_fields`.

use serde_json::Value;

use super::redact_fields::{
    is_sensitive_key, redact_env_assignments, redact_header_values, redact_quoted_key_values,
    redact_recovery_codes, redact_unquoted_key_values,
};
use super::redact_pem::{
    has_leading_orphan_pem_end, has_unclosed_pem_begin, redact_pem_blocks,
    scrub_preceding_pem_body, strip_pem_end_line, REDACTED_PRIVATE_KEY,
};

/// Replacement for every redacted secret value.
pub(crate) const REDACTED: &str = "[REDACTED]";

const SECRET_PREFIXES: &[(&str, usize)] = &[
    ("sk-ant-", 6),
    ("sk-proj-", 6),
    ("sk-svcacct-", 6),
    ("sk-admin-", 6),
    ("sk-live-", 6),
    ("sk-test-", 6),
    ("sk_live_", 6),
    ("sk_test_", 6),
    ("rk_live_", 6),
    ("github_pat_", 6),
    ("ghp_", 6),
    ("gho_", 6),
    ("ghs_", 6),
    ("ghu_", 6),
    ("ghr_", 6),
    ("glpat-", 6),
    ("xoxb-", 6),
    ("xoxp-", 6),
    ("xoxa-", 6),
    ("xoxr-", 6),
    ("xoxs-", 6),
    ("xapp-", 6),
    ("AIza", 8),
    ("ya29.", 8),
];

const AWS_KEY_ID_PREFIXES: &[&str] = &["AKIA", "ASIA"];
const AWS_KEY_ID_TAIL: usize = 16;

const TOKEN_URL_MARKERS: &[(&str, usize)] = &[
    ("/register#", 6),
    ("/set-password#", 6),
    ("/reset-password#", 6),
    ("/reset#", 6),
    ("/invite#", 6),
    ("/invites#", 6),
    ("/join#", 6),
    ("/approve#", 6),
    ("/recovery#", 6),
    ("/register/", 8),
    ("/set-password/", 8),
    ("/reset-password/", 8),
    ("/invite/", 8),
    ("/invites/", 8),
    ("/auth/approve/", 8),
];

/// Result of redacting text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Redaction {
    pub(crate) text: String,
    pub(crate) count: usize,
}

/// Redacts all known secret shapes from `input`, returning `(redacted_text, replacement_count)`.
pub(crate) fn redact_text(input: &str) -> Redaction {
    let mut total = 0usize;
    let mut out = redact_pem_blocks(input, &mut total);
    out = redact_all_prefixes(&out, &mut total);
    out = redact_auth_scheme(&out, "bearer", true, &mut total);
    out = redact_auth_scheme(&out, "basic", false, &mut total);
    out = redact_jwts(&out, &mut total);
    out = redact_url_userinfo(&out, &mut total);
    out = redact_token_urls(&out, &mut total);
    out = redact_header_values(&out, &mut total);
    out = redact_quoted_key_values(&out, &mut total);
    out = redact_unquoted_key_values(&out, &mut total);
    out = redact_env_assignments(&out, &mut total);
    out = redact_recovery_codes(&out, &mut total);
    Redaction {
        text: out,
        count: total,
    }
}

/// Redacts a sequence of log lines while tracking multi-line PEM private key blocks across lines.
pub(crate) fn redact_lines(lines: &[String]) -> (Vec<String>, usize) {
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut total = 0usize;
    let mut in_pem = false;

    for raw in lines {
        if in_pem {
            total += 1;
            if let Some(after) = strip_pem_end_line(raw) {
                in_pem = has_unclosed_pem_begin(after);
                out.push(format_pem_end_tail(after, &mut total));
            } else {
                out.push(REDACTED_PRIVATE_KEY.to_owned());
            }
            continue;
        }
        if has_leading_orphan_pem_end(raw) {
            scrub_preceding_pem_body(&mut out, &mut total);
        }
        if has_unclosed_pem_begin(raw) {
            in_pem = true;
        }
        let red = redact_text(raw);
        total += red.count;
        out.push(red.text);
    }
    (out, total)
}

fn format_pem_end_tail(after: &str, total: &mut usize) -> String {
    let tail = redact_text(after);
    *total += tail.count;
    if tail.text.is_empty() {
        REDACTED_PRIVATE_KEY.to_owned()
    } else {
        format!("{REDACTED_PRIVATE_KEY}{}", tail.text)
    }
}

/// Recursively redacts a JSON value: any field with a sensitive key becomes `[REDACTED]`, and
/// every remaining string value and key passes through [`redact_text`].
pub(crate) fn redact_json(value: &Value) -> (Value, usize) {
    let mut total = 0usize;
    let cleaned = redact_json_inner(value, &mut total);
    (cleaned, total)
}

fn redact_json_inner(value: &Value, total: &mut usize) -> Value {
    match value {
        Value::String(text) => {
            let red = redact_text(text);
            *total += red.count;
            Value::String(red.text)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| redact_json_inner(item, total))
                .collect(),
        ),
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (key, val) in map {
                let red_key = redact_text(key);
                *total += red_key.count;
                let red_val = if is_sensitive_key(key) && !val.is_null() {
                    *total += 1;
                    Value::String(REDACTED.to_owned())
                } else {
                    redact_json_inner(val, total)
                };
                out.insert(red_key.text, red_val);
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

pub(super) fn is_token_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')
}

pub(super) fn odd_backslashes_before(bytes: &[u8], idx: usize) -> bool {
    bytes[..idx]
        .iter()
        .rev()
        .take_while(|&&b| b == b'\\')
        .count()
        % 2
        == 1
}

pub(super) fn is_json_space_escape(bytes: &[u8], idx: usize) -> bool {
    matches!(bytes[idx], b'n' | b'r' | b't') && odd_backslashes_before(bytes, idx)
}

fn at_word_start(input: &str, idx: usize) -> bool {
    let bytes = input.as_bytes();
    idx == 0 || !bytes[idx - 1].is_ascii_alphanumeric() || is_json_space_escape(bytes, idx - 1)
}

pub(super) fn find_ascii_ci(haystack: &str, needle_lower: &str) -> Option<usize> {
    let h = haystack.as_bytes();
    let n = needle_lower.as_bytes();
    if n.is_empty() || n.len() > h.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| h[i..i + n.len()].eq_ignore_ascii_case(n))
}

pub(super) fn unquoted_token_len(bytes: &[u8], stops: &[u8]) -> usize {
    let mut len = 0;
    while len < bytes.len() {
        let b = bytes[len];
        if b == b'\\' {
            let run = bytes[len..].iter().take_while(|&&c| c == b'\\').count();
            if run % 2 == 1 && matches!(bytes.get(len + run), Some(b'"' | b'n' | b'r' | b't')) {
                len += run - 1;
                break;
            }
            len += run;
            continue;
        }
        if b.is_ascii_whitespace() || stops.contains(&b) {
            break;
        }
        len += 1;
    }
    len
}

fn redact_all_prefixes(input: &str, total: &mut usize) -> String {
    let mut out = input.to_owned();
    for &(prefix, min_tail) in SECRET_PREFIXES {
        out = redact_prefixed_where(&out, prefix, total, |after| {
            let tail = after.bytes().take_while(|&b| is_token_byte(b)).count();
            (tail >= min_tail).then_some(tail)
        });
    }
    out = redact_prefixed_where(&out, "sk-", total, |after| {
        let tail = after.bytes().take_while(|&b| is_token_byte(b)).count();
        let slice = &after[..tail];
        let is_key = tail >= 12 || (tail >= 8 && !slice.bytes().all(|b| b.is_ascii_lowercase()));
        is_key.then_some(tail)
    });
    for prefix in AWS_KEY_ID_PREFIXES {
        out = redact_prefixed_where(&out, prefix, total, |after| {
            let tail = after
                .bytes()
                .take_while(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
                .count();
            (tail >= AWS_KEY_ID_TAIL).then_some(tail)
        });
    }
    out
}

fn redact_prefixed_where(
    input: &str,
    prefix: &str,
    total: &mut usize,
    tail_len: impl Fn(&str) -> Option<usize>,
) -> String {
    let mut out = String::with_capacity(input.len());
    let mut pos = 0;
    while let Some(rel) = input[pos..].find(prefix) {
        let idx = pos + rel;
        let after = idx + prefix.len();
        out.push_str(&input[pos..idx]);
        match tail_len(&input[after..]).filter(|_| at_word_start(input, idx)) {
            Some(tail) => {
                out.push_str(REDACTED);
                *total += 1;
                pos = after + tail;
            }
            None => {
                out.push_str(prefix);
                pos = after;
            }
        }
    }
    out.push_str(&input[pos..]);
    out
}

fn looks_like_credential(token: &str, allow_lowercase: bool) -> bool {
    if token.len() < 6 || token == REDACTED {
        return false;
    }
    allow_lowercase || !token.bytes().all(|b| b.is_ascii_lowercase())
}

fn redact_auth_scheme(
    input: &str,
    scheme_lower: &str,
    allow_lowercase: bool,
    total: &mut usize,
) -> String {
    let mut out = String::with_capacity(input.len());
    let mut pos = 0;
    while let Some(rel) = find_ascii_ci(&input[pos..], scheme_lower) {
        let idx = pos + rel;
        let after = idx + scheme_lower.len();
        let gap = input[after..].len() - input[after..].trim_start_matches([' ', '\t']).len();
        if gap == 0 || !at_word_start(input, idx) {
            out.push_str(&input[pos..after]);
            pos = after;
            continue;
        }
        let token_start = after + gap;
        let token_len = unquoted_token_len(&input.as_bytes()[token_start..], b"\"',};)");
        let token = &input[token_start..token_start + token_len];
        out.push_str(&input[pos..token_start]);
        if looks_like_credential(token, allow_lowercase) {
            out.push_str(REDACTED);
            *total += 1;
        } else {
            out.push_str(token);
        }
        pos = token_start + token_len;
    }
    out.push_str(&input[pos..]);
    out
}

fn redact_jwts(input: &str, total: &mut usize) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(idx) = rest.find("eyJ") {
        out.push_str(&rest[..idx]);
        let cand = &rest[idx..];
        let len = cand.bytes().take_while(|&b| is_token_byte(b)).count();
        let parts: Vec<&str> = cand[..len].split('.').collect();
        if parts.len() >= 2 && parts[0].len() >= 10 && parts[1].len() >= 10 {
            out.push_str(REDACTED);
            *total += 1;
            rest = &cand[len..];
        } else {
            out.push_str("eyJ");
            rest = &cand[3..];
        }
    }
    out.push_str(rest);
    out
}

const MIN_BARE_USERINFO_TOKEN: usize = 16;

fn redact_url_userinfo(input: &str, total: &mut usize) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(idx) = rest.find("://") {
        out.push_str(&rest[..idx + 3]);
        let after = &rest[idx + 3..];
        let end = after
            .bytes()
            .position(|b| b.is_ascii_whitespace() || matches!(b, b'/' | b'?' | b'#' | b'"' | b'\''))
            .unwrap_or(after.len());
        let authority = &after[..end];
        match authority.rfind('@') {
            Some(at)
                if authority[..at] != *REDACTED
                    && (authority[..at].contains(':')
                        || authority[..at].len() >= MIN_BARE_USERINFO_TOKEN) =>
            {
                out.push_str(REDACTED);
                out.push_str(&authority[at..]);
                *total += 1;
            }
            _ => out.push_str(authority),
        }
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

fn redact_token_urls(input: &str, total: &mut usize) -> String {
    let mut out = input.to_owned();
    for &(marker, min_len) in TOKEN_URL_MARKERS {
        let mut next = String::with_capacity(out.len());
        let mut pos = 0;
        while let Some(rel) = find_ascii_ci(&out[pos..], marker) {
            let idx = pos + rel;
            let tok_start = idx + marker.len();
            next.push_str(&out[pos..tok_start]);
            let tok_len = unquoted_token_len(&out.as_bytes()[tok_start..], b"\"'&?#/,)}]>");
            let tok = &out[tok_start..tok_start + tok_len];
            if tok_len >= min_len && tok != REDACTED {
                next.push_str(REDACTED);
                *total += 1;
                pos = tok_start + tok_len;
            } else {
                pos = tok_start;
            }
        }
        next.push_str(&out[pos..]);
        out = next;
    }
    out
}
