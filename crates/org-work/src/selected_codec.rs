//! Lossless task-event JSON codec 2 (organization-data 05, ADR-0006).
//!
//! This is deliberately separate from legacy [`crate::canonical_json`]. Numbers
//! are lexical tokens, not `serde_json::Number` or floats. No I/O or authority is
//! provided here; supported event admission validates headers in addition to JSON.

use std::{collections::BTreeMap, fmt};

use sha2::{Digest, Sha256};

/// Recorded admission profile. Recovery must retain this decoder and its limits.
pub const RESOURCE_PROFILE: &str = "selected-task-resources/1";
pub const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_DEPTH: usize = 64;
pub const MAX_NODES: usize = 200_000;
pub const MAX_TOKEN_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_BLOB_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_BLOB_REFS: usize = 1_024;
pub const MAX_CHUNK_BYTES: usize = 256 * 1024;

/// Errors never contain source content, keys or number tokens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Syntax,
    InvalidUtf8,
    DuplicateKey,
    InputLimit,
    OutputLimit,
    DepthLimit,
    NodeLimit,
    TokenLimit,
    BlobLimit,
    NotAnObject,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "selected JSON: {self:?}")
    }
}

impl std::error::Error for Error {}

/// A valid JSON numeric token, retained without magnitude conversion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NumberToken(String);

impl NumberToken {
    pub fn new(token: &str) -> Result<Self, Error> {
        if token.len() > MAX_TOKEN_BYTES {
            return Err(Error::TokenLimit);
        }
        let mut parser = Reader::new(token);
        let number = parser.number()?;
        if parser.at != token.len() {
            return Err(Error::Syntax);
        }
        Ok(number)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Header integers require canonical positive decimal signed-i64 tokens.
    pub fn positive_i64(&self) -> Option<i64> {
        let text = self.as_str();
        if !text
            .as_bytes()
            .first()
            .is_some_and(|b| matches!(b, b'1'..=b'9'))
            || !text.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        text.parse().ok()
    }
}

/// Lossless JSON. Object ordering is Unicode scalar lexicographic order: Rust
/// UTF-8 string order agrees with scalar order, including supplementary scalars.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(NumberToken),
    String(String),
    Array(Vec<Json>),
    Object(BTreeMap<String, Json>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Object(map) => map.get(key),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn positive_i64(&self) -> Option<i64> {
        match self {
            Self::Number(n) => n.positive_i64(),
            _ => None,
        }
    }
}

/// Parse within the recorded byte/work budgets, checking keys before insertion.
pub fn parse(input: &[u8]) -> Result<Json, Error> {
    if input.len() > MAX_JSON_BYTES {
        return Err(Error::InputLimit);
    }
    let text = std::str::from_utf8(input).map_err(|_| Error::InvalidUtf8)?;
    let mut reader = Reader::new(text);
    let value = reader.value(0)?;
    reader.space();
    if reader.at != input.len() {
        return Err(Error::Syntax);
    }
    Ok(value)
}

/// Canonical UTF-8 without a trailing newline. Also checks constructed values.
pub fn canonical(value: &Json) -> Result<Vec<u8>, Error> {
    let mut writer = Writer::default();
    writer.value(value, 0, None)?;
    Ok(writer.bytes)
}

/// SHA-256 with only the designated top-level checksum member omitted. This
/// primitive does not establish supported schema, attribution or event coverage.
pub fn payload_sha256(value: &Json) -> Result<String, Error> {
    if !matches!(value, Json::Object(_)) {
        return Err(Error::NotAnObject);
    }
    let mut writer = Writer::default();
    writer.value(value, 0, Some("payload_sha256"))?;
    Ok(sha256(&writer.bytes))
}

pub fn sha256(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in hash {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 15)]));
    }
    out
}

/// Counts repeated references as well. Callers additionally verify actual bytes
/// and digests and make each blob durable before publishing the manifest.
pub fn check_blob_lengths(lengths: impl IntoIterator<Item = u64>) -> Result<(), Error> {
    let mut count = 0usize;
    let mut total = 0u64;
    for length in lengths {
        count = count.checked_add(1).ok_or(Error::BlobLimit)?;
        total = total.checked_add(length).ok_or(Error::BlobLimit)?;
        if count > MAX_BLOB_REFS || total > MAX_BLOB_BYTES || length > MAX_BLOB_BYTES {
            return Err(Error::BlobLimit);
        }
    }
    Ok(())
}

struct Reader<'a> {
    text: &'a str,
    at: usize,
    nodes: usize,
}

impl<'a> Reader<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            at: 0,
            nodes: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.at).copied()
    }

    fn space(&mut self) {
        while self
            .peek()
            .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
        {
            self.at += 1;
        }
    }

    fn take(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn node(&mut self) -> Result<(), Error> {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            Err(Error::NodeLimit)
        } else {
            Ok(())
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, Error> {
        self.space();
        self.node()?;
        match self.peek() {
            Some(b'{') => {
                if depth >= MAX_DEPTH {
                    return Err(Error::DepthLimit);
                }
                self.at += 1;
                self.space();
                let mut map = BTreeMap::new();
                if self.take(b'}') {
                    return Ok(Json::Object(map));
                }
                loop {
                    self.node()?;
                    let key = self.string()?;
                    if map.contains_key(&key) {
                        return Err(Error::DuplicateKey);
                    }
                    self.space();
                    if !self.take(b':') {
                        return Err(Error::Syntax);
                    }
                    let value = self.value(depth + 1)?;
                    map.insert(key, value);
                    self.space();
                    if self.take(b'}') {
                        break;
                    }
                    if !self.take(b',') {
                        return Err(Error::Syntax);
                    }
                    self.space();
                }
                Ok(Json::Object(map))
            }
            Some(b'[') => {
                if depth >= MAX_DEPTH {
                    return Err(Error::DepthLimit);
                }
                self.at += 1;
                self.space();
                let mut values = Vec::new();
                if self.take(b']') {
                    return Ok(Json::Array(values));
                }
                loop {
                    values.push(self.value(depth + 1)?);
                    self.space();
                    if self.take(b']') {
                        break;
                    }
                    if !self.take(b',') {
                        return Err(Error::Syntax);
                    }
                }
                Ok(Json::Array(values))
            }
            Some(b'"') => Ok(Json::String(self.string()?)),
            Some(b'-' | b'0'..=b'9') => Ok(Json::Number(self.number()?)),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            _ => Err(Error::Syntax),
        }
    }

    fn literal(&mut self, token: &str, value: Json) -> Result<Json, Error> {
        if !self.text[self.at..].starts_with(token) {
            return Err(Error::Syntax);
        }
        self.at += token.len();
        Ok(value)
    }

    fn number(&mut self) -> Result<NumberToken, Error> {
        let start = self.at;
        self.take(b'-');
        if !self.take(b'0') {
            if !self.peek().is_some_and(|b| matches!(b, b'1'..=b'9')) {
                return Err(Error::Syntax);
            }
            self.digits();
        }
        if self.take(b'.') {
            let before = self.at;
            self.digits();
            if before == self.at {
                return Err(Error::Syntax);
            }
        }
        if self.take(b'e') || self.take(b'E') {
            if !self.take(b'+') {
                self.take(b'-');
            }
            let before = self.at;
            self.digits();
            if before == self.at {
                return Err(Error::Syntax);
            }
        }
        let token = &self.text[start..self.at];
        if token.len() > MAX_TOKEN_BYTES {
            return Err(Error::TokenLimit);
        }
        Ok(NumberToken(token.to_owned()))
    }

    fn digits(&mut self) {
        while self.peek().is_some_and(|b| b.is_ascii_digit()) {
            self.at += 1;
        }
    }

    fn hex4(&mut self) -> Result<u32, Error> {
        let mut out = 0;
        for _ in 0..4 {
            let c = self.peek().ok_or(Error::Syntax)?;
            let n = match c {
                b'0'..=b'9' => u32::from(c - b'0'),
                b'a'..=b'f' => u32::from(c - b'a' + 10),
                b'A'..=b'F' => u32::from(c - b'A' + 10),
                _ => return Err(Error::Syntax),
            };
            out = out * 16 + n;
            self.at += 1;
        }
        Ok(out)
    }

    fn escaped(&mut self) -> Result<char, Error> {
        let byte = self.peek().ok_or(Error::Syntax)?;
        self.at += 1;
        Ok(match byte {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => {
                let first = self.hex4()?;
                let scalar = if (0xd800..=0xdbff).contains(&first) {
                    if !self.take(b'\\') || !self.take(b'u') {
                        return Err(Error::Syntax);
                    }
                    let second = self.hex4()?;
                    if !(0xdc00..=0xdfff).contains(&second) {
                        return Err(Error::Syntax);
                    }
                    0x10000 + ((first - 0xd800) << 10) + second - 0xdc00
                } else {
                    first
                };
                char::from_u32(scalar).ok_or(Error::Syntax)?
            }
            _ => return Err(Error::Syntax),
        })
    }

    fn string(&mut self) -> Result<String, Error> {
        if !self.take(b'"') {
            return Err(Error::Syntax);
        }
        let mut out = String::new();
        loop {
            if self.take(b'"') {
                return Ok(out);
            }
            let c = if self.take(b'\\') {
                self.escaped()?
            } else {
                let c = self.text[self.at..].chars().next().ok_or(Error::Syntax)?;
                if c < '\u{20}' {
                    return Err(Error::Syntax);
                }
                self.at += c.len_utf8();
                c
            };
            if out.len() + c.len_utf8() > MAX_TOKEN_BYTES {
                return Err(Error::TokenLimit);
            }
            out.push(c);
        }
    }
}

#[derive(Default)]
struct Writer {
    bytes: Vec<u8>,
    nodes: usize,
}

impl Writer {
    fn put(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if bytes.len() > MAX_JSON_BYTES - self.bytes.len() {
            return Err(Error::OutputLimit);
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn node(&mut self) -> Result<(), Error> {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            Err(Error::NodeLimit)
        } else {
            Ok(())
        }
    }

    fn string(&mut self, value: &str) -> Result<(), Error> {
        if value.len() > MAX_TOKEN_BYTES {
            return Err(Error::TokenLimit);
        }
        self.put(b"\"")?;
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for c in value.chars() {
            let escaped: Option<&[u8]> = match c {
                '"' => Some(b"\\\""),
                '\\' => Some(b"\\\\"),
                '\u{8}' => Some(b"\\b"),
                '\u{c}' => Some(b"\\f"),
                '\n' => Some(b"\\n"),
                '\r' => Some(b"\\r"),
                '\t' => Some(b"\\t"),
                _ => None,
            };
            if let Some(bytes) = escaped {
                self.put(bytes)?;
            } else if c < '\u{20}' {
                let n = c as usize;
                self.put(&[b'\\', b'u', b'0', b'0', HEX[n >> 4], HEX[n & 15]])?;
            } else {
                let mut bytes = [0; 4];
                self.put(c.encode_utf8(&mut bytes).as_bytes())?;
            }
        }
        self.put(b"\"")
    }

    fn value(&mut self, value: &Json, depth: usize, omit: Option<&str>) -> Result<(), Error> {
        self.node()?;
        match value {
            Json::Null => self.put(b"null"),
            Json::Bool(true) => self.put(b"true"),
            Json::Bool(false) => self.put(b"false"),
            Json::Number(n) => self.put(n.as_str().as_bytes()),
            Json::String(s) => self.string(s),
            Json::Array(items) => {
                if depth >= MAX_DEPTH {
                    return Err(Error::DepthLimit);
                }
                self.put(b"[")?;
                for (i, item) in items.iter().enumerate() {
                    if i != 0 {
                        self.put(b",")?;
                    }
                    self.value(item, depth + 1, None)?;
                }
                self.put(b"]")
            }
            Json::Object(map) => {
                if depth >= MAX_DEPTH {
                    return Err(Error::DepthLimit);
                }
                self.put(b"{")?;
                let mut first = true;
                for (key, value) in map {
                    if omit == Some(key.as_str()) {
                        continue;
                    }
                    self.node()?;
                    if !first {
                        self.put(b",")?;
                    }
                    first = false;
                    self.string(key)?;
                    self.put(b":")?;
                    self.value(value, depth + 1, None)?;
                }
                self.put(b"}")
            }
        }
    }
}

#[cfg(test)]
mod tests;
