//! Server-validated Chief of Staff `suggestion.{create,list,inspect,withdraw,dismiss}` store
//! and lifecycle handlers (`TASK-80.4`).

use std::collections::VecDeque;

use org_agent_cli::IfMatch;
use org_proto::tess::{
    SuggestionActionResult, SuggestionCreateParams, SuggestionCreateResult,
    SuggestionDismissParams, SuggestionInspectParams, SuggestionInspectResult,
    SuggestionListParams, SuggestionListResult, SuggestionStatus, SuggestionView,
    SuggestionWithdrawParams, UiBlock, DEFAULT_SUGGESTION_TTL_SECONDS,
    MAX_SUGGESTION_RATIONALE_BYTES, MAX_SUGGESTION_SUMMARY_BYTES, MAX_SUGGESTION_TTL_SECONDS,
    MIN_SUGGESTION_TTL_SECONDS,
};
use org_proto::{format_rfc3339_ms, ApiError, ErrorCode};

use super::preflight::{preflight_target, resolve_target_command, PreflightContext};
use super::{CallerContext, TessService};

/// Hard upper bound on runtime-ephemeral suggestions held per office process.
const MAX_STORED_SUGGESTIONS: usize = 512;
/// Hard upper bound on runtime-ephemeral suggestions held simultaneously per caller.
const MAX_SUGGESTIONS_PER_CALLER: usize = 64;
/// Hard upper bound on serialized JSON bytes for `suggestion.create`'s `input` payload.
const MAX_SUGGESTION_INPUT_BYTES: usize = 16 * 1024;
/// Maximum page size returned by `suggestion.list` (keeps responses well inside `MAX_FRAME_BYTES`).
const MAX_LIST_LIMIT: usize = 50;

#[derive(Debug, Clone)]
pub(super) struct StoredSuggestion {
    view: SuggestionView,
    ui_block: UiBlock,
    expires_at_ms: u64,
}

#[derive(Debug, Default)]
pub(super) struct SuggestionStore {
    entries: VecDeque<StoredSuggestion>,
}

fn timestamp_rfc3339(ms: u64) -> String {
    format_rfc3339_ms(ms).unwrap_or_else(|| "1970-01-01T00:00:00.000Z".into())
}

impl SuggestionStore {
    fn expire_due(&mut self, now_ms: u64) {
        let now_rfc = timestamp_rfc3339(now_ms);
        for entry in &mut self.entries {
            if entry.view.status == SuggestionStatus::Pending && entry.expires_at_ms <= now_ms {
                entry.view.status = SuggestionStatus::Expired;
                entry.view.resolved_at = Some(now_rfc.clone());
            }
        }
    }

    fn remove_first(&mut self, pred: impl Fn(&StoredSuggestion) -> bool) -> bool {
        self.entries
            .iter()
            .position(pred)
            .is_some_and(|pos| self.entries.remove(pos).is_some())
    }

    fn insert(&mut self, entry: StoredSuggestion, now_ms: u64) -> Result<(), ApiError> {
        self.expire_due(now_ms);
        let caller = entry.view.for_caller_id.as_str();
        let is_caller = |e: &StoredSuggestion| e.view.for_caller_id == caller;
        let is_closed = |e: &StoredSuggestion| e.view.status != SuggestionStatus::Pending;
        while self.entries.iter().filter(|e| is_caller(e)).count() >= MAX_SUGGESTIONS_PER_CALLER {
            if !self.remove_first(|e| is_caller(e) && is_closed(e)) && !self.remove_first(is_caller)
            {
                break;
            }
        }
        while self.entries.len() >= MAX_STORED_SUGGESTIONS {
            if self.remove_first(|e| is_caller(e) && is_closed(e))
                || self.remove_first(is_closed)
                || self.remove_first(is_caller)
            {
                continue;
            }
            return Err(ApiError::new(
                ErrorCode::RateLimited,
                "suggestion store is full of active pending proposals; wait for existing suggestions to be resolved or expire",
            ));
        }
        self.entries.push_back(entry);
        Ok(())
    }

    fn visible_to(entry: &StoredSuggestion, context: &CallerContext) -> bool {
        match context {
            CallerContext::TessActingFor {
                tess_id, caller_id, ..
            } => {
                entry.view.created_by_member_id == *tess_id
                    && entry.view.for_caller_id == *caller_id
            }
            CallerContext::HumanCaller { caller_id, .. } => entry.view.for_caller_id == *caller_id,
            CallerContext::TessWake => false,
        }
    }
}

struct ValidatedCreate {
    summary: String,
    rationale: Option<String>,
    ttl_secs: u64,
    row: &'static org_agent_cli::Command,
}

fn validate_create_params(params: &SuggestionCreateParams) -> Result<ValidatedCreate, ApiError> {
    let summary = validate_text(
        "summary",
        &params.summary,
        MAX_SUGGESTION_SUMMARY_BYTES,
        false,
    )?;
    let rationale = params
        .rationale
        .as_deref()
        .map(|r| validate_text("rationale", r, MAX_SUGGESTION_RATIONALE_BYTES, true))
        .transpose()?;
    let input_bytes = serde_json::to_vec(&params.input)
        .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "unable to serialize `input`"))?;
    if input_bytes.len() > MAX_SUGGESTION_INPUT_BYTES {
        return Err(ApiError::new(
            ErrorCode::TooLarge,
            format!(
                "`input` exceeds maximum serialized size of {MAX_SUGGESTION_INPUT_BYTES} bytes"
            ),
        ));
    }
    let ttl_secs = params.ttl_seconds.unwrap_or(DEFAULT_SUGGESTION_TTL_SECONDS);
    if !(MIN_SUGGESTION_TTL_SECONDS..=MAX_SUGGESTION_TTL_SECONDS).contains(&ttl_secs) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!(
                "`ttl_seconds` must be between {MIN_SUGGESTION_TTL_SECONDS} and {MAX_SUGGESTION_TTL_SECONDS}"
            ),
        ));
    }
    let row = resolve_target_command(&params.command)?;
    if let Err(problem) = org_agent_cli::validate_input(row, &params.input) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!("invalid target input for `{}`: {problem}", row.name()),
        ));
    }
    if row.if_match() == IfMatch::Required && params.expected_revision.is_none() {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!(
                "`{}` requires `expected_revision` to pin the record revision before confirmation",
                row.name()
            ),
        ));
    }
    Ok(ValidatedCreate {
        summary,
        rationale,
        ttl_secs,
        row,
    })
}

pub(super) fn handle_create(
    svc: &TessService,
    context: &CallerContext,
    params: SuggestionCreateParams,
) -> Result<SuggestionCreateResult, ApiError> {
    let CallerContext::TessActingFor {
        tess_id,
        caller_id,
        authority,
    } = context
    else {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "only the Chief of Staff acting for an active person may create suggestions",
        ));
    };

    let validated = validate_create_params(&params)?;
    let preflight_ctx = PreflightContext {
        tess_id,
        caller_id,
        authority,
    };
    let preflight = preflight_target(
        svc,
        &preflight_ctx,
        validated.row.call,
        &params.input,
        params.expected_revision,
    )?;

    let now_ms = svc.now_ms();
    let expires_at_ms = now_ms.saturating_add(validated.ttl_secs.saturating_mul(1_000));
    let id = svc.next_uuid_v7();
    let view = SuggestionView {
        id: id.clone(),
        status: SuggestionStatus::Pending,
        command: validated.row.name(),
        call: preflight.call,
        input: params.input,
        summary: validated.summary.clone(),
        rationale: validated.rationale,
        expected_revision: params.expected_revision,
        created_by_member_id: tess_id.clone(),
        for_caller_id: caller_id.clone(),
        preflight_ok: true,
        preflight_note: preflight.note,
        created_at: timestamp_rfc3339(now_ms),
        expires_at: timestamp_rfc3339(expires_at_ms),
        resolved_at: None,
        close_reason: None,
    };
    let ui_block = UiBlock {
        title: Some(validated.summary),
        suggestion_id: Some(id),
        ..UiBlock::default()
    };

    if !params.dry_run {
        let mut store = svc.suggestions.lock().unwrap_or_else(|p| p.into_inner());
        store.insert(
            StoredSuggestion {
                view: view.clone(),
                ui_block: ui_block.clone(),
                expires_at_ms,
            },
            now_ms,
        )?;
    }

    Ok(SuggestionCreateResult {
        suggestion: view,
        ui_block,
        dry_run: params.dry_run,
    })
}

pub(super) fn handle_list(
    svc: &TessService,
    context: &CallerContext,
    params: SuggestionListParams,
) -> Result<SuggestionListResult, ApiError> {
    if matches!(context, CallerContext::TessWake) {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "a wake run has no caller-scoped suggestions",
        ));
    }
    if matches!(params.status, Some(SuggestionStatus::Unknown)) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "unknown suggestion status filter",
        ));
    }
    let now_ms = svc.now_ms();
    let mut store = svc.suggestions.lock().unwrap_or_else(|p| p.into_inner());
    store.expire_due(now_ms);
    let limit = params
        .limit
        .map(|n| {
            usize::try_from(n)
                .unwrap_or(MAX_LIST_LIMIT)
                .min(MAX_LIST_LIMIT)
        })
        .unwrap_or(MAX_LIST_LIMIT);
    let suggestions = store
        .entries
        .iter()
        .rev()
        .filter(|e| SuggestionStore::visible_to(e, context))
        .filter(|e| params.status.is_none_or(|s| e.view.status == s))
        .take(limit)
        .map(|e| e.view.clone())
        .collect();
    Ok(SuggestionListResult { suggestions })
}

pub(super) fn handle_inspect(
    svc: &TessService,
    context: &CallerContext,
    params: SuggestionInspectParams,
) -> Result<SuggestionInspectResult, ApiError> {
    if matches!(context, CallerContext::TessWake) {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "a wake run cannot inspect caller-scoped suggestions",
        ));
    }
    let now_ms = svc.now_ms();
    let mut store = svc.suggestions.lock().unwrap_or_else(|p| p.into_inner());
    store.expire_due(now_ms);
    let found = store
        .entries
        .iter()
        .find(|e| e.view.id == params.suggestion_id && SuggestionStore::visible_to(e, context))
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "suggestion not found"))?;
    Ok(SuggestionInspectResult {
        suggestion: found.view.clone(),
        ui_block: found.ui_block.clone(),
    })
}

pub(super) fn handle_withdraw(
    svc: &TessService,
    context: &CallerContext,
    params: SuggestionWithdrawParams,
) -> Result<SuggestionActionResult, ApiError> {
    if !matches!(context, CallerContext::TessActingFor { .. }) {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "only the Chief of Staff acting for the caller may withdraw a suggestion",
        ));
    }
    close_suggestion(
        svc,
        context,
        &params.suggestion_id,
        params.reason.as_deref(),
        SuggestionStatus::Withdrawn,
        params.dry_run,
    )
}

pub(super) fn handle_dismiss(
    svc: &TessService,
    context: &CallerContext,
    params: SuggestionDismissParams,
) -> Result<SuggestionActionResult, ApiError> {
    if !matches!(context, CallerContext::HumanCaller { .. }) {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "only the addressed human caller may dismiss a suggestion",
        ));
    }
    close_suggestion(
        svc,
        context,
        &params.suggestion_id,
        params.reason.as_deref(),
        SuggestionStatus::Dismissed,
        false,
    )
}

fn close_suggestion(
    svc: &TessService,
    context: &CallerContext,
    suggestion_id: &str,
    raw_reason: Option<&str>,
    target_status: SuggestionStatus,
    dry_run: bool,
) -> Result<SuggestionActionResult, ApiError> {
    let reason = raw_reason
        .map(|r| validate_text("reason", r, MAX_SUGGESTION_SUMMARY_BYTES, false))
        .transpose()?;
    let now_ms = svc.now_ms();
    let mut store = svc.suggestions.lock().unwrap_or_else(|p| p.into_inner());
    store.expire_due(now_ms);
    let entry = store
        .entries
        .iter_mut()
        .find(|e| e.view.id == suggestion_id && SuggestionStore::visible_to(e, context))
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "suggestion not found"))?;
    if entry.view.status != SuggestionStatus::Pending {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            format!(
                "suggestion is already `{}` and cannot be {}",
                entry.view.status.as_str(),
                target_status.as_str()
            ),
        ));
    }
    let mut updated = entry.view.clone();
    updated.status = target_status;
    updated.resolved_at = Some(timestamp_rfc3339(now_ms));
    updated.close_reason = reason;
    if !dry_run {
        entry.view = updated.clone();
    }
    Ok(SuggestionActionResult {
        suggestion: updated,
    })
}

fn validate_text(
    field: &str,
    raw: &str,
    max_bytes: usize,
    allow_newlines: bool,
) -> Result<String, ApiError> {
    if raw
        .chars()
        .any(|ch| ch.is_control() && !(allow_newlines && ch == '\n'))
    {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!("`{field}` must not contain control characters"),
        ));
    }
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!("`{field}` must not be empty"),
        ));
    }
    if trimmed.len() > max_bytes {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!("`{field}` exceeds maximum length of {max_bytes} bytes"),
        ));
    }
    Ok(trimmed.to_owned())
}
