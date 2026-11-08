//! The shared JSON-lines framer ([`LineFramer`]) and encoder ([`encode_line`]).
//!
//! Every runtime speaks JSON lines. One framer and one encoder for all of them means line
//! handling can't drift between adapters, and both are pure, so they're tested once here.

use serde::ser::Error as _;
use serde::Serialize;

/// The longest line, without its `\n`, the framer passes on. Longer lines are dropped and
/// reported as [`Frame::Truncated`].
pub const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

/// One frame out of [`LineFramer`].
///
/// **Exhaustive on purpose**, like [`crate::Output`]: the pump must handle every frame, so a
/// new variant is a breaking change that fails the pump's build until it's handled. Match it
/// without a wildcard arm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// A whole line, without its `\n`. It may be empty.
    Line(Vec<u8>),
    /// A line longer than [`MAX_LINE_BYTES`] was dropped. The value is its full length in
    /// bytes, without its `\n`.
    Truncated(usize),
}

/// Splits a byte stream into lines, with bounded memory.
///
/// - It splits on `\n` (byte `0x0A`) **only**. A `\r` before it stays at the end of the line
///   (JSON parsers treat it as whitespace), and U+2028 / U+2029 are ordinary bytes.
/// - It passes lines through as bytes: it doesn't check UTF-8 or JSON.
/// - A line longer than [`MAX_LINE_BYTES`] is dropped while it streams in: its buffer never
///   holds (or reserves) more than `MAX_LINE_BYTES`. When the line ends it's reported **once**, as
///   [`Frame::Truncated`] with the line's full length, and the stream goes on with the next
///   line.
/// - Frames come out in stream order, and are the same however the stream is chunked.
#[derive(Debug, Clone)]
pub struct LineFramer {
    /// The current line so far, while it fits.
    buf: Vec<u8>,
    /// The current line's length so far, once it's too long to keep.
    dropping: Option<usize>,
    cap: usize,
}

impl Default for LineFramer {
    fn default() -> Self {
        Self::new()
    }
}

impl LineFramer {
    /// A framer with the [`MAX_LINE_BYTES`] cap.
    pub fn new() -> Self {
        LineFramer {
            buf: Vec::new(),
            dropping: None,
            cap: MAX_LINE_BYTES,
        }
    }

    /// A framer with a small cap, so tests can exercise truncation on short input.
    #[cfg(test)]
    fn with_cap(cap: usize) -> Self {
        LineFramer { cap, ..Self::new() }
    }

    /// Feeds the next bytes of the stream in, and returns the frames they complete.
    pub fn push(&mut self, mut chunk: &[u8]) -> Vec<Frame> {
        let mut frames = Vec::new();
        while let Some(end) = chunk.iter().position(|&b| b == b'\n') {
            self.extend(&chunk[..end]);
            frames.push(self.take_frame());
            chunk = &chunk[end + 1..];
        }
        self.extend(chunk);
        frames
    }

    /// The stream ended: returns the unterminated last line, if any, and resets the framer.
    ///
    /// The last line comes out as [`Frame::Line`] like any other (a runtime that dies mid-line
    /// leaves a partial line, which then fails to parse); if it was already over the cap it
    /// comes out as [`Frame::Truncated`] with the length seen before the end. `None` if the
    /// stream ended right after a `\n`, or was empty.
    pub fn finish(&mut self) -> Option<Frame> {
        if self.dropping.is_none() && self.buf.is_empty() {
            return None;
        }
        Some(self.take_frame())
    }

    fn extend(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        match &mut self.dropping {
            Some(len) => *len = len.saturating_add(bytes.len()),
            None if self.buf.len() + bytes.len() > self.cap => {
                self.dropping = Some(self.buf.len() + bytes.len());
                // Let go of the memory, not just the contents.
                self.buf = Vec::new();
            }
            None => {
                // Grow geometrically, like `Vec`, but never past the cap, so the buffer's
                // capacity (not just its length) stays within it.
                let needed = self.buf.len() + bytes.len();
                if needed > self.buf.capacity() {
                    let target = needed.max(self.buf.capacity() * 2).min(self.cap);
                    self.buf.reserve_exact(target - self.buf.len());
                }
                self.buf.extend_from_slice(bytes);
            }
        }
    }

    fn take_frame(&mut self) -> Frame {
        match self.dropping.take() {
            Some(len) => Frame::Truncated(len),
            None => Frame::Line(std::mem::take(&mut self.buf)),
        }
    }
}

/// One encoded protocol line, ready to write: compact JSON, U+2028 and U+2029 escaped, no raw
/// line break inside, and exactly one `\n`, at the end.
///
/// **Only [`encode_line`] makes one**, so a session can't build a protocol line by formatting
/// strings (where a `\n` in a person's text would inject a second message). It's what
/// [`crate::Output::Write`] and [`crate::ProbePlan::stdin_lines`] carry.
///
/// It's for output only. The framer hands input lines over as plain bytes, because a line read
/// from a runtime is arbitrary and must never be written back out as one of these.
///
/// Nothing outside this crate can build one any other way:
///
/// ```compile_fail,E0423
/// let forged = org_runtime::Line(b"{\"type\":\"user\"}\n".to_vec());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Line(Vec<u8>);

impl Line {
    /// The bytes to write, `\n` included.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Encodes `value` as one line of JSON: compact `serde_json`, with U+2028 and U+2029
/// escaped, and a `\n` appended.
///
/// Compact JSON escapes control characters, so the line holds no raw `\n` or `\r` before its
/// end. U+2028 and U+2029 are legal raw in JSON strings, but some JavaScript line readers
/// split on them, so they're written as `\u2028` / `\u2029`. Always build runtime-bound lines
/// with this, never by formatting strings.
///
/// Fails if `value` can't be serialized, or if it would put a raw `\n` or `\r` inside the line
/// (only a pre-formatted `serde_json::value::RawValue` can).
pub fn encode_line<T: Serialize + ?Sized>(value: &T) -> Result<Line, serde_json::Error> {
    let json = serde_json::to_vec(value)?;
    let mut line = Vec::with_capacity(json.len() + 1);
    let mut i = 0;
    while i < json.len() {
        match json[i..] {
            // U+2028 and U+2029 in UTF-8. In compact JSON they can only occur inside strings.
            [0xE2, 0x80, 0xA8, ..] => {
                line.extend_from_slice(br"\u2028");
                i += 3;
            }
            [0xE2, 0x80, 0xA9, ..] => {
                line.extend_from_slice(br"\u2029");
                i += 3;
            }
            [b'\n' | b'\r', ..] => {
                return Err(serde_json::Error::custom(
                    "encoded JSON holds a raw line break",
                ))
            }
            [byte, ..] => {
                line.push(byte);
                i += 1;
            }
            [] => unreachable!("i < json.len()"),
        }
    }
    line.push(b'\n');
    Ok(Line(line))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn line(s: &str) -> Frame {
        Frame::Line(s.as_bytes().to_vec())
    }

    fn all_frames(framer: &mut LineFramer, chunks: &[&[u8]]) -> Vec<Frame> {
        let mut frames: Vec<Frame> = chunks.iter().flat_map(|c| framer.push(c)).collect();
        frames.extend(framer.finish());
        frames
    }

    #[test]
    fn splits_on_newline_only() {
        let mut f = LineFramer::new();
        let frames = f.push("a\r\nb\rc\u{2028}d\u{2029}e\n\n{\"x\":1}\n".as_bytes());
        assert_eq!(
            frames,
            vec![
                // \r\n leaves the \r on the line.
                line("a\r"),
                // \r, U+2028 and U+2029 don't split.
                line("b\rc\u{2028}d\u{2029}e"),
                // An empty line is a frame.
                line(""),
                line("{\"x\":1}"),
            ]
        );
        assert_eq!(f.finish(), None);
    }

    #[test]
    fn a_line_split_across_pushes() {
        let mut f = LineFramer::new();
        assert_eq!(f.push(b"{\"a\":"), vec![]);
        assert_eq!(f.push(b""), vec![]);
        assert_eq!(f.push(b"1}"), vec![]);
        assert_eq!(f.push(b"\n{\"b\""), vec![line("{\"a\":1}")]);
        assert_eq!(f.push(b":2}\n"), vec![line("{\"b\":2}")]);
        assert_eq!(f.finish(), None);
    }

    #[test]
    fn exactly_the_cap_passes_and_one_more_is_truncated_once() {
        let mut f = LineFramer::new();
        let mut exact = vec![b'x'; MAX_LINE_BYTES];
        exact.push(b'\n');
        let frames = f.push(&exact);
        assert_eq!(frames.len(), 1);
        assert!(matches!(&frames[0], Frame::Line(l) if l.len() == MAX_LINE_BYTES));

        let mut over = vec![b'y'; MAX_LINE_BYTES + 1];
        over.extend_from_slice(b"\nnext\n");
        assert_eq!(
            f.push(&over),
            vec![Frame::Truncated(MAX_LINE_BYTES + 1), line("next")]
        );
        assert_eq!(f.finish(), None);
    }

    #[test]
    fn an_oversized_line_is_drained_with_bounded_memory() {
        let mut f = LineFramer::new();
        let chunk = vec![b'z'; 1024 * 1024];
        // 12 MiB of one line, in 1 MiB chunks: reported once, at its end, with its length.
        for _ in 0..12 {
            assert_eq!(f.push(&chunk), vec![]);
            assert!(f.buf.len() <= MAX_LINE_BYTES);
            assert!(f.buf.capacity() <= MAX_LINE_BYTES);
        }
        assert!(f.dropping.is_some());
        assert_eq!(
            f.buf.capacity(),
            0,
            "the dropped line's memory was released"
        );
        assert_eq!(
            f.push(b"tail\n{}\n"),
            vec![Frame::Truncated(12 * 1024 * 1024 + 4), line("{}")]
        );
        assert_eq!(f.finish(), None);
    }

    #[test]
    fn the_buffer_never_reserves_past_the_cap() {
        // Uneven chunks that end exactly at the cap: plain `Vec` growth would reserve 6 MiB.
        let mut f = LineFramer::new();
        assert_eq!(f.push(&vec![b'a'; 3 * 1024 * 1024]), vec![]);
        assert_eq!(f.push(&vec![b'a'; 1024 * 1024]), vec![]);
        assert_eq!(f.buf.len(), MAX_LINE_BYTES);
        assert!(f.buf.capacity() <= MAX_LINE_BYTES, "{}", f.buf.capacity());
        // Many small pushes of a size that plain doubling wouldn't land on the cap with
        // (it would reserve 6,144,000 bytes), ending with a partial chunk right at the cap.
        let mut f = LineFramer::new();
        let chunk = [b'b'; 3000];
        let mut len = 0;
        while len < MAX_LINE_BYTES {
            let n = chunk.len().min(MAX_LINE_BYTES - len);
            assert_eq!(f.push(&chunk[..n]), vec![]);
            len += n;
            assert!(f.buf.capacity() <= MAX_LINE_BYTES, "{}", f.buf.capacity());
        }
        assert_eq!(f.push(b"\n"), vec![Frame::Line(vec![b'b'; MAX_LINE_BYTES])]);
    }

    #[test]
    fn finish_returns_the_unterminated_last_line() {
        let mut f = LineFramer::new();
        assert_eq!(f.finish(), None);
        assert_eq!(f.push(b"one\ntwo"), vec![line("one")]);
        assert_eq!(f.finish(), Some(line("two")));
        // finish resets the framer.
        assert_eq!(f.finish(), None);
        assert_eq!(f.push(b"three\n"), vec![line("three")]);

        // An oversized unterminated line at EOF is Truncated with the length seen.
        let mut f = LineFramer::with_cap(4);
        assert_eq!(f.push(b"abcdefg"), vec![]);
        assert_eq!(f.finish(), Some(Frame::Truncated(7)));
        assert_eq!(f.finish(), None);
    }

    #[test]
    fn frames_do_not_depend_on_chunking() {
        // A stream with every case: CRLF, empty lines, U+2028, lines at, under and over a
        // small cap, an oversized line right at the start, and an unterminated tail.
        let stream: &[u8] =
            "toolongline\nabcd\nabcde\n\r\n\n{\"a\":\"\u{2028}\"}\nxy\nabcdefghij\nlast".as_bytes();
        let cap = 8;
        let expected = all_frames(&mut LineFramer::with_cap(cap), &[stream]);
        assert_eq!(
            expected,
            vec![
                Frame::Truncated(11),
                line("abcd"),
                line("abcde"),
                line("\r"),
                line(""),
                // `{"a":"` + 3 bytes of U+2028 + `"}`.
                Frame::Truncated(11),
                line("xy"),
                Frame::Truncated(10),
                line("last"),
            ]
        );
        // Every two-way split.
        for i in 0..=stream.len() {
            let (a, b) = stream.split_at(i);
            assert_eq!(
                all_frames(&mut LineFramer::with_cap(cap), &[a, b]),
                expected,
                "split at {i}"
            );
        }
        // Every three-way split.
        for i in 0..=stream.len() {
            for j in i..=stream.len() {
                let chunks = [&stream[..i], &stream[i..j], &stream[j..]];
                assert_eq!(
                    all_frames(&mut LineFramer::with_cap(cap), &chunks),
                    expected,
                    "split at {i}, {j}"
                );
            }
        }
        // One byte at a time.
        let bytes: Vec<&[u8]> = stream.chunks(1).collect();
        assert_eq!(all_frames(&mut LineFramer::with_cap(cap), &bytes), expected);
    }

    #[test]
    fn encode_line_escapes_line_separators_and_ends_with_newline() {
        // A struct (fields in declaration order) and one-key maps, so the golden bytes don't
        // depend on serde_json's map order (`preserve_order` or not).
        #[derive(Serialize)]
        struct Message {
            text: &'static str,
            meta: Value,
        }
        let v = Message {
            text: "a\u{2028}b\u{2029}c\nd\re",
            meta: json!({"k\u{2028}": 1}),
        };
        let line = encode_line(&v).unwrap();
        let line = line.as_bytes();
        assert_eq!(line.last(), Some(&b'\n'));
        let body = &line[..line.len() - 1];
        assert!(!body.contains(&b'\n'));
        assert!(!body.contains(&b'\r'));
        let text = std::str::from_utf8(body).unwrap();
        assert!(!text.contains('\u{2028}') && !text.contains('\u{2029}'));
        assert_eq!(
            text,
            r#"{"text":"a\u2028b\u2029c\nd\re","meta":{"k\u2028":1}}"#
        );
        // It decodes back to the same value.
        assert_eq!(
            serde_json::from_slice::<Value>(body).unwrap(),
            serde_json::to_value(&v).unwrap()
        );
    }

    #[test]
    fn encode_line_round_trips_awkward_strings() {
        let samples = [
            String::new(),
            "plain".to_owned(),
            "\n\r\t\u{0}\u{1f}\u{7f}".to_owned(),
            "\u{2028}\u{2029}\u{2028}".to_owned(),
            "e\u{301} \u{1F600} \u{FEFF} \u{85}".to_owned(),
            "\\u2028 already escaped".to_owned(),
            "\"quoted\" \\ back".to_owned(),
            // Every code point in a few interesting blocks, as one string.
            (0u32..0x3000)
                .filter_map(char::from_u32)
                .collect::<String>(),
        ];
        for s in samples {
            let v = json!({"type": "user", "message": {"content": s}, "list": [s, s]});
            let line = encode_line(&v).unwrap();
            let line = line.as_bytes();
            let body = &line[..line.len() - 1];
            assert_eq!(line.last(), Some(&b'\n'));
            assert!(!body.contains(&b'\n') && !body.contains(&b'\r'));
            assert!(!std::str::from_utf8(body)
                .unwrap()
                .contains(['\u{2028}', '\u{2029}']));
            assert_eq!(serde_json::from_slice::<Value>(body).unwrap(), v);
            // And the framer gives the line back whole.
            let mut f = LineFramer::new();
            assert_eq!(f.push(line), vec![Frame::Line(body.to_vec())]);
        }
    }

    #[test]
    fn encode_line_reports_serialize_errors() {
        use std::collections::BTreeMap;
        // Non-string map keys can't be JSON.
        let mut m = BTreeMap::new();
        m.insert(vec![1u8], 1);
        assert!(encode_line(&m).is_err());
    }
}
