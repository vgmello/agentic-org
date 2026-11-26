//! An ordered tree of values, and its three printed forms. `serde_json`'s map sorts its keys, so
//! the answer is built here in the order the contract states and written by hand: the same state
//! prints the same bytes.

use serde_json::{Map, Number, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    List(Vec<Node>),
    /// Keys in the order they print.
    Map(Vec<(String, Node)>),
}

impl Node {
    pub fn text(text: impl Into<String>) -> Self {
        Node::String(text.into())
    }

    pub fn map<const N: usize>(entries: [(&str, Node); N]) -> Self {
        Node::Map(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        )
    }

    /// A value with no stated order: map keys in name order.
    pub fn from_value(value: &Value) -> Self {
        match value {
            Value::Null => Node::Null,
            Value::Bool(b) => Node::Bool(*b),
            Value::Number(n) => Node::Number(n.clone()),
            Value::String(s) => Node::String(s.clone()),
            Value::Array(items) => Node::List(items.iter().map(Node::from_value).collect()),
            Value::Object(map) => {
                let mut entries: Vec<(String, Node)> = map
                    .iter()
                    .map(|(key, value)| (key.clone(), Node::from_value(value)))
                    .collect();
                // Sorted here, so the order does not depend on how `serde_json` keeps a map.
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                Node::Map(entries)
            }
        }
    }

    pub fn to_value(&self) -> Value {
        match self {
            Node::Null => Value::Null,
            Node::Bool(b) => Value::Bool(*b),
            Node::Number(n) => Value::Number(n.clone()),
            Node::String(s) => Value::String(s.clone()),
            Node::List(items) => Value::Array(items.iter().map(Node::to_value).collect()),
            Node::Map(entries) => Value::Object(
                entries
                    .iter()
                    .map(|(key, value)| (key.clone(), value.to_value()))
                    .collect::<Map<_, _>>(),
            ),
        }
    }

    /// One line of JSON, with no new line at the end.
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        self.write_json(&mut out);
        out
    }

    fn write_json(&self, out: &mut String) {
        match self {
            Node::Null => out.push_str("null"),
            Node::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Node::Number(n) => out.push_str(&n.to_string()),
            Node::String(s) => out.push_str(&json_string(s)),
            Node::List(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write_json(out);
                }
                out.push(']');
            }
            Node::Map(entries) => {
                out.push('{');
                for (i, (key, value)) in entries.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&json_string(key));
                    out.push(':');
                    value.write_json(out);
                }
                out.push('}');
            }
        }
    }

    /// YAML, ending with a new line. Text of several lines is block text where YAML can hold it
    /// unchanged, and quoted otherwise.
    pub fn to_yaml(&self) -> String {
        let mut out = String::new();
        match self {
            Node::Map(entries) if !entries.is_empty() => self.write_yaml_block(0, &mut out),
            Node::List(items) if !items.is_empty() => self.write_yaml_block(0, &mut out),
            _ => {
                self.write_yaml_after(0, &mut out);
                out.remove(0);
            }
        }
        out
    }

    fn write_yaml_block(&self, indent: usize, out: &mut String) {
        let pad = " ".repeat(indent);
        match self {
            Node::Map(entries) => {
                for (key, value) in entries {
                    out.push_str(&pad);
                    out.push_str(&yaml_scalar(key));
                    out.push(':');
                    value.write_yaml_after(indent, out);
                }
            }
            Node::List(items) => {
                for item in items {
                    out.push_str(&pad);
                    out.push('-');
                    item.write_yaml_after(indent, out);
                }
            }
            _ => unreachable!("a block is a map or a list"),
        }
    }

    /// Writes the value that follows `key:` or `-`, through the end of its last line.
    fn write_yaml_after(&self, indent: usize, out: &mut String) {
        match self {
            Node::Null => out.push_str(" null\n"),
            Node::Bool(b) => out.push_str(if *b { " true\n" } else { " false\n" }),
            Node::Number(n) => {
                out.push(' ');
                out.push_str(&n.to_string());
                out.push('\n');
            }
            Node::String(s) => match block_text(s) {
                Some((header, lines)) => {
                    out.push(' ');
                    out.push_str(header);
                    out.push('\n');
                    let pad = " ".repeat(indent + 2);
                    for line in lines.split('\n') {
                        if !line.is_empty() {
                            out.push_str(&pad);
                            out.push_str(line);
                        }
                        out.push('\n');
                    }
                }
                None => {
                    out.push(' ');
                    out.push_str(&yaml_scalar(s));
                    out.push('\n');
                }
            },
            Node::List(items) if items.is_empty() => out.push_str(" []\n"),
            Node::Map(entries) if entries.is_empty() => out.push_str(" {}\n"),
            Node::List(_) | Node::Map(_) => {
                out.push('\n');
                self.write_yaml_block(indent + 2, out);
            }
        }
    }

    /// The compact form: `name: value` lines for one record; for a list, one line for each item
    /// with one cell for each of `columns`, separated by tabs. A field an item does not have is
    /// an empty cell, so a column is the same field on every line. Ends with a new line unless
    /// there is nothing to print.
    ///
    /// A value is one cell on one line. Text prints as it is. It prints as a JSON string when it
    /// is empty (`""`), is the word `null`, starts with a double quote, reads as `meta:` (in any case, after white space or characters that do not show), or holds a tab, a new
    /// line, another control character, a Unicode line or paragraph separator or a
    /// bidirectional control; those characters are then written as escapes. `null` prints as `null`, so an empty cell is
    /// only ever a field the item does not have. A number, `true`, `false`, a list and an
    /// object print as JSON. A name that is not ASCII letters, digits, `_`, `-` and `.`, and the name
    /// `meta`, print as a JSON string too: a line that starts with a bare `meta:` is never data.
    pub fn to_compact(&self, columns: &[&str]) -> String {
        let mut out = String::new();
        match self {
            Node::Map(entries) => {
                for (key, value) in entries {
                    out.push_str(&compact_key(key));
                    out.push_str(": ");
                    out.push_str(&value.compact_cell());
                    out.push('\n');
                }
            }
            Node::List(items) => {
                for item in items {
                    match item {
                        Node::Map(entries) => {
                            let cells: Vec<String> = columns
                                .iter()
                                .map(|column| {
                                    entries
                                        .iter()
                                        .find(|(key, _)| key == column)
                                        .map_or_else(String::new, |(_, value)| value.compact_cell())
                                })
                                .collect();
                            out.push_str(&cells.join("\t"));
                        }
                        other => out.push_str(&other.compact_cell()),
                    }
                    out.push('\n');
                }
            }
            other => {
                out.push_str(&other.compact_cell());
                out.push('\n');
            }
        }
        out
    }

    /// One value on one line, as [`Node::to_compact`] states.
    fn compact_cell(&self) -> String {
        match self {
            Node::String(s) if is_plain_cell(s) => s.clone(),
            other => other.to_one_line(),
        }
    }

    /// JSON that no reader splits: beyond what JSON itself escapes, every other control
    /// character and the Unicode line and paragraph separators are written as escapes. In JSON
    /// they can only be inside a string, so escaping them in the whole text is safe.
    pub(crate) fn to_one_line(&self) -> String {
        let json = self.to_json();
        if !json.chars().any(breaks_a_line) {
            return json;
        }
        let mut out = String::with_capacity(json.len() + 8);
        for c in json.chars() {
            if breaks_a_line(c) {
                out.push_str(&format!("\\u{:04x}", c as u32));
            } else {
                out.push(c);
            }
        }
        out
    }
}

/// A character that ends a line, moves to the next cell or does not print.
fn breaks_a_line(c: char) -> bool {
    c.is_control() || matches!(c, '\u{2028}' | '\u{2029}')
}

fn is_plain_cell(text: &str) -> bool {
    !text.is_empty()
        && text != "null"
        && !text.starts_with('"')
        // No value can pass for the call's own `meta:` line, which only the library prints.
        && !reads_as_meta(text)
        && !text.chars().any(|c| breaks_a_line(c) || reorders(c))
}

/// Whether `text` begins with `meta:` in any case, after white space and the invisible or
/// line-breaking characters `org_docs` names. This keeps the bytes `meta: ` from starting a cell;
/// it does not cover every look-alike (a joiner or direction mark in front, an invisible inside
/// the word, another script's letters), so a reader matches the exact bytes of the last line.
fn reads_as_meta(text: &str) -> bool {
    let shown =
        text.trim_start_matches(|c: char| c.is_whitespace() || org_docs::is_invisible_or_break(c));
    shown
        .get(..5)
        .is_some_and(|start| start.eq_ignore_ascii_case("meta:"))
}

fn compact_key(key: &str) -> String {
    // A field named `meta` is quoted: a bare `meta:` line is only ever the call's.
    let plain = !key.is_empty()
        && !key.eq_ignore_ascii_case("meta")
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    if plain {
        key.to_owned()
    } else {
        Node::text(key).to_one_line()
    }
}

/// A JSON string literal. Beyond what JSON itself escapes (`U+0000..=U+001F`, `"` and `\\`),
/// DEL, the C1 controls (the 8-bit CSI `U+009B` among them), the Unicode line and paragraph
/// separators and the bidirectional controls are written as escapes: `serde_json` leaves them
/// raw, a terminal acts on them (CVE-2021-25743) and some line readers split on them. They can only be inside the literal,
/// so escaping them in the serialized text reads back as the same string.
fn json_string(text: &str) -> String {
    let json = Value::String(text.to_owned()).to_string();
    if !json.chars().any(json_leaves_raw) {
        return json;
    }
    let mut out = String::with_capacity(json.len() + 12);
    for c in json.chars() {
        if json_leaves_raw(c) {
            out.push_str(&format!("\\u{:04x}", c as u32));
        } else {
            out.push(c);
        }
    }
    out
}

/// A character `serde_json` writes raw that no printed JSON may hold raw: DEL, C1, the line
/// and paragraph separators, and the bidirectional controls ([`reorders`]).
fn json_leaves_raw(c: char) -> bool {
    matches!(c, '\u{7f}'..='\u{9f}' | '\u{2028}' | '\u{2029}') || reorders(c)
}

/// The bidirectional marks, embeddings, overrides and isolates: a terminal reorders the text
/// around them (CVE-2021-25743 class), so no printed form holds them raw. The same set the
/// operator shell escapes (`org_cli_shell::terminal::sanitize`).
fn reorders(c: char) -> bool {
    matches!(
        c,
        '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
    )
}

/// Characters YAML does not hold as they are, or holds as a line break.
fn needs_escape(c: char) -> bool {
    c.is_control()
        || reorders(c)
        || matches!(
            c,
            '\u{85}' | '\u{2028}' | '\u{2029}' | '\u{feff}' | '\u{fffe}' | '\u{ffff}'
        )
}

/// Words a YAML reader may take for something other than text.
const NOT_TEXT: &[&str] = &[
    "null", "true", "false", "yes", "no", "on", "off", "y", "n", "nan", "inf",
];

fn yaml_scalar(text: &str) -> String {
    let plain = matches!(text.chars().next(), Some(c) if c.is_ascii_alphabetic())
        && text.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '.' | '/' | '(' | ')' | '-')
        })
        && !text.ends_with(' ')
        && !NOT_TEXT.contains(&text.to_ascii_lowercase().as_str());
    if plain {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if needs_escape(c) => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The header and the lines of a block text, when `text` has several lines and a block holds
/// it unchanged: no character that needs an escape, a first line that is not empty and does not
/// start with a space, and at most one new line at the end.
fn block_text(text: &str) -> Option<(&'static str, &str)> {
    if !text.contains('\n') || text.chars().any(|c| c != '\n' && needs_escape(c)) {
        return None;
    }
    if text.starts_with('\n') || text.starts_with(' ') {
        return None;
    }
    match text.strip_suffix('\n') {
        None => Some(("|-", text)),
        Some(lines) if !lines.ends_with('\n') => Some(("|", lines)),
        Some(_) => None,
    }
}
