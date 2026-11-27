//! A last guard against secrets in a prompt. The office never adds secrets itself (the
//! registry names exactly what goes in), but a person can paste one into a text field.
//!
//! Delegates first through the Layer-0 workspace redaction choke-point (`org-redact`) and adds
//! `org-copilot`-specific fail-closed checks (headers, case-insensitive `Bearer`/`Basic` tokens
//! including short/digitless forms, JSON/YAML secret fields, `npm_`/`xoxe` prefixes, PEM private
//! key markers, and 64-hex office wire secrets / SHA-256 digests).

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

/// Copilot-specific token prefixes beyond `org-redact`'s Layer-0 set.
const EXTRA_TOKEN_PREFIXES: &[(&str, usize)] = &[("npm_", 30), ("xoxe", 10)];

/// An office's fixed-size secrets are 64 hex digits (`org-proto`); so is a SHA-256. Both are refused.
const HEX_RUN: usize = 64;

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '=' | '+' | '/')
}

fn token_run(text: &str) -> usize {
    text.chars().take_while(|c| is_token_char(*c)).count()
}

pub fn text_has_secret(text: &str) -> bool {
    org_redact::redact_text(text, &[]) != text
        || has_private_key_marker(text)
        || has_extra_token_prefix(text)
        || has_sensitive_header(text)
        || has_auth_scheme_token(text)
        || has_yaml_or_json_secret_field(text)
        || longest_hex_run(text) >= HEX_RUN
}

fn has_private_key_marker(text: &str) -> bool {
    let upper = text.to_ascii_uppercase();
    upper.contains("-----BEGIN") && upper.contains("PRIVATE KEY")
}

fn has_extra_token_prefix(text: &str) -> bool {
    EXTRA_TOKEN_PREFIXES.iter().any(|(prefix, min)| {
        text.match_indices(prefix).any(|(at, _)| {
            let starts_word = text[..at]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_ascii_alphanumeric());
            starts_word && token_run(&text[at + prefix.len()..]) >= *min
        })
    })
}

/// Pasted sensitive HTTP headers with any non-empty value (case-insensitive).
fn has_sensitive_header(text: &str) -> bool {
    const HEADERS: &[&str] = &[
        "authorization:",
        "proxy-authorization:",
        "x-auth-token:",
        "cookie:",
        "set-cookie:",
        "x-org-session-proof:",
        "x-vault-token:",
        "x-api-key:",
    ];
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    HEADERS.iter().any(|hdr| {
        lower.match_indices(hdr).any(|(at, _)| {
            let starts_boundary = at == 0
                || (!bytes[at - 1].is_ascii_alphanumeric()
                    && !matches!(bytes[at - 1], b'-' | b'_'));
            let after = lower[at + hdr.len()..].trim_start_matches([' ', '\t']);
            let val_len = after
                .bytes()
                .take_while(|b| !matches!(b, b'\n' | b'\r'))
                .count();
            starts_boundary && val_len >= 1
        })
    })
}

/// Case-insensitive `Bearer` / `Basic` token detection (including short/digitless tokens), while
/// leaving ordinary lowercase English prose such as `"basic understanding"` or
/// `"Bearer authentication flow"` readable.
fn has_auth_scheme_token(text: &str) -> bool {
    has_scheme_match(text, "bearer", true) || has_scheme_match(text, "basic", false)
}

fn has_scheme_match(text: &str, scheme_lower: &str, is_bearer: bool) -> bool {
    let bytes = text.as_bytes();
    let needle = scheme_lower.as_bytes();
    if bytes.len() <= needle.len() {
        return false;
    }
    (0..bytes.len() - needle.len()).any(|i| scheme_matches_at(text, bytes, i, needle, is_bearer))
}

fn scheme_matches_at(text: &str, bytes: &[u8], i: usize, needle: &[u8], is_bearer: bool) -> bool {
    let starts_word = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
    if !starts_word || !bytes[i..i + needle.len()].eq_ignore_ascii_case(needle) {
        return false;
    }
    let after_scheme = i + needle.len();
    let ws = text[after_scheme..]
        .bytes()
        .take_while(|b| matches!(b, b' ' | b'\t'))
        .count();
    if ws == 0 {
        return false;
    }
    let scheme_all_upper = bytes[i..after_scheme].iter().all(u8::is_ascii_uppercase);
    let token: String = text[after_scheme + ws..]
        .chars()
        .take_while(|c| is_token_char(*c))
        .collect();
    token_looks_secret(&token, scheme_all_upper, is_bearer)
}

fn token_looks_secret(token: &str, scheme_all_upper: bool, is_bearer: bool) -> bool {
    let len = token.len();
    if len == 0 {
        return false;
    }
    if scheme_all_upper {
        return true;
    }
    let all_lower = token.bytes().all(|b| b.is_ascii_lowercase());
    if is_bearer {
        (len >= 4 && !all_lower) || len >= 16
    } else {
        len >= 6 && !all_lower
    }
}

/// JSON/YAML key-value fields holding secrets (quoted or unquoted keys with `:`).
fn has_yaml_or_json_secret_field(text: &str) -> bool {
    const SECRET_FIELDS: &[&str] = &[
        "apikey",
        "password",
        "passphrase",
        "cookie",
        "setcookie",
        "vaulttoken",
        "vaultcredential",
        "vaultsecret",
        "secret",
        "clientsecret",
        "token",
        "accesstoken",
        "refreshtoken",
        "sessiontoken",
        "sessioncookie",
        "sessionproof",
        "recoverycode",
        "recoverycodes",
        "privatekey",
    ];
    for line in text.lines() {
        let bytes = line.as_bytes();
        let mut pos = 0;
        while let Some(rel) = line[pos..].find(':') {
            let colon = pos + rel;
            pos = colon + 1;
            // Skip Rust module separators `::` and URL schemes `://`.
            if bytes.get(colon + 1) == Some(&b':')
                || (colon > 0 && bytes[colon - 1] == b':')
                || line[colon..].starts_with("://")
            {
                continue;
            }
            let head = line[..colon].trim_end_matches([' ', '\t']);
            let key_raw = extract_field_key(head);
            let Some(key_raw) = key_raw else {
                continue;
            };
            let norm: String = key_raw
                .bytes()
                .filter(u8::is_ascii_alphanumeric)
                .map(|b| char::from(b.to_ascii_lowercase()))
                .collect();
            if !SECRET_FIELDS.contains(&norm.as_str()) {
                continue;
            }
            let after = line[colon + 1..].trim_start_matches([' ', '\t']);
            if field_value_non_empty(after) {
                return true;
            }
        }
    }
    false
}

fn extract_field_key(head: &str) -> Option<&str> {
    if let Some(inner) = head.strip_suffix('"') {
        let open = inner.rfind('"')?;
        let key = &inner[open + 1..];
        return (!key.is_empty()).then_some(key);
    }
    if let Some(inner) = head.strip_suffix('\'') {
        let open = inner.rfind('\'')?;
        let key = &inner[open + 1..];
        return (!key.is_empty()).then_some(key);
    }
    // Unquoted YAML key at start of line (optionally after `- ` list prefix).
    let trimmed = head.trim_start_matches([' ', '\t']);
    let trimmed = trimmed
        .strip_prefix("- ")
        .map(|s| s.trim_start_matches([' ', '\t']))
        .unwrap_or(trimmed);
    if !trimmed.is_empty()
        && trimmed
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        Some(trimmed)
    } else {
        None
    }
}

fn field_value_non_empty(after: &str) -> bool {
    if after.is_empty() || after.starts_with(['{', '[']) {
        return false;
    }
    let val = if let Some(rest) = after.strip_prefix('"') {
        rest.split('"').next().unwrap_or(rest)
    } else if let Some(rest) = after.strip_prefix('\'') {
        rest.split('\'').next().unwrap_or(rest)
    } else {
        after
            .split([',', '}', ']', ';'])
            .next()
            .unwrap_or(after)
            .trim()
    };
    !val.is_empty() && !val.eq_ignore_ascii_case("null")
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
