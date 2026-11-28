//! Pump hook wiring: suspension, turn-end drain, mid-turn staging and delivery commit, usage,
//! permits, rate limits, and failover (`TASK-115`, `doc-101` §4).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use org_office::organization_family::records::{BuiltinKey, EmployeeStatus};
use org_office::Office;

use crate::binding::RunBindingRegistry;
use crate::credentials::{now_ms, rfc3339_now};
use crate::employee_locks::EmployeeLockMap;
use crate::manager::{set_waiting_messages_count, SandboxPlan, SessionManager};
use crate::pump::{
    FailoverHook, MidTurnDeliveredHook, MidTurnStageOutcome, OpenRequiredAsksHook,
    PermitAcquireHook, PermitReleaseHook, PermitReleasePendingHook, PermitRenewHook, PumpHooks,
    RateLimitHook, StageMidTurnWakesHook, StagedMidTurnBatch, SuspendedHook, TurnEndedHook,
};
use crate::recovery::SessionStore;
use crate::wake::{
    compose_mid_turn_notice_batch, sender_conv_key, MidTurnLimits, MidTurnRateTracker,
    MidTurnSelection,
};

use super::asks::{persist_suspended_record, take_open_required_asks};
use super::drain::drain_pending_wakes_into_live_pump;
use super::usage::{flush_pending_usage, record_session_usage};

impl SessionManager {
    /// The pump hooks of one run. A `partitioned` run (the Chief of Staff's, `TASK-80.1`) gets
    /// no mid-turn delivery and no turn-end drain into its live process: a queued wake never
    /// joins the turn or the process of the member the run acts for. Its turn end instead
    /// dispatches the queue like an idle agent's, through the partition-checked start path,
    /// in her `tess:wake` partition, acting for nobody.
    pub(crate) fn build_pump_hooks(
        &self,
        employee_id: &str,
        route_digest: String,
        oauth_expires_at_ms: Option<u64>,
        sandbox_plan: SandboxPlan,
        partitioned: bool,
        turn_override: crate::router::TurnModelOverride,
    ) -> PumpHooks {
        let open_asks_check = Arc::clone(&self.open_required_asks);
        let has_open: OpenRequiredAsksHook = Arc::new(move |emp_id: &str| {
            open_asks_check
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get(emp_id)
                .is_some_and(|list| !list.is_empty())
        });

        let store_s = self.store.clone();
        let asks_s = Arc::clone(&self.open_required_asks);
        let last_s = Arc::clone(&self.last_states);
        let locks_s = Arc::clone(&self.employee_locks);
        let bindings_s = self.bindings.clone();
        let on_suspended: SuspendedHook = Arc::new(
            move |emp_id: &str,
                  reaped_bg: Vec<String>,
                  session_id: Option<String>,
                  rsid: Option<String>| {
                let _guard = locks_s.lock(emp_id);
                let asks = take_open_required_asks(&asks_s, emp_id);
                if asks.is_empty() {
                    return false;
                }
                let sid = session_id.unwrap_or_else(|| format!("sess-{emp_id}"));
                let tokens = bindings_s.active_tokens();
                let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
                persist_suspended_record(&store_s, emp_id, asks, reaped_bg, sid, rsid, &refs);
                last_s
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .remove(emp_id);
                true
            },
        );

        let on_turn_ended = build_turn_ended_hook(self, partitioned);
        let (stage_mid_turn_wakes, on_mid_turn_delivered) = build_mid_turn_hooks(self);

        let usage_office = Arc::clone(&self.office);
        let usage_store = self.store.clone();
        let usage_seq = Arc::new(AtomicU64::new(1));
        let on_usage: crate::pump::UsageHook = Arc::new(move |emp_id, run_id, usage| {
            let seq = usage_seq.fetch_add(1, Ordering::Relaxed);
            record_session_usage(&usage_office, &usage_store, emp_id, run_id, seq, usage);
        });

        let (on_permit_acquire, on_permit_renew, on_permit_release, on_permit_release_pending) =
            build_permit_hooks(
                self,
                employee_id,
                route_digest,
                oauth_expires_at_ms,
                turn_override,
            );

        let on_rate_limit = build_rate_limit_hook(self);

        let failover_weak = self.weak_self();
        let on_failover: FailoverHook = Arc::new(move |req| {
            if let Some(mgr) = failover_weak.as_ref().and_then(|w| w.upgrade()) {
                crate::manager::spawn_bg_worker(&mgr, "org-pool-failover", move |m| {
                    m.failover_relaunch(req)
                });
            }
        });

        let is_sandbox_topology_stale: Option<crate::pump::SandboxTopologyStaleHook> = {
            let runner = Arc::clone(&self.sandbox_runner);
            let office = Arc::clone(&self.office);
            let emp = employee_id.to_owned();
            Some(Arc::new(move || {
                runner.is_plan_stale(&office, &emp, &sandbox_plan)
            }))
        };

        PumpHooks {
            has_open_required_asks: Some(has_open),
            on_turn_ended: Some(on_turn_ended),
            on_suspended: Some(on_suspended),
            on_usage: Some(on_usage),
            on_permit_acquire: Some(on_permit_acquire),
            on_permit_renew: Some(on_permit_renew),
            on_permit_release: Some(on_permit_release),
            on_permit_release_pending: Some(on_permit_release_pending),
            on_rate_limit: Some(on_rate_limit),
            on_failover: Some(on_failover),
            stage_mid_turn_wakes: (!partitioned).then_some(stage_mid_turn_wakes),
            on_mid_turn_delivered: (!partitioned).then_some(on_mid_turn_delivered),
            is_sandbox_topology_stale,
            on_approval_changed: Some(build_approval_changed_hook(&self.approval_change_hook)),
        }
    }
}

fn build_approval_changed_hook(
    slot: &Arc<Mutex<Option<crate::manager::ApprovalChangeHook>>>,
) -> crate::pump::ApprovalChangedHook {
    let approval_hook = Arc::clone(slot);
    Arc::new(move || {
        let cb = approval_hook
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Some(cb) = cb {
            cb();
        }
    })
}

fn build_rate_limit_hook(mgr: &SessionManager) -> RateLimitHook {
    let rl_office = Arc::clone(&mgr.office);
    let rl_hq_permits = mgr.config.hq_permits.clone();
    Arc::new(move |conn_id, resets_at| {
        let _ = rl_office.record_connection_cooldown_internal(conn_id, None, resets_at, None);
        crate::router::report_cooldown_any(
            &rl_office,
            rl_hq_permits.as_deref(),
            conn_id,
            resets_at,
            Some("rate_limited"),
        );
    })
}

/// The turn-end hook: settles the turn's usage and budget holds, then hands queued wakes on.
/// An ordinary run drains them into its own live process. A partitioned run never does: a
/// background dispatch starts them through the partition-checked path once the process is
/// idle (`TASK-80.1`).
fn build_turn_ended_hook(mgr: &SessionManager, partitioned: bool) -> TurnEndedHook {
    let drain_ctx = mgr.turn_end_drain_ctx();
    Arc::new(move |emp_id: &str| {
        let _ = flush_pending_usage(&drain_ctx.office, &drain_ctx.store, emp_id);
        let _ = drain_ctx.office.mark_budget_holds_stopped(emp_id);
        if !partitioned {
            let busy_again = drain_pending_wakes_into_live_pump(emp_id, &drain_ctx);
            if !busy_again {
                if let Some(mgr) = drain_ctx.weak_mgr.as_ref().and_then(|weak| weak.upgrade()) {
                    mgr.hibernation
                        .note_turn_idle(emp_id, std::time::Instant::now());
                }
            }
            return;
        }
        if let Some(mgr) = drain_ctx.weak_mgr.as_ref().and_then(|weak| weak.upgrade()) {
            mgr.hibernation
                .note_turn_idle(emp_id, std::time::Instant::now());
        }
        let queued = drain_ctx
            .store
            .pending_wakes_path(emp_id)
            .is_ok_and(|path| path.exists());
        let mgr = drain_ctx.weak_mgr.as_ref().and_then(|weak| weak.upgrade());
        if let (true, Some(mgr)) = (queued, mgr) {
            let emp = emp_id.to_owned();
            crate::manager::spawn_bg_worker(&mgr, "org-partition-wake", move |m| {
                let _ = m.process_due_wakes_for(&emp);
            });
        }
    })
}

pub(super) fn build_mid_turn_hooks(
    mgr: &SessionManager,
) -> (StageMidTurnWakesHook, MidTurnDeliveredHook) {
    let stage_store = mgr.store.clone();
    let stage_office = Arc::clone(&mgr.office);
    let stage_budget = Arc::clone(&mgr.budget_hold);
    let stage_locks = Arc::clone(&mgr.employee_locks);
    let stage_dispatching = Arc::clone(&mgr.dispatching_wakes);
    let stage_rates = Arc::clone(&mgr.mid_turn_rates);
    let stage_limits: MidTurnLimits = mgr.config.mid_turn_limits;
    let delivery_seq = Arc::new(AtomicU64::new(1));

    let stage_hook: StageMidTurnWakesHook =
        Arc::new(move |emp_id, task_ref, turn_total, turn_agent| {
            stage_mid_turn_batch(
                &stage_office,
                &stage_store,
                &stage_budget,
                &stage_locks,
                &stage_dispatching,
                &stage_rates,
                &delivery_seq,
                stage_limits,
                emp_id,
                task_ref,
                turn_total,
                turn_agent,
            )
        });

    let commit_store = mgr.store.clone();
    let commit_locks = Arc::clone(&mgr.employee_locks);
    let commit_rates = Arc::clone(&mgr.mid_turn_rates);
    let commit_waiting = Arc::clone(&mgr.waiting_messages);
    let commit_bindings = mgr.bindings.clone();
    let hourly_window_ms = mgr.config.mid_turn_limits.hourly_window_ms;

    let delivered_hook: MidTurnDeliveredHook = Arc::new(move |emp_id, batch| {
        commit_mid_turn_delivered(
            &commit_store,
            &commit_locks,
            &commit_rates,
            &commit_waiting,
            &commit_bindings,
            hourly_window_ms,
            emp_id,
            batch,
        );
    });

    (stage_hook, delivered_hook)
}

pub(super) fn employee_allows_mid_turn_staging(
    office: &Office,
    store: &SessionStore,
    budget_hold: &Mutex<Option<String>>,
    emp_id: &str,
    task_ref: Option<&str>,
) -> bool {
    let Some(emp) = office.load_employee(emp_id).ok().flatten() else {
        return false;
    };
    // The Chief of Staff's turn acts for one member: another's wake never joins it.
    if emp.status != EmployeeStatus::Active || emp.builtin_key == Some(BuiltinKey::ChiefOfStaff) {
        return false;
    }
    if store.read_suspended_state(emp_id).ok().flatten().is_some() {
        return false;
    }
    if store
        .read_active_turn(emp_id)
        .ok()
        .flatten()
        .is_some_and(|rec| rec.stall_locked || rec.recovery_required)
    {
        return false;
    }
    let has_mem_hold = budget_hold
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .is_some();
    let has_db_hold = office
        .active_budget_holds(Some(emp_id), task_ref)
        .is_ok_and(|h| !h.is_empty());
    !has_mem_hold && !has_db_hold
}

pub(super) fn build_staged_mid_turn_batch(
    eligible: &[crate::wake::PendingWakeItem],
    delivery_seq: &AtomicU64,
    ts: u64,
) -> MidTurnStageOutcome {
    let (text, item_ids) = compose_mid_turn_notice_batch(eligible);
    if item_ids.is_empty() {
        return MidTurnStageOutcome::None;
    }
    let id_set: HashSet<&str> = item_ids.iter().map(String::as_str).collect();
    let included: Vec<&crate::wake::PendingWakeItem> = eligible
        .iter()
        .filter(|item| id_set.contains(item.item_id.as_str()))
        .collect();
    let mut has_agent_item = false;
    let mut agent_sender_convs = Vec::new();
    let mut attachment_ids = Vec::new();
    let mut seen_atts = HashSet::new();
    for item in included {
        if !item.sender_is_person {
            has_agent_item = true;
            let key = sender_conv_key(item);
            if !agent_sender_convs.contains(&key) {
                agent_sender_convs.push(key);
            }
        }
        for raw_att in &item.attachment_ids {
            let trimmed = raw_att.trim();
            if !trimmed.is_empty() && seen_atts.insert(trimmed.to_owned()) {
                attachment_ids.push(trimmed.to_owned());
            }
        }
    }
    let seq = delivery_seq.fetch_add(1, Ordering::Relaxed);
    MidTurnStageOutcome::Ready(StagedMidTurnBatch {
        delivery_id: format!("mt-{ts}-{seq}"),
        item_ids,
        text,
        attachment_ids,
        has_agent_item,
        agent_sender_convs,
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "internal helper separates read-only staging from PumpHooks wiring"
)]
pub(super) fn stage_mid_turn_batch(
    office: &Office,
    store: &SessionStore,
    budget_hold: &Mutex<Option<String>>,
    employee_locks: &EmployeeLockMap,
    dispatching_wakes: &Mutex<HashSet<String>>,
    mid_turn_rates: &Mutex<HashMap<String, MidTurnRateTracker>>,
    delivery_seq: &AtomicU64,
    limits: MidTurnLimits,
    emp_id: &str,
    task_ref: Option<&str>,
    turn_total: u32,
    turn_agent: u32,
) -> MidTurnStageOutcome {
    // Wait briefly if a turn-start dispatch is currently removing its claimed wakes so the warm
    // pump cannot re-stage those same items mid-turn (`TASK-115` zero-duplicate invariant).
    if dispatching_wakes
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .contains(emp_id)
    {
        return MidTurnStageOutcome::Wait(Duration::from_millis(50));
    }
    if !employee_allows_mid_turn_staging(office, store, budget_hold, emp_id, task_ref) {
        return MidTurnStageOutcome::None;
    }
    // Phase 2A: read pending-wakes.v1.json WITHOUT mutating disk so a crash or failed delivery
    // preserves every queued item for the next turn (`TASK-115` / `doc-101` §4B).
    let items = {
        let _guard = employee_locks.lock(emp_id);
        match store.read_pending_wakes(emp_id) {
            Ok(Some(queue)) if !queue.items.is_empty() => queue.items,
            _ => return MidTurnStageOutcome::None,
        }
    };
    let ts = now_ms();
    let selection = {
        let mut rates = mid_turn_rates.lock().unwrap_or_else(|p| p.into_inner());
        let tracker = rates.entry(emp_id.to_owned()).or_default();
        tracker.select_batch(&items, ts, turn_total, turn_agent, limits)
    };
    match selection {
        MidTurnSelection::None => MidTurnStageOutcome::None,
        MidTurnSelection::WaitMs(ms) => MidTurnStageOutcome::Wait(Duration::from_millis(ms.max(1))),
        MidTurnSelection::Ready(eligible) => {
            build_staged_mid_turn_batch(&eligible, delivery_seq, ts)
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "internal helper commits only runtime-acked item_ids under the employee lock"
)]
pub(super) fn commit_mid_turn_delivered(
    store: &SessionStore,
    employee_locks: &EmployeeLockMap,
    mid_turn_rates: &Mutex<HashMap<String, MidTurnRateTracker>>,
    waiting_messages: &Mutex<HashMap<String, Arc<AtomicU32>>>,
    bindings: &RunBindingRegistry,
    hourly_window_ms: u64,
    emp_id: &str,
    batch: &StagedMidTurnBatch,
) {
    let claimed: HashSet<&str> = batch.item_ids.iter().map(String::as_str).collect();
    let tokens = bindings.active_tokens();
    let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
    {
        let _guard = employee_locks.lock(emp_id);
        if let Ok(Some(mut queue)) = store.read_pending_wakes(emp_id) {
            queue
                .items
                .retain(|i| !claimed.contains(i.item_id.as_str()));
            if queue.items.is_empty() && queue.seen_item_ids.is_empty() {
                if store.clear_pending_wakes(emp_id).is_ok() {
                    set_waiting_messages_count(waiting_messages, emp_id, 0);
                }
            } else {
                if queue.items.is_empty() {
                    // Retain `seen_item_ids` across mid-turn drain for `TASK-52` P7-#12 idempotency.
                    queue.hold_reason = None;
                    queue.held_since_ms = None;
                    queue.retry_after_ms = None;
                }
                queue.updated_at = rfc3339_now();
                if store.write_pending_wakes_redacted(&queue, &refs).is_ok() {
                    set_waiting_messages_count(waiting_messages, emp_id, queue.items.len());
                }
            }
        }
    }
    {
        let mut rates = mid_turn_rates.lock().unwrap_or_else(|p| p.into_inner());
        rates
            .entry(emp_id.to_owned())
            .or_default()
            .record_delivered(
                now_ms(),
                batch.has_agent_item,
                &batch.agent_sender_convs,
                hourly_window_ms,
            );
    }
    let _ = store.append_history(
        emp_id,
        &serde_json::json!({
            "type": "mid_turn_delivered",
            "delivery_id": batch.delivery_id,
            "item_ids": batch.item_ids,
            "count": batch.item_ids.len(),
            "at": rfc3339_now(),
        }),
        &refs,
    );
}

pub(super) fn build_permit_hooks(
    mgr: &SessionManager,
    employee_id: &str,
    route_digest: String,
    oauth_expires_at_ms: Option<u64>,
    turn_override: crate::router::TurnModelOverride,
) -> (
    PermitAcquireHook,
    PermitRenewHook,
    PermitReleaseHook,
    PermitReleasePendingHook,
) {
    let acq_office = Arc::clone(&mgr.office);
    let acq_hq_permits = mgr.config.hq_permits.clone();
    let acq_emp = employee_id.to_owned();
    let on_permit_acquire: PermitAcquireHook = Arc::new(move |conn_id, task_id| {
        if crate::host_auth::is_oauth_expiring(oauth_expires_at_ms, now_ms()) {
            return Err("WARM_ROUTE_STALE: oauth token expiring".into());
        }
        let digest = (!route_digest.is_empty()).then_some(route_digest.as_str());
        match crate::router::acquire_warm_permit_for_office_with_override(
            &acq_office,
            acq_hq_permits.as_deref(),
            conn_id,
            &acq_emp,
            task_id,
            digest,
            Some(&turn_override),
        ) {
            crate::router::WarmPermitOutcome::Granted(p) => Ok(p),
            crate::router::WarmPermitOutcome::StaleWarm => {
                Err("WARM_ROUTE_STALE: session configuration changed".into())
            }
            crate::router::WarmPermitOutcome::Unavailable { reason, .. } => {
                Err(format!("POOL_EXHAUSTED: [{reason}]"))
            }
            crate::router::WarmPermitOutcome::Refused(err) => Err(err.message),
        }
    });

    let renew_office = Arc::clone(&mgr.office);
    let renew_hq_permits = mgr.config.hq_permits.clone();
    let on_permit_renew: PermitRenewHook = Arc::new(move |permit_id, generation| {
        crate::router::renew_turn_permit_any(
            &renew_office,
            renew_hq_permits.as_deref(),
            permit_id,
            generation,
        )
    });

    let release_office = Arc::clone(&mgr.office);
    let release_hq_permits = mgr.config.hq_permits.clone();
    let release_weak = mgr.weak_self();
    let on_permit_release: PermitReleaseHook = Arc::new(move |permit_id| {
        crate::router::release_turn_permit_any(
            &release_office,
            release_hq_permits.as_deref(),
            permit_id,
        );
        if let Some(m) = release_weak.as_ref().and_then(|w| w.upgrade()) {
            crate::manager::spawn_bg_worker(&m, "org-permit-wake", |m| m.process_due_wakes());
        }
    });

    let pending_office = Arc::clone(&mgr.office);
    let on_permit_release_pending: PermitReleasePendingHook = Arc::new(move |permit_id| {
        let _ = pending_office.mark_turn_permit_release_pending(permit_id);
    });

    (
        on_permit_acquire,
        on_permit_renew,
        on_permit_release,
        on_permit_release_pending,
    )
}
