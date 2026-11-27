//! Declarative per-vendor ACP profile schema shared by the generic JSON-RPC 2.0 ACP engine.

use org_agent_runtime::transcript::TranscriptParser;
use org_agent_runtime::{PermissionMode, PlanError, RuntimeHostProfile};

/// Expected ACP execution mode (never `yolo`, `auto`, `dontAsk`, or any bypass mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpectedMode {
    /// Interactive CLI `--mode` value used for PTY take-over, if any.
    pub cli_flag: Option<&'static str>,
    /// Canonical `modes.currentModeId` enforced during ACP opening and drift checks.
    pub reported_name: &'static str,
}

/// Fail-closed mapping from [`PermissionMode`] to a vendor's allowed ACP mode IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeMapping {
    /// Mode used for [`PermissionMode::Ask`].
    pub ask: ExpectedMode,
    /// Mode used for [`PermissionMode::AcceptEdits`].
    pub accept_edits: ExpectedMode,
    /// Mode used for [`PermissionMode::Plan`].
    pub plan: ExpectedMode,
}

impl ModeMapping {
    /// Resolves a [`PermissionMode`] or fails closed on unknown/unsupported modes.
    pub fn resolve(self, mode: PermissionMode) -> Result<ExpectedMode, PlanError> {
        match mode {
            PermissionMode::Ask => Ok(self.ask),
            PermissionMode::AcceptEdits => Ok(self.accept_edits),
            PermissionMode::Plan => Ok(self.plan),
            _ => Err(PlanError::Unsupported("permission mode")),
        }
    }
}

/// How standing system instructions are delivered to an ACP vendor CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemPromptDelivery {
    /// Passed via `_meta.antigravity.systemPrompt.append` on `session/new` / `session/resume`.
    MetaAntigravityAppend,
    /// Written to a `0600` file inside `LaunchContext::runtime_dir` and referenced via `env_var`.
    PlannedFileEnv {
        /// Runtime-dir relative filename (e.g. `"gemini-system.md"`).
        rel_path: &'static str,
        /// Environment variable naming the file (e.g. `"GEMINI_SYSTEM_MD"`).
        env_var: &'static str,
    },
    /// Prepended as a leading ACP text block on the session's first `session/prompt` turn.
    FirstTurnPromptBlock,
}

/// Per-vendor CLI state directory arguments for headless ACP launches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateDirStrategy {
    /// No extra CLI state-directory flags beyond `config_dir_env`.
    None,
    /// Antigravity `--gemini-dir <config_dir|runtime_dir/gemini> --app-data-dir antigravity-cli`.
    AgyGeminiDir {
        /// Runtime-dir relative fallback folder created when `config_dir` is absent.
        isolated_rel: &'static str,
        /// Relative `--app-data-dir` passed to `agy acp --stdio`.
        app_data_rel: &'static str,
    },
}

/// Interactive PTY take-over capability for a vendor profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TakeOverStrategy {
    /// Antigravity interactive `--conversation <id> --agent <name>` take-over.
    AgyConversation,
    /// Vendor does not support interactive PTY take-over (fails closed).
    Unsupported,
}

/// How [`org_agent_runtime::Probe::Auth`] checks credentials for a vendor profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthProbeStrategy {
    /// Runs a one-shot CLI command (e.g. `agy models`) and parses stdout/exit code.
    OneShot {
        /// CLI arguments appended to the runtime binary.
        args: &'static [&'static str],
        /// Auth method tag reported on success.
        method: &'static str,
    },
    /// Runs a live sans-I/O JSON-RPC 2.0 ACP `initialize` handshake via [`org_agent_runtime::ProbeSession`].
    AcpInitialize {
        /// Auth method tag reported on success.
        method: &'static str,
    },
}

/// Parsed `(major, minor, patch)` semantic version tuple.
pub type Semver = (u64, u64, u64);

/// Inclusive `(min, max)` tested semantic version range.
pub type SemverRange = (Semver, Semver);

/// Complete declarative specification of a JSON-RPC 2.0 ACP vendor CLI.
pub struct AcpProfile {
    /// Canonical [`org_agent_runtime::RuntimeId`] string (e.g. `"agy"`, `"gemini"`).
    pub id: &'static str,
    /// Human-readable display prefix used in notices and protocol errors.
    pub display_name: &'static str,
    /// Expected CLI executable name on `PATH`.
    pub binary_name: &'static str,
    /// Alternate provider/runtime aliases resolved case-insensitively.
    pub aliases: &'static [&'static str],
    /// Base argv tokens placed immediately after the binary for headless ACP stdio.
    pub headless_base_args: &'static [&'static str],
    /// CLI flag appended when `spec.sandbox.enabled` is true, if supported.
    pub sandbox_flag: Option<&'static str>,
    /// Optional state directory CLI flags (e.g. Agy `--gemini-dir`).
    pub state_dir: StateDirStrategy,
    /// CLI flag repeated per `spec.read_only_dirs` entry, if supported.
    pub read_only_dir_flag: Option<&'static str>,
    /// How standing system prompts are passed into the runtime.
    pub system_prompt: SystemPromptDelivery,
    /// JSON-RPC method used when resuming an existing session (`session/resume` or `session/load`).
    pub resume_method: &'static str,
    /// Allowed execution modes (`Ask`, `AcceptEdits`, `Plan`) — never bypass/yolo.
    pub modes: ModeMapping,
    /// Whether interactive PTY take-over is supported.
    pub take_over: TakeOverStrategy,
    /// Static non-secret environment switches added to session launches.
    pub env_add: &'static [(&'static str, &'static str)],
    /// Static non-secret environment switches added to probes.
    pub probe_env_add: &'static [(&'static str, &'static str)],
    /// Environment variable pointing to `LaunchContext::config_dir` / `ProbeContext::config_dir`.
    pub config_dir_env: &'static str,
    /// Allowlisted authentication environment variable names accepted from the office vault.
    pub auth_env_names: &'static [&'static str],
    /// Environment variable names redacted whenever they appear in runtime output.
    pub secret_env_names: &'static [&'static str],
    /// Optional custom base-URL environment variable name.
    pub endpoint_env_var: Option<&'static str>,
    /// Auth env var selected for subscription CLI credentials.
    pub subscription_auth_env_var: &'static str,
    /// Auth env var selected for API-key credentials.
    pub api_key_auth_env_var: &'static str,
    /// Host config directory, credential deny files, and sandbox hints.
    pub host_profile: RuntimeHostProfile,
    /// Strategy for [`org_agent_runtime::Probe::Auth`].
    pub auth_probe: AuthProbeStrategy,
    /// Advertised `(min, max)` tested version range strings.
    pub tested_versions: (&'static str, &'static str),
    /// Optional `(min, max)` semver tuple bounds for `ProbeResult::Version::in_tested_range`.
    pub tested_semver_range: Option<SemverRange>,
    /// Optional structured JSON stderr line prefix (e.g. `Some("AGY_ERROR:")`).
    pub stderr_error_prefix: Option<&'static str>,
    /// Sans-I/O conversation transcript parser owned by this vendor profile.
    pub transcript_parser: &'static dyn TranscriptParser,
}
