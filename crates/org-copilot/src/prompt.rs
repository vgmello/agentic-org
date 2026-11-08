//! Prompt rendering: a fixed template, with the inputs escaped and fenced as data.
//!
//! The template is the feature's own text, fixed at compile time; nothing a person types can
//! become part of it. The person's input and the context go in one JSON object between two
//! fence lines. Escaping makes the fence unforgeable: every `<`, `>`, `&` and backtick in a value
//! is written as a `\uXXXX` escape (still valid JSON, read back as the same character), every
//! newline is `\n`, so no value can contain the line `DATA>>>`, a markdown fence or a tag. The
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

/// JSON for `value` in which none of `< > & `` ` `` or the Unicode line separators appears raw.
/// Those characters can only occur inside JSON strings, so replacing them by escapes is safe.
pub fn escape_json(value: &Value) -> String {
    let text = value.to_string();
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '<' | '>' | '&' | '`' | '\u{2028}' | '\u{2029}' => {
                out.push_str(&format!("\\u{:04x}", c as u32))
            }
            other => out.push(other),
        }
    }
    out
}

/// The full prompt for one run.
pub fn render(
    feature: &Feature,
    input: &Value,
    context: &[ContextItem],
) -> Result<String, RenderError> {
    let items: Vec<Value> = context
        .iter()
        .map(|c| json!({ "kind": c.kind, "label": c.label, "text": c.text }))
        .collect();
    let data = escape_json(&json!({ "input": input, "context": items }));
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
