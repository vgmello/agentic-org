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

/// Extracts optional validated `model:<id>` and `effort:<tier>` routing overrides from task
/// labels (`TASK-136`). Fails closed if a routing-prefixed label carries an invalid value.
pub fn parse_task_routing_labels(labels: &[String]) -> Result<TurnModelOverride, ApiError> {
    let mut model = None;
    let mut effort = None;
    for label in labels {
        if let Some(raw_effort) = label.strip_prefix("effort:") {
            effort = validate_effort_override(Some(raw_effort))?;
        } else if let Some(raw_model) = label.strip_prefix("model:") {
            model = validate_model_override(Some(raw_model))?;
        }
    }
    Ok(TurnModelOverride { model, effort })
}
