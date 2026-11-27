//! Prompt and compact UI viewport-context validation and untrusted-data framing for Ask Tess
//! (`TASK-80.4` Part B).

use std::collections::BTreeMap;

use org_proto::tess::{
    TessViewportContext, UiEntityKind, MAX_TESS_PROMPT_BYTES, MAX_VIEWPORT_CONTEXT_BYTES,
};
use org_proto::{ApiError, ErrorCode};

/// Maximum filter key/value entries accepted in a compact viewport context envelope.
const MAX_VIEWPORT_FILTERS: usize = 12;

const VIEWPORT_FENCE_OPEN: &str =
    "<<<UNTRUSTED_VIEWPORT_CONTEXT kind=\"ui_viewport\" untrusted=\"true\">>>";
const VIEWPORT_FENCE_CLOSE: &str = "<<<END_UNTRUSTED_VIEWPORT_CONTEXT>>>";

pub(crate) fn validate_prompt(raw: &str) -> Result<String, ApiError> {
    if raw
        .chars()
        .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
    {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "`prompt` must not contain control characters",
        ));
    }
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "`prompt` must not be empty",
        ));
    }
    if trimmed.len() > MAX_TESS_PROMPT_BYTES {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!("`prompt` exceeds maximum length of {MAX_TESS_PROMPT_BYTES} bytes"),
        ));
    }
    Ok(trimmed.to_owned())
}

fn clean_viewport_str(field: &str, raw: &str) -> Result<String, ApiError> {
    if raw.chars().any(char::is_control) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!("`viewport_context.{field}` must not contain control characters"),
        ));
    }
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!("`viewport_context.{field}` must not be empty"),
        ));
    }
    Ok(trimmed.replace("<<<", "< < <").replace(">>>", "> > >"))
}

fn validate_viewport_route(raw: &str) -> Result<String, ApiError> {
    let route = clean_viewport_str("route", raw)?;
    if !route.starts_with('/')
        || route.contains("//")
        || route.contains("..")
        || route.contains("://")
        || route.contains('\\')
        || route.chars().any(char::is_whitespace)
    {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "`viewport_context.route` must be a safe relative in-app path starting with `/`",
        ));
    }
    Ok(route)
}

pub(crate) fn validate_and_render_viewport(vp: &TessViewportContext) -> Result<String, ApiError> {
    if matches!(vp.entity_kind, Some(UiEntityKind::Unknown)) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "unknown `viewport_context.entity_kind`",
        ));
    }
    if vp.filters.len() > MAX_VIEWPORT_FILTERS {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!(
                "`viewport_context.filters` may contain at most {MAX_VIEWPORT_FILTERS} entries"
            ),
        ));
    }
    let mut sanitized = TessViewportContext {
        route: vp
            .route
            .as_deref()
            .map(validate_viewport_route)
            .transpose()?,
        route_label: vp
            .route_label
            .as_deref()
            .map(|v| clean_viewport_str("route_label", v))
            .transpose()?,
        entity_kind: vp.entity_kind,
        entity_id: vp
            .entity_id
            .as_deref()
            .map(|v| clean_viewport_str("entity_id", v))
            .transpose()?,
        entity_title: vp
            .entity_title
            .as_deref()
            .map(|v| clean_viewport_str("entity_title", v))
            .transpose()?,
        decision_id: vp
            .decision_id
            .as_deref()
            .map(|v| clean_viewport_str("decision_id", v))
            .transpose()?,
        filters: BTreeMap::new(),
    };
    for (k, v) in &vp.filters {
        let key = clean_viewport_str("filters.key", k)?;
        let val = clean_viewport_str("filters.value", v)?;
        sanitized.filters.insert(key, val);
    }
    let encoded = serde_json::to_string(&sanitized).map_err(|_| {
        ApiError::new(
            ErrorCode::InvalidRequest,
            "unable to encode `viewport_context`",
        )
    })?;
    if encoded.len() > MAX_VIEWPORT_CONTEXT_BYTES {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!(
                "`viewport_context` exceeds maximum size of {MAX_VIEWPORT_CONTEXT_BYTES} bytes"
            ),
        ));
    }
    Ok(encoded)
}

/// Composes the volatile Ask Tess turn input. Static standing instructions remain untouched;
/// any volatile UI viewport context is appended strictly LAST behind an untrusted-data fence.
pub(crate) fn compose_ask_instruction(
    prompt: &str,
    viewport_json: Option<&str>,
    after_reset: bool,
) -> String {
    let mut out = String::new();
    if after_reset {
        out.push_str(
            "[Session reset by caller: treat this message as a fresh conversation turn.]\n\n",
        );
    }
    out.push_str(prompt);
    if let Some(vp) = viewport_json {
        out.push_str("\n\n");
        out.push_str(VIEWPORT_FENCE_OPEN);
        out.push('\n');
        out.push_str(vp);
        out.push('\n');
        out.push_str(VIEWPORT_FENCE_CLOSE);
    }
    out
}
