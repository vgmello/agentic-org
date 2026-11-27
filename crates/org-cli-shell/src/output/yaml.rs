//! Deterministic, terminal-safe YAML serialization of `serde_json::Value` for `org-cli-shell`
//! (`AGENT_OS_FORMAT=yaml` / `--format yaml`). Mirrors `org-command`'s escaping rules so DEL,
//! C0/C1 controls, Unicode line/paragraph separators, and bidirectional overrides are never
//! emitted raw (CVE-2021-25743).

use serde_json::Value;

/// Renders `value` as deterministic YAML ending with a newline.
pub(super) fn render_yaml(value: &Value) -> String {
    let mut out = String::new();
    match value {
        Value::Object(map) if !map.is_empty() => write_block(value, 0, &mut out),
        Value::Array(items) if !items.is_empty() => write_block(value, 0, &mut out),
        _ => {
            write_after(value, 0, &mut out);
            if out.starts_with(' ') {
                out.remove(0);
            }
        }
    }
    out
}

fn write_block(value: &Value, indent: usize, out: &mut String) {
    let pad = " ".repeat(indent);
    match value {
        Value::Object(map) => {
            for (key, val) in map {
                out.push_str(&pad);
                out.push_str(&yaml_scalar(key));
                out.push(':');
                write_after(val, indent, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                out.push_str(&pad);
                out.push('-');
                write_after(item, indent, out);
            }
        }
        _ => {}
    }
}

fn write_after(value: &Value, indent: usize, out: &mut String) {
    match value {
        Value::Null => out.push_str(" null\n"),
        Value::Bool(b) => out.push_str(if *b { " true\n" } else { " false\n" }),
        Value::Number(n) => {
            out.push(' ');
            out.push_str(&n.to_string());
            out.push('\n');
        }
        Value::String(s) => match block_text(s) {
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
        Value::Array(items) if items.is_empty() => out.push_str(" []\n"),
        Value::Object(map) if map.is_empty() => out.push_str(" {}\n"),
        Value::Array(_) | Value::Object(_) => {
            out.push('\n');
            write_block(value, indent + 2, out);
        }
    }
}

fn reorders(c: char) -> bool {
    matches!(
        c,
        '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
    )
}

fn needs_escape(c: char) -> bool {
    c.is_control()
        || reorders(c)
        || matches!(
            c,
            '\u{85}' | '\u{2028}' | '\u{2029}' | '\u{feff}' | '\u{fffe}' | '\u{ffff}'
        )
}

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
