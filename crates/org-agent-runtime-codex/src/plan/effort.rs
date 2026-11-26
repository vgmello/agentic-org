//! The session spec's reasoning effort in Codex's own names (`TASK-181`).

use org_agent_runtime::{PlanError, SessionSpec};

/// Codex's `model_reasoning_effort` (and `thread/start` `reasoningEffort`) for the spec's effort,
/// or `None` for Codex's default.
///
/// The office names five levels: `low`, `medium` and `high` are Codex's names too, as is
/// `xhigh`; `max` is above Codex's highest level, so it runs as `xhigh`. Codex's own `none` and
/// `minimal` pass through as they are. Any other value refuses the spec, since Codex would
/// refuse the config when it starts: the contract has an adapter apply a given effort or refuse.
pub(crate) fn codex_reasoning_effort(
    spec: &SessionSpec,
) -> Result<Option<&'static str>, PlanError> {
    let Some(effort) = &spec.effort else {
        return Ok(None);
    };
    let codex = match effort.as_str() {
        "none" => "none",
        "minimal" => "minimal",
        "low" => "low",
        "medium" => "medium",
        "high" => "high",
        "xhigh" | "max" => "xhigh",
        _ => {
            return Err(PlanError::InvalidSpec(
                "reasoning effort is not one Codex knows".to_owned(),
            ))
        }
    };
    Ok(Some(codex))
}
