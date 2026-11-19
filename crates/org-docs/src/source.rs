//! Additive source-preserving scalar editing. This module knows YAML syntax,
//! not task policy: the selected adapter owns its six-property allowlist.

use std::{collections::BTreeMap, fmt, ops::Range};

use saphyr_parser::{Event, Parser, ScalarStyle, Span};
use serde_json::{Map, Value};

use crate::{parse, strip_fence, FrontMatterError, MAX_EVENTS, MAX_FRONT_MATTER_BYTES};

/// Body budget for the initial source-editing admission profile.
pub const MAX_SOURCE_BODY_BYTES: usize = 8 * 1024 * 1024;

/// A source-editing refusal. The complete original input remains the caller's;
/// refusal never returns a truncated or partly edited document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceError {
    /// Existing strict YAML guard rejected the input or result.
    Parse(FrontMatterError),
    /// No front matter, flow root, or ambiguous/unsupported entry/token span.
    UnsupportedRepresentation,
    /// A requested decoded root key is absent.
    MissingKey,
    /// A caller requested more than one replacement for the same root key.
    DuplicateEdit,
    /// Source body exceeds the recorded editing profile.
    BodyTooLarge,
    /// Encoded front matter would exceed the existing bound.
    FrontMatterTooLarge,
    /// Reparsing or unchanged-source verification disagreed with the edit.
    InvalidResult,
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Ingress adapters may map this to a sanitized transport error. Never
        // include the source, replacement or a decoded root key here.
        match self {
            Self::Parse(_) => f.write_str("source edit: strict YAML guard rejected input"),
            other => write!(f, "source edit: {other:?}"),
        }
    }
}

impl std::error::Error for SourceError {}

impl From<FrontMatterError> for SourceError {
    fn from(value: FrontMatterError) -> Self {
        Self::Parse(value)
    }
}

/// Typed scalar replacement. Unknown extension values are never serialized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarValue<'a> {
    /// Explicit YAML null, not an implicit empty value.
    Null,
    /// An exact signed integer.
    Integer(i64),
    /// A YAML-safe quoted string, with physical line breaks escaped.
    String(&'a str),
}

/// The key is a decoded root key, not a source spelling or regular expression.
#[derive(Clone, Copy, Debug)]
pub struct RootScalarEdit<'a> {
    /// Decoded property name.
    pub key: &'a str,
    /// Intended scalar value.
    pub value: ScalarValue<'a>,
}

/// Validated Markdown plus original source and root value-token ranges.
/// A parsed `Value` is useful for known facts but never regenerates the source.
pub struct SourceDoc<'a> {
    source: &'a str,
    front: Range<usize>,
    body: &'a str,
    facts: Map<String, Value>,
    scalars: BTreeMap<String, Option<Range<usize>>>,
}

impl<'a> SourceDoc<'a> {
    /// Reuses every existing YAML guard and the initial body bound. Unsupported
    /// mutable representations remain readable through the older [`crate::parse`]
    /// API but cannot be edited here.
    pub fn parse(source: &'a str) -> Result<Self, SourceError> {
        // Bound total work even for a source with no fence. Maximum overhead is
        // BOM plus two CRLF fence lines (13 bytes).
        if source.len() > MAX_SOURCE_BODY_BYTES + MAX_FRONT_MATTER_BYTES + 13 {
            return Err(SourceError::BodyTooLarge);
        }
        let parsed = parse(source)?;
        if parsed.body.len() > MAX_SOURCE_BODY_BYTES {
            return Err(SourceError::BodyTooLarge);
        }
        let facts = parsed
            .front_matter
            .ok_or(SourceError::UnsupportedRepresentation)?;
        let text = source.strip_prefix('\u{feff}').unwrap_or(source);
        let rest = strip_fence(text).ok_or(SourceError::UnsupportedRepresentation)?;
        let start = source.len() - rest.len();
        let mut at = 0;
        let front = loop {
            let line_end = rest[at..].find('\n').map(|n| at + n + 1);
            if rest[at..line_end.unwrap_or(rest.len())].trim_end_matches(['\r', '\n']) == "---" {
                break start..start + at;
            }
            at = line_end.ok_or(SourceError::UnsupportedRepresentation)?;
        };
        let yaml = &source[front.clone()];
        let offsets: Vec<usize> = yaml
            .char_indices()
            .map(|(byte, _)| byte)
            .chain(std::iter::once(yaml.len()))
            .collect();
        // A second scan retains marks discarded by Walk::next. It is bounded
        // independently, and only runs after the existing structural guards.
        let mut events = Vec::new();
        for event in Parser::new_from_str(yaml) {
            if events.len() == MAX_EVENTS {
                return Err(SourceError::UnsupportedRepresentation);
            }
            events.push(event.map_err(|_| SourceError::UnsupportedRepresentation)?);
        }
        let mut cursor = events
            .iter()
            .position(|(e, _)| matches!(e, Event::MappingStart(..)))
            .ok_or(SourceError::UnsupportedRepresentation)?;
        let root_span = events[cursor].1;
        let root_byte = *offsets
            .get(root_span.start.index())
            .ok_or(SourceError::UnsupportedRepresentation)?;
        if yaml.as_bytes().get(root_byte) == Some(&b'{') {
            return Err(SourceError::UnsupportedRepresentation);
        }
        cursor += 1;
        let mut scalars = BTreeMap::new();
        while !matches!(events.get(cursor).map(|e| &e.0), Some(Event::MappingEnd)) {
            let (key, key_style, key_span) = match events.get(cursor) {
                Some((Event::Scalar(key, style, _, _), span)) => (key.to_string(), *style, *span),
                _ => return Err(SourceError::UnsupportedRepresentation),
            };
            cursor += 1;
            let range = match events.get(cursor) {
                Some((Event::Scalar(_, style, _, _), span)) => {
                    simple_value_span(yaml, &offsets, key_style, key_span, *style, *span)
                        .map(|r| front.start + r.start..front.start + r.end)
                }
                _ => None,
            };
            if scalars.insert(key, range).is_some() {
                return Err(SourceError::UnsupportedRepresentation);
            }
            skip_node(&events, &mut cursor)?;
        }
        Ok(Self {
            source,
            front,
            body: parsed.body,
            facts,
            scalars,
        })
    }

    /// Original source, including BOM and fences.
    pub fn source(&self) -> &'a str {
        self.source
    }

    /// Known facts may be decoded from this map. Do not serialize it as a snapshot.
    pub fn front_matter(&self) -> &Map<String, Value> {
        &self.facts
    }

    /// Original Markdown body, including its exact line endings.
    pub fn body(&self) -> &'a str {
        self.body
    }

    /// Replace only requested root scalar tokens, verify all remaining byte
    /// slices and body, then reparse and compare each intended typed value.
    pub fn replace(&self, edits: &[RootScalarEdit<'_>]) -> Result<String, SourceError> {
        if edits.len() > self.scalars.len() {
            return Err(SourceError::DuplicateEdit);
        }
        let mut planned = BTreeMap::new();
        let mut front_len = self.front.len();
        for edit in edits {
            let range = self
                .scalars
                .get(edit.key)
                .ok_or(SourceError::MissingKey)?
                .as_ref()
                .ok_or(SourceError::UnsupportedRepresentation)?;
            front_len = front_len
                .checked_sub(range.len())
                .ok_or(SourceError::FrontMatterTooLarge)?;
            if planned
                .insert(range.start, (range.clone(), *edit))
                .is_some()
            {
                return Err(SourceError::DuplicateEdit);
            }
        }
        let mut changes = BTreeMap::new();
        for (start, (range, edit)) in planned {
            let encoded = encode(edit.value)?;
            front_len = front_len
                .checked_add(encoded.len())
                .ok_or(SourceError::FrontMatterTooLarge)?;
            if front_len > MAX_FRONT_MATTER_BYTES {
                return Err(SourceError::FrontMatterTooLarge);
            }
            changes.insert(start, (range, encoded, edit));
        }
        let capacity = self.source.len() - self.front.len() + front_len;
        let mut output = String::with_capacity(capacity);
        let mut unchanged = Vec::with_capacity(changes.len() + 1);
        let mut previous = 0;
        for (range, encoded, _) in changes.values() {
            if range.start < previous {
                return Err(SourceError::InvalidResult);
            }
            let begin = output.len();
            output.push_str(&self.source[previous..range.start]);
            unchanged.push((previous..range.start, begin..output.len()));
            output.push_str(encoded);
            previous = range.end;
        }
        let begin = output.len();
        output.push_str(&self.source[previous..]);
        unchanged.push((previous..self.source.len(), begin..output.len()));
        if unchanged
            .iter()
            .any(|(old, new)| self.source[old.clone()] != output[new.clone()])
        {
            return Err(SourceError::InvalidResult);
        }
        let validated = SourceDoc::parse(&output)?;
        if validated.body != self.body {
            return Err(SourceError::InvalidResult);
        }
        for (_, _, edit) in changes.values() {
            let intended = match edit.value {
                ScalarValue::Null => Value::Null,
                ScalarValue::Integer(n) => Value::from(n),
                ScalarValue::String(s) => Value::String(s.to_owned()),
            };
            if validated.facts.get(edit.key) != Some(&intended) {
                return Err(SourceError::InvalidResult);
            }
        }
        Ok(output)
    }
}

fn skip_node(events: &[(Event<'_>, Span)], cursor: &mut usize) -> Result<(), SourceError> {
    match events.get(*cursor).map(|e| &e.0) {
        Some(Event::Scalar(..)) => *cursor += 1,
        Some(Event::MappingStart(..) | Event::SequenceStart(..)) => {
            *cursor += 1;
            while !matches!(
                events.get(*cursor).map(|e| &e.0),
                Some(Event::MappingEnd | Event::SequenceEnd)
            ) {
                skip_node(events, cursor)?;
            }
            *cursor += 1;
        }
        _ => return Err(SourceError::UnsupportedRepresentation),
    }
    Ok(())
}

fn simple_value_span(
    yaml: &str,
    offsets: &[usize],
    key_style: ScalarStyle,
    key: Span,
    style: ScalarStyle,
    value: Span,
) -> Option<Range<usize>> {
    if key.start.line() != value.start.line() {
        return None;
    }
    let key_start = *offsets.get(key.start.index())?;
    let key_end = token_end(yaml, key_start..*offsets.get(key.end.index())?, key_style)?;
    let line_start = yaml[..key_start].rfind('\n').map_or(0, |n| n + 1);
    if !yaml[line_start..key_start]
        .bytes()
        .all(|b| b == b' ' || b == b'\t')
    {
        return None;
    }
    let start = *offsets.get(value.start.index())?;
    let end = token_end(yaml, start..*offsets.get(value.end.index())?, style)?;
    let separator = yaml.get(key_end..start)?.trim_matches([' ', '\t']);
    if separator != ":" || start == end || yaml[start..end].contains(['\r', '\n']) {
        return None;
    }
    Some(start..end)
}

fn token_end(yaml: &str, range: Range<usize>, style: ScalarStyle) -> Option<usize> {
    if range.start >= range.end
        || !yaml.is_char_boundary(range.start)
        || !yaml.is_char_boundary(range.end)
    {
        return None;
    }
    let quote = match style {
        ScalarStyle::Plain => return Some(range.end),
        ScalarStyle::SingleQuoted => b'\'',
        ScalarStyle::DoubleQuoted => b'"',
        _ => return None,
    };
    let bytes = yaml.as_bytes();
    if bytes.get(range.start) != Some(&quote) {
        return None;
    }
    let mut at = range.start + 1;
    while at < range.end {
        if matches!(bytes[at], b'\r' | b'\n') {
            return None;
        }
        if bytes[at] == quote {
            if quote == b'\'' && bytes.get(at + 1) == Some(&quote) {
                at += 2;
                continue;
            }
            return Some(at + 1);
        }
        if quote == b'"' && bytes[at] == b'\\' {
            at += 1;
        }
        at += 1;
    }
    None
}

fn encode(value: ScalarValue<'_>) -> Result<String, SourceError> {
    Ok(match value {
        ScalarValue::Null => "null".to_owned(),
        ScalarValue::Integer(n) => n.to_string(),
        ScalarValue::String(s) => {
            if s.len() > MAX_FRONT_MATTER_BYTES {
                return Err(SourceError::FrontMatterTooLarge);
            }
            let mut out = String::from("\"");
            for c in s.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c if c.is_control() || matches!(c, '\u{85}' | '\u{2028}' | '\u{2029}') => {
                        use std::fmt::Write;
                        write!(out, "\\u{:04x}", c as u32)
                            .map_err(|_| SourceError::InvalidResult)?;
                    }
                    c => out.push(c),
                }
                if out.len() >= MAX_FRONT_MATTER_BYTES {
                    return Err(SourceError::FrontMatterTooLarge);
                }
            }
            out.push('"');
            out
        }
    })
}

#[cfg(test)]
mod tests;
