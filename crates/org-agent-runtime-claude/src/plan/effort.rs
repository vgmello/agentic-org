//! The session spec's reasoning effort as Claude Code's `--effort` (`TASK-181`).

use org_agent_runtime::{PlanError, SessionSpec};

/// The levels Claude Code's `--effort` accepts. They are the office's five levels by the same
/// names (`org_proto::hiring::AGENT_EFFORT_LEVELS`), so each one passes through unchanged.
const CLAUDE_EFFORT_LEVELS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// The `--effort` value for the spec's effort, or `None` for Claude Code's default.
///
/// Any other value refuses the spec (fail closed), including other runtimes' names such as
/// Codex's `none` and `minimal`, and other spellings such as `XHIGH`. The contract has an
/// adapter apply the given effort or refuse it, never drop it or pass an unknown level on.
pub(crate) fn claude_effort(spec: &SessionSpec) -> Result<Option<&'static str>, PlanError> {
    let Some(effort) = &spec.effort else {
        return Ok(None);
    };
    CLAUDE_EFFORT_LEVELS
        .iter()
        .copied()
        .find(|level| *level == effort.as_str())
        .map(Some)
        .ok_or_else(|| {
            PlanError::InvalidSpec(
                "Claude Code takes reasoning effort low, medium, high, xhigh or max".to_owned(),
            )
        })
}
