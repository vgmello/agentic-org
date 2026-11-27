//! Approval requests and resolutions, stall handling, and child process finalization.

use std::os::unix::process::ExitStatusExt;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use org_agent_runtime::{
    ApprovalRequest, Control, Decision, Escalation, ExitInfo, Rule, RuleScope, RuleTool,
    ToolCapability, ToolName,
};
use org_proto::sessions::{PendingApprovalView, SessionState};
use org_supervise::StopMode;

use crate::credentials::{format_rfc3339_ms, now_ms, rfc3339_now};
use crate::preset::{decide_escalation_with_roots, EscalationDisposition};
use crate::redact::redact_text;
use crate::redact::sanitize::sanitize_single_line_display;

use super::{persist_turn_update, PumpContext, SessionRuntimeState};

/// Internal pending approval record pairing the redacted/sanitized display view with the
/// validated concrete [`RuleTool`] permitted for `allow_always` (computed from the raw request
/// so display sanitization can never launder a crafted tool string into an eligible `ToolName`).
#[derive(Debug, Clone)]
pub(crate) struct PendingApprovalEntry {
    pub view: PendingApprovalView,
    pub allow_always_tool: Option<RuleTool>,
}

/// Validates whether `(escalation, tool)` may insert an `allow_always` rule (`TASK-102` P2-1):
/// requires `escalation == "none"`, a concrete valid [`ToolName`] (rejecting `"*"`, control
/// chars, whitespace, syntax chars, or oversized names), and a non-capability token (`cap:*`).
pub(crate) fn allow_always_rule_tool(escalation: &str, tool: &str) -> Option<RuleTool> {
    if escalation != "none" || is_reserved_capability_token(tool) {
        return None;
    }
    let valid = ToolName::try_from(tool).ok()?;
    RuleTool::try_from(valid.as_str()).ok()
}

/// Returns `true` when `(escalation, tool)` is eligible for `allow_always` action emission and
/// policy insertion (`TASK-102` P2-1).
pub fn is_allow_always_eligible(escalation: &str, tool: &str) -> bool {
    allow_always_rule_tool(escalation, tool).is_some()
}

fn is_reserved_capability_token(tool: &str) -> bool {
    ToolCapability::is_capability_token(tool)
        || tool
            .strip_prefix(|c: char| matches!(c, 'c' | 'C'))
            .and_then(|s| s.strip_prefix(|c: char| matches!(c, 'a' | 'A')))
            .and_then(|s| s.strip_prefix(|c: char| matches!(c, 'p' | 'P')))
            .is_some_and(|s| s.starts_with(':'))
}

fn build_pending_approval_entry(
    req: &ApprovalRequest,
    warning: &str,
    active_tokens: &[&str],
) -> PendingApprovalEntry {
    let escalation = escalation_label(req.effective_escalation());
    let safe_tool = sanitize_single_line_display(&redact_text(&req.tool, active_tokens));
    let safe_reason = sanitize_single_line_display(&redact_text(warning, active_tokens));
    let allow_always_tool = (safe_tool == req.tool)
        .then(|| allow_always_rule_tool(escalation, &req.tool))
        .flatten();
    PendingApprovalEntry {
        view: PendingApprovalView {
            approval_id: req.id.as_str().to_owned(),
            tool: safe_tool,
            escalation: escalation.to_owned(),
            reason: (!safe_reason.is_empty()).then_some(safe_reason),
            requested_at: format_rfc3339_ms(now_ms()),
        },
        allow_always_tool,
    }
}

pub(super) fn handle_approval_request(
    state: &mut SessionRuntimeState,
    req: &ApprovalRequest,
    active_tokens: &[&str],
) -> Option<Control> {
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
        EscalationDisposition::AllowByPolicy => Some(Control::Answer {
            request_id: req.id.clone(),
            decision: Decision::AllowOnce,
        }),
        EscalationDisposition::AutoDeny { reason } => Some(Control::Answer {
            request_id: req.id.clone(),
            decision: Decision::Deny {
                message: Some(redact_text(&reason, active_tokens)),
                stop_turn: false,
            },
        }),
        EscalationDisposition::EscalateToOwner { warning } => {
            state.state = SessionState::WaitingApproval;
            let entry = build_pending_approval_entry(req, &warning, active_tokens);
            if let Some(existing) = state
                .pending_approvals
                .iter_mut()
                .find(|p| p.view.approval_id == entry.view.approval_id)
            {
                *existing = entry;
            } else {
                state.pending_approvals.push(entry);
            }
            None
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

pub(super) fn notify_approval_changed(ctx: &PumpContext) {
    if let Some(hook) = ctx.hooks.on_approval_changed.as_ref() {
        hook();
    }
}

pub(super) fn begin_pump_teardown(ctx: &PumpContext) {
    let had_approvals = {
        let mut guard = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
        let had = !guard.pending_approvals.is_empty();
        guard.pending_approvals.clear();
        // Turns still queued die with the pump (a preserved failover carries its own objective).
        guard.queued_turns = 0;
        if !matches!(
            guard.state,
            SessionState::StallLocked | SessionState::RecoveryRequired
        ) {
            guard.state = SessionState::Stopping;
        }
        had
    };
    if had_approvals {
        notify_approval_changed(ctx);
    }
}

/// Removes `approval_id` from `guard.pending_approvals` and appends an `allow_always` policy rule
/// when eligible. Returns `true` if a matching pending approval was removed.
pub(crate) fn remove_pending_approval_locked(
    guard: &mut SessionRuntimeState,
    approval_id: &str,
    allow_always_tool: Option<&str>,
) -> bool {
    let matched_rule_tool = guard
        .pending_approvals
        .iter()
        .find(|p| p.view.approval_id == approval_id)
        .and_then(|p| {
            let requested = allow_always_tool?;
            let stored = p.allow_always_tool.as_ref()?;
            if stored.as_str() != requested {
                return None;
            }
            allow_always_rule_tool(&p.view.escalation, requested)
        });
    let before = guard.pending_approvals.len();
    guard
        .pending_approvals
        .retain(|p| p.view.approval_id != approval_id);
    if let Some(tool) = matched_rule_tool {
        guard.policy.allow.push(Rule {
            tool,
            scope: RuleScope::Any,
        });
        guard.policy.version = guard.policy.version.saturating_add(1);
    }
    guard.pending_approvals.len() < before
}

pub(super) fn apply_approval_resolution(
    state: &Arc<Mutex<SessionRuntimeState>>,
    approval_id: &str,
    allow_always_tool: Option<&str>,
) -> bool {
    let mut guard = state.lock().unwrap_or_else(|p| p.into_inner());
    let removed = remove_pending_approval_locked(&mut guard, approval_id, allow_always_tool);
    if guard.pending_approvals.is_empty() && guard.state == SessionState::WaitingApproval {
        guard.state = SessionState::Running;
    }
    removed
}

pub(super) fn check_and_handle_stall(ctx: &PumpContext, now: Instant) -> bool {
    let mut state = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
    if !state.watchdog.is_stalled(now) {
        return false;
    }
    let had_approvals = !state.pending_approvals.is_empty();
    state.pending_approvals.clear();
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
    if had_approvals {
        notify_approval_changed(ctx);
    }
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
    settle_finalized_state(ctx, cleanup_verified, suspended);
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

fn settle_finalized_state(ctx: &PumpContext, cleanup_verified: bool, suspended: bool) {
    let had_approvals = {
        let mut state = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
        let had = !state.pending_approvals.is_empty();
        state.pending_approvals.clear();
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
        had
    };
    if had_approvals {
        notify_approval_changed(ctx);
    }
}
