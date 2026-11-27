//! Two-phase human-authenticated HostChannel `suggestion.apply` execution (`TASK-80.4` Part B,
//! `TASK-254`).
//!
//! Security boundary:
//! - Strictly `via_grant: true` (verified human HostChannel grant ONLY — never Tess or any agent).
//! - Verifies `suggestion.for_caller_id == caller_id` (fails closed with `NotFound` for any other
//!   caller — zero existence oracle).
//! - Reserves `applying: true` atomically under the suggestion mutex (with a panic-safe RAII
//!   guard) and drops the mutex before re-running live preflight, `expected_revision` CAS, and
//!   direct typed RPC translation under the human caller's own `via_grant: true` authority.
//! - Transitions to `SuggestionStatus::Applied` atomically (single-use; replay returns `Conflict`).

mod translate_checklist;
mod translate_msg;
mod translate_org;
mod translate_work;

use org_command_catalogue::{Command, IfMatch};
use org_proto::tess::{
    SuggestionApplyParams, SuggestionApplyReceipt, SuggestionApplyResult, SuggestionAttribution,
    SuggestionView,
};
use org_proto::{format_rfc3339_ms, ApiError, ErrorCode, Outcome, Request};
use serde_json::{Map, Value};

use crate::ports::ApplyExecFn;
use crate::preflight::{preflight_target, resolve_target_command, PreflightContext};
use crate::{unavailable, CallerContext, TessService};

/// Maximum RPC sub-calls permitted while executing a single applied proposal.
const MAX_APPLY_SUBCALLS: usize = 8;

/// Read-only lookup RPCs permitted while executing an allowlisted write proposal (never any write,
/// auth, provider, session, or Tess method).
const APPLY_HELPER_READ_CALLS: &[&str] = &[
    "chat.channels",
    "chat.conversations",
    "docs.inspect",
    "docs.list",
    "hr.hire.inspect",
    "hr.hire.list",
    "msg.get",
    "operation.get",
    "org.snapshot",
    "process.show",
    "project.inspect",
    "project.list",
    "repo.list",
    "role.inspect",
    "role.list",
    "tasks.inspect",
    "tasks.list",
    "who",
    "worktree.list",
];

struct ApplyInFlightGuard<'a> {
    svc: &'a TessService,
    suggestion_id: String,
    caller_id: String,
    armed: bool,
}

impl<'a> ApplyInFlightGuard<'a> {
    fn new(svc: &'a TessService, suggestion_id: String, caller_id: String) -> Self {
        Self {
            svc,
            suggestion_id,
            caller_id,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ApplyInFlightGuard<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut store = self
            .svc
            .suggestions
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        store.release_applying(&self.suggestion_id, &self.caller_id);
    }
}

pub(crate) fn is_permitted_apply_call(
    method: &str,
    allowed_write_a: &str,
    allowed_write_b: &str,
) -> bool {
    if method.starts_with("suggestion.")
        || method.starts_with("tess.")
        || method.starts_with("auth.")
        || method.starts_with("provider.")
        || method.starts_with("member.session_")
        || method.starts_with("member.external_")
        || method == "git.push"
        || method == "office.remove"
        || method == "office.restart"
    {
        return false;
    }
    method == allowed_write_a
        || method == allowed_write_b
        || APPLY_HELPER_READ_CALLS.contains(&method)
}

/// Checked RPC executor enforcing [`is_permitted_apply_call`] and [`MAX_APPLY_SUBCALLS`] before
/// forwarding any wire request to the office server as `caller_id`.
struct ApplyExecContext<'a> {
    caller_id: &'a str,
    allowed_write_a: &'a str,
    allowed_write_b: &'a str,
    exec: &'a ApplyExecFn<'a>,
    subcalls: usize,
}

impl<'a> ApplyExecContext<'a> {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, ApiError> {
        if !is_permitted_apply_call(method, self.allowed_write_a, self.allowed_write_b) {
            return Err(ApiError::new(
                ErrorCode::NotAuthorized,
                format!("proposal apply cannot invoke `{method}`"),
            ));
        }
        if self.subcalls >= MAX_APPLY_SUBCALLS {
            return Err(ApiError::new(
                ErrorCode::Unavailable,
                "proposal apply exceeded maximum sub-call limit",
            ));
        }
        self.subcalls += 1;
        let request = Request {
            id: self.subcalls as u64,
            method: method.to_owned(),
            params,
            principal: None,
        };
        let response = (self.exec)(self.caller_id, &request);
        match response.outcome {
            Outcome::Result(val) => Ok(val),
            Outcome::Error(err) => Err(err),
        }
    }
}

/// Prepared wire RPC call and response normalizer for one applied suggestion.
pub(super) struct PreparedRpc {
    pub method: &'static str,
    pub params: Value,
    pub shape: OutputShape,
}

/// Normalizes a raw office RPC result into the canonical receipt shape expected by callers.
pub(super) enum OutputShape {
    Passthrough,
    OrgAccepted {
        target_id: String,
    },
    TaskRow,
    TaskComment,
    Checklist {
        fallback_ids: Vec<String>,
        removed: bool,
    },
    MsgSend,
    MsgAsk,
    MsgWithdraw,
    MsgDone,
    MsgDefer,
    MsgUnsnooze,
    ChannelCreate,
    ChannelMembership,
    ChannelMarkRead,
}

pub(crate) fn handle_apply(
    svc: &TessService,
    context: &CallerContext,
    via_grant: bool,
    apply_exec: Option<&ApplyExecFn<'_>>,
    params: SuggestionApplyParams,
) -> Result<SuggestionApplyResult, ApiError> {
    if !via_grant {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "applying suggestions requires a verified human HostChannel grant",
        ));
    }
    let CallerContext::HumanCaller { caller_id, .. } = context else {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "only the addressed human caller may apply a suggestion",
        ));
    };
    let exec = apply_exec.ok_or_else(|| {
        ApiError::new(
            ErrorCode::Unavailable,
            "proposal apply execution is unavailable on this office",
        )
    })?;
    let caller_revision =
        parse_caller_revision(params.expected_revision, params.if_match.as_deref())?;

    let now_ms = svc.now_ms();
    let reserved = {
        let mut store = svc.suggestions.lock().unwrap_or_else(|p| p.into_inner());
        store.reserve_for_apply(&params.suggestion_id, caller_id, now_ms)?
    };
    let mut guard = ApplyInFlightGuard::new(svc, reserved.id.clone(), caller_id.clone());

    let effective_revision = reconcile_revision(reserved.expected_revision, caller_revision)?;
    let live_authority = svc.office().caller_authority(caller_id).map_err(|e| {
        unavailable(
            "tess_calls.apply_authority",
            &e,
            "unable to verify caller authority",
        )
    })?;
    if !(live_authority.active && live_authority.is_person) {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "caller is no longer an active person",
        ));
    }

    let row = resolve_target_command(&reserved.command)?;
    let preflight_ctx = PreflightContext {
        tess_id: &reserved.created_by_member_id,
        caller_id,
        authority: &live_authority,
    };
    let preflight = preflight_target(
        svc,
        &preflight_ctx,
        row.call,
        &reserved.input,
        effective_revision,
    )?;

    let applied_result = execute_command_as_human(
        svc,
        caller_id,
        row,
        &preflight.call,
        &reserved,
        effective_revision,
        exec,
    )?;

    let applied_at =
        format_rfc3339_ms(svc.now_ms()).unwrap_or_else(|| "1970-01-01T00:00:00.000Z".into());
    let updated_view = {
        let mut store = svc.suggestions.lock().unwrap_or_else(|p| p.into_inner());
        let view = store.complete_apply(&reserved.id, caller_id, applied_at.clone())?;
        guard.disarm();
        view
    };

    let attribution = SuggestionAttribution {
        actor_member_id: caller_id.clone(),
        proposed_by_member_id: updated_view.created_by_member_id.clone(),
    };
    let receipt = build_receipt(&updated_view, &applied_result, applied_at);

    Ok(SuggestionApplyResult {
        suggestion: updated_view,
        applied_result,
        attribution,
        receipt,
    })
}

pub(crate) fn parse_caller_revision(
    expected_revision: Option<u64>,
    if_match: Option<&str>,
) -> Result<Option<u64>, ApiError> {
    let parsed_if_match = match if_match {
        None => None,
        Some(raw) => {
            let trimmed = raw.trim();
            let digits = trimmed
                .strip_prefix("rev-")
                .or_else(|| trimmed.strip_prefix('v'))
                .unwrap_or(trimmed);
            let rev = digits.parse::<u64>().map_err(|_| {
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    "`if_match` must be a non-negative integer revision",
                )
            })?;
            Some(rev)
        }
    };
    if let (Some(a), Some(b)) = (expected_revision, parsed_if_match) {
        if a != b {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "`expected_revision` and `if_match` disagree",
            ));
        }
    }
    Ok(expected_revision.or(parsed_if_match))
}

pub(crate) fn reconcile_revision(
    stored: Option<u64>,
    caller: Option<u64>,
) -> Result<Option<u64>, ApiError> {
    if let (Some(stored_rev), Some(caller_rev)) = (stored, caller) {
        if stored_rev != caller_rev {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                format!(
                    "proposal pinned revision {stored_rev}, which does not match caller revision {caller_rev}"
                ),
            ));
        }
    }
    Ok(stored.or(caller))
}

pub(crate) fn prepare_apply_rpc(
    next_id: &mut impl FnMut() -> String,
    resource: &str,
    verb: &str,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Option<Result<PreparedRpc, ApiError>> {
    translate_org::try_prepare(next_id, resource, verb, input, expected_revision)
        .or_else(|| translate_work::try_prepare(next_id, resource, verb, input, expected_revision))
        .or_else(|| {
            translate_checklist::try_prepare(next_id, resource, verb, input, expected_revision)
        })
        .or_else(|| translate_msg::try_prepare(next_id, resource, verb, input))
}

fn execute_command_as_human(
    svc: &TessService,
    caller_id: &str,
    row: &'static Command,
    preflight_call: &str,
    suggestion: &SuggestionView,
    effective_revision: Option<u64>,
    exec: &ApplyExecFn<'_>,
) -> Result<Value, ApiError> {
    let rev = (row.if_match() != IfMatch::No)
        .then_some(effective_revision)
        .flatten();
    let mut next_id = || svc.next_uuid_v7();
    let prepared = prepare_apply_rpc(&mut next_id, row.resource, row.verb, &suggestion.input, rev)
        .ok_or_else(|| {
            ApiError::new(
                ErrorCode::Unsupported,
                format!("proposal apply does not support `{}`", row.name()),
            )
        })??;

    let mut ctx = ApplyExecContext {
        caller_id,
        allowed_write_a: preflight_call,
        allowed_write_b: row.call,
        exec,
        subcalls: 0,
    };
    let raw = ctx.call(prepared.method, prepared.params)?;
    shape_output(prepared.method, prepared.shape, raw)
}

fn shape_output(method: &str, shape: OutputShape, raw: Value) -> Result<Value, ApiError> {
    match shape {
        OutputShape::Passthrough => Ok(raw),
        OutputShape::OrgAccepted { target_id } => {
            translate_org::shape_org_accepted(method, &target_id, raw)
        }
        OutputShape::TaskRow => translate_work::shape_task_row(method, raw),
        OutputShape::TaskComment => translate_work::shape_task_comment(method, raw),
        OutputShape::Checklist {
            fallback_ids,
            removed,
        } => translate_checklist::shape_receipt(method, raw, &fallback_ids, removed),
        OutputShape::MsgSend => translate_msg::shape_msg_send(raw),
        OutputShape::MsgAsk => translate_msg::shape_msg_ask(raw),
        OutputShape::MsgWithdraw => translate_msg::shape_msg_withdraw(raw),
        OutputShape::MsgDone => translate_msg::shape_msg_done(raw),
        OutputShape::MsgDefer => translate_msg::shape_msg_defer(raw),
        OutputShape::MsgUnsnooze => translate_msg::shape_msg_unsnooze(raw),
        OutputShape::ChannelCreate => translate_msg::shape_channel_create(raw),
        OutputShape::ChannelMembership => translate_msg::shape_channel_membership(method, raw),
        OutputShape::ChannelMarkRead => translate_msg::shape_channel_mark_read(raw),
    }
}

fn build_receipt(
    suggestion: &SuggestionView,
    applied_result: &Value,
    applied_at: String,
) -> SuggestionApplyReceipt {
    let (entity_href, entity_label) = extract_entity_link(&suggestion.call, applied_result);
    SuggestionApplyReceipt {
        summary: suggestion.summary.clone(),
        entity_href,
        entity_label,
        applied_at,
    }
}

fn field_str(data: &Value, key: &str) -> Option<String> {
    data.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn nested_field_str(data: &Value, parent: &str, key: &str) -> Option<String> {
    data.get(parent)
        .and_then(|v| v.get(key))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn extract_task_link(data: &Value) -> Option<(String, String)> {
    let key = field_str(data, "key")?;
    let label = field_str(data, "title").unwrap_or_else(|| key.clone());
    Some((format!("/work/{key}"), label))
}

fn extract_project_link(call: &str, data: &Value) -> Option<(String, String)> {
    let allow_bare_id = !matches!(call, "project.add_team" | "project.remove_team");
    let id = field_str(data, "project_id")
        .or_else(|| allow_bare_id.then(|| field_str(data, "id")).flatten())?;
    let label = field_str(data, "name")
        .or_else(|| field_str(data, "key"))
        .unwrap_or_else(|| id.clone());
    Some((format!("/projects/{id}"), label))
}

fn extract_process_link(data: &Value) -> Option<(String, String)> {
    let id = field_str(data, "document_id")
        .or_else(|| nested_field_str(data, "document", "id"))
        .or_else(|| field_str(data, "id"))?;
    let label = nested_field_str(data, "document", "title")
        .or_else(|| field_str(data, "title"))
        .unwrap_or_else(|| id.clone());
    Some((format!("/settings/processes/documents/{id}"), label))
}

fn extract_id_link(
    data: &Value,
    primary_key: &str,
    fallback_key: &str,
    label_key: &str,
    prefix: &str,
) -> Option<(String, String)> {
    let id = field_str(data, primary_key).or_else(|| field_str(data, fallback_key))?;
    let label = field_str(data, label_key).unwrap_or_else(|| id.clone());
    Some((format!("{prefix}{id}"), label))
}

fn extract_org_entity_link(call: &str, data: &Value) -> Option<(String, String)> {
    match call.split('.').next().unwrap_or("") {
        "project" => extract_project_link(call, data),
        "team" if call == "team.set_title" => {
            extract_id_link(data, "member_id", "id", "name", "/members/")
        }
        "team" => extract_id_link(data, "team_id", "id", "name", "/teams/"),
        "member" => extract_id_link(data, "member_id", "id", "name", "/members/"),
        "role" => extract_id_link(data, "role_id", "id", "name", "/settings/roles/"),
        _ => None,
    }
}

fn extract_work_or_chat_link(call: &str, data: &Value) -> Option<(String, String)> {
    if call.starts_with("tasks.") {
        return extract_task_link(data);
    }
    if call.starts_with("process.") {
        return extract_process_link(data);
    }
    if call.starts_with("chat.channel_") {
        return extract_id_link(data, "id", "conversation_id", "name", "/chat?c=");
    }
    if call.starts_with("hr.hire") {
        let id = field_str(data, "request_id").or_else(|| field_str(data, "id"))?;
        return Some(("/teams/hires".to_owned(), format!("Hire {id}")));
    }
    None
}

pub(crate) fn extract_entity_link(call: &str, data: &Value) -> (Option<String>, Option<String>) {
    match extract_work_or_chat_link(call, data).or_else(|| extract_org_entity_link(call, data)) {
        Some((href, label)) => (Some(href), Some(label)),
        None => (None, None),
    }
}

// --- Shared pure input helpers for apply translators ---

pub(super) fn invalid(msg: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::InvalidRequest, msg)
}

pub(super) fn unreadable(method: &str) -> ApiError {
    ApiError::new(
        ErrorCode::Unavailable,
        format!("office returned an unreadable `{method}` reply"),
    )
}

pub(super) fn opt_str<'a>(input: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    input.get(key).and_then(Value::as_str)
}

pub(super) fn opt_string(input: &Map<String, Value>, key: &str) -> Option<String> {
    opt_str(input, key).map(str::to_owned)
}

pub(super) fn require_str<'a>(
    input: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a str, ApiError> {
    opt_str(input, key).ok_or_else(|| invalid(format!("missing required `{key}`")))
}

pub(super) fn opt_bool(input: &Map<String, Value>, key: &str) -> Option<bool> {
    input.get(key).and_then(Value::as_bool)
}

pub(super) fn is_set(input: &Map<String, Value>, key: &str) -> bool {
    opt_bool(input, key).unwrap_or(false)
}

pub(super) fn opt_u64(input: &Map<String, Value>, key: &str) -> Result<Option<u64>, ApiError> {
    match input.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .map(Some)
            .ok_or_else(|| invalid(format!("`{key}` must be a non-negative integer"))),
    }
}

pub(super) fn opt_str_list(input: &Map<String, Value>, key: &str) -> Option<Vec<String>> {
    input.get(key).and_then(Value::as_array).map(|items| {
        items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    })
}

pub(super) fn require_rev(expected_revision: Option<u64>) -> Result<u64, ApiError> {
    expected_revision.ok_or_else(|| invalid("this update requires `expected_revision`"))
}

pub(super) fn insert_opt(map: &mut Map<String, Value>, key: &str, val: Option<&str>) {
    if let Some(val) = val {
        map.insert(key.into(), Value::String(val.to_owned()));
    }
}
