//! The runtime port: how the copilot asks the user's own agent runtime (`claude -p`,
//! `codex exec`) to run one prompt. The crate owns the port; adapters implement it elsewhere,
//! so this crate has no process or network code.

use std::time::Duration;

use serde_json::Value;

/// One run: a prompt, the schema its answer must match, and the limits to enforce.
#[derive(Debug, Clone)]
pub struct RunRequest {
    /// The whole prompt (see [`crate::prompt`]). May hold a person's text: never log it.
    pub prompt: String,
    /// The JSON Schema of the one JSON document the answer must be. For runtimes that can
    /// enforce a schema natively; the crate validates the answer either way.
    pub output_schema: Value,
    /// The adapter must stop the run (kill the process) after this long.
    pub timeout: Duration,
    /// The adapter must read no more than this many bytes of answer.
    pub max_output_bytes: usize,
}

/// What a run returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutput {
    /// The runtime's final message, expected to be one JSON document.
    pub text: String,
    pub usage: Option<Usage>,
}

/// Token usage, when the runtime reports it (for the "Copilot" cost line).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

/// Why a run failed. There is deliberately no message: a runtime's error text can echo the
/// prompt, and nothing here may carry a body. Adapters log their own non-body diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeError {
    /// No runtime is set up (any more).
    NotSetUp,
    /// The adapter stopped the run at the timeout.
    TimedOut,
    /// Any other failure: it didn't start, it exited with an error, it was cut off.
    Failed,
}

/// The adapter's duties: run **tool-less and read-only** (no tools, no files, no network, no MCP
/// servers, an empty temporary working directory), enforce `timeout` and `max_output_bytes`, and
/// return the final message. The service does not trust it: it checks time, size, JSON and
/// schema again.
///
/// The call blocks; the office calls it from a worker thread.
pub trait CopilotRuntime {
    /// True when a runtime is set up here. When false, no feature is offered.
    fn available(&self) -> bool;

    fn run(&self, request: &RunRequest) -> Result<RunOutput, RuntimeError>;
}
