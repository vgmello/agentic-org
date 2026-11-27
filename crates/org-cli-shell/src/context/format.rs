//! Shared `AGENT_OS_FORMAT` environment override, agent-harness PTY detection, and the
//! output-format precedence ladder (`TASK-212`):
//!
//! explicit CLI flags (`-q`, `--json`, `--format`) > `AGENT_OS_FORMAT` > agent-harness PTY /
//! non-TTY pipe > human TTY table.

use std::ffi::OsString;

use crate::terminal::Terminal;

use super::{forced, set, Modes};

/// Environment variable selecting the default output format across `agent-os` and `agentic-org`.
pub const OUTPUT_FORMAT_ENV: &str = "AGENT_OS_FORMAT";

/// Environment markers identifying a known AI agent harness. When one is active (non-empty and not
/// `"0"`) on a TTY and `AGENT_OS_FORMAT` is unset, `org-cli-shell` avoids truncated human tables
/// in favor of untruncated machine-readable JSON while preserving non-TTY piped TSV for shell
/// scripts.
pub const AGENT_HARNESS_ENV_VARS: &[&str] = &[
    "AGENT_OS_SESSION_TOKEN",
    "ANTIGRAVITY_AGENT",
    "CLAUDECODE",
    "CODEX_CI",
];

/// Effective output format for `agentic-org` (`org-cli-shell`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    Json,
    Compact,
    Yaml,
    Tsv,
    Table,
}

impl OutputFormat {
    /// Parses a format word (case-insensitive, surrounding whitespace ignored).
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "json" => Some(Self::Json),
            "compact" => Some(Self::Compact),
            "yaml" => Some(Self::Yaml),
            "tsv" => Some(Self::Tsv),
            "table" => Some(Self::Table),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Compact => "compact",
            Self::Yaml => "yaml",
            Self::Tsv => "tsv",
            Self::Table => "table",
        }
    }
}

/// Returns true when any known agent harness marker is set in `env` (non-empty and not `"0"`).
pub fn is_agent_harness(env: &dyn Fn(&str) -> Option<OsString>) -> bool {
    AGENT_HARNESS_ENV_VARS.iter().any(|name| forced(env, name))
}

/// Reads and validates `AGENT_OS_FORMAT` from `env`.
///
/// - Unset, empty, or whitespace-only -> `Ok(None)`.
/// - Valid format (`json|compact|yaml|tsv|table`, case-insensitive) -> `Ok(Some(format))`.
/// - Non-UTF-8 or unrecognized value -> `Err(display_value)`.
pub fn parse_env_format(
    env: &dyn Fn(&str) -> Option<OsString>,
) -> Result<Option<OutputFormat>, String> {
    let Some(raw) = set(env, OUTPUT_FORMAT_ENV) else {
        return Ok(None);
    };
    let Some(text) = raw.to_str() else {
        return Err(raw.to_string_lossy().into_owned());
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    OutputFormat::parse(trimmed)
        .map(Some)
        .ok_or_else(|| trimmed.to_owned())
}

/// Resolves the effective output format following the precedence ladder:
///
/// 1. Explicit CLI flags (`--json`, `--format`)
/// 2. `AGENT_OS_FORMAT` environment variable (valid value)
/// 3. Non-TTY pipe (`OutputFormat::Tsv`, preserving shell scripts) or agent-harness TTY
///    (`OutputFormat::Json`, avoiding truncated tables)
/// 4. Human TTY (`OutputFormat::Table`)
///
/// Note: `-q`/`--quiet` is handled separately by callers (IDs only) before format dispatch.
pub fn resolve_output_format(
    modes: &Modes,
    terminal: &Terminal,
    env: &dyn Fn(&str) -> Option<OsString>,
) -> OutputFormat {
    if modes.json {
        return OutputFormat::Json;
    }
    if let Some(format) = modes.format {
        return format;
    }
    if !modes.quiet && !modes.ignore_env_format {
        if let Ok(Some(format)) = parse_env_format(env) {
            return format;
        }
    }
    fallback_format(terminal, env)
}

fn fallback_format(terminal: &Terminal, env: &dyn Fn(&str) -> Option<OsString>) -> OutputFormat {
    if !terminal.stdout_tty {
        // Preserve existing non-TTY piped TSV behavior for shell scripts even inside an agent
        // harness environment.
        OutputFormat::Tsv
    } else if is_agent_harness(env) {
        OutputFormat::Json
    } else {
        OutputFormat::Table
    }
}
