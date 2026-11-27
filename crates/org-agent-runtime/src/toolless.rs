//! Sans-I/O contract for strict tool-less, single-prompt model runs (`WATCH-4`, `TASK-259`).
//!
//! Runtime adapters that support a true zero-tool, zero-file, zero-MCP CLI mode implement
//! [`ToollessPromptAdapter`] purely in memory (argv planning and stdout parsing). Process
//! spawning, temporary work directories, timeouts, and cancellation live strictly in the
//! caller (`org-office-server`). Runtimes without a true tool-less mode return `None` from
//! [`crate::AgentRuntime::toolless_prompt`] and fail closed.

/// Token usage extracted from a tool-less prompt response, if reported by the runtime.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ToollessPromptUsage {
    /// Prompt/input token count reported by the runtime.
    pub input_tokens: Option<u64>,
    /// Completion/output token count reported by the runtime.
    pub output_tokens: Option<u64>,
}

/// Parsed response from a single tool-less prompt execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToollessPromptOutput {
    /// Normalized response text (for example with outer Markdown JSON fences stripped).
    pub text: String,
    /// Optional token usage metadata from the runtime envelope.
    pub usage: Option<ToollessPromptUsage>,
}

/// Sans-I/O adapter describing how a runtime executes a strict tool-less single-turn prompt.
///
/// Implementations perform **no I/O and spawn no processes**: they only declare availability
/// probe arguments, per-run tool-less CLI arguments, and how to parse bounded stdout bytes.
pub trait ToollessPromptAdapter: Send + Sync {
    /// CLI arguments used to check whether the runtime binary is executable (e.g. `["--version"]`).
    fn availability_args(&self) -> &'static [&'static str];

    /// CLI arguments for a single strict tool-less prompt read from `stdin` (`WATCH-4`: no
    /// tools, no files, no MCP servers, no slash commands, no session persistence) targeting
    /// `model_key`.
    fn run_args(&self, model_key: &str) -> Vec<String>;

    /// Parses bounded raw stdout bytes from the runtime process into normalized output text
    /// and optional token usage. Returns `None` on malformed output or runtime error envelopes.
    fn parse_output(&self, stdout_bytes: &[u8]) -> Option<ToollessPromptOutput>;
}
