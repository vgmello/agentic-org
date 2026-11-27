//! Two-stage warm/fresh session affinity guard (`TASK-92.1`, ADR-0028).
//!
//! Validates `MemberSessionStartParams::{fresh, resume_runtime_session_id}` and decides whether
//! a requested `runtime_session_id` may be resumed safely or must fall back to a clean fresh
//! session with an explicit notice (guarding runtimes where `resume_ignores_cwd` is true or
//! `resume_applies_instructions` is false, such as Claude Code).

use org_agent_runtime::Capabilities;
use org_proto::sessions::{MemberSessionStartParams, SessionState};
use org_proto::{ApiError, ErrorCode};

use crate::binding::RunBindingRegistry;
use crate::credentials::rfc3339_now;
use crate::recovery::{SessionAffinityEntry, SessionStore};
use crate::transcript::is_safe_session_id;

/// How a session start treats an already-live warm handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum WarmReuseControl<'a> {
    /// Normal behavior: reuse an idle/running handle when workspace & route still match.
    #[default]
    Default,
    /// Force a brand-new clean session (`fresh = true`).
    ForceFresh,
    /// Target an explicit prior runtime session ID; recycle an idle handle so launch runs
    /// the `(same_runtime && same_workspace && same_spec_hash)` guard.
    RequireSession(&'a str),
}

/// Decides whether `control` forces recycling an idle warm handle or refuses a busy one.
pub(super) fn control_stop_reason(
    control: WarmReuseControl<'_>,
    state: SessionState,
) -> Result<Option<&'static str>, ApiError> {
    match control {
        WarmReuseControl::Default => Ok(None),
        WarmReuseControl::ForceFresh => {
            if state != SessionState::Idle {
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    "cannot start a fresh session while agent is busy",
                ));
            }
            Ok(Some("fresh_session_requested"))
        }
        WarmReuseControl::RequireSession(_) => {
            if state != SessionState::Idle {
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    "cannot switch runtime session while agent is busy",
                ));
            }
            Ok(Some("session_switch_requested"))
        }
    }
}

/// Runs [`resolve_safe_resume`] and appends a `resume_fallback_fresh` history event on fallback.
pub(super) fn resolve_and_audit_resume(
    store: &SessionStore,
    bindings: &RunBindingRegistry,
    check: &ResumeAffinityCheck<'_>,
) -> ResumeResolution {
    let res = resolve_safe_resume(store, check);
    if let Some(reason) = res.fallback_reason {
        let tokens = bindings.active_tokens();
        let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
        let _ = store.append_history(
            check.employee_id,
            &serde_json::json!({
                "type": "resume_fallback_fresh",
                "requested_runtime_session_id": check.requested_resume,
                "reason": reason,
                "at": rfc3339_now(),
            }),
            &refs,
        );
    }
    res
}

/// Inputs to the safe-resume affinity check.
pub(super) struct ResumeAffinityCheck<'a> {
    pub(super) employee_id: &'a str,
    pub(super) requested_resume: Option<&'a str>,
    pub(super) force_fresh: bool,
    pub(super) enforce_affinity_guard: bool,
    /// `true` strictly for a `WakeKind::SandboxRefresh` continuation recycle (`TASK-160`), where
    /// mid-turn `worktree.prepare` deliberately updated sandbox writable roots / settings without
    /// changing runtime or workspace.
    pub(super) allow_sandbox_spec_change: bool,
    pub(super) runtime_id: &'a str,
    pub(super) workspace: &'a str,
    pub(super) spec_hash: &'a str,
    pub(super) capabilities: Capabilities,
}

/// Outcome of deciding whether to pass `resume` to the runtime or fall back to fresh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResumeResolution {
    pub(super) effective_resume: Option<String>,
    pub(super) fallback_notice: Option<String>,
    pub(super) fallback_reason: Option<&'static str>,
}

/// Validates `fresh` and `resume_runtime_session_id` on `MemberSessionStartParams`.
pub(super) fn validate_start_affinity_params(
    params: &MemberSessionStartParams,
) -> Result<WarmReuseControl<'_>, ApiError> {
    if params.fresh && params.resume_runtime_session_id.is_some() {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "fresh cannot be combined with resume_runtime_session_id",
        ));
    }
    if let Some(raw) = params.resume_runtime_session_id.as_deref() {
        if raw.trim().is_empty() || !is_safe_session_id(raw) {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "invalid resume_runtime_session_id",
            ));
        }
        return Ok(WarmReuseControl::RequireSession(raw));
    }
    if params.fresh {
        return Ok(WarmReuseControl::ForceFresh);
    }
    Ok(WarmReuseControl::Default)
}

/// Resolves whether `check.requested_resume` may safely be resumed under `(same_runtime &&
/// same_workspace && same_spec_hash)`.
pub(super) fn resolve_safe_resume(
    store: &SessionStore,
    check: &ResumeAffinityCheck<'_>,
) -> ResumeResolution {
    if check.force_fresh {
        return ResumeResolution {
            effective_resume: None,
            fallback_notice: None,
            fallback_reason: None,
        };
    }
    let Some(requested) = check
        .requested_resume
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return ResumeResolution {
            effective_resume: None,
            fallback_notice: None,
            fallback_reason: None,
        };
    };
    let recorded = store
        .lookup_session_affinity(check.employee_id, requested)
        .ok()
        .flatten();
    let needs_strict_guard = check.enforce_affinity_guard || check.capabilities.resume_ignores_cwd;
    match recorded {
        Some(entry) => evaluate_recorded_entry(requested, &entry, check),
        None if needs_strict_guard => fallback(
            requested,
            "unverified_session_affinity",
            "no recorded workspace/instruction snapshot matched that session",
        ),
        None => ResumeResolution {
            effective_resume: Some(requested.to_owned()),
            fallback_notice: None,
            fallback_reason: None,
        },
    }
}

fn evaluate_recorded_entry(
    requested: &str,
    entry: &SessionAffinityEntry,
    check: &ResumeAffinityCheck<'_>,
) -> ResumeResolution {
    let same_runtime = entry.runtime_id == check.runtime_id;
    let same_workspace = entry.workspace == check.workspace;
    let same_spec_hash = entry.spec_hash == check.spec_hash;
    if same_runtime && same_workspace && same_spec_hash {
        return ResumeResolution {
            effective_resume: Some(requested.to_owned()),
            fallback_notice: None,
            fallback_reason: None,
        };
    }
    let strict_mismatch = check.enforce_affinity_guard
        || !same_runtime
        || (!same_workspace && check.capabilities.resume_ignores_cwd)
        || (!same_spec_hash
            && !check.capabilities.resume_applies_instructions
            && !check.allow_sandbox_spec_change);
    if !strict_mismatch {
        return ResumeResolution {
            effective_resume: Some(requested.to_owned()),
            fallback_notice: None,
            fallback_reason: None,
        };
    }
    let (reason, detail) = if !same_runtime {
        (
            "runtime_changed",
            "the agent's active runtime changed since that session ran",
        )
    } else if !same_workspace {
        (
            "workspace_changed",
            "the session workspace changed since that session ran",
        )
    } else {
        (
            "instructions_or_spec_changed",
            "the agent's instructions or execution spec changed since that session ran",
        )
    };
    fallback(requested, reason, detail)
}

fn fallback(requested: &str, reason: &'static str, detail: &str) -> ResumeResolution {
    ResumeResolution {
        effective_resume: None,
        fallback_notice: Some(format!(
            "[Session notice: started a fresh session instead of resuming `{requested}` because {detail}. Re-inspect the full target context before concluding.]"
        )),
        fallback_reason: Some(reason),
    }
}

/// Prepends `notice` to `objective` when a warm resume fell back to a fresh session.
pub(super) fn prepend_fallback_notice(notice: Option<&str>, objective: String) -> String {
    let Some(notice) = notice else {
        return objective;
    };
    if objective.is_empty() {
        notice.to_owned()
    } else {
        format!("{notice}\n\n{objective}")
    }
}

/// Builds a `session_affinity` entry for the current timestamp.
pub(super) fn affinity_entry(
    runtime_session_id: impl Into<String>,
    runtime_id: impl Into<String>,
    workspace: impl Into<String>,
    spec_hash: impl Into<String>,
) -> SessionAffinityEntry {
    SessionAffinityEntry {
        runtime_session_id: runtime_session_id.into(),
        runtime_id: runtime_id.into(),
        workspace: workspace.into(),
        spec_hash: spec_hash.into(),
        recorded_at: rfc3339_now(),
    }
}

pub(super) fn next_uuid_v4() -> Result<String, ApiError> {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut b = [0_u8; 16];
    SystemRandom::new()
        .fill(&mut b)
        .map_err(|_| ApiError::new(ErrorCode::Unavailable, "system entropy unavailable"))?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    ))
}

pub(super) fn ensure_warm_sandbox_not_stale(sandbox_topology_stale: bool) -> Result<(), ApiError> {
    if sandbox_topology_stale {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "sandboxed session topology is stale while busy; wait for current turn to finish or stop session first",
        ));
    }
    Ok(())
}

pub(super) fn is_warm_state_reusable(state: SessionState) -> bool {
    !matches!(
        state,
        SessionState::Stopped
            | SessionState::Stopping
            | SessionState::Suspended
            | SessionState::StallLocked
            | SessionState::RecoveryRequired
    )
}
