//! Per-task / per-turn model and reasoning-effort override validation (`TASK-136`).
//!
//! Fails closed on unrecognized effort tiers, control characters, whitespace, or leading `-`
//! flags so untrusted task/turn parameters can never inject runtime argv flags.

use org_agent_runtime::Token;
use org_office::organization_family::records::ReasoningEffort;
use org_proto::hiring::AGENT_EFFORT_LEVELS;
use org_proto::{ApiError, ErrorCode};

/// Validated per-turn / per-task model and reasoning effort override.
/// Unset (`None`) fields fall back cleanly to the agent's configured profile defaults.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TurnModelOverride {
    pub model: Option<String>,
    pub effort: Option<ReasoningEffort>,
}

impl TurnModelOverride {
    pub fn is_empty(&self) -> bool {
        self.model.is_none() && self.effort.is_none()
    }

    pub fn effort_str(&self) -> Option<&'static str> {
        self.effort.map(ReasoningEffort::as_str)
    }

    pub fn effort_token(&self) -> Option<Token> {
        self.effort.and_then(|e| Token::try_from(e.as_str()).ok())
    }

    pub fn model_token(&self) -> Option<Token> {
        self.model.as_deref().and_then(|m| Token::try_from(m).ok())
    }
}

/// Validates a single optional effort override strictly against [`AGENT_EFFORT_LEVELS`].
pub fn validate_effort_override(raw: Option<&str>) -> Result<Option<ReasoningEffort>, ApiError> {
    let Some(val) = raw else {
        return Ok(None);
    };
    if val.is_empty()
        || val.starts_with('-')
        || val.chars().any(|c| c.is_control() || c.is_whitespace())
        || !AGENT_EFFORT_LEVELS.contains(&val)
    {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!(
                "invalid reasoning effort override; expected one of: {}",
                AGENT_EFFORT_LEVELS.join(", ")
            ),
        ));
    }
    ReasoningEffort::parse(val).map(Some).ok_or_else(|| {
        ApiError::new(
            ErrorCode::InvalidRequest,
            "invalid reasoning effort override",
        )
    })
}

/// Validates a single optional model override as a safe runtime [`Token`] without leading `-`,
/// whitespace, or control characters (preventing argv/flag injection).
pub fn validate_model_override(raw: Option<&str>) -> Result<Option<String>, ApiError> {
    let Some(val) = raw else {
        return Ok(None);
    };
    if val.is_empty()
        || val.starts_with('-')
        || val.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "invalid model override: must be a safe identifier without leading '-' or control characters",
        ));
    }
    let token = Token::try_from(val).map_err(|_| {
        ApiError::new(
            ErrorCode::InvalidRequest,
            "invalid model override: must be a safe identifier without leading '-' or control characters",
        )
    })?;
    Ok(Some(token.as_str().to_owned()))
}

/// Validates both optional `model` and `effort` overrides together, failing closed on any
/// invalid value.
pub fn validate_turn_override(
    model: Option<&str>,
    effort: Option<&str>,
) -> Result<TurnModelOverride, ApiError> {
    Ok(TurnModelOverride {
        model: validate_model_override(model)?,
        effort: validate_effort_override(effort)?,
    })
}

/// Returns `true` when `raw` (trimmed, case-insensitively) uses a reserved `model:` or
/// `effort:` task routing label prefix (`TASK-136`).
pub fn is_task_routing_label(raw: &str) -> bool {
    let lower = raw.trim().to_ascii_lowercase();
    lower.starts_with("model:") || lower.starts_with("effort:")
}

/// Validates every `(model, effort)` pair in a wake batch and returns the last non-empty
/// atomic pair without ever mixing `model` from one item with `effort` from another (`TASK-136`).
pub(crate) fn select_atomic_turn_override<'a>(
    pairs: impl IntoIterator<Item = (Option<&'a str>, Option<&'a str>)>,
) -> Result<TurnModelOverride, ApiError> {
    let mut effective = TurnModelOverride::default();
    for (model, effort) in pairs {
        let candidate = validate_turn_override(model, effort)?;
        if !candidate.is_empty() {
            effective = candidate;
        }
    }
    Ok(effective)
}

/// Extracts optional validated `model:<id>` and `effort:<tier>` routing overrides from task
/// labels (`TASK-136`). Fails closed on invalid values, non-canonical prefix casing/whitespace,
/// or duplicate/conflicting `model:*` / `effort:*` labels.
pub fn parse_task_routing_labels(labels: &[String]) -> Result<TurnModelOverride, ApiError> {
    let mut model = None;
    let mut effort = None;
    for label in labels {
        if let Some(raw_effort) = label.strip_prefix("effort:") {
            let parsed = validate_effort_override(Some(raw_effort))?;
            if effort.is_some() {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "duplicate or conflicting effort:* task routing labels; specify at most one effort:<tier> label",
                ));
            }
            effort = parsed;
        } else if let Some(raw_model) = label.strip_prefix("model:") {
            let parsed = validate_model_override(Some(raw_model))?;
            if model.is_some() {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "duplicate or conflicting model:* task routing labels; specify at most one model:<id> label",
                ));
            }
            model = parsed;
        } else if is_task_routing_label(label) {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "task routing labels must use lowercase `model:<id>` or `effort:<tier>` without surrounding whitespace",
            ));
        }
    }
    Ok(TurnModelOverride { model, effort })
}

/// Parses task routing labels for wake delivery, falling back safely to the empty default
/// override if a legacy task carries malformed or conflicting routing labels so assignment and
/// comment wakes are never silently dropped (`TASK-136`).
pub fn parse_task_routing_labels_or_default(labels: &[String]) -> TurnModelOverride {
    parse_task_routing_labels(labels).unwrap_or_default()
}
