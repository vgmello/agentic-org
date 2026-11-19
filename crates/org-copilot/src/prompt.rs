//! Prompt rendering: a fixed template, with the inputs escaped and fenced as data.
//!
//! The template is the feature's own text, fixed at compile time; nothing a person types can
//! become part of it. The person's input and the context go in one JSON object between two
//! fence lines. Escaping makes the fence unforgeable: every `<`, `>`, `&` and backtick in a value,
//! and every control character, is written as a `\uXXXX` escape (still valid JSON, read back as the
//! same character), every newline is `\n`, so no value can contain the line `DATA>>>`, a markdown fence or a tag. The
//! holes are filled in one pass, so data that looks like `{{schema}}` is never expanded.

use std::fmt;

use serde_json::{json, Value};

use crate::contract::ContextItem;
use crate::feature::Feature;

/// The line that opens the data block.
pub const FENCE_OPEN: &str = "<<<DATA";
/// The line that closes the data block.
pub const FENCE_CLOSE: &str = "DATA>>>";

/// Rendering failed: the data could not be fenced safely. Cannot happen with the escaping
/// below; it is checked anyway so a future change to the escaping fails loudly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderError;

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the data could not be fenced safely")
    }
}

impl std::error::Error for RenderError {}

/// Characters written as `\uXXXX` rather than raw: the ones a fence or a tag is made of, every
/// control character (C0, DEL and C1, so U+0085 too), the Unicode line and paragraph separators,
/// and the invisible format characters that can hide text from a reader (soft hyphen, direction
/// marks, zero-width and joiner characters, the word joiner block, annotation marks, and the
/// "tag" characters U+E0000..U+E007F that can carry a hidden message).
fn must_escape(c: char) -> bool {
    matches!(c, '<' | '>' | '&' | '`')
        || c.is_control()
        || matches!(
            c,
            '\u{00AD}'
                | '\u{061C}'
                | '\u{180E}'
                | '\u{200B}'..='\u{200F}'
                | '\u{2028}'..='\u{202E}'
                | '\u{2060}'..='\u{206F}'
                | '\u{FEFF}'
                | '\u{FFF9}'..='\u{FFFB}'
                | '\u{E0000}'..='\u{E007F}'
        )
}

/// JSON for `value` in which none of the characters in [`must_escape`] appears raw. They can only
/// occur inside JSON strings (outside strings JSON holds only ASCII punctuation, digits and
/// letters), so replacing them by escapes is safe, and reads back as the same value.
pub fn escape_json(value: &Value) -> String {
    let text = value.to_string();
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if must_escape(c) {
            // JSON escapes are UTF-16 units: a character past U+FFFF is a surrogate pair.
            for unit in c.encode_utf16(&mut [0; 2]) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The data block's content: the input and the context as one escaped JSON line.
pub(crate) fn data_json(input: &Value, context: &[ContextItem]) -> String {
    let items: Vec<Value> = context
        .iter()
        .map(|c| json!({ "kind": c.kind, "label": c.label, "text": c.text }))
        .collect();
    escape_json(&json!({ "input": input, "context": items }))
}

/// The full prompt for one run.
pub fn render(
    feature: &Feature,
    input: &Value,
    context: &[ContextItem],
) -> Result<String, RenderError> {
    let data = data_json(input, context);
    if data.contains(['<', '>', '\n']) {
        return Err(RenderError);
    }
    let schema = escape_json(feature.output_schema().document());
    let t = feature.template();
    Ok(format!(
        "{head}{schema}{middle}\n{FENCE_OPEN}\n{data}\n{FENCE_CLOSE}\n{tail}",
        head = t.head,
        middle = t.middle,
        tail = t.tail
    ))
}
