//! Runtime output application and session event handling, including turn-end failover
//! classification (`doc-88` §7).

use super::*;

pub(super) fn rollback_failed_write_delivery(loop_state: &mut PumpLoopState, delivery_id: &str) {
    if loop_state
        .in_flight_delivery
        .as_ref()
        .is_some_and(|b| b.delivery_id == delivery_id)
    {
        loop_state.in_flight_delivery = None;
    }
}

pub(super) fn apply_outputs(
    outputs: Vec<org_agent_runtime::Output>,
    stdin: &mut org_supervise::ChildStdin,
    ctx: &PumpContext,
    loop_state: &mut PumpLoopState,
    session: &mut Box<dyn org_agent_runtime::Session>,
) {
    let mut follow_up = Vec::new();
    let mut write_failed = false;
    for output in outputs {
        match output {
            org_agent_runtime::Output::Write(line) => {
                if stdin.write_all(line.as_bytes()).is_err() || stdin.flush().is_err() {
                    write_failed = true;
                }
            }
            org_agent_runtime::Output::CloseStdin => {
                let _ = stdin.close();
            }
            org_agent_runtime::Output::Event(mut event) => {
                if write_failed {
                    if let SessionEvent::MidTurnDelivered { delivery_id } = &event {
                        rollback_failed_write_delivery(loop_state, delivery_id);
                        continue;
                    }
                }
                let tokens = ctx.bindings.active_tokens();
                let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
                // Evaluate approval requests against the ORIGINAL unredacted structured input
                // (`TASK-94`) so secret redaction can never mask shell metacharacters or denied
                // paths before policy checks; redact the event copy used for history and display.
                let unredacted_approval = match &event {
                    SessionEvent::ApprovalRequested(req) => Some(req.clone()),
                    _ => None,
                };
                event.redact(&mut |s| {
                    *s = redact_secrets(s, &refs);
                });
                if let Ok(json_val) = serde_json::to_value(&event) {
                    let _ = ctx.store.append_history(&ctx.employee_id, &json_val, &refs);
                }
                let ctrl = if let Some(raw_req) = unredacted_approval {
                    let mut state = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
                    state.watchdog.record_activity(Instant::now());
                    handle_approval_request(&mut state, &raw_req, &refs)
                } else {
                    handle_event(&event, ctx, loop_state)
                };
                if let Some(ctrl) = ctrl {
                    follow_up.push(ctrl);
                }
            }
        }
    }
    for ctrl in follow_up {
        if let Ok(outs) = session.control(ctrl, Instant::now()) {
            apply_outputs(outs, stdin, ctx, loop_state, session);
        }
    }
}

pub(super) fn handle_event(
    event: &SessionEvent,
    ctx: &PumpContext,
    loop_state: &mut PumpLoopState,
) -> Option<Control> {
    let mut state = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
    state.watchdog.record_activity(Instant::now());
    match event {
        SessionEvent::Ready { .. } => {
            loop_state.ready = true;
            state.state = SessionState::Running;
        }
        SessionEvent::SessionStarted { runtime_session_id } => {
            let id = runtime_session_id.as_str().to_owned();
            state.runtime_session_id = Some(id.clone());
            drop(state);
            persist_turn_update(ctx, |rec| rec.runtime_session_id = Some(id));
        }
        SessionEvent::TurnStarted { .. } => {
            if !loop_state.turn_active {
                loop_state.in_flight_delivery = None;
                loop_state.mid_turn_total_this_turn = 0;
                loop_state.mid_turn_agent_this_turn = 0;
            }
            loop_state.turn_active = true;
            loop_state.turn_reply = None;
            loop_state.mid_turn_dirty = true;
            state.state = SessionState::Running;
            drop(state);
            persist_turn_update(ctx, |rec| rec.active = true);
        }
        SessionEvent::MidTurnDelivered { delivery_id } => {
            let matched = loop_state
                .in_flight_delivery
                .as_ref()
                .is_some_and(|b| b.delivery_id == *delivery_id)
                .then(|| loop_state.in_flight_delivery.take())
                .flatten();
            if let Some(batch) = matched {
                loop_state.mid_turn_total_this_turn =
                    loop_state.mid_turn_total_this_turn.saturating_add(1);
                if batch.has_agent_item {
                    loop_state.mid_turn_agent_this_turn =
                        loop_state.mid_turn_agent_this_turn.saturating_add(1);
                }
                loop_state.mid_turn_dirty = true;
                loop_state.mid_turn_next_check = None;
                let emp_id = ctx.employee_id.clone();
                let hook = ctx.hooks.on_mid_turn_delivered.clone();
                drop(state);
                if let Some(cb) = hook {
                    cb(&emp_id, &batch);
                }
            }
        }
        SessionEvent::MidTurnDeliveryFailed { delivery_id, .. } => {
            // Rollback without touching disk so pending items drain cleanly on TurnEnded (`doc-101` §4B).
            if loop_state
                .in_flight_delivery
                .as_ref()
                .is_some_and(|b| b.delivery_id == *delivery_id)
            {
                loop_state.in_flight_delivery = None;
            }
        }
        SessionEvent::AssistantText { text, .. } => {
            state.last_reply = Some(text.clone());
            loop_state.turn_reply = Some(text.clone());
        }
        SessionEvent::TurnEnded {
            outcome, errors, ..
        } => {
            handle_turn_ended(&mut state, loop_state, ctx, outcome, errors);
        }
        SessionEvent::StartFailed { reason } => {
            state.last_error = Some(format!("start failed: {reason:?}"));
            state.state = SessionState::Stopped;
        }
        SessionEvent::Usage(usage) => {
            state.backgrounds.record_usage(usage);
            let run_id = ctx.run_id.clone();
            let emp_id = ctx.employee_id.clone();
            let hook = ctx.hooks.on_usage.clone();
            drop(state);
            if let Some(cb) = hook {
                cb(&emp_id, &run_id, usage);
            }
        }
        SessionEvent::RateLimit(rl) => {
            if rl.status == org_agent_runtime::LimitStatus::Rejected {
                loop_state.rate_limited = true;
                if rl.resets_at.is_some() {
                    loop_state.rate_limit_resets_at = rl.resets_at.clone();
                }
                let conn_id = ctx.connection_id.clone();
                let hook = ctx.hooks.on_rate_limit.clone();
                drop(state);
                if let (Some(cid), Some(cb)) = (conn_id, hook) {
                    cb(&cid, rl.resets_at.as_deref());
                }
            }
        }
        SessionEvent::ToolStarted { .. } => {
            loop_state.had_external_effect = true;
            return handle_tool_or_approval_event(&mut state, loop_state, event);
        }
        other => return handle_tool_or_approval_event(&mut state, loop_state, other),
    }
    None
}

pub(super) fn handle_tool_or_approval_event(
    state: &mut SessionRuntimeState,
    loop_state: &mut PumpLoopState,
    event: &SessionEvent,
) -> Option<Control> {
    match event {
        SessionEvent::ToolStarted {
            tool_use_id,
            name,
            capability: Some(org_agent_runtime::ToolCapability::Subagent),
            ..
        } => {
            state.backgrounds.register(
                tool_use_id.clone(),
                BackgroundKind::Subagent,
                name.clone(),
                None,
                rfc3339_now(),
            );
        }
        SessionEvent::ToolFinished {
            tool_use_id,
            is_error,
        } => {
            let _ = state.backgrounds.complete(
                tool_use_id,
                !*is_error,
                "sub-agent finished",
                rfc3339_now(),
            );
        }
        SessionEvent::Usage(usage) => state.backgrounds.record_usage(usage),
        SessionEvent::ApprovalRequested(req) => return handle_approval_request(state, req, &[]),
        SessionEvent::ApprovalCancelled { id } => {
            state
                .pending_approvals
                .retain(|p| p.approval_id != id.as_str());
            if state.pending_approvals.is_empty() && state.state == SessionState::WaitingApproval {
                state.state = SessionState::Running;
                loop_state.mid_turn_dirty = true;
            }
        }
        _ => {}
    }
    None
}

pub(super) fn handle_turn_ended(
    state: &mut SessionRuntimeState,
    loop_state: &mut PumpLoopState,
    ctx: &PumpContext,
    outcome: &org_agent_runtime::TurnOutcome,
    errors: &[String],
) {
    loop_state.turn_active = false;
    loop_state.turn_just_ended = true;
    loop_state.in_flight_delivery = None;
    loop_state.mid_turn_dirty = false;
    loop_state.mid_turn_next_check = None;
    state.watchdog.record_turn_completed(Instant::now());
    if !errors.is_empty() {
        state.last_error = Some(errors.join("; "));
    }
    let failover_blocked_recovery = evaluate_turn_failover(state, loop_state, ctx, outcome, errors);
    if state.state == SessionState::Running {
        state.state = SessionState::Idle;
    }
    persist_turn_update(ctx, |rec| {
        rec.active = false;
        rec.consecutive_stalls = 0;
        if failover_blocked_recovery {
            rec.recovery_required = true;
        }
        let mem_cmds: Vec<String> = ctx
            .bindings
            .accepted_commands(&ctx.employee_id)
            .into_iter()
            .map(|c| c.summary)
            .collect();
        if rec.accepted_commands.len() < mem_cmds.len() {
            rec.accepted_commands = mem_cmds;
        }
    });
}

/// Maps a finished turn to a pool failover reason (`doc-88` §7). A turn that completed without
/// external effects whose final reply is the runtime's in-band "credentials expired" signal
/// ([`org_agent_runtime::AgentRuntime::reply_signals_auth_expired`]) counts as `auth_expired`.
pub(crate) fn classify_turn_failover_reason(
    runtime: &dyn org_agent_runtime::AgentRuntime,
    outcome: &org_agent_runtime::TurnOutcome,
    errors: &[String],
    turn_reply: Option<&str>,
    has_effects: bool,
    rate_limited: bool,
) -> Option<&'static str> {
    let reply_auth_expired =
        !has_effects && turn_reply.is_some_and(|reply| runtime.reply_signals_auth_expired(reply));
    if matches!(outcome, org_agent_runtime::TurnOutcome::Completed) && !reply_auth_expired {
        return None;
    }
    if reply_auth_expired || matches!(outcome, org_agent_runtime::TurnOutcome::SignedOut) {
        return Some("auth_expired");
    }
    if let Some(reason) = crate::router::classify_failover_error(errors, rate_limited) {
        return Some(reason);
    }
    if let org_agent_runtime::TurnOutcome::Failed { reason } = outcome {
        return crate::router::classify_failover_error(std::slice::from_ref(reason), false);
    }
    None
}

pub(super) fn evaluate_turn_failover(
    state: &mut SessionRuntimeState,
    loop_state: &mut PumpLoopState,
    ctx: &PumpContext,
    outcome: &org_agent_runtime::TurnOutcome,
    errors: &[String],
) -> bool {
    let turn_cmds = ctx.bindings.accepted_commands(&ctx.employee_id).len();
    let has_effects =
        loop_state.had_external_effect || turn_cmds > loop_state.accepted_commands_baseline;
    let Some(reason) = classify_turn_failover_reason(
        ctx.runtime.as_ref(),
        outcome,
        errors,
        loop_state.turn_reply.as_deref(),
        has_effects,
        loop_state.rate_limited,
    ) else {
        if matches!(outcome, org_agent_runtime::TurnOutcome::Completed) {
            loop_state.oauth_refresh_attempted = false;
        }
        return false;
    };
    let Some(conn_id) = ctx.connection_id.as_ref() else {
        return false;
    };
    if has_effects {
        state.state = SessionState::RecoveryRequired;
        state.recovery_required = true;
        state.last_error = Some(format!("failover blocked after side effects ({reason})"));
        loop_state.stop_requested = true;
        return true;
    }
    let mut tried = ctx.tried_connections.clone();
    if !tried.contains(conn_id) {
        tried.push(conn_id.clone());
    }
    let mut objective = loop_state.current_objective.clone();
    if !loop_state.pending_turns.is_empty() {
        let queued = std::mem::take(&mut loop_state.pending_turns).join("\n\n");
        if objective.is_empty() {
            objective = queued;
        } else {
            objective = format!("{objective}\n\n{queued}");
        }
    }
    loop_state.failover_pending = Some(FailoverRequest {
        employee_id: ctx.employee_id.clone(),
        failed_run_id: ctx.run_id.clone(),
        failed_connection_id: conn_id.clone(),
        objective,
        workspace: ctx.workspace.clone(),
        task_ref: ctx.task_ref.clone(),
        tried_connections: tried,
        oauth_refresh_attempted: loop_state.oauth_refresh_attempted,
        oauth_token_fp: ctx.oauth_token_fp.clone(),
        reason,
        rate_limit_resets_at: loop_state.rate_limit_resets_at.clone(),
    });
    loop_state.stop_requested = true;
    false
}

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
