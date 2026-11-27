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
    ("sk_live_", 16),
    ("sk_test_", 16),
    ("rk_live_", 16),
    ("glpat-", 16),
    ("npm_", 30),
    ("xapp-", 10),
    ("xoxe", 10),
    ("AKIA", 16),
    ("ASIA", 16),
    ("AIza", 30),
    ("xoxb-", 10),
    ("xoxp-", 10),
    ("xoxa-", 10),
    ("xoxs-", 10),
];

/// An office's fixed-size secrets are 64 hex digits (`org-proto`); so is a SHA-256. Both are refused.
const HEX_RUN: usize = 64;

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '=' | '+' | '/')
}

fn token_run(text: &str) -> usize {
    text.chars().take_while(|c| is_token_char(*c)).count()
}

pub fn text_has_secret(text: &str) -> bool {
    has_private_key_marker(text)
        || has_token_prefix(text)
        || has_bearer_token(text)
        || has_sensitive_header(text)
        || has_url_userinfo(text)
        || has_secret_assignment(text)
        || has_jwt(text)
        || longest_hex_run(text) >= HEX_RUN
}

fn has_private_key_marker(text: &str) -> bool {
    text.contains("-----BEGIN") && text.contains("PRIVATE KEY")
}

fn has_token_prefix(text: &str) -> bool {
    TOKEN_PREFIXES.iter().any(|(prefix, min)| {
        text.match_indices(prefix).any(|(at, _)| {
            let starts_word = text[..at]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_ascii_alphanumeric());
            starts_word && token_run(&text[at + prefix.len()..]) >= *min
        })
    })
}

/// A pasted "Authorization: Bearer <token>" header: a long run that holds a digit (plain
/// words after "Bearer" are not a token).
fn has_bearer_token(text: &str) -> bool {
    ["Bearer ", "bearer "].iter().any(|marker| {
        text.match_indices(marker).any(|(at, _)| {
            let run: String = text[at + marker.len()..]
                .chars()
                .take_while(|c| is_token_char(*c))
                .collect();
            run.chars().count() >= 20 && run.chars().any(|c| c.is_ascii_digit())
        })
    })
}

/// Pasted sensitive HTTP headers (`Cookie:`, `Set-Cookie:`, `X-Org-Session-Proof:`, `X-Vault-Token:`).
fn has_sensitive_header(text: &str) -> bool {
    const HEADERS: &[&str] = &[
        "cookie:",
        "set-cookie:",
        "x-org-session-proof:",
        "x-vault-token:",
        "x-api-key:",
    ];
    let lower = text.to_ascii_lowercase();
    HEADERS.iter().any(|hdr| {
        lower.match_indices(hdr).any(|(at, _)| {
            let starts_boundary = at == 0
                || !lower.as_bytes()[at - 1].is_ascii_alphanumeric()
                    && lower.as_bytes()[at - 1] != b'-';
            let after = lower[at + hdr.len()..].trim_start_matches([' ', '\t']);
            let val_len = after
                .bytes()
                .take_while(|b| !matches!(b, b'\n' | b'\r'))
                .count();
            starts_boundary && val_len >= 6
        })
    })
}

/// Embedded credentials in a URL (`https://user:pass@host`).
fn has_url_userinfo(text: &str) -> bool {
    let mut rest = text;
    while let Some(pos) = rest.find("://") {
        let after = &rest[pos + 3..];
        let authority_end = after
            .find(['/', '?', '#', ' ', '\t', '\n', '\r', '"', '\''])
            .unwrap_or(after.len());
        let authority = &after[..authority_end];
        if let Some((userinfo, host)) = authority.split_once('@') {
            if !host.is_empty()
                && userinfo
                    .split_once(':')
                    .is_some_and(|(_, pw)| !pw.is_empty())
            {
                return true;
            }
        }
        rest = after;
    }
    false
}

/// Explicit `KEY=value` secret assignments (`API_KEY=...`, `VAULT_TOKEN=...`, `PASSWORD=...`).
fn has_secret_assignment(text: &str) -> bool {
    const KEYS: &[&str] = &[
        "api_key",
        "access_token",
        "refresh_token",
        "session_token",
        "client_secret",
        "vault_token",
        "vault_credential",
        "password",
        "passphrase",
        "private_key",
        "agent_os_session_token",
    ];
    let bytes = text.as_bytes();
    let mut pos = 0;
    while let Some(rel) = text[pos..].find('=') {
        let eq = pos + rel;
        let key_start = eq
            - bytes[..eq]
                .iter()
                .rev()
                .take_while(|b| b.is_ascii_alphanumeric() || **b == b'_')
                .count();
        let key = text[key_start..eq].to_ascii_lowercase();
        let after = &text[eq + 1..];
        if !after.starts_with('=') && KEYS.iter().any(|k| key == *k || key.ends_with(k)) {
            let trimmed = after.trim_start_matches(['"', '\'']);
            let val_len = trimmed
                .bytes()
                .take_while(|b| {
                    !b.is_ascii_whitespace() && !matches!(b, b'"' | b'\'' | b';' | b',')
                })
                .count();
            if val_len >= 6 {
                return true;
            }
        }
        pos = eq + 1;
    }
    false
}

/// A JSON web token: `eyJ` (a base64 `{"`), then two more dot-separated parts.
fn has_jwt(text: &str) -> bool {
    text.match_indices("eyJ").any(|(at, _)| {
        let token: String = text[at..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
            .collect();
        let parts: Vec<&str> = token.split('.').collect();
        parts.len() >= 3 && parts.iter().take(3).all(|p| p.len() >= 10)
    })
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
