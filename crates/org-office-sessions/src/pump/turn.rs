//! Turn dispatch: turn permits and lease heartbeats, queued turns, and mid-turn delivery.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use org_agent_runtime::{Control, ControlError, ImageInputMode, MidTurnDelivery, Output};
use org_office::attachments::DeliveryScope;
use org_proto::sessions::SessionState;

use crate::credentials::{now_ms, rfc3339_now};
use crate::turn_attachments::{
    prepare_turn_attachments, purge_turn_dir, PrepareTurnAttachmentsRequest, PreparedTurnInput,
    MAX_ATTACHMENTS_PER_TURN, MAX_DOC_STAGING_BYTES_PER_TURN,
};

use super::commands::{build_stale_failover_request, release_active_permit, take_pending_turns};
use super::events::apply_outputs;
use super::queue::{merge_queued_turns, QueuedTurn};
use super::{persist_turn_update, MidTurnStageOutcome, PumpContext, PumpLoopState};

fn delivery_scope(ctx: &PumpContext) -> DeliveryScope {
    ctx.partition
        .as_ref()
        .map(|p| p.key().delivery_scope())
        .unwrap_or(DeliveryScope::None)
}

fn stage_entropy_hex() -> String {
    if let Ok(hex) = crate::credentials::random_hex(8) {
        return hex;
    }
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher as _, Hasher as _};
    static FALLBACK_SEQ: AtomicU64 = AtomicU64::new(1);
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(FALLBACK_SEQ.fetch_add(1, Ordering::Relaxed));
    hasher.write_u64(now_ms());
    format!("{:016x}", hasher.finish())
}

fn next_stage_turn_id(run_id: &str, seq: u64, force_text_only: bool) -> String {
    let rand = stage_entropy_hex();
    let raw = if force_text_only {
        format!("{run_id}-t{seq}-fb-{rand}")
    } else {
        format!("{run_id}-t{seq}-{rand}")
    };
    if raw.len() <= 128 {
        raw
    } else {
        format!("t{seq}-{rand}")
    }
}

fn next_mid_turn_stage_id(run_id: &str, delivery_id: &str) -> String {
    let rand = stage_entropy_hex();
    let raw = format!("{run_id}-{delivery_id}-{rand}");
    if raw.len() <= 128 {
        raw
    } else {
        format!("mt-{rand}")
    }
}

fn send_turn_with_fallback(
    ctx: &PumpContext,
    session: &mut Box<dyn org_agent_runtime::Session>,
    queued: &QueuedTurn,
    next_seq: u64,
) -> (Result<Vec<Output>, ControlError>, PreparedTurnInput, bool) {
    let scope = delivery_scope(ctx);
    let stage_turn_id = next_stage_turn_id(&ctx.run_id, next_seq, queued.force_text_only);
    let mut prepared = prepare_turn_attachments(PrepareTurnAttachmentsRequest {
        employee_id: &ctx.employee_id,
        delivery_scope: &scope,
        turn_id: &stage_turn_id,
        base_text: queued.text.clone(),
        attachment_ids: &queued.attachment_ids,
        image_input_mode: ctx.runtime.capabilities().image_input,
        allow_protocol_images: true,
        model_supports_vision: ctx.model_supports_vision,
        force_text_only: queued.force_text_only,
        remaining_staging_budget: MAX_DOC_STAGING_BYTES_PER_TURN,
        remaining_attachment_slots: MAX_ATTACHMENTS_PER_TURN,
        attachments_root: &ctx.attachments_root,
        resolver: ctx.attachment_resolver.as_ref(),
    });
    let mut retried_text_only = queued.force_text_only;
    let first = session.control(
        Control::Send {
            text: std::mem::take(&mut prepared.text),
            images: std::mem::take(&mut prepared.images),
        },
        Instant::now(),
    );
    match first {
        Err(ControlError::Unsupported)
            if prepared.had_protocol_images && !queued.force_text_only =>
        {
            if let Some(tid) = prepared.staged_turn_id.take() {
                purge_turn_dir(&ctx.attachments_root, &tid);
            }
            let fb_id = next_stage_turn_id(&ctx.run_id, next_seq, true);
            prepared = prepare_turn_attachments(PrepareTurnAttachmentsRequest {
                employee_id: &ctx.employee_id,
                delivery_scope: &scope,
                turn_id: &fb_id,
                base_text: queued.text.clone(),
                attachment_ids: &queued.attachment_ids,
                image_input_mode: ctx.runtime.capabilities().image_input,
                allow_protocol_images: true,
                model_supports_vision: ctx.model_supports_vision,
                force_text_only: true,
                remaining_staging_budget: MAX_DOC_STAGING_BYTES_PER_TURN,
                remaining_attachment_slots: MAX_ATTACHMENTS_PER_TURN,
                attachments_root: &ctx.attachments_root,
                resolver: ctx.attachment_resolver.as_ref(),
            });
            retried_text_only = true;
            let note = serde_json::json!({
                "type": "image_fallback_retry",
                "reason": "control_send_unsupported",
                "attachment_count": queued.attachment_ids.len(),
                "at": rfc3339_now(),
            });
            let _ = ctx.store.append_history(&ctx.employee_id, &note, &[]);
            let retry = session.control(
                Control::Send {
                    text: std::mem::take(&mut prepared.text),
                    images: std::mem::take(&mut prepared.images),
                },
                Instant::now(),
            );
            (retry, prepared, retried_text_only)
        }
        other => (other, prepared, retried_text_only),
    }
}

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
            // Every queued turn is refused with this one; a turn queued since stays (and keeps
            // the session busy) for the next dispatch.
            let refused = loop_state.take_queued_turns();
            let mut st = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
            st.last_error = Some(reason);
            st.dequeue_turns(refused.len());
            st.settle_idle();
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
    // `turn_just_ended`: the last turn's end (and its permit's release) is settled first, even
    // when a runtime ended it synchronously in this pass.
    if !loop_state.ready
        || loop_state.turn_active
        || loop_state.turn_just_ended
        || loop_state.stop_requested
        || loop_state.pending_turns.is_empty()
    {
        return;
    }
    match ensure_permit_for_turn(ctx, loop_state) {
        PermitCheck::Ready => {}
        PermitCheck::StaleWarm => {
            let (objective, attachment_ids) =
                merge_queued_turns(take_pending_turns(ctx, loop_state));
            loop_state.failover_pending =
                Some(build_stale_failover_request(ctx, objective, attachment_ids));
            loop_state.stop_requested = true;
            return;
        }
        PermitCheck::Refused => return,
    }
    let queued = loop_state.pending_turns.remove(0);
    loop_state.front_is_steer = false;
    ctx.state
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .dequeue_turns(1);
    let next_seq = loop_state.turns_dispatched.saturating_add(1);
    let (send_res, mut prepared, retried_text_only) =
        send_turn_with_fallback(ctx, session, &queued, next_seq);
    let objective_for_disk = queued.text.clone();
    let attachment_ids_for_disk = queued.attachment_ids.clone();
    loop_state.current_objective = queued.text;
    loop_state.current_attachment_ids = queued.attachment_ids;
    loop_state.current_turn_staged_id = prepared.staged_turn_id.take();
    loop_state.current_turn_had_protocol_images = prepared.had_protocol_images;
    loop_state.current_turn_image_fallback_retried = retried_text_only;
    loop_state.current_turn_remaining_staging_budget = prepared.remaining_staging_budget;
    loop_state.current_turn_remaining_attachment_slots = prepared.remaining_attachment_slots;
    loop_state.turn_reply = None;
    loop_state.had_external_effect = false;
    loop_state.accepted_commands_baseline = ctx.bindings.accepted_commands(&ctx.employee_id).len();
    loop_state.rate_limited = false;
    loop_state.rate_limit_resets_at = None;
    loop_state.last_permit_renew = Instant::now();
    match send_res {
        Ok(outs) => {
            loop_state.turn_active = true;
            loop_state.turns_dispatched = next_seq;
            loop_state.in_flight_delivery = None;
            loop_state.in_flight_delivery_staged_id = None;
            loop_state.mid_turn_staged_ids.clear();
            loop_state.mid_turn_total_this_turn = 0;
            loop_state.mid_turn_agent_this_turn = 0;
            loop_state.mid_turn_dirty = true;
            loop_state.mid_turn_next_check = None;
            persist_turn_update(ctx, |rec| {
                rec.objective = objective_for_disk;
                rec.attachment_ids = attachment_ids_for_disk;
                rec.active = true;
            });
            apply_outputs(outs, stdin, ctx, loop_state, session);
        }
        Err(err) => {
            if let Some(tid) = loop_state.current_turn_staged_id.take() {
                purge_turn_dir(&ctx.attachments_root, &tid);
            }
            loop_state.current_turn_had_protocol_images = false;
            loop_state.current_turn_image_fallback_retried = false;
            loop_state.current_turn_remaining_staging_budget = MAX_DOC_STAGING_BYTES_PER_TURN;
            loop_state.current_turn_remaining_attachment_slots = MAX_ATTACHMENTS_PER_TURN;
            release_active_permit(ctx);
            loop_state.permit_lease_deadline = None;
            let err_msg = format!("turn dispatch failed: {err:?}");
            {
                // The turn never started: record the error clearly and settle back to idle
                // unless another turn is queued behind it.
                let mut st = ctx.state.lock().unwrap_or_else(|p| p.into_inner());
                st.last_error = Some(err_msg);
                st.settle_idle();
            }
            let note = serde_json::json!({
                "type": "turn_dispatch_error",
                "error": format!("{err:?}"),
                "attachment_count": loop_state.current_attachment_ids.len(),
                "at": rfc3339_now(),
            });
            let _ = ctx.store.append_history(&ctx.employee_id, &note, &[]);
        }
    }
}

pub(super) const MID_TURN_NOT_READY_RETRY: Duration = Duration::from_millis(50);

pub(super) fn maybe_deliver_mid_turn(
    stdin: &mut org_supervise::ChildStdin,
    ctx: &PumpContext,
    loop_state: &mut PumpLoopState,
    session: &mut Box<dyn org_agent_runtime::Session>,
) {
    // A turn being interrupted (`TASK-103`) takes no wake: delivered now it could restart the
    // turn (Agy) or be lost with it (Codex, Claude); the wakes stay staged for the next turn.
    if !loop_state.turn_active
        || loop_state.stop_requested
        || loop_state.suspend_now
        || loop_state.interrupt_wanted
        || loop_state.interrupt_sent
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
            let caps = ctx.runtime.capabilities();
            let allow_protocol_images = caps.image_input == ImageInputMode::LocalPath
                && caps.mid_turn_delivery == MidTurnDelivery::NativeSteer;
            let scope = delivery_scope(ctx);
            let stage_id = next_mid_turn_stage_id(&ctx.run_id, &batch.delivery_id);
            let prepared = prepare_turn_attachments(PrepareTurnAttachmentsRequest {
                employee_id: &ctx.employee_id,
                delivery_scope: &scope,
                turn_id: &stage_id,
                base_text: batch.text.clone(),
                attachment_ids: &batch.attachment_ids,
                image_input_mode: caps.image_input,
                allow_protocol_images,
                model_supports_vision: ctx.model_supports_vision,
                force_text_only: false,
                remaining_staging_budget: loop_state.current_turn_remaining_staging_budget,
                remaining_attachment_slots: loop_state.current_turn_remaining_attachment_slots,
                attachments_root: &ctx.attachments_root,
                resolver: ctx.attachment_resolver.as_ref(),
            });
            let ctrl = Control::DeliverMidTurn {
                delivery_id: batch.delivery_id.clone(),
                text: prepared.text,
                images: prepared.images,
            };
            match session.control(ctrl, Instant::now()) {
                Ok(outs) => {
                    loop_state.in_flight_delivery = Some(batch);
                    loop_state.in_flight_delivery_staged_id = prepared.staged_turn_id;
                    loop_state.current_turn_remaining_staging_budget =
                        prepared.remaining_staging_budget;
                    loop_state.current_turn_remaining_attachment_slots =
                        prepared.remaining_attachment_slots;
                    apply_outputs(outs, stdin, ctx, loop_state, session);
                }
                Err(org_agent_runtime::ControlError::NotReady) => {
                    if let Some(tid) = prepared.staged_turn_id {
                        purge_turn_dir(&ctx.attachments_root, &tid);
                    }
                    loop_state.mid_turn_next_check =
                        Some(Instant::now() + MID_TURN_NOT_READY_RETRY);
                }
                Err(_) => {
                    if let Some(tid) = prepared.staged_turn_id {
                        purge_turn_dir(&ctx.attachments_root, &tid);
                    }
                }
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
