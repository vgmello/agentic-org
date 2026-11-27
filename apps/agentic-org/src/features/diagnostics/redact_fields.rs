//! Field-level secret redaction passes for `features::diagnostics::redact`:
//! HTTP headers, quoted JSON/log key-value fields, unquoted `key: value` secret fields,
//! `KEY=value` / query parameter assignments, and recovery codes.

/// Replacement for every redacted secret value.
pub(crate) const REDACTED: &str = "[REDACTED]";

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

const SENSITIVE_KEYS: &[&str] = &[
    "authorization",
    "proxyauthorization",
    "apikey",
    "accesstoken",
    "refreshtoken",
    "idtoken",
    "sessiontoken",
    "invitetoken",
    "invitationtoken",
    "registrationtoken",
    "resettoken",
    "clientsecret",
    "password",
    "passwd",
    "pwd",
    "passphrase",
    "privatekey",
    "secret",
    "secretkey",
    "secretaccesskey",
    "awssecretaccesskey",
    "accesskey",
    "secrethex",
    "secretb64",
    "secretbase64",
    "token",
    "credential",
    "credentials",
    "sig",
    "signature",
    "recoverycode",
    "recoverycodes",
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
    "masterkey",
    "signingkey",
    "encryptionkey",
    "sessionkey",
    "authkey",
    "keyhex",
    "keyb64",
    "keybase64",
    "keymaterial",
    "hmac",
    "invite",
];

const SENSITIVE_HEADERS: &[&str] = &[
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

const MAX_QUOTED_KEY: usize = 64;

/// Returns true when a key, header, or variable name identifies a secret value.
pub(super) fn is_sensitive_key(key: &str) -> bool {
    let norm = normalize_key(key);
    if SENSITIVE_KEYS.contains(&norm.as_str()) {
        return true;
    }
    [
        "token",
        "secret",
        "secretkey",
        "accesskey",
        "apikey",
        "password",
        "passphrase",
        "privatekey",
        "signature",
        "recoverycode",
        "credential",
        "credentials",
        "keyhex",
        "keyb64",
        "keybase64",
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

pub(super) fn redact_header_values(input: &str, total: &mut usize) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b':' || bytes.get(i + 1) == Some(&b':') {
            i += 1;
            continue;
        }
        let Some((value_start, value_end)) = header_value_span(input, i) else {
            i += 1;
            continue;
        };
        if &input[value_start..value_end] != REDACTED {
            out.push_str(&input[copied..value_start]);
            out.push_str(REDACTED);
            *total += 1;
            copied = value_end;
        }
        i = value_end.max(i + 1);
    }
    out.push_str(&input[copied..]);
    out
}

fn header_value_span(input: &str, colon: usize) -> Option<(usize, usize)> {
    let (name, starts_word) = unquoted_name_before_colon(input, colon);
    if name.is_empty() || !starts_word || !SENSITIVE_HEADERS.contains(&normalize_key(name).as_str())
    {
        return None;
    }
    let rest = &input[colon + 1..];
    let value_start = colon + 1 + (rest.len() - rest.trim_start_matches([' ', '\t']).len());
    let value_end = header_value_end(input.as_bytes(), value_start);
    (value_end > value_start).then_some((value_start, value_end))
}

fn unquoted_name_before_colon(input: &str, colon: usize) -> (&str, bool) {
    let bytes = input.as_bytes();
    let mut name_start = colon
        - input[..colon]
            .bytes()
            .rev()
            .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
            .count();
    let mut starts_word =
        name_start == 0 || !matches!(bytes[name_start - 1], b':' | b'.' | b'/' | b'\\');
    if name_start < colon && is_json_space_escape(bytes, name_start) {
        name_start += 1;
        starts_word = true;
    }
    (&input[name_start..colon], starts_word)
}

fn header_value_end(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    while i < bytes.len() {
        let run = bytes[i..].iter().take_while(|&&b| b == b'\\').count();
        let at = i + run;
        match bytes.get(at) {
            Some(b'\n' | b'\r') => return at,
            Some(b'n' | b'r') if run % 2 == 1 => return at - 1,
            Some(b'"') if run % 2 == 1 || closes_json_string(bytes, at) => return i,
            _ => i = at + 1,
        }
    }
    bytes.len()
}

fn closes_json_string(bytes: &[u8], idx: usize) -> bool {
    let next = bytes[idx + 1..]
        .iter()
        .find(|b| !matches!(b, b' ' | b'\t'))
        .copied();
    matches!(next, None | Some(b',' | b'}' | b']'))
}

pub(super) fn redact_quoted_key_values(input: &str, total: &mut usize) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        let is_sep =
            matches!(bytes[i], b':' | b'=') && !matches!(bytes.get(i + 1), Some(b':' | b'='));
        let span = is_sep
            .then(|| quoted_key_before(input, i))
            .flatten()
            .filter(|key| is_sensitive_key(key))
            .and_then(|_| value_span_after(input, i + 1));
        match span {
            Some((start, end)) if start >= copied && &input[start..end] != REDACTED => {
                out.push_str(&input[copied..start]);
                out.push_str(REDACTED);
                *total += 1;
                copied = end;
                i = end.max(i + 1);
            }
            _ => i += 1,
        }
    }
    out.push_str(&input[copied..]);
    out
}

/// Redacts unquoted `sensitive_key: value` log/YAML fields (e.g. `secret_hex: deadbeef...`).
pub(super) fn redact_unquoted_key_values(input: &str, total: &mut usize) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b':' || !matches!(bytes.get(i + 1), Some(b' ' | b'\t')) {
            i += 1;
            continue;
        }
        let (name, starts_word) = unquoted_name_before_colon(input, i);
        if name.is_empty() || !starts_word || !is_sensitive_key(name) {
            i += 1;
            continue;
        }
        let Some((start, end)) = value_span_after(input, i + 1) else {
            i += 1;
            continue;
        };
        let val = &input[start..end];
        if start >= copied && is_redactable_unquoted_value(val) {
            out.push_str(&input[copied..start]);
            out.push_str(REDACTED);
            *total += 1;
            copied = end;
            i = end.max(i + 1);
        } else {
            i += 1;
        }
    }
    out.push_str(&input[copied..]);
    out
}

fn is_redactable_unquoted_value(val: &str) -> bool {
    val.len() >= 4
        && val != REDACTED
        && !matches!(
            val.to_ascii_lowercase().as_str(),
            "null" | "none" | "true" | "false" | "empty" | "missing"
        )
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

fn escaped_closing_quote(bytes: &[u8], from: usize) -> Option<usize> {
    let mut idx = from;
    while idx < bytes.len() {
        let run = bytes[idx..].iter().take_while(|&&b| b == b'\\').count();
        match (bytes.get(idx + run), run % 2) {
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

pub(super) fn redact_env_assignments(input: &str, total: &mut usize) -> String {
    let lines: Vec<String> = input
        .split('\n')
        .map(|line| redact_env_line(line, total))
        .collect();
    lines.join("\n")
}

fn redact_env_line(line: &str, total: &mut usize) -> String {
    let mut out = String::with_capacity(line.len());
    let mut pos = 0;
    while let Some(rel_eq) = line[pos..].find('=') {
        let eq = pos + rel_eq;
        let mut key_start = eq
            - line[..eq]
                .bytes()
                .rev()
                .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
                .count();
        if key_start < eq && is_json_space_escape(line.as_bytes(), key_start) {
            key_start += 1;
        }
        let key = &line[key_start..eq];
        out.push_str(&line[pos..=eq]);
        pos = eq + 1;
        if key.is_empty() || line[pos..].starts_with('=') || !is_sensitive_key(key) {
            continue;
        }
        let (replacement, consumed, changed) = redact_env_value(&line[pos..]);
        if changed {
            *total += 1;
        }
        out.push_str(&replacement);
        pos += consumed;
    }
    out.push_str(&line[pos..]);
    out
}

fn redact_env_value(after: &str) -> (String, usize, bool) {
    if after.starts_with("\\\"") {
        let end = escaped_closing_quote(after.as_bytes(), 2).unwrap_or(after.len());
        let changed = &after[2..end] != REDACTED;
        return (format!("\\\"{REDACTED}"), end, changed);
    }
    if let Some(q) = after.bytes().next().filter(|b| matches!(b, b'"' | b'\'')) {
        let close = if q == b'"' {
            closing_quote(after.as_bytes(), 1, q)
        } else {
            after[1..].find('\'').map(|len| 1 + len)
        };
        let qc = char::from(q);
        return match close {
            Some(end) => {
                let changed = &after[1..end] != REDACTED;
                (format!("{qc}{REDACTED}{qc}"), end + 1, changed)
            }
            None => (format!("{qc}{REDACTED}"), after.len(), true),
        };
    }
    let len = unquoted_token_len(after.as_bytes(), b";,\"'&#)}]");
    if len == 0 {
        return (String::new(), 0, false);
    }
    let changed = &after[..len] != REDACTED;
    (REDACTED.to_owned(), len, changed)
}

pub(super) fn redact_recovery_codes(input: &str, total: &mut usize) -> String {
    let labeled = redact_labeled_recovery(input, total);
    redact_grouped_recovery_codes(&labeled, total)
}

fn redact_labeled_recovery(input: &str, total: &mut usize) -> String {
    let mut out = String::with_capacity(input.len());
    let mut pos = 0;
    while let Some(rel) = find_ascii_ci(&input[pos..], "recovery code") {
        let idx = pos + rel;
        let after = idx + "recovery code".len();
        let sep_len = input[after..].len()
            - input[after..]
                .trim_start_matches([' ', '\t', ':', '='])
                .len();
        let val_start = after + sep_len;
        let val_len = unquoted_token_len(&input.as_bytes()[val_start..], b"\"',;)}]");
        let val = &input[val_start..val_start + val_len];
        out.push_str(&input[pos..val_start]);
        if sep_len > 0 && val_len >= 6 && val != REDACTED {
            out.push_str(REDACTED);
            *total += 1;
            pos = val_start + val_len;
        } else {
            pos = val_start;
        }
    }
    out.push_str(&input[pos..]);
    out
}

fn redact_grouped_recovery_codes(input: &str, total: &mut usize) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut pos = 0;
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_alphanumeric() || (i > 0 && bytes[i - 1].is_ascii_alphanumeric()) {
            i += 1;
            continue;
        }
        let end = i + input[i..]
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'-')
            .count();
        let token = &input[i..end];
        if is_grouped_recovery_code(token) {
            out.push_str(&input[pos..i]);
            out.push_str(REDACTED);
            *total += 1;
            pos = end;
        }
        i = end;
    }
    out.push_str(&input[pos..]);
    out
}

fn is_grouped_recovery_code(token: &str) -> bool {
    let groups: Vec<&str> = token.split('-').collect();
    if groups.len() < 4 || groups.len() > 8 {
        return false;
    }
    if groups.len() == 5 && groups[0].len() == 8 && groups[4].len() == 12 {
        return false;
    }
    let uniform = groups.iter().all(|g| {
        (4..=6).contains(&g.len())
            && g.bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
    });
    uniform && token.bytes().any(|b| b.is_ascii_uppercase())
}
