//! Bounded, strict YAML front matter and mapping parser for offline organization-state integrity
//! checks (`agentic-org doctor`).
//!
//! Accepts the canonical YAML subset emitted by Agentic Org (`doc-77`, `doc-79`, `org-docs`):
//! - Markdown documents with `---` .. `---` YAML front matter fences (rejecting unclosed fences)
//! - Bare YAML mapping files (`project.yaml`, `feature.yaml`)
//! - Block mappings, block sequences (including sequences of mappings like `links:`), flow
//!   sequences/mappings (`[...]`, `{...}`), quoted strings, block scalars (`|` / `>`), numbers,
//!   booleans, and nulls
//! - Refuses tabs in indentation, duplicate keys in a mapping, anchors/aliases, merge keys (`<<`),
//!   unclosed quotes or brackets, non-mapping roots, and inputs over 64 KiB.

use serde_json::{Map, Value};

pub(crate) const MAX_FRONT_MATTER_BYTES: usize = 64 * 1024;
const MAX_DEPTH: usize = 16;

/// Parsed Markdown document split into optional front matter and Markdown body.
#[derive(Debug)]
pub(crate) struct ParsedDoc {
    pub(crate) front_matter: Option<Map<String, Value>>,
}

/// Parses Markdown text that may start with `---` YAML front matter.
pub(crate) fn parse_markdown_front_matter(text: &str) -> Result<ParsedDoc, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(rest) = strip_open_fence(text) else {
        return Ok(ParsedDoc { front_matter: None });
    };
    let mut at = 0usize;
    let front = loop {
        let line_end = rest[at..].find('\n').map(|n| at + n + 1);
        let line = &rest[at..line_end.unwrap_or(rest.len())];
        if line.trim_end_matches(['\r', '\n']) == "---" {
            break &rest[..at];
        }
        match line_end {
            Some(next) => at = next,
            None => return Err("unclosed YAML front matter fence `---`".to_owned()),
        }
    };
    let map = parse_yaml_mapping(front)?;
    Ok(ParsedDoc {
        front_matter: Some(map),
    })
}

/// Parses a bare YAML mapping string (up to 64 KiB) into a JSON object map.
pub(crate) fn parse_yaml_mapping(yaml: &str) -> Result<Map<String, Value>, String> {
    let yaml = yaml.strip_prefix('\u{feff}').unwrap_or(yaml);
    if yaml.len() > MAX_FRONT_MATTER_BYTES {
        return Err(format!("YAML exceeds {MAX_FRONT_MATTER_BYTES}-byte limit"));
    }
    let lines: Vec<Line<'_>> = yaml
        .lines()
        .enumerate()
        .map(|(idx, raw)| prepare_line(idx + 1, raw))
        .collect::<Result<Vec<_>, _>>()?;
    let mut cursor = 0usize;
    skip_blank(&lines, &mut cursor);
    if cursor >= lines.len() {
        return Ok(Map::new());
    }
    if lines[cursor].content == "---" {
        cursor += 1;
        skip_blank(&lines, &mut cursor);
    }
    if cursor >= lines.len() || lines[cursor].content == "..." {
        return Ok(Map::new());
    }
    let base_indent = lines[cursor].indent;
    if lines[cursor].content.starts_with("- ") || lines[cursor].content == "-" {
        return Err(format!(
            "line {}: YAML root must be a mapping, not a sequence",
            lines[cursor].num
        ));
    }
    let map = parse_mapping(&lines, &mut cursor, base_indent, 1)?;
    skip_blank(&lines, &mut cursor);
    if cursor < lines.len() && lines[cursor].content != "..." {
        return Err(format!(
            "line {}: unexpected trailing YAML content",
            lines[cursor].num
        ));
    }
    Ok(map)
}

#[derive(Clone, Copy)]
struct Line<'a> {
    num: usize,
    indent: usize,
    raw: &'a str,
    content: &'a str,
}

fn strip_open_fence(text: &str) -> Option<&str> {
    text.strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
}

fn prepare_line(num: usize, raw: &str) -> Result<Line<'_>, String> {
    let raw = raw.strip_suffix('\r').unwrap_or(raw);
    let indent = raw.bytes().take_while(|&b| b == b' ').count();
    if raw[indent..].starts_with('\t') {
        return Err(format!(
            "line {num}: tab character is not allowed in YAML indentation"
        ));
    }
    let rest = &raw[indent..];
    let content = strip_yaml_comment(rest).trim_end();
    Ok(Line {
        num,
        indent,
        raw,
        content,
    })
}

fn strip_yaml_comment(text: &str) -> &str {
    let bytes = text.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if in_double => {
                i += 2;
                continue;
            }
            b'"' if !in_single => in_double = !in_double,
            b'\'' if !in_double => in_single = !in_single,
            b'#' if !in_single && !in_double && (i == 0 || bytes[i - 1] == b' ') => {
                return &text[..i];
            }
            _ => {}
        }
        i += 1;
    }
    text
}

fn skip_blank(lines: &[Line<'_>], cursor: &mut usize) {
    while *cursor < lines.len() && lines[*cursor].content.is_empty() {
        *cursor += 1;
    }
}

fn parse_mapping(
    lines: &[Line<'_>],
    cursor: &mut usize,
    indent: usize,
    depth: usize,
) -> Result<Map<String, Value>, String> {
    if depth > MAX_DEPTH {
        return Err("YAML nesting exceeds maximum depth".to_owned());
    }
    let mut map = Map::new();
    while *cursor < lines.len() {
        skip_blank(lines, cursor);
        if *cursor >= lines.len() {
            break;
        }
        let line = lines[*cursor];
        if line.content == "..." || line.content == "---" || line.indent < indent {
            break;
        }
        if line.indent > indent {
            return Err(format!(
                "line {}: unexpected indentation ({} > {indent})",
                line.num, line.indent
            ));
        }
        let (key, val_part) = split_key_value(line.num, line.content)?;
        if key == "<<" {
            return Err(format!(
                "line {}: YAML merge key `<<` is forbidden",
                line.num
            ));
        }
        if map.contains_key(&key) {
            return Err(format!("line {}: duplicate YAML key `{key}`", line.num));
        }
        *cursor += 1;
        let value = parse_value_after_key(lines, cursor, indent, depth, line.num, val_part)?;
        map.insert(key, value);
    }
    Ok(map)
}

fn parse_value_after_key(
    lines: &[Line<'_>],
    cursor: &mut usize,
    parent_indent: usize,
    depth: usize,
    line_num: usize,
    val_part: &str,
) -> Result<Value, String> {
    if val_part.is_empty() {
        return parse_nested_block(lines, cursor, parent_indent, depth);
    }
    if matches!(val_part, "|" | "|-" | "|+" | ">" | ">-" | ">+") {
        return Ok(Value::String(parse_block_scalar(
            lines,
            cursor,
            parent_indent,
        )));
    }
    parse_inline_value(line_num, val_part)
}

fn parse_nested_block(
    lines: &[Line<'_>],
    cursor: &mut usize,
    parent_indent: usize,
    depth: usize,
) -> Result<Value, String> {
    skip_blank(lines, cursor);
    if *cursor >= lines.len() {
        return Ok(Value::Null);
    }
    let next = lines[*cursor];
    let is_seq = next.content == "-" || next.content.starts_with("- ");
    if next.indent < parent_indent || (next.indent == parent_indent && !is_seq) {
        return Ok(Value::Null);
    }
    if is_seq {
        parse_sequence(lines, cursor, next.indent, depth + 1).map(Value::Array)
    } else {
        parse_mapping(lines, cursor, next.indent, depth + 1).map(Value::Object)
    }
}

fn parse_sequence(
    lines: &[Line<'_>],
    cursor: &mut usize,
    indent: usize,
    depth: usize,
) -> Result<Vec<Value>, String> {
    if depth > MAX_DEPTH {
        return Err("YAML nesting exceeds maximum depth".to_owned());
    }
    let mut items = Vec::new();
    while *cursor < lines.len() {
        skip_blank(lines, cursor);
        if *cursor >= lines.len() {
            break;
        }
        let line = lines[*cursor];
        if line.indent < indent {
            break;
        }
        if line.indent > indent {
            return Err(format!(
                "line {}: unexpected sequence indentation",
                line.num
            ));
        }
        let Some(rest) = strip_seq_dash(line.content) else {
            break;
        };
        *cursor += 1;
        items.push(parse_seq_item(
            lines, cursor, indent, depth, line.num, rest,
        )?);
    }
    Ok(items)
}

fn parse_seq_item(
    lines: &[Line<'_>],
    cursor: &mut usize,
    seq_indent: usize,
    depth: usize,
    line_num: usize,
    rest: &str,
) -> Result<Value, String> {
    if rest.is_empty() {
        return parse_nested_block(lines, cursor, seq_indent, depth);
    }
    if let Ok((key, val_part)) = split_key_value(line_num, rest) {
        let item_indent = seq_indent + 2;
        let first_val =
            parse_value_after_key(lines, cursor, item_indent, depth + 1, line_num, val_part)?;
        let mut map = Map::new();
        map.insert(key, first_val);
        let extra = parse_mapping(lines, cursor, item_indent, depth + 1)?;
        for (k, v) in extra {
            if map.contains_key(&k) {
                return Err(format!("line {line_num}: duplicate YAML key `{k}`"));
            }
            map.insert(k, v);
        }
        return Ok(Value::Object(map));
    }
    parse_inline_value(line_num, rest)
}

fn strip_seq_dash(content: &str) -> Option<&str> {
    if content == "-" {
        Some("")
    } else {
        content.strip_prefix("- ").map(str::trim_start)
    }
}

fn parse_block_scalar(lines: &[Line<'_>], cursor: &mut usize, parent_indent: usize) -> String {
    let mut block_indent: Option<usize> = None;
    let mut out = Vec::new();
    while *cursor < lines.len() {
        let line = lines[*cursor];
        if line.raw.trim().is_empty() {
            out.push("");
            *cursor += 1;
            continue;
        }
        if line.indent <= parent_indent {
            break;
        }
        let cut = *block_indent.get_or_insert(line.indent);
        if line.indent < cut {
            break;
        }
        out.push(&line.raw[cut..]);
        *cursor += 1;
    }
    out.join("\n").trim_end_matches('\n').to_owned()
}

fn split_key_value(line_num: usize, content: &str) -> Result<(String, &str), String> {
    let bytes = content.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if in_double => {
                i += 2;
                continue;
            }
            b'"' if !in_single => in_double = !in_double,
            b'\'' if !in_double => in_single = !in_single,
            b':' if !in_single && !in_double => {
                let next = bytes.get(i + 1).copied();
                if next.is_none() || next == Some(b' ') {
                    let raw_key = content[..i].trim();
                    let key = parse_key_scalar(line_num, raw_key)?;
                    let val = content[i + 1..].trim();
                    return Ok((key, val));
                }
            }
            _ => {}
        }
        i += 1;
    }
    Err(format!(
        "line {line_num}: expected `key: value` mapping entry"
    ))
}

fn parse_key_scalar(line_num: usize, raw: &str) -> Result<String, String> {
    if raw.is_empty() {
        return Err(format!("line {line_num}: empty YAML mapping key"));
    }
    if raw.starts_with(['&', '*', '!']) {
        return Err(format!(
            "line {line_num}: anchors, aliases, and tags are forbidden"
        ));
    }
    if raw.starts_with('"') {
        return parse_double_quoted(line_num, raw);
    }
    if raw.starts_with('\'') {
        return parse_single_quoted(line_num, raw);
    }
    Ok(raw.to_owned())
}

fn parse_inline_value(line_num: usize, raw: &str) -> Result<Value, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(Value::Null);
    }
    if raw.starts_with(['&', '*', '!']) {
        return Err(format!(
            "line {line_num}: anchors, aliases, and tags are forbidden"
        ));
    }
    if raw.starts_with('"') {
        return parse_double_quoted(line_num, raw).map(Value::String);
    }
    if raw.starts_with('\'') {
        return parse_single_quoted(line_num, raw).map(Value::String);
    }
    if raw.starts_with('[') {
        return parse_flow_sequence(line_num, raw);
    }
    if raw.starts_with('{') {
        return parse_flow_mapping(line_num, raw);
    }
    Ok(parse_plain_scalar(raw))
}

fn parse_double_quoted(line_num: usize, raw: &str) -> Result<String, String> {
    if raw.len() < 2 || !raw.ends_with('"') {
        return Err(format!(
            "line {line_num}: unclosed double-quoted YAML string"
        ));
    }
    serde_json::from_str::<String>(raw)
        .map_err(|e| format!("line {line_num}: invalid double-quoted YAML string: {e}"))
}

fn parse_single_quoted(line_num: usize, raw: &str) -> Result<String, String> {
    if raw.len() < 2 || !raw.ends_with('\'') {
        return Err(format!(
            "line {line_num}: unclosed single-quoted YAML string"
        ));
    }
    Ok(raw[1..raw.len() - 1].replace("''", "'"))
}

fn parse_flow_sequence(line_num: usize, raw: &str) -> Result<Value, String> {
    if !raw.ends_with(']') {
        return Err(format!("line {line_num}: unclosed YAML flow sequence `[`"));
    }
    let inner = raw[1..raw.len() - 1].trim();
    if inner.is_empty() {
        return Ok(Value::Array(Vec::new()));
    }
    let parts = split_flow_items(line_num, inner)?;
    let mut items = Vec::with_capacity(parts.len());
    for part in parts {
        items.push(parse_inline_value(line_num, part)?);
    }
    Ok(Value::Array(items))
}

fn parse_flow_mapping(line_num: usize, raw: &str) -> Result<Value, String> {
    if !raw.ends_with('}') {
        return Err(format!("line {line_num}: unclosed YAML flow mapping `{{`"));
    }
    let inner = raw[1..raw.len() - 1].trim();
    if inner.is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    let mut map = Map::new();
    for part in split_flow_items(line_num, inner)? {
        let (key, val) = split_key_value(line_num, part)?;
        if map.contains_key(&key) {
            return Err(format!("line {line_num}: duplicate YAML key `{key}`"));
        }
        map.insert(key, parse_inline_value(line_num, val)?);
    }
    Ok(Value::Object(map))
}

fn split_flow_items(line_num: usize, inner: &str) -> Result<Vec<&str>, String> {
    let bytes = inner.as_bytes();
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if in_double => {
                i += 2;
                continue;
            }
            b'"' if !in_single => in_double = !in_double,
            b'\'' if !in_double => in_single = !in_single,
            b'[' | b'{' if !in_single && !in_double => depth += 1,
            b']' | b'}' if !in_single && !in_double => depth -= 1,
            b',' if !in_single && !in_double && depth == 0 => {
                parts.push(inner[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
        if depth < 0 {
            return Err(format!(
                "line {line_num}: unbalanced bracket in YAML flow collection"
            ));
        }
        i += 1;
    }
    if in_single || in_double || depth != 0 {
        return Err(format!(
            "line {line_num}: unclosed quote or bracket in YAML flow collection"
        ));
    }
    let tail = inner[start..].trim();
    if !tail.is_empty() {
        parts.push(tail);
    }
    Ok(parts)
}

fn parse_plain_scalar(raw: &str) -> Value {
    match raw {
        "null" | "Null" | "NULL" | "~" => Value::Null,
        "true" | "True" | "TRUE" => Value::Bool(true),
        "false" | "False" | "FALSE" => Value::Bool(false),
        _ => {
            if let Ok(n) = raw.parse::<i64>() {
                Value::from(n)
            } else {
                Value::String(raw.to_owned())
            }
        }
    }
}
