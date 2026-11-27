//! Shared catalog and DTOs for supported local CLI agent runtimes (`TASK-271`).
//!
//! Pure data definitions with zero I/O so host setup, onboarding, CLI diagnostics, and session
//! orchestration share one canonical Top-6 runtime inventory (`claude`, `codex`, `agy`, `gemini`,
//! `kimi`, `pi`).

use serde::{Deserialize, Serialize};

/// Static metadata for one supported local agent CLI runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownCliRuntime {
    /// Canonical runtime driver identifier (e.g. `"claude"`, `"agy"`, `"gemini"`).
    pub id: &'static str,
    /// Human-readable display name.
    pub display_name: &'static str,
    /// Executable binary name probed on `PATH` and standard user-local bin folders.
    pub binary_name: &'static str,
    /// Wire protocol family spoken by the runtime (`"stream_json"`, `"app_server"`, `"acp"`).
    pub protocol: &'static str,
    /// Default provider type used when registering a connection for this runtime.
    pub provider_type: &'static str,
    /// Alternate aliases matched ASCII case-insensitively.
    pub aliases: &'static [&'static str],
}

/// Canonical Top-6 CLI runtimes supported by Agentic Org onboarding and auto-detection.
pub const KNOWN_CLI_RUNTIMES: &[KnownCliRuntime] = &[
    KnownCliRuntime {
        id: "claude",
        display_name: "Claude Code",
        binary_name: "claude",
        protocol: "stream_json",
        provider_type: "anthropic",
        aliases: &["anthropic", "claude-code"],
    },
    KnownCliRuntime {
        id: "codex",
        display_name: "Codex CLI",
        binary_name: "codex",
        protocol: "app_server",
        provider_type: "openai",
        aliases: &["openai", "gpt"],
    },
    KnownCliRuntime {
        id: "agy",
        display_name: "Antigravity CLI",
        binary_name: "agy",
        protocol: "acp",
        provider_type: "google",
        aliases: &["google", "antigravity", "antigravity-cli"],
    },
    KnownCliRuntime {
        id: "gemini",
        display_name: "Gemini CLI",
        binary_name: "gemini",
        protocol: "acp",
        provider_type: "google",
        aliases: &["gemini-cli"],
    },
    KnownCliRuntime {
        id: "kimi",
        display_name: "Kimi Code CLI",
        binary_name: "kimi",
        protocol: "acp",
        provider_type: "moonshot",
        aliases: &["kimi-cli", "kimi-code", "moonshot"],
    },
    KnownCliRuntime {
        id: "pi",
        display_name: "Pi ACP",
        binary_name: "pi-acp",
        protocol: "acp",
        provider_type: "pi",
        aliases: &["pi-acp", "pi-coding-agent"],
    },
];

/// Resolves a runtime identifier or alias against [`KNOWN_CLI_RUNTIMES`] (ASCII case-insensitive).
///
/// Canonical IDs always win over aliases; unknown names return `None`.
pub fn known_cli_runtime(name: &str) -> Option<&'static KnownCliRuntime> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return None;
    }
    KNOWN_CLI_RUNTIMES
        .iter()
        .find(|rt| rt.id.eq_ignore_ascii_case(trimmed))
        .or_else(|| {
            KNOWN_CLI_RUNTIMES.iter().find(|rt| {
                rt.aliases
                    .iter()
                    .any(|alias| alias.eq_ignore_ascii_case(trimmed))
            })
        })
}

/// Serializable host auto-detection record for one CLI runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectedCliRuntime {
    pub runtime_id: String,
    pub display_name: String,
    pub binary_name: String,
    pub protocol: String,
    pub provider_type: String,
    pub installed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary_path: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_cli_runtimes_cover_top_six_with_unique_ids_and_binaries() {
        assert_eq!(KNOWN_CLI_RUNTIMES.len(), 6);
        let ids: Vec<&str> = KNOWN_CLI_RUNTIMES.iter().map(|r| r.id).collect();
        assert_eq!(ids, ["claude", "codex", "agy", "gemini", "kimi", "pi"]);

        for (query, expected) in [
            ("claude", "claude"),
            ("ANTHROPIC", "claude"),
            ("codex", "codex"),
            ("gpt", "codex"),
            ("agy", "agy"),
            ("antigravity", "agy"),
            ("gemini", "gemini"),
            ("gemini-cli", "gemini"),
            ("kimi", "kimi"),
            ("moonshot", "kimi"),
            ("pi", "pi"),
            ("pi-acp", "pi"),
        ] {
            assert_eq!(
                known_cli_runtime(query).map(|r| r.id),
                Some(expected),
                "query={query}"
            );
        }
        assert!(known_cli_runtime("").is_none());
        assert!(known_cli_runtime("unknown-cli").is_none());
    }
}
