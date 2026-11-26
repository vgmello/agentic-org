//! Turn dispatch: turn permits and lease heartbeats, queued turns, and mid-turn delivery.

use super::*;

pub(super) enum PermitCheck {
    Ready,
    StaleWarm,
    Refused,
}

pub(super) fn ensure_permit_for_turn(
    ctx: &PumpContext,
    loop_state: &mut PumpLoopState,
) -> PermitCheck {
    // Only proactively recycle on OAuth pre-expiry during warm reuse (Turn 2+). Turn 1 of a fresh
    // cold start already attempted pre-launch refresh and must run so an unrotatable/revoked token
    // surfaces bounded `auth_expired` failover instead of an infinite cold-start loop.
    if loop_state.turns_dispatched > 0
        && crate::host_auth::is_oauth_expiring(ctx.oauth_expires_at_ms, now_ms())
    {
        return PermitCheck::StaleWarm;
    }
    if loop_state.turns_dispatched > 0
        && ctx
            .hooks
            .is_bwrap_topology_stale
            .as_ref()
            .is_some_and(|check| check())
    {
        return PermitCheck::StaleWarm;
    }
    let Some(conn_id) = ctx.connection_id.as_deref() else {
        return PermitCheck::Ready;
    };
    let already_held = ctx
        .active_permit
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .is_some();
    if already_held {
        loop_state
            .permit_lease_deadline
            .get_or_insert_with(|| Instant::now() + Duration::from_secs(60));
        return PermitCheck::Ready;
    }
    let Some(acquire) = ctx.hooks.on_permit_acquire.as_ref() else {
        return PermitCheck::Ready;
    };
    match acquire(conn_id, ctx.task_ref.as_deref()) {
        Ok(permit) => {
            *ctx.active_permit.lock().unwrap_or_else(|p| p.into_inner()) = Some(permit);
            loop_state.permit_lease_deadline = Some(Instant::now() + Duration::from_secs(60));
            PermitCheck::Ready
        }
        Err(reason) if reason.starts_with("WARM_ROUTE_STALE") => PermitCheck::StaleWarm,
        Err(reason) => {
            let mut st = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
            st.last_error = Some(reason);
            if st.state == SessionState::Running {
                st.state = SessionState::Idle;
            }
            PermitCheck::Refused
        }
    }
}

pub(super) fn dispatch_next_turn(
    stdin: &mut org_supervise::ChildStdin,
    ctx: &PumpContext,
    loop_state: &mut PumpLoopState,
    session: &mut Box<dyn org_agent_runtime::Session>,
) {
    if !loop_state.ready
        || loop_state.turn_active
        || loop_state.stop_requested
        || loop_state.pending_turns.is_empty()
    {
        return;
    }
    match ensure_permit_for_turn(ctx, loop_state) {
        PermitCheck::Ready => {}
        PermitCheck::StaleWarm => {
            let objective = std::mem::take(&mut loop_state.pending_turns).join("\n\n");
            loop_state.failover_pending = Some(build_stale_failover_request(ctx, objective));
            loop_state.stop_requested = true;
            return;
        }
        PermitCheck::Refused => {
            loop_state.pending_turns.clear();
            return;
        }
    }
    let text = loop_state.pending_turns.remove(0);
    let objective_for_disk = text.clone();
    loop_state.current_objective = text.clone();
    loop_state.turn_reply = None;
    loop_state.had_external_effect = false;
    loop_state.accepted_commands_baseline = ctx.bindings.accepted_commands(&ctx.employee_id).len();
    loop_state.rate_limited = false;
    loop_state.rate_limit_resets_at = None;
    loop_state.last_permit_renew = Instant::now();
    if let Ok(outs) = session.control(Control::Send { text }, Instant::now()) {
        loop_state.turn_active = true;
        loop_state.turns_dispatched = loop_state.turns_dispatched.saturating_add(1);
        loop_state.in_flight_delivery = None;
        loop_state.mid_turn_total_this_turn = 0;
        loop_state.mid_turn_agent_this_turn = 0;
        loop_state.mid_turn_dirty = true;
        loop_state.mid_turn_next_check = None;
        persist_turn_update(ctx, |rec| {
            rec.objective = objective_for_disk;
            rec.active = true;
        });
        apply_outputs(outs, stdin, ctx, loop_state, session);
    } else {
        release_active_permit(ctx);
        loop_state.permit_lease_deadline = None;
    }
}

pub(super) const MID_TURN_NOT_READY_RETRY: Duration = Duration::from_millis(50);

pub(super) fn maybe_deliver_mid_turn(
    stdin: &mut org_supervise::ChildStdin,
    ctx: &PumpContext,
    loop_state: &mut PumpLoopState,
    session: &mut Box<dyn org_agent_runtime::Session>,
) {
    if !loop_state.turn_active
        || loop_state.stop_requested
        || loop_state.suspend_now
        || loop_state.in_flight_delivery.is_some()
        || !ctx.supports_mid_turn
    {
        return;
    }
    let Some(stage_hook) = ctx.hooks.stage_mid_turn_wakes.as_ref() else {
        return;
    };
    let due = loop_state.mid_turn_dirty
        || loop_state
            .mid_turn_next_check
            .is_some_and(|at| Instant::now() >= at);
    if !due {
        return;
    }
    {
        let st = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
        if st.state != SessionState::Running || !st.pending_approvals.is_empty() {
            return;
        }
    }
    loop_state.mid_turn_dirty = false;
    loop_state.mid_turn_next_check = None;
    match stage_hook(
        &ctx.employee_id,
        ctx.task_ref.as_deref(),
        loop_state.mid_turn_total_this_turn,
        loop_state.mid_turn_agent_this_turn,
    ) {
        MidTurnStageOutcome::None => {}
        MidTurnStageOutcome::Wait(dur) => {
            loop_state.mid_turn_next_check = Some(Instant::now() + dur);
        }
        MidTurnStageOutcome::Ready(batch) => {
            let ctrl = Control::DeliverMidTurn {
                delivery_id: batch.delivery_id.clone(),
                text: batch.text.clone(),
            };
            match session.control(ctrl, Instant::now()) {
                Ok(outs) => {
                    loop_state.in_flight_delivery = Some(batch);
                    apply_outputs(outs, stdin, ctx, loop_state, session);
                }
                Err(org_agent_runtime::ControlError::NotReady) => {
                    loop_state.mid_turn_next_check =
                        Some(Instant::now() + MID_TURN_NOT_READY_RETRY);
                }
                Err(_) => {}
            }
        }
    }
}

pub(super) const LEASE_RENEW_INTERVAL: Duration = Duration::from_secs(10);

pub(super) const LEASE_SAFETY_MARGIN: Duration = Duration::from_secs(20);

pub(super) const LEASE_WINDOW: Duration = Duration::from_secs(60);

pub(super) fn check_permit_heartbeat(
    ctx: &PumpContext,
    loop_state: &mut PumpLoopState,
    now: Instant,
) {
    if !loop_state.turn_active {
        return;
    }
    if loop_state.last_permit_renew.elapsed() >= LEASE_RENEW_INTERVAL {
        loop_state.last_permit_renew = now;
        let current = ctx
            .active_permit
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let (Some((pid, generation)), Some(cb)) = (current, ctx.hooks.on_permit_renew.as_ref()) {
            if cb(&pid, generation) {
                loop_state.permit_lease_deadline = Some(now + LEASE_WINDOW);
            } else {
                let mut st = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
                st.last_error = Some("turn permit lease renewal failed; stopping turn".into());
                loop_state.stop_requested = true;
                return;
            }
        }
    }
    if loop_state
        .permit_lease_deadline
        .is_some_and(|d| d.saturating_duration_since(now) <= LEASE_SAFETY_MARGIN)
    {
        let mut st = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
        st.last_error = Some("turn permit lease approaching expiry; stopping turn".into());
        loop_state.stop_requested = true;
    }
}
