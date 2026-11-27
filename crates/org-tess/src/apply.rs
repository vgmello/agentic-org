//! Two-phase human-authenticated HostChannel `suggestion.apply` execution (`TASK-80.4` Part B).
//!
//! Security boundary:
//! - Strictly `via_grant: true` (verified human HostChannel grant ONLY — never Tess or any agent).
//! - Verifies `suggestion.for_caller_id == caller_id` (fails closed with `NotFound` for any other
//!   caller — zero existence oracle).
//! - Reserves `applying: true` atomically under the suggestion mutex (with a panic-safe RAII
//!   guard) and drops the mutex before re-running live preflight, `expected_revision` CAS, and
//!   target command execution under the human caller's own `via_grant: true` authority.
//! - Transitions to `SuggestionStatus::Applied` atomically (single-use; replay returns `Conflict`).

use org_agent_cli::{IfMatch, OfficeTransport, Refusal, TransportOffice};
use org_proto::tess::{
    SuggestionApplyParams, SuggestionApplyReceipt, SuggestionApplyResult, SuggestionAttribution,
    SuggestionView,
};
use org_proto::{format_rfc3339_ms, ApiError, ErrorCode, Outcome, Request};
use serde_json::Value;

use crate::ports::ApplyExecFn;
use crate::preflight::{preflight_target, resolve_target_command, PreflightContext};
use crate::{unavailable, CallerContext, TessService};

/// Maximum RPC sub-calls permitted while executing a single applied proposal.
const MAX_APPLY_SUBCALLS: usize = 8;

/// Read-only lookup RPCs that `agent-os` command handlers may issue while executing an allowlisted
/// write proposal (never any write, auth, provider, session, or Tess method).
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

struct ServerApplyTransport<'a> {
    caller_id: &'a str,
    allowed_write_a: &'a str,
    allowed_write_b: &'a str,
    exec: &'a ApplyExecFn<'a>,
    subcalls: usize,
    last_error: Option<ApiError>,
}

impl<'a> ServerApplyTransport<'a> {
    fn is_permitted_call(&self, method: &str) -> bool {
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
        method == self.allowed_write_a
            || method == self.allowed_write_b
            || APPLY_HELPER_READ_CALLS.contains(&method)
    }
}

impl OfficeTransport for ServerApplyTransport<'_> {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        if !self.is_permitted_call(method) {
            let err = ApiError::new(
                ErrorCode::NotAuthorized,
                format!("proposal apply cannot invoke `{method}`"),
            );
            self.last_error = Some(err.clone());
            return Err(org_agent_cli::api_refusal(err));
        }
        if self.subcalls >= MAX_APPLY_SUBCALLS {
            let err = ApiError::new(
                ErrorCode::Unavailable,
                "proposal apply exceeded maximum sub-call limit",
            );
            self.last_error = Some(err.clone());
            return Err(org_agent_cli::api_refusal(err));
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
            Outcome::Error(err) => {
                self.last_error = Some(err.clone());
                Err(org_agent_cli::api_refusal(err))
            }
        }
    }

    fn is_bound(&self) -> bool {
        true
    }

    fn caller_member_id(&self) -> Option<String> {
        Some(self.caller_id.to_owned())
    }
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

fn execute_command_as_human(
    svc: &TessService,
    caller_id: &str,
    row: &'static org_agent_cli::Command,
    preflight_call: &str,
    suggestion: &SuggestionView,
    effective_revision: Option<u64>,
    exec: &ApplyExecFn<'_>,
) -> Result<Value, ApiError> {
    let idempotency_key = svc.next_uuid_v7();
    let input_json =
        serde_json::to_string(&Value::Object(suggestion.input.clone())).map_err(|_| {
            ApiError::new(
                ErrorCode::InvalidRequest,
                "unable to serialize proposal input",
            )
        })?;
    let mut args = vec![
        row.resource.to_owned(),
        row.verb.to_owned(),
        "--format=json".to_owned(),
        format!("--idempotency-key={idempotency_key}"),
        "--json".to_owned(),
        input_json,
    ];
    if row.if_match() != IfMatch::No {
        if let Some(rev) = effective_revision {
            args.push(format!("--if-match={rev}"));
        }
    }

    let mut transport = ServerApplyTransport {
        caller_id,
        allowed_write_a: preflight_call,
        allowed_write_b: row.call,
        exec,
        subcalls: 0,
        last_error: None,
    };
    let output =
        org_agent_cli::execute_with_office("agent-os", &args, &mut TransportOffice(&mut transport));
    let parsed: Value = serde_json::from_str(output.stdout.trim()).map_err(|_| {
        transport.last_error.clone().unwrap_or_else(|| {
            ApiError::new(
                ErrorCode::Unavailable,
                "applied command returned unreadable output",
            )
        })
    })?;
    if output.exit_code == 0 && parsed.get("ok").and_then(Value::as_bool) == Some(true) {
        return Ok(parsed.get("data").cloned().unwrap_or(Value::Null));
    }
    if let Some(err) = transport.last_error {
        return Err(err);
    }
    let code = parsed
        .pointer("/error/code")
        .and_then(Value::as_str)
        .and_then(|c| serde_json::from_value::<ErrorCode>(Value::String(c.to_owned())).ok())
        .unwrap_or(ErrorCode::InvalidRequest);
    let message = parsed
        .pointer("/error/message")
        .and_then(Value::as_str)
        .unwrap_or("proposal execution failed")
        .to_owned();
    Err(ApiError::new(code, message))
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

fn extract_entity_link(call: &str, data: &Value) -> (Option<String>, Option<String>) {
    let str_at = |key: &str| data.get(key).and_then(Value::as_str).map(str::to_owned);
    if call.starts_with("tasks.") {
        if let Some(key) = str_at("key") {
            let label = str_at("title").unwrap_or_else(|| key.clone());
            return (Some(format!("/tasks/{key}")), Some(label));
        }
    } else if call.starts_with("project.") {
        if let Some(key) = str_at("key") {
            let label = str_at("name").unwrap_or_else(|| key.clone());
            return (Some(format!("/projects/{key}")), Some(label));
        }
    } else if call.starts_with("team.") {
        if let Some(id) = str_at("team_id").or_else(|| str_at("id")) {
            let label = str_at("name").unwrap_or_else(|| id.clone());
            return (Some(format!("/org/teams/{id}")), Some(label));
        }
    } else if call.starts_with("chat.channel_") {
        if let Some(id) = str_at("id") {
            let label = str_at("name").unwrap_or_else(|| id.clone());
            return (Some(format!("/chat/channels/{id}")), Some(label));
        }
    } else if call.starts_with("hr.hire") {
        if let Some(id) = str_at("request_id").or_else(|| str_at("id")) {
            return (Some(format!("/hr/hires/{id}")), Some(format!("Hire {id}")));
        }
    }
    (None, None)
}
