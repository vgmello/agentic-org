//! Live hiring scope, same-caller draft ownership, and revision preflight for `hr.hire*`
//! `suggestion.create` proposals (`TASK-80.4`).

use org_proto::hiring::{HireRequestRecord, HrHireInspect, HrHireInspectParams};
use org_proto::{ApiError, ErrorCode, Outcome, Request};
use serde_json::{Map, Value};

use super::{PreflightContext, PreflightOutcome, HIRE_NOT_VISIBLE};
use crate::tess_calls::TessService;

pub(super) fn preflight_hire(
    svc: &TessService,
    ctx: &PreflightContext<'_>,
    call: &str,
    input: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> Result<PreflightOutcome, ApiError> {
    if !(ctx.authority.is_owner || ctx.authority.may_hire) {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "caller does not have hiring authority",
        ));
    }
    let is_draft = input.get("draft").and_then(Value::as_bool).unwrap_or(false);
    let is_withdraw = input
        .get("withdraw")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match call {
        "hr.hire" | "hr.hire.draft" => {
            if let Some(req_id) = input.get("id").and_then(Value::as_str) {
                if is_draft || call == "hr.hire.draft" {
                    return Err(ApiError::new(
                        ErrorCode::InvalidRequest,
                        "`draft` cannot be combined with `id`",
                    ));
                }
                let resolved_call = if is_withdraw {
                    "hr.hire.withdraw"
                } else {
                    "hr.hire.confirm"
                };
                return preflight_existing_hire(svc, ctx, resolved_call, req_id, expected_revision);
            }
            if is_withdraw {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "`withdraw` requires `id`",
                ));
            }
            let team = input
                .get("team")
                .or_else(|| input.get("home_team_id"))
                .and_then(Value::as_str)
                .ok_or_else(|| ApiError::new(ErrorCode::InvalidRequest, "missing target team"))?;
            check_caller_hire_team(svc, ctx, team)?;
            let resolved_call = if is_draft || call == "hr.hire.draft" {
                "hr.hire.draft"
            } else {
                "hr.hire"
            };
            Ok(PreflightOutcome::ok(resolved_call, "hiring scope verified"))
        }
        "hr.hire.confirm" | "hr.hire.withdraw" => {
            let req_id = input
                .get("id")
                .or_else(|| input.get("request"))
                .or_else(|| input.get("request_id"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ApiError::new(ErrorCode::InvalidRequest, "missing hire request ID")
                })?;
            preflight_existing_hire(svc, ctx, call, req_id, expected_revision)
        }
        _ => Ok(PreflightOutcome::ok(call, "hiring authority verified")),
    }
}

fn preflight_existing_hire(
    svc: &TessService,
    ctx: &PreflightContext<'_>,
    resolved_call: &str,
    req_id: &str,
    expected_revision: Option<u64>,
) -> Result<PreflightOutcome, ApiError> {
    let record = inspect_hire_request(svc, ctx.caller_id, req_id)?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, HIRE_NOT_VISIBLE))?;
    if !ctx.authority.is_owner {
        let may_hire = svc
            .office()
            .caller_may_hire_into(ctx.caller_id, &record.home_team_id)
            .map_err(|err| {
                crate::logged("tess_calls.may_hire_into", &err);
                ApiError::new(ErrorCode::Unavailable, "unable to check hiring scope")
            })?;
        let owns_draft = record.requested_by_employee_id == ctx.caller_id
            || svc.draft_caller(req_id).as_deref() == Some(ctx.caller_id);
        if !(may_hire && owns_draft) {
            return Err(ApiError::new(ErrorCode::NotFound, HIRE_NOT_VISIBLE));
        }
    }
    let Some(expected) = expected_revision else {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "confirming or withdrawing an existing hire request requires `expected_revision`",
        ));
    };
    if record.revision != expected {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            format!(
                "hire request `{req_id}` is at revision {}, not expected revision {expected}",
                record.revision
            ),
        ));
    }
    Ok(PreflightOutcome::ok(
        resolved_call,
        format!("verified hire request at revision {}", record.revision),
    ))
}

fn inspect_hire_request(
    svc: &TessService,
    caller_id: &str,
    req_id: &str,
) -> Result<Option<HireRequestRecord>, ApiError> {
    if !org_proto::is_uuid_v7(req_id) {
        return Ok(None);
    }
    let req = Request::new::<HrHireInspect>(
        0,
        &HrHireInspectParams {
            request_id: req_id.to_owned(),
        },
    )
    .map_err(|_| ApiError::new(ErrorCode::Unavailable, "unable to construct hire lookup"))?;
    let Some(resp) = svc.office().dispatch_hiring_rpc(Some(caller_id), &req) else {
        return Err(ApiError::new(
            ErrorCode::Unavailable,
            "hire lookup is unavailable",
        ));
    };
    match resp.outcome {
        Outcome::Result(v) => serde_json::from_value(v)
            .map(Some)
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "invalid hire response")),
        Outcome::Error(err) if err.code == ErrorCode::NotFound => Ok(None),
        Outcome::Error(err) => Err(err),
    }
}

fn check_caller_hire_team(
    svc: &TessService,
    ctx: &PreflightContext<'_>,
    team_id: &str,
) -> Result<(), ApiError> {
    let snap = svc.office().org_snapshot().map_err(|err| {
        crate::logged("tess_calls.org_snapshot", &err);
        ApiError::new(ErrorCode::Unavailable, "unable to inspect organization")
    })?;
    if !snap.teams.iter().any(|t| t.id == team_id) {
        return Err(ApiError::new(
            ErrorCode::NotFound,
            format!("target team `{team_id}` not found"),
        ));
    }
    if ctx.authority.is_owner {
        return Ok(());
    }
    let ok = svc
        .office()
        .caller_may_hire_into(ctx.caller_id, team_id)
        .map_err(|err| {
            crate::logged("tess_calls.may_hire_into", &err);
            ApiError::new(ErrorCode::Unavailable, "unable to check hiring scope")
        })?;
    if !ok {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "caller may not hire into the requested team",
        ));
    }
    Ok(())
}
