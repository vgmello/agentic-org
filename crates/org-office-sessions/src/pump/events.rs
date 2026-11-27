//! Runtime output application and session event handling, including turn-end failover
//! classification (`doc-88` §7).

use std::io::Write;
use std::time::Instant;

use org_agent_runtime::{Control, SessionEvent};
use org_proto::sessions::SessionState;

use crate::credentials::rfc3339_now;
use crate::redact::redact_text;
use crate::subagents::BackgroundKind;
use crate::turn_attachments::{is_provider_image_rejection_error, purge_turn_dir};

use super::activity::track_activity;
use super::approval::handle_approval_request;
use super::queue::{dedup_attachment_ids, merge_queued_turns, QueuedTurn};
use super::{
    persist_turn_update, FailoverRequest, PumpContext, PumpLoopState, SessionRuntimeState,
};

pub(super) fn rollback_failed_write_delivery(
    ctx: &PumpContext,
    loop_state: &mut PumpLoopState,
    delivery_id: &str,
) {
    if loop_state
        .in_flight_delivery
        .as_ref()
        .is_some_and(|b| b.delivery_id == delivery_id)
    {
        loop_state.in_flight_delivery = None;
        if let Some(tid) = loop_state.in_flight_delivery_staged_id.take() {
            purge_turn_dir(&ctx.attachments_root, &tid);
        }
    }
}

fn purge_active_turn_attachments(ctx: &PumpContext, loop_state: &mut PumpLoopState) {
    if let Some(tid) = loop_state.current_turn_staged_id.take() {
        purge_turn_dir(&ctx.attachments_root, &tid);
    }
    if let Some(tid) = loop_state.in_flight_delivery_staged_id.take() {
        purge_turn_dir(&ctx.attachments_root, &tid);
    }
    for tid in std::mem::take(&mut loop_state.mid_turn_staged_ids) {
        purge_turn_dir(&ctx.attachments_root, &tid);
    }
}

/// Redacts `event` in place through the crate's single choke-point and appends it to the
/// durable history (`ENROLL-3`, `TASK-190`), which redacts the stored JSON structurally again.
pub(super) fn redact_event_into_history(
    ctx: &PumpContext,
    event: &mut SessionEvent,
    active_secrets: &[&str],
) {
    event.redact(&mut |s| *s = redact_text(s, active_secrets));
    if let Ok(json_val) = serde_json::to_value(&*event) {
        let _ = ctx
            .store
            .append_history(&ctx.employee_id, &json_val, active_secrets);
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
                        rollback_failed_write_delivery(ctx, loop_state, delivery_id);
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
                redact_event_into_history(ctx, &mut event, &refs);
                // Coarse live activity comes from the redacted event only (`TASK-103`).
                track_activity(ctx, &event, loop_state.interrupt_sent);
                let ctrl = if let Some(raw_req) = unredacted_approval {
                    let (ctrl, denied_reason) = {
                        let mut state = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
                        state.watchdog.record_activity(Instant::now());
                        handle_approval_request(&mut state, &raw_req, &refs)
                    };
                    if let Some(reason) = denied_reason {
                        if let Some(t) = ctx.telemetry.as_ref() {
                            t.on_auto_deny(
                                &ctx.employee_id,
                                &raw_req.tool,
                                raw_req.tool_use_id.as_deref(),
                                Some(&reason),
                            );
                        }
                    }
                    ctrl
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
            let affinity =
                state
                    .runtime_id
                    .clone()
                    .map(|runtime_id| crate::recovery::SessionAffinityEntry {
                        runtime_session_id: id.clone(),
                        runtime_id,
                        workspace: state.workspace_spec.clone(),
                        spec_hash: state.spec_hash.clone(),
                        recorded_at: rfc3339_now(),
                    });
            drop(state);
            if let Some(entry) = affinity {
                let _ = ctx.store.record_session_affinity(&ctx.employee_id, entry);
            }
            // The partition keeps its own conversation from the moment the runtime names it, so
            // a stop, exit or crash of the run never loses it (`TASK-80.1`).
            if let Some(partition) = ctx.partition.as_ref() {
                partition.record_runtime_session(&id);
            }
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
            if let Some(t) = ctx.telemetry.as_ref() {
                t.on_turn_started(&ctx.employee_id);
            }
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
                if let Some(tid) = loop_state.in_flight_delivery_staged_id.take() {
                    loop_state.mid_turn_staged_ids.push(tid);
                }
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
            } else {
                // A person's steer the runtime took into the turn (`TASK-103`).
                loop_state
                    .in_flight_steers
                    .retain(|(id, _)| id != delivery_id);
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
                if let Some(tid) = loop_state.in_flight_delivery_staged_id.take() {
                    purge_turn_dir(&ctx.attachments_root, &tid);
                }
            }
            // A refused steer runs as the next turn instead (`TASK-103`).
            if let Some(at) = loop_state
                .in_flight_steers
                .iter()
                .position(|(id, _)| id == delivery_id)
            {
                let (_, text) = loop_state.in_flight_steers.remove(at);
                loop_state.requeue_steer(&mut state, text);
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
            drop(state);
            if let Some(t) = ctx.telemetry.as_ref() {
                t.on_turn_ended(
                    &ctx.employee_id,
                    matches!(outcome, org_agent_runtime::TurnOutcome::Completed),
                    errors,
                );
            }
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
            if let Some(t) = ctx.telemetry.as_ref() {
                t.on_usage(&emp_id, &run_id, usage);
            }
            // How full the partition's conversation is: at 60% its next turn rotates.
            if let Some(partition) = ctx.partition.as_ref() {
                partition.observe_usage(usage);
            }
            if let Some(cb) = hook {
                cb(&emp_id, &run_id, usage);
            }
        }
        SessionEvent::ActionDenied {
            tool,
            tool_use_id,
            message,
        } => {
            drop(state);
            if let Some(t) = ctx.telemetry.as_ref() {
                t.on_action_denied(
                    &ctx.employee_id,
                    tool,
                    tool_use_id.as_deref(),
                    message.as_deref(),
                );
            }
        }
        SessionEvent::Notice { text, .. } => {
            drop(state);
            if let Some(t) = ctx.telemetry.as_ref() {
                t.on_notice(&ctx.employee_id, text);
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
        SessionEvent::ApprovalRequested(req) => return handle_approval_request(state, req, &[]).0,
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
    // Immediately purge every ephemeral turn-scoped `0400` attachment file staged for this turn
    // (including any mid-turn deliveries) as soon as the turn finishes (`TASK-98.3` AC #3).
    purge_active_turn_attachments(ctx, loop_state);
    // A turn a person interrupted (`TASK-103`) ends as they asked: its errors are expected, it
    // never fails over to another connection (or blocks on recovery), and an approval it was
    // waiting on is moot. The warm process lives on for the next turn.
    let interrupted = std::mem::take(&mut loop_state.interrupt_sent);
    loop_state.interrupt_wanted = false;
    loop_state.turn_active = false;
    loop_state.turn_just_ended = true;
    loop_state.in_flight_delivery = None;
    loop_state.mid_turn_dirty = false;
    loop_state.mid_turn_next_check = None;
    state.watchdog.record_turn_completed(Instant::now());
    if !errors.is_empty() && !interrupted {
        state.last_error = Some(errors.join("; "));
    }
    // Steers the turn never confirmed run next, ahead of the queue (at least once).
    for (_, text) in std::mem::take(&mut loop_state.in_flight_steers) {
        loop_state.requeue_steer(state, text);
    }
    let retried_text_only = !interrupted
        && maybe_retry_image_rejection_text_only(state, loop_state, ctx, outcome, errors);
    loop_state.current_turn_had_protocol_images = false;
    loop_state.current_turn_image_fallback_retried = false;
    loop_state.current_turn_remaining_staging_budget =
        crate::turn_attachments::MAX_DOC_STAGING_BYTES_PER_TURN;
    loop_state.current_turn_remaining_attachment_slots =
        crate::turn_attachments::MAX_ATTACHMENTS_PER_TURN;
    let failover_blocked_recovery = !interrupted
        && !retried_text_only
        && evaluate_turn_failover(state, loop_state, ctx, outcome, errors);
    if interrupted {
        state.pending_approvals.clear();
        if state.state == SessionState::WaitingApproval {
            state.state = SessionState::Running;
        }
    }
    // A turn queued behind this one keeps the session busy until it is dispatched.
    state.settle_idle();
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
/// An interrupted turn never fails over, whatever its errors look like (`TASK-103`).
pub(crate) fn classify_turn_failover_reason(
    runtime: &dyn org_agent_runtime::AgentRuntime,
    outcome: &org_agent_runtime::TurnOutcome,
    errors: &[String],
    turn_reply: Option<&str>,
    has_effects: bool,
    rate_limited: bool,
) -> Option<&'static str> {
    if matches!(outcome, org_agent_runtime::TurnOutcome::Interrupted) {
        return None;
    }
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
    let mut attachment_ids = loop_state.current_attachment_ids.clone();
    if loop_state.has_queued_turns() {
        let queued = loop_state.take_queued_turns();
        state.dequeue_turns(queued.len());
        let (queued_text, queued_atts) = merge_queued_turns(queued);
        if !queued_text.is_empty() {
            if objective.is_empty() {
                objective = queued_text;
            } else {
                objective = format!("{objective}\n\n{queued_text}");
            }
        }
        attachment_ids.extend(queued_atts);
    }
    let attachment_ids = dedup_attachment_ids(attachment_ids);
    loop_state.failover_pending = Some(FailoverRequest {
        employee_id: ctx.employee_id.clone(),
        failed_run_id: ctx.run_id.clone(),
        failed_connection_id: conn_id.clone(),
        objective,
        attachment_ids,
        workspace: ctx.workspace.clone(),
        task_ref: ctx.task_ref.clone(),
        tried_connections: tried,
        oauth_refresh_attempted: loop_state.oauth_refresh_attempted,
        oauth_token_fp: ctx.oauth_token_fp.clone(),
        reason,
        rate_limit_resets_at: loop_state.rate_limit_resets_at.clone(),
        stop_epoch: ctx.stop_epoch,
        partition: ctx.partition_key(),
        turn_override: ctx.turn_override.clone(),
    });
    loop_state.stop_requested = true;
    false
}

/// When a turn that carried protocol-level images fails BEFORE any side effect because the
/// provider/model rejected image input, re-queues the turn once in `force_text_only` mode so
/// the agent still receives the text, untrusted manifest, and read-only `staged_path` (`AC #4`).
fn maybe_retry_image_rejection_text_only(
    state: &mut SessionRuntimeState,
    loop_state: &mut PumpLoopState,
    ctx: &PumpContext,
    outcome: &org_agent_runtime::TurnOutcome,
    errors: &[String],
) -> bool {
    if !loop_state.current_turn_had_protocol_images
        || loop_state.current_turn_image_fallback_retried
        || loop_state.current_attachment_ids.is_empty()
        || matches!(outcome, org_agent_runtime::TurnOutcome::Interrupted)
    {
        return false;
    }
    if matches!(outcome, org_agent_runtime::TurnOutcome::Completed) && errors.is_empty() {
        return false;
    }
    let turn_cmds = ctx.bindings.accepted_commands(&ctx.employee_id).len();
    let has_effects =
        loop_state.had_external_effect || turn_cmds > loop_state.accepted_commands_baseline;
    if has_effects {
        return false;
    }
    let mut all_errors = errors.to_vec();
    if let org_agent_runtime::TurnOutcome::Failed { reason } = outcome {
        all_errors.push(reason.clone());
    }
    if !is_provider_image_rejection_error(&all_errors, loop_state.turn_reply.as_deref()) {
        return false;
    }
    loop_state.pending_turns.insert(
        0,
        QueuedTurn {
            text: loop_state.current_objective.clone(),
            attachment_ids: loop_state.current_attachment_ids.clone(),
            force_text_only: true,
        },
    );
    loop_state.front_is_steer = false;
    state.queued_turns = state.queued_turns.saturating_add(1);
    if state.state == SessionState::Idle {
        state.state = SessionState::Running;
    }
    state.last_error = None;
    let note = serde_json::json!({
        "type": "image_fallback_retry",
        "attachment_count": loop_state.current_attachment_ids.len(),
        "at": rfc3339_now(),
    });
    let _ = ctx.store.append_history(&ctx.employee_id, &note, &[]);
    true
}
