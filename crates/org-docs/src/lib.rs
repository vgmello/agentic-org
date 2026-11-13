//! Docs are Markdown with YAML front matter (`docs/design/tasks-projects-docs.md` §4.9). This
//! crate splits a doc and parses its front matter into a plain value tree, strictly and within
//! fixed bounds. It knows no doc kind; a kind's crate decodes the tree with exact field paths.
//!
//! The YAML is read as YAML 1.2 core schema, typed here rather than by the parser: `no`, `yes`,
//! `on` and `off` are strings, `0777` is the integer 777, `1_000` is a string. Anchors, aliases,
//! tags and `<<` merge keys are refused, as are duplicate or non-string keys, a root that isn't
//! a mapping and a second document. Pure: no I/O.
//!
//! It also holds the shared check for text that one-line fields must not contain
//! ([`is_invisible_or_break`]).

#![warn(missing_docs)]

use std::fmt;

use saphyr_parser::{Event, Parser, ScalarStyle, ScanError};
use serde_json::{Map, Number, Value};

/// The largest front matter read, in bytes.
pub const MAX_FRONT_MATTER_BYTES: usize = 64 * 1024;
/// The deepest nesting of mappings and sequences, the root mapping counting as one.
pub const MAX_DEPTH: usize = 16;
/// The most parser events one front matter may produce.
pub const MAX_EVENTS: usize = 10_000;

/// A doc split into its front matter (`None` when it has none) and its Markdown body.
#[derive(Debug, Clone, PartialEq)]
pub struct Doc<'a> {
    /// The front matter's top-level mapping.
    pub front_matter: Option<Map<String, Value>>,
    /// Everything after the closing fence. With no front matter it is the whole text, except that
    /// a leading byte-order mark is stripped first (as it is before the fence is looked for).
    pub body: &'a str,
}

/// Why front matter was refused, and where: `field` is a path like `checklist[2].text`, empty
/// for the front matter as a whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontMatterError {
    /// The field the problem is in.
    pub field: String,
    /// What the problem is.
    pub reason: Reason,
}

/// What was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// An opening `---` fence with no closing one.
    Unclosed,
    /// Longer than [`MAX_FRONT_MATTER_BYTES`].
    TooLarge,
    /// Not valid YAML (the parser's text).
    Syntax(String),
    /// The root isn't a mapping.
    NotAMapping,
    /// An alias (`*name`).
    Alias,
    /// An anchor (`&name`).
    Anchor,
    /// A tag (`!name`, `!!str`).
    Tag,
    /// A `<<` merge key.
    MergeKey,
    /// The same key twice in one mapping.
    DuplicateKey,
    /// A key that isn't a string.
    NonStringKey,
    /// Nested deeper than [`MAX_DEPTH`].
    TooDeep,
    /// More than [`MAX_EVENTS`] events.
    TooManyEvents,
    /// More than one YAML document.
    MultipleDocuments,
    /// A number that is infinite, not a number, or out of range.
    BadNumber,
}

impl fmt::Display for FrontMatterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let field = if self.field.is_empty() {
            "front matter"
        } else {
            &self.field
        };
        write!(f, "{field}: {:?}", self.reason)
    }
}

impl std::error::Error for FrontMatterError {}

/// What `char::is_control` lets through but a one-line text field must not hold; callers check
/// `char::is_control` too (`\n`, `\r` and the other control characters are not in this set).
/// The set: the Unicode line and paragraph separators (U+2028, U+2029), the zero-width space
/// (U+200B), the bidirectional embeddings, overrides and isolates (U+202A-202E, U+2066-2069),
/// the word joiner and invisible operators (U+2060-2064), the deprecated formatting characters
/// (U+206A-206F), the byte order mark (U+FEFF), the Mongolian vowel separator (U+180E), the
/// interlinear annotation characters (U+FFF9-FFFB), the invisible fillers (U+115F, U+1160,
/// U+3164, U+FFA0), the tag block (U+E0000-E007F) and the variation selectors supplement
/// (U+E0100-E01EF). The zero-width joiner and non-joiner and the left/right marks are not in it:
/// emoji sequences and Persian, Indic and mixed-direction text need them. The same set as
/// org-office's status lines (organization/status.rs), which keeps its own copy.
pub fn is_invisible_or_break(c: char) -> bool {
    matches!(
        c,
        '\u{2028}'
            | '\u{2029}'
            | '\u{200b}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206f}'
            | '\u{feff}'
            | '\u{180e}'
            | '\u{fff9}'..='\u{fffb}'
            | '\u{115f}'
            | '\u{1160}'
            | '\u{3164}'
            | '\u{ffa0}'
            | '\u{e0000}'..='\u{e007f}'
            | '\u{e0100}'..='\u{e01ef}'
    )
}

/// Splits `text` and parses its front matter. A doc has front matter only when it starts (after
/// an optional byte-order mark) with a `---` line; it ends at the next `---` line.
pub fn parse(text: &str) -> Result<Doc<'_>, FrontMatterError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(rest) = strip_fence(text) else {
        return Ok(Doc {
            front_matter: None,
            body: text,
        });
    };
    let mut at = 0;
    let (front, body) = loop {
        let line_end = rest[at..].find('\n').map(|n| at + n + 1);
        let line = &rest[at..line_end.unwrap_or(rest.len())];
        if line.trim_end_matches(['\r', '\n']) == "---" {
            break (&rest[..at], &rest[line_end.unwrap_or(rest.len())..]);
        }
        match line_end {
            Some(next) => at = next,
            None => return Err(error(&[], Reason::Unclosed)),
        }
    };
    if front.len() > MAX_FRONT_MATTER_BYTES {
        return Err(error(&[], Reason::TooLarge));
    }
    let front_matter = Walk {
        parser: Parser::new_from_str(front),
        events: 0,
        path: vec![],
    }
    .document()?;
    Ok(Doc {
        front_matter: Some(front_matter),
        body,
    })
}

fn strip_fence(text: &str) -> Option<&str> {
    text.strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
}

enum Segment {
    Key(String),
    Index(usize),
}

fn render(path: &[Segment]) -> String {
    let mut out = String::new();
    for segment in path {
        match segment {
            Segment::Key(key) if out.is_empty() => out.push_str(key),
            Segment::Key(key) => {
                out.push('.');
                out.push_str(key);
            }
            Segment::Index(n) => out.push_str(&format!("[{n}]")),
        }
    }
    out
}

fn error(path: &[Segment], reason: Reason) -> FrontMatterError {
    FrontMatterError {
        field: render(path),
        reason,
    }
}

struct Walk<'a> {
    parser: Parser<'a, saphyr_parser::StrInput<'a>>,
    events: usize,
    path: Vec<Segment>,
}

impl<'a> Walk<'a> {
    fn next(&mut self) -> Result<Event<'a>, FrontMatterError> {
        self.events += 1;
        if self.events > MAX_EVENTS {
            return Err(error(&self.path, Reason::TooManyEvents));
        }
        match self.parser.next() {
            Some(Ok((event, _))) => Ok(event),
            Some(Err(scan)) => Err(self.syntax(&scan)),
            None => Ok(Event::StreamEnd),
        }
    }

    fn syntax(&self, scan: &ScanError) -> FrontMatterError {
        error(&self.path, Reason::Syntax(scan.info().to_owned()))
    }

    /// The whole stream: one document whose root is a mapping (an empty one when blank).
    fn document(mut self) -> Result<Map<String, Value>, FrontMatterError> {
        let mut root = Map::new();
        let mut documents = 0;
        loop {
            match self.next()? {
                Event::StreamStart | Event::Nothing => {}
                Event::StreamEnd => return Ok(root),
                Event::DocumentStart(_) => {
                    documents += 1;
                    if documents > 1 {
                        return Err(error(&[], Reason::MultipleDocuments));
                    }
                }
                Event::DocumentEnd => {}
                event @ Event::MappingStart(..) => match self.node(event, 0)? {
                    Value::Object(map) => root = map,
                    _ => unreachable!("a mapping start makes an object"),
                },
                _ => return Err(error(&[], Reason::NotAMapping)),
            }
        }
    }

    fn node(&mut self, event: Event<'a>, depth: usize) -> Result<Value, FrontMatterError> {
        match event {
            Event::Alias(_) => Err(error(&self.path, Reason::Alias)),
            Event::Scalar(text, style, anchor, tag) => {
                self.plain_node(anchor, tag.is_some())?;
                typed(&text, style).ok_or_else(|| error(&self.path, Reason::BadNumber))
            }
            Event::SequenceStart(anchor, tag) => {
                self.collection(anchor, tag.is_some(), depth)?;
                let mut items = Vec::new();
                loop {
                    match self.next()? {
                        Event::SequenceEnd => return Ok(Value::Array(items)),
                        event => {
                            self.path.push(Segment::Index(items.len()));
                            items.push(self.node(event, depth + 1)?);
                            self.path.pop();
                        }
                    }
                }
            }
            Event::MappingStart(anchor, tag) => {
                self.collection(anchor, tag.is_some(), depth)?;
                let mut map = Map::new();
                loop {
                    let key = match self.next()? {
                        Event::MappingEnd => return Ok(Value::Object(map)),
                        Event::Scalar(text, style, anchor, tag) => {
                            self.plain_node(anchor, tag.is_some())?;
                            match typed(&text, style) {
                                Some(Value::String(key)) => key,
                                _ => return Err(error(&self.path, Reason::NonStringKey)),
                            }
                        }
                        Event::Alias(_) => return Err(error(&self.path, Reason::Alias)),
                        _ => return Err(error(&self.path, Reason::NonStringKey)),
                    };
                    self.path.push(Segment::Key(key.clone()));
                    if key == "<<" {
                        return Err(error(&self.path, Reason::MergeKey));
                    }
                    if map.contains_key(&key) {
                        return Err(error(&self.path, Reason::DuplicateKey));
                    }
                    let event = self.next()?;
                    let value = self.node(event, depth + 1)?;
                    self.path.pop();
                    map.insert(key, value);
                }
            }
            _ => Err(error(&self.path, Reason::Syntax("unexpected event".into()))),
        }
    }

    fn plain_node(&self, anchor: usize, tagged: bool) -> Result<(), FrontMatterError> {
        match (anchor, tagged) {
            (0, false) => Ok(()),
            (0, true) => Err(error(&self.path, Reason::Tag)),
            _ => Err(error(&self.path, Reason::Anchor)),
        }
    }

    fn collection(
        &self,
        anchor: usize,
        tagged: bool,
        depth: usize,
    ) -> Result<(), FrontMatterError> {
        self.plain_node(anchor, tagged)?;
        if depth + 1 > MAX_DEPTH {
            return Err(error(&self.path, Reason::TooDeep));
        }
        Ok(())
    }
}

/// YAML 1.2 core schema. A quoted or block scalar is always a string; a plain one may be null,
/// a bool, an integer or a float. `None`: a number JSON can't hold (infinite, NaN, too large).
fn typed(text: &str, style: ScalarStyle) -> Option<Value> {
    if style != ScalarStyle::Plain {
        return Some(Value::String(text.to_owned()));
    }
    match text {
        "" | "~" | "null" | "Null" | "NULL" => return Some(Value::Null),
        "true" | "True" | "TRUE" => return Some(Value::Bool(true)),
        "false" | "False" | "FALSE" => return Some(Value::Bool(false)),
        _ => {}
    }
    let digits = |s: &str, radix: u32| !s.is_empty() && s.chars().all(|c| c.is_digit(radix));
    let unsigned = text.strip_prefix(['-', '+']).unwrap_or(text);
    if digits(unsigned, 10) {
        return text.parse::<i64>().ok().map(Value::from);
    }
    for (prefix, radix) in [("0o", 8), ("0x", 16)] {
        if let Some(rest) = text.strip_prefix(prefix).filter(|rest| digits(rest, radix)) {
            return i64::from_str_radix(rest, radix).ok().map(Value::from);
        }
    }
    if matches!(
        unsigned,
        ".inf" | ".Inf" | ".INF" | ".nan" | ".NaN" | ".NAN"
    ) {
        return None;
    }
    if is_core_float(unsigned) {
        return text
            .parse::<f64>()
            .ok()
            .and_then(Number::from_f64)
            .map(Value::Number);
    }
    Some(Value::String(text.to_owned()))
}

/// `( \. [0-9]+ | [0-9]+ ( \. [0-9]* )? ) ( [eE] [-+]? [0-9]+ )?`, the sign already removed.
fn is_core_float(s: &str) -> bool {
    let (mantissa, exponent) = match s.find(['e', 'E']) {
        Some(at) => (&s[..at], Some(&s[at + 1..])),
        None => (s, None),
    };
    let all_digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let mantissa_ok = match mantissa.split_once('.') {
        Some(("", fraction)) => all_digits(fraction),
        Some((whole, fraction)) => {
            all_digits(whole) && fraction.bytes().all(|b| b.is_ascii_digit())
        }
        None => all_digits(mantissa),
    };
    let exponent_ok = exponent.is_none_or(|e| all_digits(e.strip_prefix(['-', '+']).unwrap_or(e)));
    mantissa_ok && exponent_ok && (mantissa.contains('.') || exponent.is_some())
}

#[cfg(test)]
mod tests;
