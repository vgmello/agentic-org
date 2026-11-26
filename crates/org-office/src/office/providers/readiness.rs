//! Agent readiness: evaluating the provider pool candidates an agent could run on.

use org_proto::providers::{
    AgentReadinessCode, AgentReadinessGetParams, AgentReadinessResult, ConnectionCandidateStatus,
    ConnectionReadinessStatus, DiscoveredModelEntitlement, TurnPermitState,
};
use org_proto::{ApiError, ErrorCode};

use crate::error::OfficeError;
use crate::office::Office;
use crate::organization::records::ModelProfile;

pub(super) fn op_agent_readiness(
    office: &Office,
    actor: &str,
    params: AgentReadinessGetParams,
) -> Result<Result<AgentReadinessResult, ApiError>, OfficeError> {
    if let Err(e) = authorize_agent_readiness(office, actor, &params)? {
        return Ok(Err(e));
    }
    let Some(exec) = office.load_agent_execution(&params.employee_id)? else {
        return Ok(Ok(empty_readiness(
            &params.employee_id,
            AgentReadinessCode::NoExecutionConfig,
        )));
    };
    if !exec.enabled {
        return Ok(Ok(AgentReadinessResult {
            employee_id: params.employee_id,
            ready: false,
            status: AgentReadinessCode::ExecutionDisabled,
            model_profile_id: Some(exec.model_profile_id.as_str().to_owned()),
            model_id: Some(exec.model_id),
            candidates: Vec::new(),
            earliest_cooldown_until: None,
        }));
    }
    let Some(profile) = office.load_model_profile(exec.model_profile_id.as_str())? else {
        return Ok(Ok(empty_readiness(
            &params.employee_id,
            AgentReadinessCode::ProfileArchived,
        )));
    };
    if profile.archived {
        return Ok(Ok(AgentReadinessResult {
            employee_id: params.employee_id,
            ready: false,
            status: AgentReadinessCode::ProfileArchived,
            model_profile_id: Some(profile.id.as_str().to_owned()),
            model_id: Some(exec.model_id),
            candidates: Vec::new(),
            earliest_cooldown_until: None,
        }));
    }
    let holds = office.active_budget_holds(Some(&params.employee_id), params.task_id.as_deref())?;
    let budget_held = !holds.is_empty();
    let now_ms = office.monotonic_now_ms();
    let snapshot = office
        .providers
        .quota_snapshot(&office.info.org_id, None, now_ms)?;
    let eval = evaluate_pool_candidates(office, &profile, &exec.model_id, &snapshot, now_ms)?;

    let (ready, status) = if budget_held {
        (false, AgentReadinessCode::BudgetHeld)
    } else if eval.any_ready {
        (true, AgentReadinessCode::Ready)
    } else if eval.any_exhausted {
        (false, AgentReadinessCode::PoolExhausted)
    } else if eval.any_unverified {
        (false, AgentReadinessCode::ModelUnverified)
    } else {
        (false, AgentReadinessCode::NoReadyConnection)
    };
    Ok(Ok(AgentReadinessResult {
        employee_id: params.employee_id,
        ready,
        status,
        model_profile_id: Some(profile.id.as_str().to_owned()),
        model_id: Some(exec.model_id),
        candidates: eval.candidates,
        earliest_cooldown_until: eval.earliest_cooldown,
    }))
}

fn empty_readiness(emp_id: &str, status: AgentReadinessCode) -> AgentReadinessResult {
    AgentReadinessResult {
        employee_id: emp_id.to_owned(),
        ready: false,
        status,
        model_profile_id: None,
        model_id: None,
        candidates: Vec::new(),
        earliest_cooldown_until: None,
    }
}

struct PoolCandidateEvaluation {
    candidates: Vec<ConnectionCandidateStatus>,
    earliest_cooldown: Option<String>,
    any_ready: bool,
    any_unverified: bool,
    any_exhausted: bool,
}

fn evaluate_pool_candidates(
    office: &Office,
    profile: &ModelProfile,
    target_model: &str,
    snapshot: &crate::providers::PermitSnapshot,
    now_ms: u64,
) -> Result<PoolCandidateEvaluation, OfficeError> {
    let mut candidates = Vec::new();
    let mut earliest_cd: Option<String> = None;
    let mut any_ready = false;
    let mut any_unverified = false;
    let mut any_exhausted = false;

    for (idx, cid) in profile.connection_ids.iter().enumerate() {
        let priority = (idx as u64) + 1;
        let cand = evaluate_single_candidate(
            office,
            cid.as_str(),
            priority,
            target_model,
            snapshot,
            now_ms,
        )?;
        if cand.eligible {
            any_ready = true;
        } else {
            match cand.skip_reason.as_deref() {
                Some("MODEL_UNVERIFIED") => any_unverified = true,
                Some("COOLDOWN") | Some("CAPACITY_FULL") => any_exhausted = true,
                _ => {}
            }
        }
        if let Some(ref cd) = cand.cooldown_until {
            if earliest_cd.as_deref().is_none_or(|e| cd.as_str() < e) {
                earliest_cd = Some(cd.clone());
            }
        }
        candidates.push(cand);
    }
    Ok(PoolCandidateEvaluation {
        candidates,
        earliest_cooldown: earliest_cd,
        any_ready,
        any_unverified,
        any_exhausted,
    })
}

fn evaluate_single_candidate(
    office: &Office,
    cid: &str,
    priority: u64,
    target_model: &str,
    snapshot: &crate::providers::PermitSnapshot,
    now_ms: u64,
) -> Result<ConnectionCandidateStatus, OfficeError> {
    let Some(conn) = office.load_provider_connection(cid)? else {
        return Ok(skip_cand(cid, priority, "CONNECTION_NOT_FOUND", None));
    };
    if conn.archived || !conn.enabled {
        return Ok(skip_cand(cid, priority, "CONNECTION_DISABLED", None));
    }
    let Some(binding) = office.load_connection_binding(cid)? else {
        return Ok(skip_cand(cid, priority, "NOT_BOUND", None));
    };
    let Some(state) = office.load_connection_state(cid)? else {
        return Ok(skip_cand(cid, priority, "STATE_UNKNOWN", None));
    };
    if state.is_stale(binding.revision, now_ms) || state.status != ConnectionReadinessStatus::Ready
    {
        let code = if state.status == ConnectionReadinessStatus::Ready {
            "STATE_STALE".to_owned()
        } else {
            state
                .issue_code
                .unwrap_or_else(|| state.status.as_str().to_ascii_uppercase())
        };
        return Ok(skip_cand(cid, priority, &code, None));
    }
    let models = office.load_connection_models(cid)?;
    let matched = models.into_iter().find(|m| {
        m.model_id == target_model
            && m.entitlement == DiscoveredModelEntitlement::Confirmed
            && !m.is_stale(now_ms)
    });
    let Some(model) = matched else {
        return Ok(skip_cand(cid, priority, "MODEL_UNVERIFIED", None));
    };
    let cd = snapshot
        .cooldowns
        .iter()
        .filter(|q| {
            q.connection_id == cid
                && (q.model_id.is_none() || q.model_id.as_deref() == Some(target_model))
                && q.is_cooling_down(now_ms)
        })
        .filter_map(|q| q.cooldown_until.clone())
        .max();
    if let Some(until) = cd {
        return Ok(skip_cand(cid, priority, "COOLDOWN", Some(until)));
    }
    let occupied = snapshot
        .permits
        .iter()
        .filter(|p| p.connection_id == cid && p.state != TurnPermitState::Released)
        .count() as u64;
    if occupied >= conn.max_concurrent_turns {
        return Ok(skip_cand(cid, priority, "CAPACITY_FULL", None));
    }
    Ok(ConnectionCandidateStatus {
        connection_id: cid.to_owned(),
        priority,
        eligible: true,
        skip_reason: None,
        cooldown_until: None,
        driver_model_id: Some(model.driver_model_id),
    })
}

fn skip_cand(
    cid: &str,
    priority: u64,
    reason: &str,
    cooldown_until: Option<String>,
) -> ConnectionCandidateStatus {
    ConnectionCandidateStatus {
        connection_id: cid.to_owned(),
        priority,
        eligible: false,
        skip_reason: Some(reason.to_owned()),
        cooldown_until,
        driver_model_id: None,
    }
}

fn authorize_agent_readiness(
    office: &Office,
    actor: &str,
    params: &AgentReadinessGetParams,
) -> Result<Result<(), ApiError>, OfficeError> {
    if let Err(e) = office.require_active_member(actor)? {
        return Ok(Err(e));
    }
    if actor == office.owner_member_id() {
        return Ok(Ok(()));
    }
    let Some(emp) = office.load_employee(actor)? else {
        return Ok(Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "caller not found",
        )));
    };
    if emp.kind == crate::organization::records::EmployeeKind::Person {
        return Ok(Ok(()));
    }
    if params.employee_id == actor && params.task_id.is_none() {
        return Ok(Ok(()));
    }
    Ok(Err(ApiError::new(
        ErrorCode::NotAuthorized,
        "agents may only inspect their own readiness without task_id probing",
    )))
}
