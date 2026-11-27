//! Single secret-redaction choke-point for `agentic-org` diagnostics (`logs`, `doctor`, `support`).
//!
//! Composes directly on Layer-0 [`org_redact::redact_text`] for shared secret-pattern scrubbing
//! (prefixes, AWS keys, Bearer/Basic auth schemes, JWTs, URL userinfo, HTTP headers, JSON/env
//! secrets) and layers CLI-specific diagnostics helpers on top: multi-line/orphan PEM private-key
//! scrubbing (`redact_pem`), invite/reset/registration token URLs, unquoted YAML/log fields, extra
//! diagnostic secret keys, and recovery codes (`redact_fields`).

use serde_json::Value;

pub(crate) use super::redact_fields::REDACTED;
use super::redact_fields::{
    find_ascii_ci, is_sensitive_key, redact_env_assignments, redact_quoted_key_values,
    redact_recovery_codes, redact_unquoted_key_values, unquoted_token_len,
};
use super::redact_pem::{
    has_leading_orphan_pem_end, has_unclosed_pem_begin, redact_pem_blocks,
    scrub_preceding_pem_body, strip_pem_end_line, REDACTED_PRIVATE_KEY,
};

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

fn count_redaction_markers(text: &str) -> usize {
    text.matches(REDACTED).count() + text.matches(REDACTED_PRIVATE_KEY).count()
}

fn apply_shared_org_redact(input: &str, total: &mut usize) -> String {
    let before_markers = count_redaction_markers(input);
    let after = org_redact::redact_text(input, &[]);
    if after != input {
        let after_markers = count_redaction_markers(&after);
        *total += after_markers.saturating_sub(before_markers).max(1);
    }
    after
}

/// Redacts all known secret shapes from `input`, returning `(redacted_text, replacement_count)`.
pub(crate) fn redact_text(input: &str) -> Redaction {
    let mut total = 0usize;
    let mut out = redact_pem_blocks(input, &mut total);
    out = apply_shared_org_redact(&out, &mut total);
    out = redact_token_urls(&out, &mut total);
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
