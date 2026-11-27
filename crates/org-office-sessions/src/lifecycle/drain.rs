//! Turn-end drain of queued wakes into a live warm session and stale warm-handle recycling
//! (`TASK-115`, `TASK-170`).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, Mutex};

use org_office::organization_family::records::{BuiltinKey, EmployeeStatus};
use org_office::Office;

use crate::binding::RunBindingRegistry;
use crate::credentials::{now_ms, rfc3339_now};
use crate::employee_locks::EmployeeLockMap;
use crate::manager::{set_waiting_messages_count, SandboxRunner, SessionManager, SessionRegistry};
use crate::pump::PumpCommand;
use crate::recovery::SessionStore;
use crate::wake::{WakeHoldReason, WakeQuotaRecord};

use super::wake_turn::{assigned_task_refs, compose_wake_turn, office_budget_held};
use super::DispatchWakeGuard;

pub(super) fn recycle_stale_warm_handle(
    employee_id: &str,
    expected_run_id: &str,
    reason: &str,
    ctx: &TurnEndDrainCtx,
) -> bool {
    let Some(mgr) = ctx.weak_mgr.as_ref().and_then(|w| w.upgrade()) else {
        return false;
    };
    crate::manager::recycle_stale_warm_handle_in(&mgr, employee_id, expected_run_id, reason)
}

pub(super) fn try_send_wake_on_live_handle(
    employee_id: &str,
    queue_task_id: Option<&str>,
    turn: crate::pump::QueuedTurn,
    ctx: &TurnEndDrainCtx,
) -> Result<bool, WakeHoldReason> {
    let handle_info = {
        let reg = ctx.registry.lock().unwrap_or_else(|p| p.into_inner());
        reg.live.get(employee_id).map(|handle| {
            let run_id = handle
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .run_id
                .clone()
                .unwrap_or_default();
            (
                run_id,
                handle.tx.clone(),
                handle.connection_id.clone(),
                handle.route_digest.clone(),
                handle.oauth_expires_at_ms,
                Arc::clone(&handle.active_permit),
                handle.sandbox_plan.clone(),
            )
        })
    };
    let Some((run_id, tx, conn_id, route_digest, oauth_expires_at_ms, active_permit, sandbox_plan)) =
        handle_info
    else {
        return Ok(false);
    };
    if crate::host_auth::is_oauth_expiring(oauth_expires_at_ms, now_ms()) {
        recycle_stale_warm_handle(employee_id, &run_id, "oauth_token_expiring", ctx);
        return Ok(false);
    }
    if ctx
        .sandbox_runner
        .is_plan_stale(&ctx.office, employee_id, &sandbox_plan)
    {
        recycle_stale_warm_handle(employee_id, &run_id, "sandbox_topology_changed", ctx);
        return Ok(false);
    }
    if conn_id.is_none()
        && ctx
            .office
            .load_agent_execution(employee_id)
            .ok()
            .flatten()
            .is_some()
    {
        recycle_stale_warm_handle(employee_id, &run_id, "warm_route_stale", ctx);
        return Ok(false);
    }
    let acquired_permit = match acquire_live_handle_permit(
        &ctx.acquire_warm_permit,
        &ctx.office,
        ctx.hq_permits.as_deref(),
        conn_id.as_deref(),
        (!route_digest.is_empty()).then_some(route_digest.as_str()),
        &active_permit,
        employee_id,
        queue_task_id,
    ) {
        Ok(permit) => permit,
        Err(LivePermitError::Stale) => {
            recycle_stale_warm_handle(employee_id, &run_id, "warm_route_stale", ctx);
            return Ok(false);
        }
        Err(LivePermitError::Hold(reason)) => return Err(reason),
    };
    let (sent, release_pid) = commit_wake_send_locked(
        &ctx.registry,
        employee_id,
        &run_id,
        &tx,
        &active_permit,
        acquired_permit,
        turn,
    );
    if let Some(pid) = release_pid {
        crate::router::release_turn_permit_any(&ctx.office, ctx.hq_permits.as_deref(), &pid);
    }
    Ok(sent)
}

pub(super) fn commit_wake_send_locked(
    registry: &Arc<Mutex<SessionRegistry>>,
    employee_id: &str,
    run_id: &str,
    tx: &std::sync::mpsc::Sender<PumpCommand>,
    active_permit: &Arc<Mutex<Option<(String, u64)>>>,
    acquired_permit: Option<(String, u64)>,
    turn: crate::pump::QueuedTurn,
) -> (bool, Option<String>) {
    let reg = registry.lock().unwrap_or_else(|p| p.into_inner());
    let live = (!reg.draining && !reg.shutting_down && !reg.starting.contains(employee_id))
        .then(|| reg.live.get(employee_id))
        .flatten()
        .filter(|handle| {
            handle
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .run_id
                .as_deref()
                == Some(run_id)
        });
    let Some(handle) = live else {
        return (false, acquired_permit.map(|(pid, _)| pid));
    };
    let (installed_pid, redundant_pid) = match acquired_permit {
        Some(p) => {
            let mut slot = active_permit.lock().unwrap_or_else(|e| e.into_inner());
            if slot.is_none() {
                let pid = p.0.clone();
                *slot = Some(p);
                (Some(pid), None)
            } else {
                (None, Some(p.0))
            }
        }
        None => (None, None),
    };
    // Counted and marked busy before the send, in the `registry` critical section that checked
    // the run is still live (`TASK-80.1`).
    if crate::pump::queue_turn_with_attachments(&handle.state, tx, turn) {
        return (true, redundant_pid);
    }
    let rollback_pid = installed_pid.and_then(|pid| {
        let mut slot = active_permit.lock().unwrap_or_else(|e| e.into_inner());
        if slot.as_ref().is_some_and(|(cur, _)| cur == &pid) {
            slot.take().map(|(id, _)| id)
        } else {
            None
        }
    });
    (false, rollback_pid.or(redundant_pid))
}

pub(super) enum LivePermitError {
    Stale,
    Hold(WakeHoldReason),
}

/// Acquires a warm turn permit for a live handle: [`crate::router::acquire_warm_permit_for_office`]
/// in production, which on a satellite is an HQ permit RPC (after worktree and bare-repository
/// digest walks). The drain calls it with no employee lock held (`TASK-170`).
pub(crate) type WarmPermitAcquirer = Arc<
    dyn Fn(
            &Office,
            Option<&dyn crate::router::HqPermitCoordinator>,
            &str,
            &str,
            Option<&str>,
            Option<&str>,
        ) -> crate::router::WarmPermitOutcome
        + Send
        + Sync,
>;

#[expect(
    clippy::too_many_arguments,
    reason = "permit acquisition takes the acquirer, office, HQ port, route, permit slot, employee, and task"
)]
pub(super) fn acquire_live_handle_permit(
    acquire_warm_permit: &WarmPermitAcquirer,
    office: &Office,
    hq_permits: Option<&dyn crate::router::HqPermitCoordinator>,
    conn_id: Option<&str>,
    route_digest: Option<&str>,
    active_permit: &Arc<Mutex<Option<(String, u64)>>>,
    employee_id: &str,
    task_id: Option<&str>,
) -> Result<Option<(String, u64)>, LivePermitError> {
    let Some(cid) = conn_id else {
        return Ok(None);
    };
    if active_permit
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .is_some()
    {
        return Ok(None);
    }
    match acquire_warm_permit(office, hq_permits, cid, employee_id, task_id, route_digest) {
        crate::router::WarmPermitOutcome::Granted(p) => Ok(Some(p)),
        crate::router::WarmPermitOutcome::StaleWarm => Err(LivePermitError::Stale),
        crate::router::WarmPermitOutcome::Unavailable { .. } => {
            Err(LivePermitError::Hold(WakeHoldReason::AdmissionRefused))
        }
        crate::router::WarmPermitOutcome::Refused(err) => {
            let reason = if err.message.starts_with("BUDGET_HELD") {
                WakeHoldReason::BudgetHold
            } else {
                WakeHoldReason::AdmissionRefused
            };
            Err(LivePermitError::Hold(reason))
        }
    }
}

/// Turn-end drain of queued wakes into the live warm session (`TASK-115`, `TASK-170`).
///
/// Three phases so the employee lock is never held across the warm-permit acquisition (an HQ
/// permit RPC on satellites, plus worktree and bare-repository digest walks) or the sandbox
/// staleness check: claim under the lock, send without it, then commit under the lock against a
/// fresh read of the queue so wakes enqueued meanwhile are never lost. The per-employee
/// [`DispatchWakeGuard`] stays held throughout, so no other dispatcher or mid-turn stager can claim
/// the same items in between. A wake withdrawn while the permit RPC runs may still reach the
/// session in this turn (as on the cold-start path, `launch_claimed_wakes`); it is pruned from
/// the durable queue either way.
pub(super) fn drain_pending_wakes_into_live_pump(employee_id: &str, ctx: &TurnEndDrainCtx) -> bool {
    if ctx
        .store
        .read_pending_wakes(employee_id)
        .ok()
        .flatten()
        .is_none_or(|q| q.items.is_empty())
    {
        return false;
    }
    let Some(_dispatch_guard) = DispatchWakeGuard::try_acquire(&ctx.dispatching_wakes, employee_id)
    else {
        return false;
    };
    let Some(claim) = claim_live_drain_batch(employee_id, ctx) else {
        return false;
    };
    if claim.requires_session_recycle {
        let run_id = {
            let reg = ctx.registry.lock().unwrap_or_else(|p| p.into_inner());
            reg.live.get(employee_id).and_then(|h| {
                h.state
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .run_id
                    .clone()
            })
        };
        if let Some(run_id) = run_id {
            return recycle_stale_warm_handle(
                employee_id,
                &run_id,
                "session_affinity_requested",
                ctx,
            );
        }
        return false;
    }
    if !claim.task_refs.is_empty() {
        if let Some(mgr) = ctx.weak_mgr.as_ref().and_then(|w| w.upgrade()) {
            let refs: Vec<&str> = claim.task_refs.iter().map(String::as_str).collect();
            mgr.preprepare_task_worktrees(employee_id, &refs);
        }
    }
    let sent = try_send_wake_on_live_handle(
        employee_id,
        claim.task_id.as_deref(),
        crate::pump::QueuedTurn::new(claim.objective.clone(), claim.attachment_ids.clone()),
        ctx,
    );
    let busy_again = matches!(sent, Ok(true));
    commit_live_drain(employee_id, ctx, &claim, sent);
    busy_again
}

/// Runs one turn-end drain for `employee_id` with `acquirer` standing in for the warm-permit
/// acquisition (the `TASK-170` lock-scope regression test blocks inside it).
#[cfg(test)]
pub(crate) fn drain_into_live_pump_with_acquirer(
    mgr: &SessionManager,
    employee_id: &str,
    acquirer: WarmPermitAcquirer,
) {
    let mut ctx = mgr.turn_end_drain_ctx();
    ctx.acquire_warm_permit = acquirer;
    let _ = drain_pending_wakes_into_live_pump(employee_id, &ctx);
}

/// Items claimed from the durable queue for one live-pump drain, with the provenance recorded in
/// history once the turn is actually sent.
pub(super) struct LiveDrainClaim {
    pub(super) item_ids: HashSet<String>,
    pub(super) objective: String,
    pub(super) attachment_ids: Vec<String>,
    pub(super) task_id: Option<String>,
    pub(super) task_refs: Vec<String>,
    pub(super) person_triggered: bool,
    pub(super) requires_session_recycle: bool,
    pub(super) trigger: serde_json::Value,
    pub(super) ts: u64,
}

/// Phase 1 of the drain, under the employee lock: reads the queue, applies every hold that
/// needs no network or disk walk (suspension, inactive agent, budget hold, hourly quota), and
/// composes the turn input, holding the batch when its tasks' must-follow processes cannot
/// travel with it.
pub(super) fn claim_live_drain_batch(
    employee_id: &str,
    ctx: &TurnEndDrainCtx,
) -> Option<LiveDrainClaim> {
    let _lock = ctx.employee_locks.lock(employee_id);
    let Ok(Some(mut queue)) = ctx.store.read_pending_wakes(employee_id) else {
        return None;
    };
    if queue.items.is_empty() {
        if queue.seen_item_ids.is_empty() {
            let _ = ctx.store.clear_pending_wakes(employee_id);
        }
        return None;
    }
    if ctx
        .store
        .read_suspended_state(employee_id)
        .ok()
        .flatten()
        .is_some()
    {
        return None;
    }
    let emp = ctx.office.load_employee(employee_id).ok().flatten()?;
    // The Chief of Staff's live process acts for the member whose turn it ran: a wake never
    // drains into it (`TASK-80.1`); her queue starts through the partition-checked path.
    if emp.status != EmployeeStatus::Active || emp.builtin_key == Some(BuiltinKey::ChiefOfStaff) {
        return None;
    }
    let ts = now_ms();
    let tokens = ctx.bindings.active_tokens();
    let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
    let task_refs = assigned_task_refs(&queue.items);
    let task_id = task_refs.first().map(|t| (*t).to_owned());
    let task_refs_owned: Vec<String> = task_refs.iter().map(|t| (*t).to_owned()).collect();
    if office_budget_held(&ctx.office, employee_id, &task_refs)
        || ctx
            .budget_hold
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
    {
        queue.set_hold(WakeHoldReason::BudgetHold, ts, None);
        let _ = ctx.store.write_pending_wakes_redacted(&queue, &refs);
        return None;
    }
    let person_triggered = queue.has_person_item();
    if !person_triggered {
        let mut quota = ctx
            .store
            .read_wake_quota(employee_id)
            .ok()
            .flatten()
            .unwrap_or_else(|| WakeQuotaRecord::empty(employee_id));
        if let Err(retry_after) = quota.check_limit(ts, ctx.window_ms, ctx.hourly_limit) {
            queue.set_hold(WakeHoldReason::HourlyLimit, ts, Some(retry_after));
            let _ = ctx.store.write_pending_wakes_redacted(&queue, &refs);
            return None;
        }
    }
    // The same turn input as a fresh start: the tasks' must-follow rules travel with it, and a
    // batch they cannot travel with stays queued instead of reaching the agent without them.
    let Ok(objective) = compose_wake_turn(&ctx.office, employee_id, &queue.items, &[]) else {
        queue.set_hold(WakeHoldReason::AdmissionRefused, ts, None);
        let _ = ctx.store.write_pending_wakes_redacted(&queue, &refs);
        return None;
    };
    let first = &queue.items[0];
    let trigger = serde_json::json!({
        "type": "turn_trigger",
        "kind": first.kind.as_str(),
        "sender_id": first.sender_id,
        "source_id": first.source_id,
        "coalesced_count": queue.items.len(),
        "held_reason": queue.hold_reason.map(|r| r.as_str()),
        "waited_ms": queue.held_since_ms.map(|since| ts.saturating_sub(since)),
        "at": rfc3339_now(),
    });
    let attachment_ids = crate::wake::collect_wake_attachment_ids(&queue.items);
    let requires_session_recycle = queue.items.iter().any(|i| i.session_affinity.is_some());
    Some(LiveDrainClaim {
        item_ids: queue.items.iter().map(|i| i.item_id.clone()).collect(),
        objective,
        attachment_ids,
        task_id,
        task_refs: task_refs_owned,
        person_triggered,
        requires_session_recycle,
        trigger,
        ts,
    })
}

/// Phase 3 of the drain, under the employee lock again: re-reads the queue and quota so wakes
/// enqueued while the permit RPC ran are kept, then records the hold or the sent turn.
pub(super) fn commit_live_drain(
    employee_id: &str,
    ctx: &TurnEndDrainCtx,
    claim: &LiveDrainClaim,
    sent: Result<bool, WakeHoldReason>,
) {
    let _lock = ctx.employee_locks.lock(employee_id);
    let tokens = ctx.bindings.active_tokens();
    let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
    match sent {
        Err(reason) => {
            if let Ok(Some(mut queue)) = ctx.store.read_pending_wakes(employee_id) {
                queue.set_hold(reason, claim.ts, None);
                let _ = ctx.store.write_pending_wakes_redacted(&queue, &refs);
            }
        }
        Ok(false) => {}
        Ok(true) => {
            if let Some(mgr) = ctx.weak_mgr.as_ref().and_then(|w| w.upgrade()) {
                mgr.hibernation
                    .note_activity(employee_id, std::time::Instant::now());
            }
            if !claim.person_triggered {
                let mut quota = ctx
                    .store
                    .read_wake_quota(employee_id)
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| WakeQuotaRecord::empty(employee_id));
                quota.record_start(claim.ts, ctx.window_ms);
                let _ = ctx.store.write_wake_quota(&quota);
            }
            let _ = ctx.store.append_history(employee_id, &claim.trigger, &refs);
            let remaining_mid_turn = remove_claimed_wakes(
                &ctx.store,
                &ctx.waiting_messages,
                &refs,
                employee_id,
                &claim.item_ids,
            );
            if remaining_mid_turn {
                let reg = ctx.registry.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(handle) = reg.live.get(employee_id).filter(|h| h.supports_mid_turn) {
                    let _ = handle.tx.send(PumpCommand::NotifyPendingWakes);
                }
            }
        }
    }
}

/// Removes `claimed` item IDs from `emp_id`'s durable queue (callers hold the employee lock),
/// keeping `seen_item_ids` so a replayed relay never starts a duplicate turn (`TASK-52` P7-#12,
/// P8-#10). Returns whether mid-turn-eligible items that arrived meanwhile remain queued.
pub(crate) fn remove_claimed_wakes(
    store: &SessionStore,
    waiting_messages: &Mutex<HashMap<String, Arc<AtomicU32>>>,
    redaction_tokens: &[&str],
    emp_id: &str,
    claimed: &HashSet<String>,
) -> bool {
    let Ok(Some(mut queue)) = store.read_pending_wakes(emp_id) else {
        return false;
    };
    queue.items.retain(|i| !claimed.contains(&i.item_id));
    let has_remaining_mid_turn = queue.items.iter().any(|i| i.kind.is_mid_turn_eligible());
    if queue.items.is_empty() && queue.seen_item_ids.is_empty() {
        if store.clear_pending_wakes(emp_id).is_ok() {
            set_waiting_messages_count(waiting_messages, emp_id, 0);
        }
        return has_remaining_mid_turn;
    }
    if queue.items.is_empty() {
        queue.hold_reason = None;
        queue.held_since_ms = None;
        queue.retry_after_ms = None;
    }
    queue.updated_at = rfc3339_now();
    let _ = store.write_pending_wakes_redacted(&queue, redaction_tokens);
    set_waiting_messages_count(waiting_messages, emp_id, queue.items.len());
    has_remaining_mid_turn
}

pub(super) struct TurnEndDrainCtx {
    pub(super) store: SessionStore,
    pub(super) registry: Arc<Mutex<SessionRegistry>>,
    pub(super) budget_hold: Arc<Mutex<Option<String>>>,
    pub(super) employee_locks: Arc<EmployeeLockMap>,
    pub(super) dispatching_wakes: Arc<Mutex<HashSet<String>>>,
    pub(super) waiting_messages: Arc<Mutex<HashMap<String, Arc<AtomicU32>>>>,
    pub(super) bindings: RunBindingRegistry,
    pub(super) office: Arc<Office>,
    pub(super) sandbox_runner: Arc<dyn SandboxRunner>,
    pub(super) hq_permits: Option<Arc<dyn crate::router::HqPermitCoordinator>>,
    pub(super) acquire_warm_permit: WarmPermitAcquirer,
    pub(super) weak_mgr: Option<std::sync::Weak<SessionManager>>,
    pub(super) hourly_limit: usize,
    pub(super) window_ms: u64,
}

impl SessionManager {
    /// The turn-end drain context of this manager's pump hooks; it acquires warm permits through
    /// the office router (an HQ permit RPC on satellites).
    pub(super) fn turn_end_drain_ctx(&self) -> TurnEndDrainCtx {
        TurnEndDrainCtx {
            store: self.store.clone(),
            registry: Arc::clone(&self.registry),
            budget_hold: Arc::clone(&self.budget_hold),
            employee_locks: Arc::clone(&self.employee_locks),
            dispatching_wakes: Arc::clone(&self.dispatching_wakes),
            waiting_messages: Arc::clone(&self.waiting_messages),
            bindings: self.bindings.clone(),
            office: Arc::clone(&self.office),
            sandbox_runner: Arc::clone(&self.sandbox_runner),
            hq_permits: self.config.hq_permits.clone(),
            acquire_warm_permit: Arc::new(crate::router::acquire_warm_permit_for_office),
            weak_mgr: self.weak_self(),
            hourly_limit: self.config.hourly_wake_limit,
            window_ms: self.config.wake_window.as_millis() as u64,
        }
    }
}
