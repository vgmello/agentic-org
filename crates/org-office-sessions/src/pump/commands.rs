//! Pump command draining: stop/suspend merging, background wakes, and permit release.

use std::sync::mpsc::Receiver;
use std::time::Instant;

use org_agent_runtime::{ApprovalId, Control};
use org_proto::sessions::SessionState;

use crate::instructions::{compose_turn_objective, TurnTrigger};

use super::approval::{apply_approval_resolution, record_stop_note};
use super::controls::{request_interrupt, steer};
use super::events::apply_outputs;
use super::queue::{merge_queued_turns, QueuedTurn};
use super::{FailoverRequest, PumpCommand, PumpContext, PumpLoopState};

pub(super) fn release_active_permit(ctx: &PumpContext) {
    let taken = ctx
        .active_permit
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take();
    if let (Some((permit_id, _)), Some(cb)) = (taken, ctx.hooks.on_permit_release.as_ref()) {
        cb(&permit_id);
    }
}

pub(super) fn mark_active_permit_release_pending(ctx: &PumpContext) {
    let taken = ctx
        .active_permit
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take();
    if let (Some((permit_id, _)), Some(cb)) = (taken, ctx.hooks.on_permit_release_pending.as_ref())
    {
        cb(&permit_id);
    }
}

pub(super) fn build_stale_failover_request(
    ctx: &PumpContext,
    objective: String,
    attachment_ids: Vec<String>,
) -> FailoverRequest {
    FailoverRequest {
        employee_id: ctx.employee_id.clone(),
        failed_run_id: ctx.run_id.clone(),
        failed_connection_id: ctx.connection_id.clone().unwrap_or_default(),
        objective,
        attachment_ids,
        workspace: ctx.workspace.clone(),
        task_ref: ctx.task_ref.clone(),
        tried_connections: Vec::new(),
        oauth_refresh_attempted: false,
        oauth_token_fp: ctx.oauth_token_fp.clone(),
        reason: "warm_route_stale",
        rate_limit_resets_at: None,
        stop_epoch: ctx.stop_epoch,
        partition: ctx.partition_key(),
    }
}

pub(super) fn drain_commands(
    commands: &Receiver<PumpCommand>,
    stdin: &mut org_supervise::ChildStdin,
    ctx: &PumpContext,
    loop_state: &mut PumpLoopState,
    session: &mut Box<dyn org_agent_runtime::Session>,
) {
    let mut stop_preserve: Option<bool> = None;
    while let Ok(cmd) = commands.try_recv() {
        match cmd {
            PumpCommand::SendTurn(turn) => loop_state.pending_turns.push(turn),
            PumpCommand::InterruptTurn => request_interrupt(stdin, ctx, loop_state, session),
            PumpCommand::SteerTurn(request) => steer(stdin, ctx, loop_state, session, request),
            PumpCommand::NotifyPendingWakes => {
                loop_state.mid_turn_dirty = true;
                loop_state.mid_turn_next_check = None;
            }
            PumpCommand::AnswerApproval {
                approval_id,
                decision,
                allow_always_tool,
            } => {
                apply_approval_resolution(&ctx.state, &approval_id, allow_always_tool.as_deref());
                // Wake mid-turn staging once pending approvals clear (`doc-101` §4A).
                if ctx
                    .state
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .pending_approvals
                    .is_empty()
                {
                    loop_state.mid_turn_dirty = true;
                }
                let ctrl = Control::Answer {
                    request_id: ApprovalId::new(approval_id),
                    decision,
                };
                if let Ok(outs) = session.control(ctrl, Instant::now()) {
                    apply_outputs(outs, stdin, ctx, loop_state, session);
                }
            }
            PumpCommand::ArmSuspend { deadline } => {
                loop_state.suspend_deadline = Some(match loop_state.suspend_deadline {
                    Some(existing) => existing.min(deadline),
                    None => deadline,
                });
            }
            PumpCommand::Stop { reason } => {
                loop_state.stop_requested = true;
                stop_preserve = merge_stop_preserve(stop_preserve, &reason);
                if let Ok(outs) = session.control(Control::Close, Instant::now()) {
                    apply_outputs(outs, stdin, ctx, loop_state, session);
                }
                record_stop_note(ctx, &reason);
            }
        }
    }
    finalize_drained_stop(stop_preserve, ctx, loop_state);
}

pub(super) fn merge_stop_preserve(current: Option<bool>, reason: &str) -> Option<bool> {
    if current == Some(false) {
        return Some(false);
    }
    Some(matches!(
        reason,
        "warm_route_stale"
            | "oauth_token_expiring"
            | "pool_reroute"
            | "sandbox_isolation_refresh"
            | "sandbox_topology_changed"
    ))
}

pub(super) fn finalize_drained_stop(
    stop_preserve: Option<bool>,
    ctx: &PumpContext,
    loop_state: &mut PumpLoopState,
) {
    match stop_preserve {
        Some(true) if loop_state.has_queued_turns() => {
            let (objective, attachment_ids) =
                merge_queued_turns(take_pending_turns(ctx, loop_state));
            loop_state.failover_pending =
                Some(build_stale_failover_request(ctx, objective, attachment_ids));
        }
        Some(_) => {
            loop_state.failover_pending = None;
        }
        None => {}
    }
}

/// Takes every queued turn out of the pump's queue and forgets it in
/// [`SessionRuntimeState::queued_turns`](super::SessionRuntimeState::queued_turns).
pub(super) fn take_pending_turns(
    ctx: &PumpContext,
    loop_state: &mut PumpLoopState,
) -> Vec<QueuedTurn> {
    let taken = loop_state.take_queued_turns();
    ctx.state
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .dequeue_turns(taken.len());
    taken
}

pub(super) fn enqueue_background_wakes(ctx: &PumpContext, loop_state: &mut PumpLoopState) {
    let wakes = {
        let mut guard = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
        let wakes = guard.backgrounds.drain_wakes();
        // A background wake is a queued turn like any other: the session is busy until it ends.
        if !wakes.is_empty() {
            guard.queued_turns = guard.queued_turns.saturating_add(wakes.len());
            if guard.state == SessionState::Idle {
                guard.state = SessionState::Running;
            }
        }
        wakes
    };
    for wake in wakes {
        let text = compose_turn_objective(&TurnTrigger::BackgroundWake {
            task_id: wake.task_id,
            label: wake.label,
            summary: wake.summary,
        });
        loop_state.pending_turns.push(QueuedTurn::text_only(text));
    }
}
