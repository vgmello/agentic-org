//! Escaping untrusted text (CVE-2021-25743 class): a name, a message or a path that reaches a
//! terminal can carry escape sequences that recolor, move the cursor, rewrite the title or
//! reorder the line. Every text the shell prints passes through [`sanitize`]; every JSON
//! document it prints passes through [`escape_json_controls`]. Re-exported by
//! [`crate::terminal`].

use std::borrow::Cow;
use std::fmt::Write as _;

/// A character that must never reach a terminal as itself: every control character (C0,
/// DEL, C1, including tab and line breaks), the Unicode line and paragraph separators, and
/// the bidirectional marks, embeddings, overrides and isolates.
pub(crate) fn is_unsafe_char(c: char) -> bool {
    c.is_control() || breaks_or_reorders(c)
}

/// Characters that some terminals and viewers show as a line break, or that reorder the text
/// around them.
fn breaks_or_reorders(c: char) -> bool {
    matches!(
        c,
        '\u{2028}' | '\u{2029}' // line and paragraph separators
            | '\u{061C}' | '\u{200E}' | '\u{200F}' // direction marks
            | '\u{202A}'..='\u{202E}' // embeddings and overrides
            | '\u{2066}'..='\u{2069}' // isolates
    )
}

/// `s` with every [unsafe character](is_unsafe_char) written as visible `\u{..}` text (the
/// form of [`char::escape_unicode`], e.g. ESC becomes `\u{1b}` and a tab `\u{9}`). Borrowed
/// when nothing changes.
pub fn sanitize(s: &str) -> Cow<'_, str> {
    let Some(first) = s.find(is_unsafe_char) else {
        return Cow::Borrowed(s);
    };
    let mut out = String::with_capacity(s.len() + 16);
    out.push_str(&s[..first]);
    for c in s[first..].chars() {
        if is_unsafe_char(c) {
            out.extend(c.escape_unicode());
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

/// Characters `serde_json` leaves raw inside strings that a terminal or a JavaScript
/// consumer can still act on: DEL, the C1 controls (an 8-bit CSI is `U+009B`), the line
/// and paragraph separators, and the bidirectional marks, embeddings, overrides and isolates
/// (a `U+202E` in a value reorders what a terminal shows of the line around it).
fn needs_json_escape(c: char) -> bool {
    matches!(c, '\u{7F}'..='\u{9F}') || breaks_or_reorders(c)
}

/// [`sanitize`] for text of several lines: each line is sanitized and the line breaks
/// (`\n`) are kept, so prose with untrusted values in it still reads as lines.
pub fn sanitize_lines(text: &str) -> String {
    text.split('\n')
        .map(|line| sanitize(line).into_owned())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Already-serialized JSON with DEL, C1, `U+2028`/`U+2029` and the bidirectional controls
/// written as `\uXXXX` escapes. JSON whitespace is ASCII, so these characters can only occur
/// inside strings and the document parses to the identical value. Returned unchanged (no copy)
/// when there is nothing to escape.
pub fn escape_json_controls(json: String) -> String {
    if !json.contains(needs_json_escape) {
        return json;
    }
    let mut out = String::with_capacity(json.len() + 16);
    for c in json.chars() {
        if needs_json_escape(c) {
            // Writing to a `String` cannot fail.
            let _ = write!(out, "\\u{:04x}", u32::from(c));
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests;
