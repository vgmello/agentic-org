//! Validated names and tokens: values that end up in argv, config files or environment
//! names, so they're restricted to characters that can't break out of any of them.
//!
//! Each is checked when made (`TryFrom<&str>`) and when decoded, so a value of these types is
//! always valid. Each serializes as a plain string.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

/// Characters no path, pattern or command prefix may hold:
/// - controls (general category Cc: NUL, line breaks, tabs, escapes, DEL, C1);
/// - the line and paragraph separators U+2028 and U+2029 (some JSON-lines readers split on
///   them);
/// - every **format character** (general category Cf, as of Unicode 17.0): invisible
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
/// The table is written out (no Unicode dependency): 170 code points in 21 ranges, as of
/// Unicode 17.0. A newer Unicode's new Cf characters get added here.
pub(crate) fn is_unsafe_char(c: char) -> bool {
    c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') || is_format_char(c)
}

/// Unicode general category Cf (format), as of Unicode 17.0.
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

/// Why a value can't be one of this crate's validated names, tokens or patterns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidValue {
    what: &'static str,
    rule: &'static str,
}

impl InvalidValue {
    pub(crate) fn new(what: &'static str, rule: &'static str) -> Self {
        InvalidValue { what, rule }
    }
}

impl fmt::Display for InvalidValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid {}: {}", self.what, self.rule)
    }
}

impl std::error::Error for InvalidValue {}

/// Defines a validated string newtype: private field, `TryFrom<&str>`, `as_str`, and serde as
/// a plain string that's checked on decode.
macro_rules! validated_string {
    (
        $(#[$doc:meta])*
        $name:ident, what: $what:literal, rule: $rule:literal, check: $check:expr
    ) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// The value as a string.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            fn parse(value: &str) -> Result<Self, InvalidValue> {
                let check: fn(&str) -> bool = $check;
                if check(value) {
                    Ok($name(value.to_owned()))
                } else {
                    Err(InvalidValue::new($what, $rule))
                }
            }
        }

        impl TryFrom<&str> for $name {
            type Error = InvalidValue;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::parse(value)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = String::deserialize(deserializer)?;
                Self::parse(&value).map_err(serde::de::Error::custom)
            }
        }
    };
}

/// Whether `value` is a token: `[A-Za-z0-9._:\[\]-]{1,128}`, not starting with `-`.
fn is_token(value: &str) -> bool {
    !value.starts_with('-')
        && chars_within(value, 128, |c| {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '[' | ']' | '-')
        })
}

/// Whether `value` is a tool name: `[A-Za-z0-9_.:-]{1,128}`, not starting with `-`.
fn is_tool_name(value: &str) -> bool {
    !value.starts_with('-')
        && chars_within(value, 128, |c| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-')
        })
}

/// Whether `value` is 1 to `max` characters, each matching `allowed`.
fn chars_within(value: &str, max: usize, allowed: fn(char) -> bool) -> bool {
    !value.is_empty() && value.chars().count() <= max && value.chars().all(allowed)
}

validated_string! {
    /// An environment variable's name: `[A-Za-z_][A-Za-z0-9_]*`.
    ///
    /// Used wherever the port names a variable ([`crate::LaunchContext::office_env_names`],
    /// [`crate::LaunchPlan::env_add`], [`crate::LaunchPlan::secret_env_names`]), so a name can't
    /// carry `=`, a NUL, or anything a runtime's config would read differently.
    EnvName,
    what: "environment variable name",
    rule: "it must match [A-Za-z_][A-Za-z0-9_]*",
    check: |v| {
        let mut chars = v.chars();
        matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
            && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
    }
}

validated_string! {
    /// An agent's runtime-safe name: `[a-z0-9][a-z0-9-]{0,63}`.
    ///
    /// Runtimes select the agent by it (Claude `--agent`, Antigravity's agent file name), so it
    /// goes into argv and file names as is.
    AgentName,
    what: "agent name",
    rule: "it must match [a-z0-9][a-z0-9-]{0,63}",
    check: |v| {
        let lower_or_digit = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
        matches!(v.chars().next(), Some(c) if lower_or_digit(c))
            && chars_within(v, 64, |c| c == '-' || c.is_ascii_lowercase() || c.is_ascii_digit())
    }
}

validated_string! {
    /// A short runtime setting, such as a model or a reasoning effort, in the runtime's own
    /// naming: `[A-Za-z0-9._:\[\]-]{1,128}` (e.g. `opus`, `claude-opus-4[1m]`, `high`).
    ///
    /// It goes into argv and protocol messages, so it can't start with `-` either: as the value
    /// after a flag (`--model <token>`) it could otherwise read as another flag. In a typed
    /// format an adapter still quotes it as a string (in a Codex `-c key="value"`, `true` or
    /// `1.5` are valid tokens that TOML would read as a bool or a number).
    Token,
    what: "token",
    rule: "it must match [A-Za-z0-9._:\\[\\]-]{1,128} and not start with `-`",
    check: is_token
}

validated_string! {
    /// Which runtime, e.g. `claude`, `codex`, `agy`: a [`Token`] by rule. Each adapter reports
    /// its own through [`crate::AgentRuntime::id`], and the office picks the adapter whose id
    /// matches [`crate::SessionSpec::runtime`].
    RuntimeId,
    what: "runtime id",
    rule: "it must match [A-Za-z0-9._:\\[\\]-]{1,128} and not start with `-`",
    check: is_token
}

impl std::fmt::Display for RuntimeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

validated_string! {
    /// A runtime's id for a conversation (Claude's session uuid, Codex's thread id, an
    /// Antigravity conversation id): `[A-Za-z0-9._:-]{1,128}`, not starting with `-`.
    ///
    /// It reaches argv (`--resume`, `--session-id`) and protocol messages. It comes from the
    /// runtime, so an adapter that reads one it can't wrap reports a
    /// [`crate::SessionEvent::ProtocolError`] instead of using it. It isn't all dots, so it's
    /// never `.` or `..` if an adapter ever names a file after one.
    SessionId,
    what: "session id",
    rule: "it must match [A-Za-z0-9._:-]{1,128}, not start with `-`, and not be all dots",
    check: |v| {
        !v.starts_with('-')
            && !v.chars().all(|c| c == '.')
            && chars_within(v, 128, |c| {
                c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-')
            })
    }
}

validated_string! {
    /// An id the office made (a task's, a one-off run's): 1 to 64 ASCII letters and digits,
    /// the same plain-id rule as `org-proto`'s office ids (a ULID today; don't parse it).
    PlainId,
    what: "plain id",
    rule: "it must be 1 to 64 ASCII letters and digits",
    check: |v| chars_within(v, 64, |c| c.is_ascii_alphanumeric())
}

validated_string! {
    /// A tool's name, in the runtime's naming: `[A-Za-z0-9_.:-]{1,128}` (e.g. `Bash`, `Read`,
    /// `mcp__office__send`).
    ///
    /// It goes into argv and runtime config as is, so it can't carry a rule's own syntax (no
    /// `(`, `)`, `*` or spaces), and can't start with `-` (it could read as a flag).
    ToolName,
    what: "tool name",
    rule: "it must match [A-Za-z0-9_.:-]{1,128} and not start with `-`",
    check: is_tool_name
}

validated_string! {
    /// The tool a [`crate::Rule`] covers: a [`ToolName`], or `*` for any tool.
    RuleTool,
    what: "rule tool",
    rule: "it must be `*` or a tool name ([A-Za-z0-9_.:-]{1,128}, not starting with `-`)",
    check: |v| v == "*" || is_tool_name(v)
}

validated_string! {
    /// An MCP server's name: `[A-Za-z0-9_-]{1,64}`.
    ///
    /// Runtimes key their MCP config by it and build tool names from it. It doesn't start
    /// with `-`, like the other names that can reach argv.
    McpServerName,
    what: "MCP server name",
    rule: "it must match [A-Za-z0-9_-]{1,64} and not start with `-`",
    check: |v| {
        !v.starts_with('-')
            && chars_within(v, 64, |c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
    }
}

validated_string! {
    /// The start of a shell command a rule covers (e.g. `git `, `cargo test`). It's matched
    /// as a literal prefix; the adapter adds its runtime's own "anything after" syntax.
    ///
    /// At most 1024 characters. It starts with a character that's neither whitespace nor
    /// `-` (so it names a command, and a blank prefix can't match every command). It holds no
    /// `(` or `)` (runtimes write rules as `Tool(prefix...)`, so a parenthesis could end the
    /// rule early), no `*` or `\` (wildcard and escape syntax in rule formats), no control
    /// character, no line or paragraph separator and no format character (Unicode Cf).
    CommandPrefix,
    what: "command prefix",
    rule: "it must be 1-1024 characters, start with neither whitespace nor `-`, and hold none \
           of ( ) * \\ or control, line-separator or format characters",
    check: |v| {
        matches!(v.chars().next(), Some(c) if !c.is_whitespace() && c != '-')
            && v.chars().count() <= 1024
            && !v
                .chars()
                .any(|c| matches!(c, '(' | ')' | '*' | '\\') || is_unsafe_char(c))
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

    /// Checks `T` accepts every `ok` and refuses every `bad`, directly and when decoded.
    fn check<T>(ok: &[&str], bad: &[String])
    where
        T: for<'a> TryFrom<&'a str, Error = InvalidValue>
            + Serialize
            + serde::de::DeserializeOwned
            + PartialEq
            + std::fmt::Debug,
    {
        for &v in ok {
            let value = T::try_from(v).unwrap_or_else(|e| panic!("{v:?}: {e}"));
            let json = serde_json::to_value(&value).unwrap();
            assert_eq!(json, Value::from(v));
            assert_eq!(serde_json::from_value::<T>(json).unwrap(), value);
        }
        for v in bad {
            assert!(T::try_from(v.as_str()).is_err(), "{v:?} accepted");
            assert!(
                serde_json::from_value::<T>(Value::from(v.as_str())).is_err(),
                "{v:?} decoded"
            );
        }
    }

    fn owned(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn env_names() {
        check::<EnvName>(
            &[
                "ORG_SESSION_TOKEN",
                "_",
                "a",
                "A1_b2",
                "CLAUDE_CODE_MESSAGING_TOKEN",
            ],
            &owned(&["", "1A", "A-B", "A=B", "A B", "A\0", "É", "A.B"]),
        );
        assert_eq!(
            EnvName::try_from("A=B").unwrap_err().to_string(),
            "invalid environment variable name: it must match [A-Za-z_][A-Za-z0-9_]*"
        );
    }

    #[test]
    fn agent_names() {
        let longest = "a".repeat(64);
        check::<AgentName>(
            &["ada", "ac-dev-ada", "0", "a-", &longest],
            &[
                owned(&[
                    "", "-ada", "Ada", "aDa", "ada_x", "ada x", "ada.md", "../x", "é",
                ]),
                vec!["a".repeat(65)],
            ]
            .concat(),
        );
    }

    #[test]
    fn tokens() {
        let longest = "x".repeat(128);
        check::<Token>(
            &[
                "opus",
                "claude-opus-4[1m]",
                "gpt-5.1:high",
                "high",
                "a_b",
                &longest,
            ],
            &[
                owned(&[
                    "", "op us", "a/b", "--model", "-x", "a;b", "a\nb", "a=b", "é",
                ]),
                vec!["x".repeat(129)],
            ]
            .concat(),
        );
    }

    #[test]
    fn tool_names() {
        // The length limit, both sides of it.
        assert!(ToolName::try_from("t".repeat(128).as_str()).is_ok());
        assert!(ToolName::try_from("t".repeat(129).as_str()).is_err());
        check::<ToolName>(
            &[
                "Bash",
                "Read",
                "mcp__office__send",
                "run_shell_command",
                "a.b:c-d",
            ],
            &owned(&[
                "",
                "Bash(git *)",
                "*",
                "Bash ",
                "a/b",
                "a\nb",
                "a,b",
                "-x",
                "--tools",
            ]),
        );
    }

    #[test]
    fn runtime_ids() {
        check::<RuntimeId>(
            &["claude", "codex", "agy"],
            &owned(&["", "-x", "a b", "a/b"]),
        );
        assert_eq!(RuntimeId::try_from("agy").unwrap().to_string(), "agy");
    }

    #[test]
    fn session_ids() {
        let longest = "s".repeat(128);
        check::<SessionId>(
            &[
                "0b3f0c6e-2f7e-4f5a-9d1c-6a1b2c3d4e5f",
                "thr_01J9Z",
                "a.b:c",
                &longest,
            ],
            &[
                owned(&[
                    "", "-x", "--resume", "a b", "a/b", "a=b", "a\nb", "a[1]", ".", "..", "...",
                ]),
                vec!["s".repeat(129)],
            ]
            .concat(),
        );
    }

    #[test]
    fn plain_ids() {
        let longest = "A".repeat(64);
        check::<PlainId>(
            &["01J9ZK3Y8Q2W4E6R8T0Y2U4I6O", "t1", "a", &longest],
            &[
                owned(&["", "t-1", "a_b", "a.b", "-1", "é"]),
                vec!["A".repeat(65)],
            ]
            .concat(),
        );
    }

    #[test]
    fn rule_tools() {
        check::<RuleTool>(
            &["*", "Bash", "mcp__office__send"],
            &owned(&["", "**", "Bash(x)", "Bash) Read(/etc", "a\nb", "-x", "* "]),
        );
    }

    #[test]
    fn mcp_server_names() {
        check::<McpServerName>(
            &["office", "agent-os", "a_b", "A1"],
            &[
                owned(&["", "my server", "a.b", "a/b", "a:b", "a\"b", "-x", "--help"]),
                vec!["s".repeat(65)],
            ]
            .concat(),
        );
    }

    #[test]
    fn command_prefixes() {
        let mut bad = owned(&[
            "",
            " ",
            "   ",
            " git",
            "\u{a0}git",
            "-rf",
            "git (",
            "rm )",
            "Bash(x)",
            "git *",
            "*",
            "echo \\",
            "a\\b",
        ]);
        bad.push("g".repeat(1025));
        for &c in UNSAFE {
            bad.push(format!("git{c}status"));
        }
        bad.push("a\0".to_owned());
        let longest = "g".repeat(1024);
        check::<CommandPrefix>(
            &[
                "git ",
                "cargo test",
                "ls -la",
                "npm run",
                "é",
                "a-b",
                &longest,
            ],
            &bad,
        );
    }
}
