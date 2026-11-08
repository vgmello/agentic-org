//! A last guard against secrets in a prompt. The office never adds secrets itself (the
//! registry names exactly what goes in), but a person can paste one into a text field. This
//! looks for the common shapes and refuses the run. It is a heuristic, not a promise: it
//! will miss some secrets and flag a few hashes.

use serde_json::Value;

/// True when any string in `value` (keys included) looks like a secret.
pub fn contains_secret(value: &Value) -> bool {
    match value {
        Value::String(s) => text_has_secret(s),
        Value::Array(items) => items.iter().any(contains_secret),
        Value::Object(map) => map
            .iter()
            .any(|(k, v)| text_has_secret(k) || contains_secret(v)),
        _ => false,
    }
}

/// Prefixes that start a token, and how many token characters must follow.
const TOKEN_PREFIXES: &[(&str, usize)] = &[
    ("ghp_", 20),
    ("gho_", 20),
    ("ghu_", 20),
    ("ghs_", 20),
    ("ghr_", 20),
    ("github_pat_", 20),
    ("sk-", 20),
    ("AKIA", 16),
    ("ASIA", 16),
    ("AIza", 30),
    ("xoxb-", 10),
    ("xoxp-", 10),
    ("xoxa-", 10),
    ("xoxs-", 10),
];

/// The office's app token is 64 hex digits (`org-proto`); so is a SHA-256. Both are refused.
const HEX_RUN: usize = 64;

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '=' | '+' | '/')
}

fn token_run(text: &str) -> usize {
    text.chars().take_while(|c| is_token_char(*c)).count()
}

pub fn text_has_secret(text: &str) -> bool {
    if text.contains("-----BEGIN") && text.contains("PRIVATE KEY") {
        return true;
    }
    for (prefix, min) in TOKEN_PREFIXES {
        for (at, _) in text.match_indices(prefix) {
            let starts_word = text[..at]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_ascii_alphanumeric());
            if starts_word && token_run(&text[at + prefix.len()..]) >= *min {
                return true;
            }
        }
    }
    // A pasted "Authorization: Bearer <token>" header.
    for marker in ["Bearer ", "bearer "] {
        for (at, _) in text.match_indices(marker) {
            if token_run(&text[at + marker.len()..]) >= 20 {
                return true;
            }
        }
    }
    longest_hex_run(text) >= HEX_RUN
}

fn longest_hex_run(text: &str) -> usize {
    let (mut best, mut now) = (0, 0);
    for c in text.chars() {
        if c.is_ascii_hexdigit() {
            now += 1;
            best = best.max(now);
        } else {
            now = 0;
        }
    }
    best
}
