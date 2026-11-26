//! The single redaction choke-point of `org-office-sessions` (`doc-8` TASK-178, `doc-7` #12,
//! `REQ-05`, `ENROLL-3`, `TASK-190`).
//!
//! Every external, subprocess, provider, error, transcript, wake and history string goes through
//! [`redact_text`] (or [`redact_json`] for structured values) before it is logged, persisted,
//! returned in an error or sent over the wire. This is a leaf module: it imports nothing from
//! the rest of the crate, so it can never take part in a module cycle.
//!
//! Covered secret shapes: the caller's active secrets, PEM private-key blocks (fail closed to
//! the end of the text when the `END` line is missing), provider key prefixes (`sk-ant-`,
//! `github_pat_`, `xoxb-`, AWS `AKIA`/`ASIA` key ids, ...), case-insensitive `Bearer`/`Basic`
//! credentials, JWTs, URL userinfo, sensitive header lines (`Authorization:`, `X-Api-Key:`,
//! `Cookie:`), quoted secret fields (`"token": "…"`, `'password': '…'`, `\"api_key\":\"…\"`) and
//! sensitive `KEY=value` / `?token=` assignments. A partial match never hides the rest of the
//! text: every pass keeps scanning after each redaction.

use serde_json::Value;

/// The replacement for every redacted secret value.
pub const REDACTED: &str = "[REDACTED]";
const REDACTED_PRIVATE_KEY: &str = "[REDACTED PRIVATE KEY]";

/// Active secrets shorter than this are not replaced verbatim (too many false positives); the
/// shape passes below still apply to them.
const MIN_ACTIVE_SECRET_LEN: usize = 6;

/// Provider key prefixes and the shortest tail that makes a match a key. Longer, more specific
/// prefixes come first; the catch-all `sk-` is last.
const SECRET_PREFIXES: &[(&str, usize)] = &[
    ("sk-ant-", 8),
    ("sk-proj-", 8),
    ("sk-svcacct-", 8),
    ("sk-admin-", 8),
    ("sk-live-", 8),
    ("sk-test-", 8),
    ("sk_live_", 8),
    ("sk_test_", 8),
    ("rk_live_", 8),
    ("github_pat_", 8),
    ("ghp_", 8),
    ("gho_", 8),
    ("ghs_", 8),
    ("ghu_", 8),
    ("ghr_", 8),
    ("glpat-", 8),
    ("xoxb-", 8),
    ("xoxp-", 8),
    ("xoxa-", 8),
    ("xoxr-", 8),
    ("xoxs-", 8),
    ("xapp-", 8),
    ("AIza", 8),
    ("ya29.", 8),
    ("sk-", 20),
];

/// AWS access key id prefixes: the prefix plus 16 upper-case letters or digits.
const AWS_KEY_ID_PREFIXES: &[&str] = &["AKIA", "ASIA"];
const AWS_KEY_ID_TAIL: usize = 16;

/// Field, header and variable names (lower-case, ASCII alphanumerics only) whose values are
/// always secret. Names ending in `token`, `secret`, `apikey`, `password`, `privatekey` or
/// `signature` are secret too (see [`is_sensitive_key`]).
const SENSITIVE_KEYS_NORMALIZED: &[&str] = &[
    "authorization",
    "proxyauthorization",
    "apikey",
    "accesstoken",
    "refreshtoken",
    "idtoken",
    "sessiontoken",
    "clientsecret",
    "password",
    "passwd",
    "pwd",
    "passphrase",
    "privatekey",
    "secret",
    "secretkey",
    "token",
    "credential",
    "credentials",
    "sig",
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

/// Header names (normalized as above) whose whole value, to the end of the line, is secret even
/// when the name is not quoted (`Authorization: token …`, `x-api-key: …`, `Cookie: …`).
const SENSITIVE_HEADERS_NORMALIZED: &[&str] = &[
    "authorization",
    "proxyauthorization",
    "cookie",
    "setcookie",
    "xapikey",
    "apikey",
    "xauthtoken",
    "authtoken",
    "privatetoken",
    "xgoogapikey",
    "xamzsecuritytoken",
    "xvaulttoken",
    "xaccesstoken",
];

/// Redacts every secret shape this crate knows from `input` (see the module docs).
pub fn redact_text(input: &str, active_secrets: &[&str]) -> String {
    let mut out = redact_active_secrets(input, active_secrets);
    out = redact_pem_blocks(&out);
    for &(prefix, min_tail) in SECRET_PREFIXES {
        out = redact_prefixed(&out, prefix, min_tail);
    }
    for prefix in AWS_KEY_ID_PREFIXES {
        out = redact_aws_key_ids(&out, prefix);
    }
    out = redact_auth_scheme(&out, "bearer");
    out = redact_auth_scheme(&out, "basic");
    out = redact_jwts(&out);
    out = redact_url_userinfo(&out);
    out = redact_header_values(&out);
    out = redact_quoted_key_values(&out);
    redact_env_assignments(&out)
}

/// Recursively redacts a JSON value: every value under a sensitive key becomes [`REDACTED`],
/// and every other string (object keys included) goes through [`redact_text`].
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
                let redacted = if is_sensitive_key(k) {
                    Value::String(REDACTED.to_owned())
                } else {
                    redact_json(v, active_secrets)
                };
                out.insert(redact_text(k, active_secrets), redacted);
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

/// `true` when a field, header or variable name holds a secret (case, `-`, `_` and other
/// punctuation are ignored: `X-Api-Key`, `api_key` and `apiKey` all match).
pub fn is_sensitive_key(key: &str) -> bool {
    let norm = normalize_key(key);
    if SENSITIVE_KEYS_NORMALIZED.contains(&norm.as_str()) {
        return true;
    }
    [
        "token",
        "secret",
        "apikey",
        "password",
        "privatekey",
        "signature",
    ]
    .iter()
    .any(|suffix| norm.ends_with(suffix))
}

fn normalize_key(key: &str) -> String {
    key.bytes()
        .filter(u8::is_ascii_alphanumeric)
        .map(|b| char::from(b.to_ascii_lowercase()))
        .collect()
}

fn redact_active_secrets(input: &str, active_secrets: &[&str]) -> String {
    let mut secrets: Vec<&str> = active_secrets
        .iter()
        .copied()
        .filter(|s| s.len() >= MIN_ACTIVE_SECRET_LEN)
        .collect();
    // A longer secret that contains a shorter one is replaced first, never left half-visible.
    secrets.sort_unstable_by_key(|s| std::cmp::Reverse(s.len()));
    let mut out = input.to_owned();
    for secret in secrets {
        if out.contains(secret) {
            out = out.replace(secret, REDACTED);
        }
    }
    out
}

const PEM_BEGIN: &str = "-----BEGIN ";
const PEM_END: &str = "-----END ";
const PEM_DASHES: &str = "-----";
const PEM_MAX_LABEL: usize = 64;

fn redact_pem_blocks(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find(PEM_BEGIN) {
        let after_begin = &rest[start + PEM_BEGIN.len()..];
        let label = match after_begin.find(PEM_DASHES) {
            Some(end) if end <= PEM_MAX_LABEL => &after_begin[..end],
            // A truncated header still names its key type within the first bytes.
            _ => &after_begin[..after_begin.floor_char_boundary(PEM_MAX_LABEL)],
        };
        if !label.to_ascii_uppercase().contains("PRIVATE KEY") {
            out.push_str(&rest[..start + PEM_BEGIN.len()]);
            rest = after_begin;
            continue;
        }
        out.push_str(&rest[..start]);
        out.push_str(REDACTED_PRIVATE_KEY);
        rest = skip_past_pem_end(after_begin);
    }
    out.push_str(rest);
    out
}

/// The text after the block's `-----END … -----` line, or nothing when the block never ends
/// (fail closed: the rest of the text may be key material).
fn skip_past_pem_end(after_begin: &str) -> &str {
    let Some(end_idx) = after_begin.find(PEM_END) else {
        return "";
    };
    let tail = &after_begin[end_idx + PEM_END.len()..];
    tail.find(PEM_DASHES)
        .map_or("", |off| &tail[off + PEM_DASHES.len()..])
}

fn is_token_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')
}

/// `true` when the byte before `idx` cannot be part of the same word (so `task-…` never
/// matches the `sk-` prefix).
fn at_word_start(input: &str, idx: usize) -> bool {
    idx == 0 || !input.as_bytes()[idx - 1].is_ascii_alphanumeric()
}

fn redact_prefixed(input: &str, prefix: &str, min_tail: usize) -> String {
    redact_prefixed_where(input, prefix, |after| {
        let tail = after.bytes().take_while(|&b| is_token_byte(b)).count();
        (tail >= min_tail).then_some(tail)
    })
}

fn redact_aws_key_ids(input: &str, prefix: &str) -> String {
    redact_prefixed_where(input, prefix, |after| {
        let tail = after
            .bytes()
            .take_while(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
            .count();
        (tail >= AWS_KEY_ID_TAIL).then_some(tail)
    })
}

/// Replaces `prefix` plus the tail `tail_len` accepts (its byte length) with [`REDACTED`]
/// wherever the prefix starts a word.
fn redact_prefixed_where(
    input: &str,
    prefix: &str,
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

/// `true` when a word after `Bearer`/`Basic` looks like a credential rather than prose: at
/// least 6 bytes and not only lower-case letters (`basic understanding` stays readable).
fn looks_like_credential(token: &str) -> bool {
    token.len() >= 6 && !token.bytes().all(|b| b.is_ascii_lowercase())
}

/// Redacts the credential after an `Authorization` scheme (`Bearer`, `Basic`), matched without
/// regard to case and followed by one or more spaces or tabs.
fn redact_auth_scheme(input: &str, scheme_lower: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut pos = 0;
    while let Some(rel) = find_ascii_case_insensitive(&input[pos..], scheme_lower) {
        let idx = pos + rel;
        let after_scheme = idx + scheme_lower.len();
        let gap = input[after_scheme..].len()
            - input[after_scheme..].trim_start_matches([' ', '\t']).len();
        if gap == 0 || !at_word_start(input, idx) {
            out.push_str(&input[pos..after_scheme]);
            pos = after_scheme;
            continue;
        }
        let token_start = after_scheme + gap;
        let token_len = input[token_start..]
            .bytes()
            .take_while(|b| !b.is_ascii_whitespace() && !matches!(b, b'"' | b'\'' | b',' | b'}'))
            .count();
        let token = &input[token_start..token_start + token_len];
        out.push_str(&input[pos..token_start]);
        if token != REDACTED && looks_like_credential(token) {
            out.push_str(REDACTED);
        } else {
            out.push_str(token);
        }
        pos = token_start + token_len;
    }
    out.push_str(&input[pos..]);
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

/// Redacts `eyJ…` JSON Web Tokens: a header and payload of 10+ base64url bytes each, with an
/// optional signature segment.
fn redact_jwts(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(idx) = rest.find("eyJ") {
        out.push_str(&rest[..idx]);
        let cand = &rest[idx..];
        let len = cand.bytes().take_while(|&b| is_token_byte(b)).count();
        let parts: Vec<&str> = cand[..len].split('.').collect();
        if parts.len() >= 2 && parts[0].len() >= 10 && parts[1].len() >= 10 {
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

/// Minimum length of a userinfo without `:` that is treated as a token (`https://<token>@host`);
/// shorter names such as `git@` stay readable.
const MIN_BARE_USERINFO_TOKEN: usize = 16;

fn redact_url_userinfo(input: &str) -> String {
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
        match authority.rfind('@') {
            Some(at)
                if authority[..at].contains(':')
                    || authority[..at].len() >= MIN_BARE_USERINFO_TOKEN =>
            {
                out.push_str(REDACTED);
                out.push_str(&authority[at..]);
            }
            _ => out.push_str(authority),
        }
        rest = &after[authority_end..];
    }
    out.push_str(rest);
    out
}

/// Redacts the value of an unquoted sensitive header (`Authorization: token …`) up to the end
/// of its line or of the JSON string holding it.
fn redact_header_values(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b':' || bytes.get(i + 1) == Some(&b':') {
            i += 1;
            continue;
        }
        let name_start = input[..i]
            .bytes()
            .rev()
            .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
            .count();
        let name = &input[i - name_start..i];
        let starts_word =
            i - name_start == 0 || !matches!(bytes[i - name_start - 1], b':' | b'.' | b'/' | b'\\');
        if name.is_empty()
            || !starts_word
            || !SENSITIVE_HEADERS_NORMALIZED.contains(&normalize_key(name).as_str())
        {
            i += 1;
            continue;
        }
        let value_start =
            i + 1 + (input[i + 1..].len() - input[i + 1..].trim_start_matches([' ', '\t']).len());
        let value_end = header_value_end(bytes, value_start);
        if value_end > value_start {
            out.push_str(&input[copied..value_start]);
            out.push_str(REDACTED);
            copied = value_end;
        }
        i = value_end.max(i + 1);
    }
    out.push_str(&input[copied..]);
    out
}

/// The end of a sensitive header value: the end of its line, an escaped line break, or the quote
/// closing the (possibly JSON-escaped) string that holds the header. A quote's escape level is
/// the trailing ones of its backslash run (`"` and `\\"` are 0, `\"` is 1, `\\\"` is 2). A quote
/// opening a parameter (after `=`, or at the value start) pairs with the next quote of its level,
/// and a deeper quote inside that pair is a literal, so `Digest response="…", nc=…` and
/// `Cookie: a="1", sid=…` are redacted whole while `{"msg":"Authorization: …","ok":true}` keeps
/// its JSON structure.
fn header_value_end(bytes: &[u8], start: usize) -> usize {
    let mut open: Option<u32> = None;
    let mut i = start;
    while i < bytes.len() {
        let run = bytes[i..].iter().take_while(|&&b| b == b'\\').count();
        let at = i + run;
        match bytes.get(at) {
            Some(b'\n' | b'\r') => return at,
            // An escaped line break outside any quoted parameter: the header sits in a JSON string.
            Some(b'n' | b'r') if run % 2 == 1 && open.is_none() => return at - 1,
            Some(b'"') => {}
            _ => {
                i = at + 1;
                continue;
            }
        }
        let level = run.trailing_ones();
        match open {
            Some(cur) if level == cur => open = None,
            // A deeper (more escaped) quote is a literal inside the open parameter.
            Some(cur) if level > cur => {}
            // A shallower quote closes the string that holds the header.
            Some(_) => return i,
            None if opens_header_param(bytes, start, i) && !closes_json_string(bytes, at) => {
                open = Some(level);
            }
            // An unmatched escaped quote, or a raw one ending a JSON string, ends the value.
            None if level > 0 || closes_json_string(bytes, at) => return i,
            None => {}
        }
        i = at + 1;
    }
    bytes.len()
}

/// `true` when a quote starting at `idx` opens a header parameter value: it starts the header
/// value or follows `=` (with optional blanks).
fn opens_header_param(bytes: &[u8], value_start: usize, idx: usize) -> bool {
    let before = bytes[value_start..idx]
        .iter()
        .rev()
        .find(|b| !matches!(b, b' ' | b'\t'));
    matches!(before, None | Some(b'='))
}

/// `true` when the `"` at `idx` ends a JSON string: what follows is the end, `,`, `}` or `]`.
fn closes_json_string(bytes: &[u8], idx: usize) -> bool {
    let next = bytes[idx + 1..]
        .iter()
        .find(|b| !matches!(b, b' ' | b'\t'))
        .copied();
    matches!(next, None | Some(b',' | b'}' | b']'))
}

/// Longest quoted key considered before a `:` or `=`.
const MAX_QUOTED_KEY: usize = 64;

/// Redacts the value of every quoted sensitive key followed by `:` or `=` (`"token": "…"`,
/// `'password'='…'`, `\"api_key\":\"…\"`, `"secret": 42`). An unterminated quoted value is
/// redacted to the end of its line.
fn redact_quoted_key_values(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        let is_separator =
            matches!(bytes[i], b':' | b'=') && !matches!(bytes.get(i + 1), Some(b':' | b'='));
        let span = is_separator
            .then(|| quoted_key_before(input, i))
            .flatten()
            .filter(|key| is_sensitive_key(key))
            .and_then(|_| value_span_after(input, i + 1));
        match span {
            Some((start, end)) if start >= copied => {
                out.push_str(&input[copied..start]);
                out.push_str(REDACTED);
                copied = end;
                i = end.max(i + 1);
            }
            _ => i += 1,
        }
    }
    out.push_str(&input[copied..]);
    out
}

fn quoted_key_before(input: &str, sep: usize) -> Option<&str> {
    let head = input[..sep].trim_end_matches([' ', '\t']);
    let (head, quote) = if let Some(h) = head.strip_suffix("\\\"") {
        (h, "\\\"")
    } else if let Some(h) = head.strip_suffix('"') {
        (h, "\"")
    } else {
        (head.strip_suffix('\'')?, "'")
    };
    let window = head.floor_char_boundary(head.len().saturating_sub(MAX_QUOTED_KEY + 2));
    let open = window + head[window..].rfind(quote)?;
    let key = &head[open + quote.len()..];
    (!key.is_empty() && key.len() <= MAX_QUOTED_KEY && !key.contains(['"', '\'', '\n']))
        .then_some(key)
}

/// The byte span of the value after a key separator, without its quotes.
fn value_span_after(input: &str, from: usize) -> Option<(usize, usize)> {
    let rest = &input[from..];
    let start = from + (rest.len() - rest.trim_start_matches([' ', '\t']).len());
    let rest = &input[start..];
    if rest.starts_with("\\\"") {
        let body = start + 2;
        let end =
            escaped_closing_quote(input.as_bytes(), body).unwrap_or_else(|| line_end(input, body));
        return Some((body, end));
    }
    if let Some(q) = rest.bytes().next().filter(|b| matches!(b, b'"' | b'\'')) {
        let body = start + 1;
        let end = closing_quote(input.as_bytes(), body, q).unwrap_or_else(|| line_end(input, body));
        return Some((body, end));
    }
    if rest.starts_with(['{', '[']) {
        return None;
    }
    let len = rest
        .bytes()
        .take_while(|b| !b.is_ascii_whitespace() && !matches!(b, b',' | b'}' | b']' | b';'))
        .count();
    (len > 0).then_some((start, start + len))
}

fn closing_quote(bytes: &[u8], from: usize, quote: u8) -> Option<usize> {
    let mut idx = from;
    while idx < bytes.len() {
        if quote == b'"' && bytes[idx] == b'\\' {
            idx += 2;
            continue;
        }
        if bytes[idx] == quote {
            return Some(idx);
        }
        idx += 1;
    }
    None
}

/// Where a JSON-escaped string value (`\"…\"`) that starts at `from` ends: at the level-1 `\"`
/// that closes it, or at a raw (level-0) `"` that closes the enclosing JSON string. A deeper
/// quote (`\\\"`) is an escaped quote inside the value (levels as in [`header_value_end`]).
fn escaped_closing_quote(bytes: &[u8], from: usize) -> Option<usize> {
    let mut idx = from;
    while idx < bytes.len() {
        let run = bytes[idx..].iter().take_while(|&&b| b == b'\\').count();
        match (bytes.get(idx + run), run.trailing_ones()) {
            (Some(b'"'), 1) => return Some(idx + run - 1),
            (Some(b'"'), 0) => return Some(idx + run),
            (Some(_), _) => idx += run + 1,
            (None, _) => return None,
        }
    }
    None
}

fn line_end(input: &str, from: usize) -> usize {
    input[from..]
        .find(['\n', '\r'])
        .map_or(input.len(), |off| from + off)
}

/// Redacts sensitive `KEY=value` assignments (shell, `.env`, URL query parameters), line by
/// line, with bare, quoted and JSON-escaped (`\"…\"`) values.
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
        let key_start = line[..eq]
            .bytes()
            .rev()
            .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            .count();
        let key = &line[eq - key_start..eq];
        out.push_str(&line[pos..=eq]);
        pos = eq + 1;
        if key.is_empty() || line[pos..].starts_with('=') || !is_sensitive_key(key) {
            continue;
        }
        let (replacement, consumed) = redact_env_value(&line[pos..]);
        out.push_str(&replacement);
        pos += consumed;
    }
    out.push_str(&line[pos..]);
    out
}

fn redact_env_value(after: &str) -> (String, usize) {
    if after.starts_with("\\\"") {
        // The closing `\"` (or a raw `"` ending the JSON string) is copied by the caller.
        let end = escaped_closing_quote(after.as_bytes(), 2).unwrap_or(after.len());
        return (format!("\\\"{REDACTED}"), end);
    }
    if let Some(q) = after.bytes().next().filter(|b| matches!(b, b'"' | b'\'')) {
        // A double-quoted value honours `\"` escapes; an unclosed one fails closed to the end
        // of the line.
        let close = if q == b'"' {
            closing_quote(after.as_bytes(), 1, q)
        } else {
            after[1..].find('\'').map(|len| 1 + len)
        };
        let q = char::from(q);
        return match close {
            Some(end) => (format!("{q}{REDACTED}{q}"), end + 1),
            None => (format!("{q}{REDACTED}"), after.len()),
        };
    }
    let bytes = after.as_bytes();
    let mut len = 0;
    while len < bytes.len() {
        let b = bytes[len];
        let json_escape = b == b'\\'
            && matches!(
                bytes.get(len + 1),
                Some(b'"' | b'n' | b'r' | b't' | b'\\' | b'/' | b'u')
            );
        if b.is_ascii_whitespace() || matches!(b, b';' | b',' | b'"' | b'\'') || json_escape {
            break;
        }
        len += 1;
    }
    if len == 0 {
        return (String::new(), 0);
    }
    (REDACTED.to_owned(), len)
}

#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
mod tests;
