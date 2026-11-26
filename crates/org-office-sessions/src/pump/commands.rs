//! Pump command draining: stop/suspend merging, background wakes, and permit release.

use super::*;

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
) -> FailoverRequest {
    FailoverRequest {
        employee_id: ctx.employee_id.clone(),
        failed_run_id: ctx.run_id.clone(),
        failed_connection_id: ctx.connection_id.clone().unwrap_or_default(),
        objective,
        workspace: ctx.workspace.clone(),
        task_ref: ctx.task_ref.clone(),
        tried_connections: Vec::new(),
        oauth_refresh_attempted: false,
        oauth_token_fp: ctx.oauth_token_fp.clone(),
        reason: "warm_route_stale",
        rate_limit_resets_at: None,
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
            PumpCommand::SendTurn(text) => loop_state.pending_turns.push(text),
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
            | "bwrap_sibling_isolation_refresh"
            | "bwrap_sibling_topology_changed"
    ))
}

pub(super) fn finalize_drained_stop(
    stop_preserve: Option<bool>,
    ctx: &PumpContext,
    loop_state: &mut PumpLoopState,
) {
    match stop_preserve {
        Some(true) if !loop_state.pending_turns.is_empty() => {
            let objective = std::mem::take(&mut loop_state.pending_turns).join("\n\n");
            loop_state.failover_pending = Some(build_stale_failover_request(ctx, objective));
        }
        Some(_) => {
            loop_state.failover_pending = None;
        }
        None => {}
    }
}

pub(super) fn enqueue_background_wakes(ctx: &PumpContext, loop_state: &mut PumpLoopState) {
    let wakes = {
        let mut guard = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
        guard.backgrounds.drain_wakes()
    };
    for wake in wakes {
        let text = compose_turn_objective(&TurnTrigger::BackgroundWake {
            task_id: wake.task_id,
            label: wake.label,
            summary: wake.summary,
        });
        loop_state.pending_turns.push(text);
    }
}
