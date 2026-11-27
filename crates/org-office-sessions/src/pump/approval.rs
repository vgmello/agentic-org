//! Approval requests and resolutions, stall handling, and child process finalization.

use std::os::unix::process::ExitStatusExt;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use org_agent_runtime::{
    ApprovalRequest, Control, Decision, Escalation, ExitInfo, Rule, RuleScope, RuleTool,
};
use org_proto::sessions::{PendingApprovalView, SessionState};
use org_supervise::StopMode;

use crate::credentials::{format_rfc3339_ms, now_ms, rfc3339_now};
use crate::preset::{decide_escalation_with_roots, EscalationDisposition};
use crate::redact::redact_text;

use super::{PumpContext, SessionRuntimeState};

/// Applies `mutate` to this run's durable `active-turn.v1.json` record (a newer run's record is
/// never touched) and stamps `updated_at`.
pub(super) fn persist_turn_update(
    ctx: &PumpContext,
    mutate: impl FnOnce(&mut crate::recovery::ActiveTurnRecord),
) {
    let _ = ctx.store.update_active_turn(&ctx.employee_id, |persisted| {
        if persisted.run_id != ctx.run_id {
            return;
        }
        mutate(persisted);
        persisted.updated_at = rfc3339_now();
    });
}

pub(super) fn handle_approval_request(
    state: &mut SessionRuntimeState,
    req: &ApprovalRequest,
    active_tokens: &[&str],
) -> (Option<Control>, Option<String>) {
    let writable_roots = state
        .sandbox_enabled
        .then_some(state.writable_roots.as_slice());
    match decide_escalation_with_roots(
        req,
        &state.policy,
        state.preset,
        state.allowed_tools.as_deref(),
        writable_roots,
    ) {
        EscalationDisposition::AllowByPolicy => (
            Some(Control::Answer {
                request_id: req.id.clone(),
                decision: Decision::AllowOnce,
            }),
            None,
        ),
        EscalationDisposition::AutoDeny { reason } => {
            let redacted = redact_text(&reason, active_tokens);
            (
                Some(Control::Answer {
                    request_id: req.id.clone(),
                    decision: Decision::Deny {
                        message: Some(redacted.clone()),
                        stop_turn: false,
                    },
                }),
                Some(redacted),
            )
        }
        EscalationDisposition::EscalateToOwner { warning } => {
            state.state = SessionState::WaitingApproval;
            state.pending_approvals.push(PendingApprovalView {
                approval_id: req.id.as_str().to_owned(),
                tool: redact_text(&req.tool, active_tokens),
                escalation: escalation_label(req.effective_escalation()).to_owned(),
                reason: Some(redact_text(&warning, active_tokens)),
                requested_at: format_rfc3339_ms(now_ms()),
            });
            (None, None)
        }
    }
}

pub(super) fn escalation_label(e: Escalation) -> &'static str {
    match e {
        Escalation::None => "none",
        Escalation::SandboxBypass => "sandbox_bypass",
        Escalation::PermissionChange => "permission_change",
        _ => "unknown",
    }
}

pub(super) fn apply_approval_resolution(
    state: &Arc<Mutex<SessionRuntimeState>>,
    approval_id: &str,
    allow_always_tool: Option<&str>,
) {
    let mut guard = state.lock().unwrap_or_else(|p| p.into_inner());
    let matched_escalation = guard
        .pending_approvals
        .iter()
        .find(|p| p.approval_id == approval_id)
        .map(|p| p.escalation.clone());
    guard
        .pending_approvals
        .retain(|p| p.approval_id != approval_id);
    if let Some(tool_name) = allow_always_tool {
        if matched_escalation.as_deref() == Some("none") {
            if let Ok(tool) = RuleTool::try_from(tool_name) {
                guard.policy.allow.push(Rule {
                    tool,
                    scope: RuleScope::Any,
                });
                guard.policy.version = guard.policy.version.saturating_add(1);
            }
        }
    }
    if guard.pending_approvals.is_empty() && guard.state == SessionState::WaitingApproval {
        guard.state = SessionState::Running;
    }
}

pub(super) fn check_and_handle_stall(ctx: &PumpContext, now: Instant) -> bool {
    let mut state = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
    if !state.watchdog.is_stalled(now) {
        return false;
    }
    let locked = state.watchdog.record_stall(now);
    let _ = state.backgrounds.reap_all(rfc3339_now());
    state.state = if locked {
        SessionState::StallLocked
    } else {
        SessionState::Stopped
    };
    state.last_error = Some("turn stalled and was reaped by watchdog".to_owned());
    let stalls = state.watchdog.consecutive_stalls();
    drop(state);
    persist_turn_update(ctx, |rec| {
        rec.active = false;
        rec.consecutive_stalls = stalls;
        rec.stall_locked = locked;
    });
    true
}

pub(super) fn record_stop_note(ctx: &PumpContext, reason: &str) {
    let mut state = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
    state.state = SessionState::Stopping;
    let _ = state.backgrounds.reap_all(rfc3339_now());
    let note = serde_json::json!({
        "type": "office_stop",
        "reason": reason,
        "at": rfc3339_now(),
    });
    let _ = ctx.store.append_history(&ctx.employee_id, &note, &[]);
}

pub(crate) fn finalize_child(
    mut child: org_supervise::OwnedChild,
    stdin: org_supervise::ChildStdin,
    session: &mut Box<dyn org_agent_runtime::Session>,
    ctx: &PumpContext,
    mode: StopMode,
    suspended: bool,
) -> bool {
    let accepted = ctx
        .bindings
        .accepted_commands(&ctx.employee_id)
        .into_iter()
        .map(|c| c.summary)
        .collect::<Vec<_>>();
    ctx.bindings.revoke(&ctx.employee_id, Some(&ctx.run_id));
    let _ = std::fs::remove_file(&ctx.binding_file);
    if let Ok(policy) = org_supervise::runtime::stop_policy(&ctx.stop_plan) {
        let _ = child.stop(&stdin, &policy, mode);
    } else {
        let _ = stdin.close();
    }
    let sockets = org_supervise::runtime::socket_cleanup(&ctx.cleanup_paths).unwrap_or_default();
    let (cleanup_verified, exit_info) = match child.cleanup_and_reap(&sockets) {
        Ok(report) => (
            true,
            ExitInfo {
                code: report.status.code(),
                signal: report.status.signal(),
            },
        ),
        Err(_) => (
            false,
            ExitInfo {
                code: None,
                signal: None,
            },
        ),
    };
    let _ = session.on_exit(exit_info, Instant::now());
    // Purge every leftover turn-scoped staged attachment directory on child exit/crash/stop (`TASK-98.3` AC #3).
    crate::turn_attachments::purge_all_stale_turn_dirs(&ctx.attachments_root);
    let mut state = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
    let _ = state.backgrounds.reap_all(rfc3339_now());
    if !cleanup_verified {
        state.state = SessionState::RecoveryRequired;
        state.recovery_required = true;
        state.last_error = Some("child process cleanup could not be verified".into());
    } else if suspended {
        state.state = SessionState::Suspended;
    } else if !matches!(
        state.state,
        SessionState::StallLocked | SessionState::RecoveryRequired
    ) {
        state.state = SessionState::Stopped;
    }
    drop(state);
    persist_turn_update(ctx, |rec| {
        rec.active = false;
        if !cleanup_verified {
            rec.recovery_required = true;
        }
        if rec.accepted_commands.len() < accepted.len() {
            rec.accepted_commands = accepted;
        }
    });
    cleanup_verified
}
