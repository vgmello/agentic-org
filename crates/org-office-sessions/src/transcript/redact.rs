//! Secret and credential redaction for conversation transcripts (`REQ-05`, `ENROLL-3`).
//!
//! Runs on the office before any transcript text, tool input, tool output, diff preview,
//! file path, metadata field, or session preview leaves the office process.

use serde_json::Value;

use org_proto::sessions::{
    SessionSummaryView, SessionTranscriptResult, SubagentTranscriptView, TranscriptEntry,
};

const REDACTED: &str = "[REDACTED]";

const SECRET_PREFIXES: &[&str] = &[
    "sk-ant-oat",
    "sk-ant-ort",
    "sk-ant-api",
    "sk-ant-",
    "sk-proj-",
    "sk-svcacct-",
    "sk-live-",
    "sk-test-",
    "gho_",
    "ghp_",
    "ghs_",
    "ghu_",
    "github_pat_",
    "AIza",
    "ya29.",
    "AKIA",
    "ASIA",
    "xoxb-",
    "xoxp-",
    "xoxa-",
    "xoxr-",
];

const SENSITIVE_KEYS_NORMALIZED: &[&str] = &[
    "authorization",
    "apikey",
    "accesstoken",
    "refreshtoken",
    "idtoken",
    "sessiontoken",
    "clientsecret",
    "password",
    "passwd",
    "privatekey",
    "secret",
    "secretkey",
    "token",
    "agentossessiontoken",
    "anthropicapikey",
    "anthropicauthtoken",
    "openaiapikey",
    "geminiapikey",
    "googleapikey",
    "claudecodeoauthtoken",
    "claudecodeoauthrefreshtoken",
    "claudecodesessionsoauthtoken",
    "codexapikey",
    "xapikey",
    "xauthtoken",
    "cookie",
    "setcookie",
];

/// Deep-redacts any active runtime/provider secrets, known provider key prefixes,
/// case-insensitive Bearer/Basic headers, JWTs, PEM/PGP private keys, embedded URL
/// credentials, JSON secret fields, and quoted/unquoted sensitive env assignments.
pub fn redact_text(input: &str, active_secrets: &[&str]) -> String {
    let mut out = crate::credentials::redact_secrets(input, active_secrets);
    out = redact_pem_blocks(&out);
    for prefix in SECRET_PREFIXES {
        out = redact_prefixed(&out, prefix, 8);
    }
    out = redact_prefixed(&out, "sk-", 20);
    out = redact_auth_scheme(&out, "bearer ");
    out = redact_auth_scheme(&out, "basic ");
    out = redact_jwts(&out);
    out = redact_url_credentials(&out);
    out = redact_json_key_values(&out);
    out = redact_env_assignments(&out);
    out
}

/// Recursively redacts every string and sensitive-key value inside a JSON value.
pub fn redact_json(value: &Value, active_secrets: &[&str]) -> Value {
    match value {
        Value::String(s) => Value::String(redact_text(s, active_secrets)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|v| redact_json(v, active_secrets))
                .collect(),
        ),
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                if is_sensitive_key(k) {
                    out.insert(k.clone(), Value::String(REDACTED.to_owned()));
                } else {
                    out.insert(k.clone(), redact_json(v, active_secrets));
                }
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

/// Redacts all user-visible fields of a [`SessionSummaryView`].
pub fn redact_summary(summary: &mut SessionSummaryView, active_secrets: &[&str]) {
    if let Some(ref preview) = summary.preview {
        summary.preview = Some(redact_text(preview, active_secrets));
    }
    if let Some(ref started) = summary.started_at {
        summary.started_at = Some(redact_text(started, active_secrets));
    }
}

/// Redacts an entire [`SessionTranscriptResult`] including nested sub-agents and file summaries.
pub fn redact_transcript(result: &mut SessionTranscriptResult, active_secrets: &[&str]) {
    redact_opt_str(&mut result.usage.model, active_secrets);
    for file in &mut result.files {
        file.path = redact_text(&file.path, active_secrets);
    }
    for entry in &mut result.entries {
        redact_entry(entry, active_secrets);
    }
}

fn redact_entry(entry: &mut TranscriptEntry, active_secrets: &[&str]) {
    redact_opt_str(&mut entry.timestamp, active_secrets);
    redact_opt_str(&mut entry.role, active_secrets);
    redact_opt_str(&mut entry.text, active_secrets);
    redact_opt_str(&mut entry.tool_name, active_secrets);
    redact_opt_str(&mut entry.tool_use_id, active_secrets);
    redact_opt_str(&mut entry.tool_output, active_secrets);
    redact_opt_str(&mut entry.file_path, active_secrets);
    redact_opt_str(&mut entry.diff_preview, active_secrets);
    redact_opt_str(&mut entry.raw_type, active_secrets);
    if let Some(ref mut usage) = entry.usage {
        redact_opt_str(&mut usage.model, active_secrets);
    }
    if let Some(ref input) = entry.tool_input {
        entry.tool_input = Some(redact_json(input, active_secrets));
    }
    if let Some(ref mut sub) = entry.subagent {
        redact_subagent(sub, active_secrets);
    }
}

fn redact_opt_str(slot: &mut Option<String>, active_secrets: &[&str]) {
    if let Some(ref val) = *slot {
        *slot = Some(redact_text(val, active_secrets));
    }
}

fn redact_subagent(sub: &mut SubagentTranscriptView, active_secrets: &[&str]) {
    redact_opt_str(&mut sub.label, active_secrets);
    redact_opt_str(&mut sub.parent_tool_use_id, active_secrets);
    redact_opt_str(&mut sub.usage.model, active_secrets);
    for file in &mut sub.files {
        file.path = redact_text(&file.path, active_secrets);
    }
    for entry in &mut sub.entries {
        redact_entry(entry, active_secrets);
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let norm: String = key
        .bytes()
        .filter(|b| b.is_ascii_alphanumeric())
        .map(|b| (b as char).to_ascii_lowercase())
        .collect();
    if SENSITIVE_KEYS_NORMALIZED.contains(&norm.as_str()) {
        return true;
    }
    norm.ends_with("token")
        || norm.ends_with("secret")
        || norm.ends_with("apikey")
        || norm.ends_with("password")
        || norm.ends_with("privatekey")
}

fn redact_pem_blocks(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("-----BEGIN ") {
        let after_begin = &rest[start..];
        let header_end = after_begin
            .find('\n')
            .unwrap_or_else(|| after_begin.floor_char_boundary(80));
        let header = &after_begin[..header_end];
        if !header.contains("PRIVATE KEY") {
            result.push_str(&rest[..start + 11]);
            rest = &rest[start + 11..];
            continue;
        }
        result.push_str(&rest[..start]);
        result.push_str("[REDACTED PRIVATE KEY]");
        rest = skip_pem_footer(after_begin);
    }
    result.push_str(rest);
    result
}

fn skip_pem_footer(after_begin: &str) -> &str {
    let Some(end_idx) = after_begin.find("-----END ") else {
        return "";
    };
    let tail = &after_begin[end_idx..];
    tail[9..]
        .find("-----")
        .map_or("", |off| &tail[9 + off + 5..])
}

fn redact_prefixed(input: &str, prefix: &str, min_tail_len: usize) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(idx) = rest.find(prefix) {
        out.push_str(&rest[..idx]);
        let after = &rest[idx + prefix.len()..];
        let token_len = after
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            .count();
        if token_len >= min_tail_len {
            out.push_str(REDACTED);
            rest = &after[token_len..];
        } else {
            out.push_str(prefix);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

fn redact_auth_scheme(input: &str, scheme_lower: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    let scheme_len = scheme_lower.len();
    while let Some(idx) = find_ascii_case_insensitive(rest, scheme_lower) {
        out.push_str(&rest[..idx + scheme_len]);
        let after = &rest[idx + scheme_len..];
        if let Some(stripped) = after.strip_prefix(REDACTED) {
            out.push_str(REDACTED);
            rest = stripped;
            continue;
        }
        let token_len = after
            .bytes()
            .take_while(|b| !b.is_ascii_whitespace() && !matches!(b, b'"' | b'\'' | b',' | b'}'))
            .count();
        if token_len >= 6 {
            out.push_str(REDACTED);
            rest = &after[token_len..];
        } else {
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

fn find_ascii_case_insensitive(haystack: &str, needle_lower: &str) -> Option<usize> {
    let h = haystack.as_bytes();
    let n = needle_lower.as_bytes();
    if n.is_empty() || n.len() > h.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| h[i..i + n.len()].eq_ignore_ascii_case(n))
}

fn redact_jwts(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(idx) = rest.find("eyJ") {
        out.push_str(&rest[..idx]);
        let cand = &rest[idx..];
        let len = cand
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            .count();
        let token = &cand[..len];
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() == 3 && parts[0].len() >= 10 && parts[1].len() >= 10 && parts[2].len() >= 8 {
            out.push_str(REDACTED);
            rest = &cand[len..];
        } else {
            out.push_str("eyJ");
            rest = &cand[3..];
        }
    }
    out.push_str(rest);
    out
}

fn redact_url_credentials(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(idx) = rest.find("://") {
        out.push_str(&rest[..idx + 3]);
        let after = &rest[idx + 3..];
        let authority_end = after
            .bytes()
            .position(|b| b.is_ascii_whitespace() || matches!(b, b'/' | b'?' | b'#' | b'"' | b'\''))
            .unwrap_or(after.len());
        let authority = &after[..authority_end];
        if let Some(at) = authority.rfind('@') {
            if authority[..at].contains(':') {
                out.push_str("[REDACTED]@");
                out.push_str(&authority[at + 1..]);
                rest = &after[authority_end..];
                continue;
            }
        }
        out.push_str(authority);
        rest = &after[authority_end..];
    }
    out.push_str(rest);
    out
}

/// Redacts `"<sensitive_key>": "<value>"` JSON patterns inside free-text (`Raw` / `tool_output`).
fn redact_json_key_values(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut pos = 0;
    while pos < bytes.len() {
        let Some(rel_q) = input[pos..].find('"') else {
            out.push_str(&input[pos..]);
            break;
        };
        let q_start = pos + rel_q;
        let Some(rel_end) = input[q_start + 1..].find('"') else {
            out.push_str(&input[pos..]);
            break;
        };
        let q_end = q_start + 1 + rel_end;
        let key = &input[q_start + 1..q_end];
        if key.len() <= 64 && is_sensitive_key(key) {
            if let Some((prefix_end, val_end)) = match_json_string_value(input, q_end + 1) {
                out.push_str(&input[pos..prefix_end]);
                out.push_str(REDACTED);
                out.push('"');
                pos = val_end + 1;
                continue;
            }
        }
        out.push_str(&input[pos..q_end]);
        pos = q_end;
    }
    out
}

fn match_json_string_value(input: &str, after_key_quote: usize) -> Option<(usize, usize)> {
    let rest = &input[after_key_quote..];
    let ws1 = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    let after_colon = rest[ws1..].strip_prefix(':')?;
    let ws2 = after_colon.len() - after_colon.trim_start_matches([' ', '\t']).len();
    let val_quote_idx = after_key_quote + ws1 + 1 + ws2;
    if !input[val_quote_idx..].starts_with('"') {
        return None;
    }
    let val_start = val_quote_idx + 1;
    let mut idx = val_start;
    let b = input.as_bytes();
    while idx < b.len() {
        if b[idx] == b'\\' && idx + 1 < b.len() {
            idx += 2;
            continue;
        }
        if b[idx] == b'"' {
            return Some((val_start, idx));
        }
        idx += 1;
    }
    None
}

fn redact_env_assignments(input: &str) -> String {
    input
        .split('\n')
        .map(redact_env_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn redact_env_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut pos = 0;
    while let Some(rel_eq) = line[pos..].find('=') {
        let eq = pos + rel_eq;
        let key_start = find_env_key_start(line, eq);
        let key = &line[key_start..eq];
        if !key.is_empty() && is_sensitive_key(key) {
            out.push_str(&line[pos..=eq]);
            let after = &line[eq + 1..];
            let (replaced, consumed) = redact_env_value(after);
            out.push_str(&replaced);
            pos = eq + 1 + consumed;
        } else {
            out.push_str(&line[pos..=eq]);
            pos = eq + 1;
        }
    }
    out.push_str(&line[pos..]);
    out
}

fn find_env_key_start(line: &str, eq_idx: usize) -> usize {
    let bytes = line.as_bytes();
    let mut idx = eq_idx;
    while idx > 0 {
        let b = bytes[idx - 1];
        if b.is_ascii_alphanumeric() || b == b'_' || b == b'-' {
            idx -= 1;
        } else {
            break;
        }
    }
    idx
}

fn redact_env_value(after: &str) -> (String, usize) {
    let first = after.as_bytes().first().copied();
    if matches!(first, Some(b'"' | b'\'')) {
        let q = first.unwrap() as char;
        let inner = &after[1..];
        let close = inner.find(q);
        return match close {
            Some(len) => (format!("{q}{REDACTED}{q}"), 1 + len + 1),
            None => (format!("{q}{REDACTED}"), after.len()),
        };
    }
    let val_len = after
        .bytes()
        .take_while(|b| !b.is_ascii_whitespace() && !matches!(b, b';' | b',' | b'"' | b'\''))
        .count();
    if val_len > 0 {
        (REDACTED.to_owned(), val_len)
    } else {
        (String::new(), 0)
    }
}
