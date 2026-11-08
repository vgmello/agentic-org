//! The runtime port: how the copilot asks the user's own agent runtime (`claude -p`,
//! `codex exec`) to run one prompt. The crate owns the port; adapters implement it elsewhere,
//! so this crate has no process or network code.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

/// Raised by the copilot when a run's deadline has passed (or the run is abandoned). The adapter
/// polls it while it waits for its child process, and kills the child when it is set. Cloning
/// shares the flag.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    /// True once the copilot has given up on this run: stop and kill the child.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    pub(crate) fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// One run: a prompt, the schema its answer must match, and the limits to enforce.
#[derive(Debug, Clone)]
pub struct RunRequest {
    /// The whole prompt (see [`crate::prompt`]). May hold a person's text: never log it.
    pub prompt: String,
    /// The JSON Schema of the one JSON document the answer must be. For runtimes that can
    /// enforce a schema natively; the crate validates the answer either way.
    pub output_schema: Value,
    /// The copilot stops waiting after this long and sets the run's [`CancelToken`]. Adapters may
    /// also use it to set the child's own limit.
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
    /// The adapter stopped the run at the timeout (or when cancelled).
    TimedOut,
    /// Any other failure: it didn't start, it exited with an error, it was cut off.
    Failed,
}

/// The adapter's duties: run **tool-less and read-only** (no tools, no files, no network, no MCP
/// servers, an empty temporary working directory), cap the bytes it reads at `max_output_bytes`,
/// and return the final message. **Poll `cancel` while waiting, and kill the child when it is set.**
///
/// The copilot owns the deadline. It runs the adapter on a worker thread and waits at most
/// `timeout`; at the deadline it sets `cancel` and answers `timeout`, whether or not the adapter
/// has returned, so a hung adapter never holds the person's slot or the office's worker. What it
/// can't do is stop the thread: an adapter that ignores `cancel` leaves a thread (and its child)
/// behind. [`crate::Limits::max_concurrent_runs`] bounds how many such runs can pile up.
pub trait CopilotRuntime: Send + Sync + 'static {
    /// True when a runtime is set up here. When false, no feature is offered.
    fn available(&self) -> bool;

    fn run(&self, request: &RunRequest, cancel: &CancelToken) -> Result<RunOutput, RuntimeError>;
}
