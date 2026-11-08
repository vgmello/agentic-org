//! Validated paths: [`SpecPath`] (an absolute literal path), [`PathPattern`] (a path with
//! wildcards, for deny lists and rules) and [`RelPath`] (a path inside a plan's location).
//!
//! Each is checked when made (and the first two when decoded), so a value of any of them is
//! always valid, and all are UTF-8 by type, so the spec's canonical JSON is total.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};

use crate::InvalidValue;

/// Characters no path, pattern or command prefix may hold:
/// - controls (general category Cc: NUL, line breaks, tabs, escapes, DEL, C1);
/// - the line and paragraph separators U+2028 and U+2029 (some JSON-lines readers split on
///   them);
/// - every **format character** (general category Cf, as of Unicode 15.1): invisible
///   characters that change how text displays or compares, so a value could look like a
///   different one (e.g. in Needs you). They are the bidirectional controls (U+061C,
///   U+200E-U+200F, U+202A-U+202E, U+2066-U+2069), the zero-width and joiner characters
///   (U+200B-U+200D, U+2060, U+FEFF), the soft hyphen (U+00AD), the invisible operators and
///   other format marks (U+2061-U+2064, U+206A-U+206F, U+180E, U+FFF9-U+FFFB), the
///   Arabic, Syriac and Kaithi number marks (U+0600-U+0605, U+06DD, U+070F, U+0890-U+0891,
///   U+08E2, U+110BD, U+110CD), the Egyptian hieroglyph and shorthand format controls
///   (U+13430-U+1343F, U+1BCA0-U+1BCA3), the musical symbol format controls
///   (U+1D173-U+1D17A), and the tag characters (U+E0001, U+E0020-U+E007F).
///
/// The table is written out (no Unicode dependency): 170 code points in 21 ranges, unchanged
/// through Unicode 17.0. A newer Unicode's new Cf characters get added here.
pub(crate) fn is_unsafe_char(c: char) -> bool {
    c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') || is_format_char(c)
}

/// Unicode general category Cf (format), as of Unicode 15.1.
fn is_format_char(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{0600}'..='\u{0605}'
            | '\u{061C}'
            | '\u{06DD}'
            | '\u{070F}'
            | '\u{0890}'..='\u{0891}'
            | '\u{08E2}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{110BD}'
            | '\u{110CD}'
            | '\u{13430}'..='\u{1343F}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0001}'
            | '\u{E0020}'..='\u{E007F}'
    )
}

/// The checks every spec path and pattern share.
fn check_absolute(path: &str) -> Result<(), SpecPathError> {
    check_chars(path)?;
    let Some(rest) = path.strip_prefix('/') else {
        return Err(SpecPathError::NotAbsolute);
    };
    if rest.is_empty() {
        return Ok(());
    }
    check_components(rest)
}

fn check_chars(path: &str) -> Result<(), SpecPathError> {
    if path.contains('\0') {
        return Err(SpecPathError::ContainsNul);
    }
    if path.chars().any(is_unsafe_char) {
        return Err(SpecPathError::ControlChar);
    }
    Ok(())
}

/// `components`: what follows the leading `/` (or a whole relative path), not empty.
fn check_components(components: &str) -> Result<(), SpecPathError> {
    if components.ends_with('/') {
        return Err(SpecPathError::TrailingSlash);
    }
    for component in components.split('/') {
        match component {
            "" => return Err(SpecPathError::EmptyComponent),
            "." | ".." => return Err(SpecPathError::DotComponent),
            _ => {}
        }
    }
    Ok(())
}

/// A relative path inside one of a plan's locations ([`crate::PlannedFile::rel_path`],
/// [`crate::PlannedDir::rel_path`]).
///
/// Not empty, no leading or trailing `/`, no `.`, `..` or empty component, and the same
/// character rules as [`SpecPath`] (no NUL, control, line-separator or format characters). So
/// it can only name something inside its location; the supervisor still walks it without
/// following symlinks.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RelPath(String);

impl RelPath {
    /// The path as a string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The path as a `PathBuf`.
    pub fn to_path_buf(&self) -> PathBuf {
        PathBuf::from(&self.0)
    }
}

impl TryFrom<&str> for RelPath {
    type Error = InvalidValue;

    fn try_from(path: &str) -> Result<Self, Self::Error> {
        const WHAT: &str = "relative path";
        if path.is_empty() {
            return Err(InvalidValue::new(WHAT, "it's empty"));
        }
        if path.starts_with('/') {
            return Err(InvalidValue::new(WHAT, "it starts with `/`"));
        }
        check_chars(path)
            .and_then(|()| check_components(path))
            .map_err(|e| InvalidValue::new(WHAT, e.message()))?;
        Ok(RelPath(path.to_owned()))
    }
}

/// An absolute, normalized, UTF-8 path in a [`crate::SessionSpec`].
///
/// The spec is stored, hashed and passed to runtimes as text (often inside their JSON or TOML
/// config), so its paths are UTF-8 by type and kept to one plain spelling:
/// - absolute (starts with `/`);
/// - no `.` or `..` component, and no empty one (`//`);
/// - no trailing `/`, except the root itself;
/// - no NUL byte, no other control character, no U+2028 / U+2029, and no format character
///   (Unicode category Cf: bidirectional controls, zero-width characters, ...), so a path
///   can't break a runtime's config, a log line, or what a person sees.
///
/// That's one spelling per string, not per file: there's no case or Unicode normalization
/// (macOS file systems ignore both, so `/Work` and `/work` are different `SpecPath`s with
/// different hashes). The office supplies the real paths it recorded.
///
/// It's checked when made ([`SpecPath::new`], `TryFrom<&str>`) and when decoded, so a
/// `SpecPath` is always valid. It isn't canonicalized against the file system: symlinks are
/// the office's business (it records real paths).
///
/// A real path may hold characters that are syntax elsewhere (`*`, `?`, `[`, `(`, a space).
/// It's a literal path, so an adapter that writes one into a runtime's pattern or rule
/// syntax **escapes** those characters for that syntax.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SpecPath(String);

impl SpecPath {
    /// Checks `path` and wraps it.
    pub fn new(path: &Path) -> Result<Self, SpecPathError> {
        path.to_str()
            .ok_or(SpecPathError::NotUtf8)
            .and_then(Self::parse)
    }

    /// The path as a string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The path as a `PathBuf`, for building a plan.
    pub fn to_path_buf(&self) -> PathBuf {
        PathBuf::from(&self.0)
    }

    fn parse(path: &str) -> Result<Self, SpecPathError> {
        check_absolute(path)?;
        Ok(SpecPath(path.to_owned()))
    }
}

impl TryFrom<&str> for SpecPath {
    type Error = SpecPathError;

    fn try_from(path: &str) -> Result<Self, Self::Error> {
        Self::parse(path)
    }
}

impl<'de> Deserialize<'de> for SpecPath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let path = String::deserialize(deserializer)?;
        Self::parse(&path).map_err(serde::de::Error::custom)
    }
}

/// Why a path can't be a [`SpecPath`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SpecPathError {
    /// It isn't valid UTF-8.
    NotUtf8,
    /// It doesn't start with `/`.
    NotAbsolute,
    /// It has a `.` or `..` component.
    DotComponent,
    /// It has an empty component (`//`).
    EmptyComponent,
    /// It ends with `/` and isn't the root.
    TrailingSlash,
    /// It has a NUL byte.
    ContainsNul,
    /// It has a control character other than NUL (a line break, a tab, ...), a line or
    /// paragraph separator (U+2028, U+2029), or a format character (Unicode category Cf,
    /// e.g. a bidirectional control or a zero-width space).
    ControlChar,
}

impl SpecPathError {
    pub(crate) fn message(self) -> &'static str {
        match self {
            SpecPathError::NotUtf8 => "path isn't valid UTF-8",
            SpecPathError::NotAbsolute => "path isn't absolute",
            SpecPathError::DotComponent => "path has a `.` or `..` component",
            SpecPathError::EmptyComponent => "path has an empty component",
            SpecPathError::TrailingSlash => "path ends with `/`",
            SpecPathError::ContainsNul => "path has a NUL byte",
            SpecPathError::ControlChar => "path has a control, line-separator or format character",
        }
    }
}

impl fmt::Display for SpecPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for SpecPathError {}

/// An absolute path pattern, for the sandbox's deny lists and path rules.
///
/// It's a [`SpecPath`] (same rules: absolute, no `.`, `..` or empty components, no trailing
/// `/`, no control, line-separator or format characters) that may also use wildcards, **only as
/// whole segments**:
/// - `*` matches exactly one segment (any name);
/// - `**` matches any number of segments, none included.
///
/// Both match names that start with `.` too (`/work/**` covers `/work/.git/config`), so an
/// adapter whose runtime's globs skip dotfiles by default translates them to forms that
/// don't.
///
/// So `/office/offices/*/*.db` is refused (`*.db` isn't a whole segment), while
/// `/office/offices/*/office.db` and `/work/.git/hooks/**` are fine.
///
/// It also holds none of `( ) ? [ ] { } \`, no `~`, and no segment that ends in whitespace.
/// Runtimes write path rules as `Tool(pattern)` and read the rest as glob syntax (some, like
/// gitignore, also drop trailing spaces), so any of them could end a rule early or make a deny
/// miss the path it names; and the office writes home paths out in full. With those gone,
/// each adapter translates a pattern to its runtime's syntax without escaping.
///
/// A path that needs one of those characters can't be denied by pattern, so the office
/// **fails closed** for it: it refuses to build the spec rather than leave that deny out.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct PathPattern(String);

impl PathPattern {
    /// The pattern as a string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn parse(pattern: &str) -> Result<Self, InvalidValue> {
        const WHAT: &str = "path pattern";
        check_absolute(pattern).map_err(|e| InvalidValue::new(WHAT, e.message()))?;
        if pattern.contains('~') {
            return Err(InvalidValue::new(WHAT, "it holds a `~`"));
        }
        if pattern
            .split('/')
            .any(|segment| segment.ends_with(char::is_whitespace))
        {
            return Err(InvalidValue::new(WHAT, "a segment ends in whitespace"));
        }
        if pattern.contains(['(', ')', '?', '[', ']', '{', '}', '\\']) {
            return Err(InvalidValue::new(
                WHAT,
                "it holds rule or glob syntax: one of ( ) ? [ ] { } \\",
            ));
        }
        let wildcard_inside = pattern
            .split('/')
            .any(|segment| segment.contains('*') && segment != "*" && segment != "**");
        if wildcard_inside {
            return Err(InvalidValue::new(
                WHAT,
                "`*` and `**` may only be whole segments",
            ));
        }
        Ok(PathPattern(pattern.to_owned()))
    }
}

impl TryFrom<&str> for PathPattern {
    type Error = InvalidValue;

    fn try_from(pattern: &str) -> Result<Self, Self::Error> {
        Self::parse(pattern)
    }
}

impl<'de> Deserialize<'de> for PathPattern {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let pattern = String::deserialize(deserializer)?;
        Self::parse(&pattern).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::Value;

    /// The characters every path, pattern and command prefix refuses, beyond NUL.
    pub(crate) const UNSAFE: &[char] = &[
        '\n',
        '\r',
        '\t',
        '\u{1b}',
        '\u{7f}',
        '\u{85}',
        '\u{2028}',
        '\u{2029}',
        '\u{202A}',
        '\u{202B}',
        '\u{202C}',
        '\u{202D}',
        '\u{202E}',
        '\u{2066}',
        '\u{2067}',
        '\u{2068}',
        '\u{2069}',
        '\u{00AD}',
        '\u{0600}',
        '\u{061C}',
        '\u{200B}',
        '\u{200C}',
        '\u{200D}',
        '\u{200E}',
        '\u{200F}',
        '\u{2060}',
        '\u{2064}',
        '\u{206F}',
        '\u{FEFF}',
        '\u{FFF9}',
        '\u{110BD}',
        '\u{13430}',
        '\u{1343F}',
        '\u{1D173}',
        '\u{E0001}',
        '\u{E0041}',
        '\u{E007F}',
    ];

    /// Every range of the Cf table: (first, last).
    const FORMAT_RANGES: &[(u32, u32)] = &[
        (0x00AD, 0x00AD),
        (0x0600, 0x0605),
        (0x061C, 0x061C),
        (0x06DD, 0x06DD),
        (0x070F, 0x070F),
        (0x0890, 0x0891),
        (0x08E2, 0x08E2),
        (0x180E, 0x180E),
        (0x200B, 0x200F),
        (0x202A, 0x202E),
        (0x2060, 0x2064),
        (0x2066, 0x206F),
        (0xFEFF, 0xFEFF),
        (0xFFF9, 0xFFFB),
        (0x110BD, 0x110BD),
        (0x110CD, 0x110CD),
        (0x13430, 0x1343F),
        (0x1BCA0, 0x1BCA3),
        (0x1D173, 0x1D17A),
        (0xE0001, 0xE0001),
        (0xE0020, 0xE007F),
    ];

    #[test]
    fn the_format_table_is_exactly_cf() {
        // Every code point, once: 170 format characters, no more.
        let count = (0..=0x10FFFF)
            .filter_map(char::from_u32)
            .filter(|&c| is_format_char(c))
            .count();
        assert_eq!(count, 170);
        // Each range's ends are in, and the code points just outside are out (no two ranges
        // touch: U+2065, between two of them, is unassigned).
        let is_in = |cp: u32| char::from_u32(cp).is_some_and(is_format_char);
        for &(first, last) in FORMAT_RANGES {
            assert!(is_in(first) && is_in(last), "{first:04X}-{last:04X}");
            assert!(!is_in(first - 1), "{:04X}", first - 1);
            assert!(!is_in(last + 1), "{:04X}", last + 1);
        }
        assert_eq!(
            FORMAT_RANGES.iter().map(|(a, b)| b - a + 1).sum::<u32>(),
            170
        );
    }

    #[test]
    fn neighbours_of_the_refused_ranges_are_fine() {
        // Just outside each range (letters, marks, unassigned or private use), so the table's
        // edges are exactly where they should be.
        for c in [
            '\u{00AC}',
            '\u{00AE}',
            '\u{0606}',
            '\u{061B}',
            '\u{061D}',
            '\u{200A}',
            '\u{2010}',
            '\u{2027}',
            '\u{202F}',
            '\u{205F}',
            '\u{2065}',
            '\u{2070}',
            '\u{FEFE}',
            '\u{FFFC}',
            '\u{E0000}',
            '\u{E0080}',
            'é',
            '\u{1F600}',
        ] {
            assert!(!is_unsafe_char(c), "{:04X}", c as u32);
        }
    }

    #[test]
    fn spec_path_accepts_absolute_normalized_utf8_paths() {
        for ok in [
            "/",
            "/a",
            "/a/b.c",
            "/work/t-1/.git",
            "/a/...",
            "/a/b~",
            "/with space/é/\u{1F600}",
            // U+2065 is unassigned, not a format character.
            "/a\u{2065}b",
        ] {
            let path = SpecPath::try_from(ok).unwrap();
            assert_eq!(path.as_str(), ok);
            assert_eq!(path.to_path_buf(), PathBuf::from(ok));
            assert_eq!(SpecPath::new(Path::new(ok)).unwrap(), path);
        }
    }

    #[test]
    fn spec_path_rejects_everything_else() {
        let mut cases = vec![
            ("".to_owned(), SpecPathError::NotAbsolute),
            ("a/b".to_owned(), SpecPathError::NotAbsolute),
            ("./a".to_owned(), SpecPathError::NotAbsolute),
            ("~/a".to_owned(), SpecPathError::NotAbsolute),
            ("/a/".to_owned(), SpecPathError::TrailingSlash),
            ("//".to_owned(), SpecPathError::TrailingSlash),
            ("//a".to_owned(), SpecPathError::EmptyComponent),
            ("/a//b".to_owned(), SpecPathError::EmptyComponent),
            ("/a/./b".to_owned(), SpecPathError::DotComponent),
            ("/a/../b".to_owned(), SpecPathError::DotComponent),
            ("/..".to_owned(), SpecPathError::DotComponent),
            ("/a/.".to_owned(), SpecPathError::DotComponent),
            ("/a\0b".to_owned(), SpecPathError::ContainsNul),
        ];
        for &c in UNSAFE {
            cases.push((format!("/work/a{c}b"), SpecPathError::ControlChar));
        }
        for (bad, expected) in cases {
            let bad = bad.as_str();
            assert_eq!(SpecPath::try_from(bad), Err(expected), "{bad:?}");
            assert_eq!(SpecPath::new(Path::new(bad)), Err(expected), "{bad:?}");
            // Decoding checks it the same way.
            let err = serde_json::from_value::<SpecPath>(Value::String(bad.to_owned()))
                .unwrap_err()
                .to_string();
            assert_eq!(err, expected.to_string(), "{bad:?}");
        }
        assert!(serde_json::from_value::<SpecPath>(serde_json::json!(1)).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn spec_path_rejects_non_utf8() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let path = Path::new(OsStr::from_bytes(b"/work/\xff"));
        assert_eq!(SpecPath::new(path), Err(SpecPathError::NotUtf8));
    }

    #[test]
    fn spec_path_round_trips_as_a_plain_string() {
        let path = SpecPath::try_from("/work/t-1").unwrap();
        assert_eq!(
            serde_json::to_value(&path).unwrap(),
            Value::from("/work/t-1")
        );
        assert_eq!(
            serde_json::from_value::<SpecPath>(Value::from("/work/t-1")).unwrap(),
            path
        );
    }

    #[test]
    fn rel_paths() {
        for ok in [
            "a",
            "settings.json",
            ".agents/agents/ada.md",
            "a b/é",
            "a/..b/c.",
            "...",
        ] {
            let rel = RelPath::try_from(ok).unwrap();
            assert_eq!(rel.as_str(), ok);
            assert_eq!(rel.to_path_buf(), PathBuf::from(ok));
        }
        let mut bad: Vec<String> = [
            "", "/", "/a", "a/", "a//b", ".", "..", "./a", "a/.", "../a", "a/../b", "a/./b", "a\0b",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        for &c in UNSAFE {
            bad.push(format!("a{c}b"));
        }
        for bad in bad {
            assert!(RelPath::try_from(bad.as_str()).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn path_pattern_accepts_whole_segment_wildcards() {
        for ok in [
            "/",
            "/office/offices/*/office.db",
            "/work/.git/hooks/**",
            "/**",
            "/*",
            "/a/**/b/*",
            "/office/run/ada.sock",
            "/with space/a,b",
        ] {
            let pattern = PathPattern::try_from(ok).unwrap();
            assert_eq!(pattern.as_str(), ok);
            let json = serde_json::to_value(&pattern).unwrap();
            assert_eq!(json, Value::from(ok));
            assert_eq!(
                serde_json::from_value::<PathPattern>(json).unwrap(),
                pattern
            );
        }
    }

    #[test]
    fn path_pattern_rejects_everything_else() {
        let mut bad: Vec<String> = [
            "",
            "office/*",
            "~/.claude/**",
            "/home/~ada",
            "/office/offices/*/*.db",
            "/a/b*",
            "/a/***",
            "/a/*b/c",
            "/a/",
            "/a//b",
            "/a/../b",
            "/a/./b",
            "/a\0b",
            "/a/b)",
            "/a/(x",
            "/a/?",
            "/a/[ab]",
            "/work/[draft]",
            "/a/{x,y}",
            "/a/{",
            "/a/}",
            "/a/[",
            "/a/]",
            "/office/secret ",
            "/a /b",
            "/a/b\u{a0}",
            "/a/b\\",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        for &c in UNSAFE {
            bad.push(format!("/work/a{c}b/**"));
        }
        for bad in bad {
            assert!(PathPattern::try_from(bad.as_str()).is_err(), "{bad:?}");
            assert!(
                serde_json::from_value::<PathPattern>(Value::String(bad.clone())).is_err(),
                "{bad:?} decoded"
            );
        }
        assert_eq!(
            PathPattern::try_from("/a/b*").unwrap_err().to_string(),
            "invalid path pattern: `*` and `**` may only be whole segments"
        );
        assert_eq!(
            PathPattern::try_from("/a/../b").unwrap_err().to_string(),
            "invalid path pattern: path has a `.` or `..` component"
        );
    }
}
