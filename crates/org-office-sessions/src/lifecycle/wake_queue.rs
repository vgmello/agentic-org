//! Durable pending-wake queue dispatch: hold reasons, the mutual-deadlock breaker, and claimed
//! wake launches (`C8`, `WAKE-1..7`, `LIVE-1..8`).

use std::collections::HashSet;

use org_office::organization_family::records::{Employee, EmployeeStatus};
use org_proto::sessions::SessionState;

use crate::credentials::{now_ms, rfc3339_now};
use crate::manager::SessionManager;
use crate::wake::{
    PendingWakeItem, PendingWakeQueueRecord, SuspendedStateRecord, WakeHoldReason, WakeKind,
    WakeOutcome, WakeQuotaRecord,
};

use super::asks::insert_open_required_ask;
use super::drain::remove_claimed_wakes;
use super::wake_turn::{assigned_task_refs, compose_wake_turn};
use super::DispatchWakeGuard;

/// Why a partition's wakes stay queued: the hold, when to retry, and its first wake.
type PartitionHold = (WakeHoldReason, Option<u64>, PendingWakeItem);

impl SessionManager {
    pub(super) fn queue_hold_reason(
        &self,
        employee: &Employee,
        task_refs: &[&str],
        person_triggered: bool,
        ts: u64,
    ) -> Option<(WakeHoldReason, Option<u64>)> {
        if employee.status == EmployeeStatus::Paused {
            return Some((WakeHoldReason::Paused, None));
        }
        if let Ok(Some(rec)) = self.store.read_active_turn(employee.id.as_str()) {
            if rec.stall_locked {
                return Some((WakeHoldReason::StallLocked, None));
            }
            if rec.recovery_required {
                return Some((WakeHoldReason::RecoveryRequired, None));
            }
        }
        let live_needs_recovery = {
            let reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
            reg.live.get(employee.id.as_str()).is_some_and(|handle| {
                let st = handle.state.lock().unwrap_or_else(|p| p.into_inner());
                st.recovery_required || st.state == SessionState::RecoveryRequired
            })
        };
        if live_needs_recovery {
            return Some((WakeHoldReason::RecoveryRequired, None));
        }
        if self.has_active_managed_turn(employee.id.as_str()) {
            return Some((WakeHoldReason::Busy, None));
        }
        // Single active lease: an external workspace drives this agent, so the wake stays
        // durable (and counted in `waiting_messages`) instead of spawning a competing run.
        if self.has_active_external_lease(employee.id.as_str()) {
            return Some((WakeHoldReason::Busy, None));
        }
        if self.is_budget_held_for(employee.id.as_str(), task_refs) {
            return Some((WakeHoldReason::BudgetHold, None));
        }
        if !person_triggered {
            let mut quota = self
                .store
                .read_wake_quota(employee.id.as_str())
                .ok()
                .flatten()
                .unwrap_or_else(|| WakeQuotaRecord::empty(employee.id.as_str()));
            let window_ms = self.config.wake_window.as_millis() as u64;
            if let Err(retry_after) =
                quota.check_limit(ts, window_ms, self.config.hourly_wake_limit)
            {
                return Some((WakeHoldReason::HourlyLimit, Some(retry_after)));
            }
        }
        None
    }

    /// The wakes of the first partition of `items` that may start now
    /// ([`SessionManager::claim_one_partition`]): each partition's hold is weighed on its own
    /// wakes alone, so one partition never borrows another's person-triggered quota bypass,
    /// and a budget-held assignment never holds another partition's DM. Anyone but the Chief
    /// of Staff has one partition, all of `items`. When every partition is held, the first
    /// one's hold, with its first wake.
    fn first_unheld_partition(
        &self,
        employee: &Employee,
        mut items: Vec<PendingWakeItem>,
        ts: u64,
    ) -> Result<Vec<PendingWakeItem>, Box<PartitionHold>> {
        let mut first_hold = None;
        while !items.is_empty() {
            let (claimed, _) = self.claim_one_partition(employee, items.clone());
            let claimed_ids: HashSet<&str> = claimed.iter().map(|i| i.item_id.as_str()).collect();
            let person_triggered = claimed.iter().any(|i| i.sender_is_person);
            let refs = assigned_task_refs(&claimed);
            match self.queue_hold_reason(employee, &refs, person_triggered, ts) {
                None => return Ok(claimed),
                // Only a budget or quota hold differs between partitions.
                Some((reason, retry_after)) => {
                    let partition_only = matches!(
                        reason,
                        WakeHoldReason::BudgetHold | WakeHoldReason::HourlyLimit
                    );
                    first_hold.get_or_insert((reason, retry_after, claimed[0].clone()));
                    if !partition_only {
                        break;
                    }
                }
            }
            items.retain(|i| !claimed_ids.contains(i.item_id.as_str()));
        }
        // Held, or (never, for a dispatched queue) nothing to claim at all.
        match first_hold {
            Some(hold) => Err(Box::new(hold)),
            None => Ok(items),
        }
    }

    pub(super) fn try_dispatch_deadlock_breaker(
        &self,
        employee: &Employee,
        susp: SuspendedStateRecord,
    ) -> WakeOutcome {
        let emp_id = employee.id.as_str();
        if self
            .store
            .read_pending_wakes(emp_id)
            .ok()
            .flatten()
            .is_none_or(|q| q.items.is_empty())
        {
            return WakeOutcome::Ignored;
        }
        let Some(_dispatch_guard) = DispatchWakeGuard::try_acquire(&self.dispatching_wakes, emp_id)
        else {
            return WakeOutcome::Queued {
                reason: WakeHoldReason::Busy,
            };
        };
        let ts = now_ms();
        let (matching, held_since) = {
            let _lock = self.employee_locks.lock(emp_id);
            let Ok(Some(mut queue)) = self.store.read_pending_wakes(emp_id) else {
                return WakeOutcome::Ignored;
            };
            if queue.items.is_empty() {
                if queue.seen_item_ids.is_empty() {
                    let _ = self.store.clear_pending_wakes(emp_id);
                }
                return WakeOutcome::Ignored;
            }
            let matching: Vec<PendingWakeItem> = queue
                .items
                .iter()
                .filter(|item| susp.allows_deadlock_breaker(item))
                .cloned()
                .collect();
            if matching.is_empty() {
                queue.set_hold(WakeHoldReason::Suspended, ts, None);
                self.write_wakes_redacted(&queue);
                return WakeOutcome::Queued {
                    reason: WakeHoldReason::Suspended,
                };
            }
            let matching = match self.first_unheld_partition(employee, matching, ts) {
                Ok(claimed) => claimed,
                Err(hold) => {
                    let (reason, retry_after, first) = *hold;
                    queue.set_hold(reason, ts, retry_after);
                    self.write_wakes_redacted(&queue);
                    self.record_wake_held(emp_id, &first, reason, retry_after);
                    return WakeOutcome::Queued { reason };
                }
            };
            (matching, queue.held_since_ms)
        };
        self.run_deadlock_breaker_launch(employee, susp, matching, held_since, ts)
    }

    pub(super) fn run_deadlock_breaker_launch(
        &self,
        employee: &Employee,
        susp: SuspendedStateRecord,
        matching: Vec<PendingWakeItem>,
        held_since: Option<u64>,
        ts: u64,
    ) -> WakeOutcome {
        let emp_id = employee.id.as_str();
        let (matching, partition) = self.claim_one_partition(employee, matching);
        let person_triggered = matching.iter().any(|i| i.sender_is_person);
        let Ok(objective) = compose_wake_turn(&self.office, emp_id, &matching, &susp.asks) else {
            return self.hold_refused_batch(emp_id, ts);
        };
        {
            let _lock = self.employee_locks.lock(emp_id);
            for open in &susp.asks {
                insert_open_required_ask(&self.open_required_asks, emp_id, open.clone());
            }
        }
        let first = &matching[0];
        let trigger = serde_json::json!({
            "type": "turn_trigger",
            "kind": first.kind.as_str(),
            "sender_id": first.sender_id,
            "source_id": first.source_id,
            "deadlock_breaker": true,
            "waited_ms": held_since.map(|s| ts.saturating_sub(s)),
            "at": rfc3339_now(),
        });
        let task_refs = assigned_task_refs(&matching);
        self.preprepare_task_worktrees(emp_id, &task_refs);
        let task_ref = task_refs.first().copied();
        let attachment_ids = crate::wake::collect_wake_attachment_ids(&matching);
        match self.spawn_or_send_autonomous_with_attachments(
            employee,
            objective,
            attachment_ids,
            susp.runtime_session_id.as_deref(),
            task_ref,
            trigger,
            partition,
        ) {
            Ok(_) => {
                let notify_remaining = {
                    let _lock = self.employee_locks.lock(emp_id);
                    if !person_triggered {
                        self.record_quota_start_locked(emp_id, ts);
                    }
                    let claimed: HashSet<String> =
                        matching.into_iter().map(|i| i.item_id).collect();
                    self.remove_claimed_wakes_locked(emp_id, &claimed)
                };
                if notify_remaining {
                    self.notify_live_pump_pending_wakes(emp_id);
                }
                WakeOutcome::Started
            }
            Err(_) => self.hold_refused_batch(emp_id, ts),
        }
    }

    /// Keeps a batch the office refused to start (admission, or must-follow processes over
    /// budget) queued under [`WakeHoldReason::AdmissionRefused`] for a later retry.
    fn hold_refused_batch(&self, emp_id: &str, ts: u64) -> WakeOutcome {
        self.mark_queue_hold_locked(emp_id, WakeHoldReason::AdmissionRefused, ts, None);
        WakeOutcome::Queued {
            reason: WakeHoldReason::AdmissionRefused,
        }
    }

    pub(super) fn try_dispatch_pending_queue(&self, employee: &Employee) -> WakeOutcome {
        let emp_id = employee.id.as_str();
        if self
            .store
            .read_pending_wakes(emp_id)
            .ok()
            .flatten()
            .is_none_or(|q| q.items.is_empty())
        {
            return WakeOutcome::Ignored;
        }
        let Some(_dispatch_guard) = DispatchWakeGuard::try_acquire(&self.dispatching_wakes, emp_id)
        else {
            return WakeOutcome::Queued {
                reason: WakeHoldReason::Busy,
            };
        };
        let ts = now_ms();
        let queue = {
            let _lock = self.employee_locks.lock(emp_id);
            let Ok(Some(mut queue)) = self.store.read_pending_wakes(emp_id) else {
                return WakeOutcome::Ignored;
            };
            if queue.items.is_empty() {
                if queue.seen_item_ids.is_empty() {
                    let _ = self.store.clear_pending_wakes(emp_id);
                }
                return WakeOutcome::Ignored;
            }
            match self.first_unheld_partition(employee, queue.items.clone(), ts) {
                Ok(claimed) => {
                    queue.items = claimed;
                    queue
                }
                Err(hold) => {
                    let (reason, retry_after, first) = *hold;
                    let has_mid_turn_eligible =
                        queue.items.iter().any(|i| i.kind.is_mid_turn_eligible());
                    let changed = queue.hold_reason != Some(reason);
                    queue.set_hold(reason, ts, retry_after);
                    self.write_wakes_redacted(&queue);
                    if changed {
                        self.record_wake_held(emp_id, &first, reason, retry_after);
                    }
                    if reason == WakeHoldReason::Busy && has_mid_turn_eligible {
                        self.notify_live_pump_pending_wakes(emp_id);
                    }
                    return WakeOutcome::Queued { reason };
                }
            }
        };
        self.launch_claimed_wakes(employee, queue, ts)
    }

    /// Launches one turn of the queue's wakes. The Chief of Staff's turn claims only the wakes
    /// of the first one's partition ([`SessionManager::claim_one_partition`]): the rest stay
    /// queued for their own partition's turn, so no sender's private message ever shares a
    /// turn, or a conversation, with another caller's (`TASK-80.1`).
    pub(super) fn launch_claimed_wakes(
        &self,
        employee: &Employee,
        mut queue: PendingWakeQueueRecord,
        ts: u64,
    ) -> WakeOutcome {
        let emp_id = employee.id.as_str();
        let (claimed, partition) =
            self.claim_one_partition(employee, std::mem::take(&mut queue.items));
        queue.items = claimed;
        let person_triggered = queue.has_person_item();
        let Ok(objective) = compose_wake_turn(&self.office, emp_id, &queue.items, &[]) else {
            return self.hold_refused_batch(emp_id, ts);
        };
        let waited_ms = queue.held_since_ms.map(|since| ts.saturating_sub(since));
        let first = &queue.items[0];
        let force_fresh = queue
            .items
            .iter()
            .any(|i| i.session_affinity.as_ref().is_some_and(|a| a.fresh));
        let affinity_resume = (!force_fresh)
            .then(|| {
                queue.items.iter().rev().find_map(|i| {
                    i.session_affinity
                        .as_ref()
                        .and_then(|a| a.resume_runtime_session_id.clone())
                })
            })
            .flatten();
        let enforce_affinity_guard = affinity_resume.is_some();
        let allow_sandbox_spec_change = !force_fresh
            && !enforce_affinity_guard
            && queue
                .items
                .iter()
                .any(|i| i.kind == WakeKind::SandboxRefresh);
        let wants_resume = !force_fresh
            && affinity_resume.is_none()
            && queue.items.iter().any(|i| {
                matches!(
                    i.kind,
                    WakeKind::AskAnswer | WakeKind::WatcherAnswer | WakeKind::SandboxRefresh
                )
            });
        let resume_rsid = affinity_resume.or_else(|| {
            wants_resume
                .then(|| {
                    self.store
                        .read_active_turn(emp_id)
                        .ok()
                        .flatten()
                        .and_then(|r| r.runtime_session_id)
                })
                .flatten()
        });
        let trigger = serde_json::json!({
            "type": "turn_trigger",
            "kind": first.kind.as_str(),
            "sender_id": first.sender_id,
            "source_id": first.source_id,
            "coalesced_count": queue.items.len(),
            "held_reason": queue.hold_reason.map(|r| r.as_str()),
            "waited_ms": waited_ms,
            "fresh_session": force_fresh,
            "at": rfc3339_now(),
        });
        let task_refs = assigned_task_refs(&queue.items);
        self.preprepare_task_worktrees(emp_id, &task_refs);
        let task_ref = task_refs.first().copied();
        let attachment_ids = crate::wake::collect_wake_attachment_ids(&queue.items);
        match self.spawn_or_send_autonomous_with_affinity(
            employee,
            objective,
            attachment_ids,
            resume_rsid.as_deref(),
            force_fresh,
            enforce_affinity_guard,
            allow_sandbox_spec_change,
            task_ref,
            trigger,
            partition,
        ) {
            Ok(_) => {
                let notify_remaining = {
                    let _lock = self.employee_locks.lock(emp_id);
                    if !person_triggered {
                        self.record_quota_start_locked(emp_id, ts);
                    }
                    let claimed: HashSet<String> =
                        queue.items.into_iter().map(|i| i.item_id).collect();
                    self.remove_claimed_wakes_locked(emp_id, &claimed)
                };
                if notify_remaining {
                    self.notify_live_pump_pending_wakes(emp_id);
                }
                WakeOutcome::Started
            }
            Err(_) => self.hold_refused_batch(emp_id, ts),
        }
    }

    pub(super) fn record_quota_start_locked(&self, emp_id: &str, ts: u64) {
        let window_ms = self.config.wake_window.as_millis() as u64;
        let mut quota = self
            .store
            .read_wake_quota(emp_id)
            .ok()
            .flatten()
            .unwrap_or_else(|| WakeQuotaRecord::empty(emp_id));
        quota.record_start(ts, window_ms);
        let _ = self.store.write_wake_quota(&quota);
    }

    pub(super) fn remove_claimed_wakes_locked(
        &self,
        emp_id: &str,
        claimed: &HashSet<String>,
    ) -> bool {
        let tokens = self.bindings.active_tokens();
        let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
        remove_claimed_wakes(&self.store, &self.waiting_messages, &refs, emp_id, claimed)
    }

    pub(super) fn mark_queue_hold_locked(
        &self,
        emp_id: &str,
        reason: WakeHoldReason,
        ts: u64,
        retry_after: Option<u64>,
    ) {
        let _lock = self.employee_locks.lock(emp_id);
        if let Ok(Some(mut queue)) = self.store.read_pending_wakes(emp_id) {
            queue.set_hold(reason, ts, retry_after);
            self.write_wakes_redacted(&queue);
        }
    }

    pub(super) fn write_wakes_redacted(&self, queue: &PendingWakeQueueRecord) {
        let tokens = self.bindings.active_tokens();
        let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
        let _ = self.store.write_pending_wakes_redacted(queue, &refs);
    }

    pub(super) fn record_wake_held(
        &self,
        employee_id: &str,
        item: &PendingWakeItem,
        reason: WakeHoldReason,
        retry_after_ms: Option<u64>,
    ) {
        let tokens = self.bindings.active_tokens();
        let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
        let _ = self.store.append_history(
            employee_id,
            &serde_json::json!({
                "type": "wake_held",
                "kind": item.kind.as_str(),
                "sender_id": item.sender_id,
                "source_id": item.source_id,
                "reason": reason.as_str(),
                "retry_after_ms": retry_after_ms,
                "at": rfc3339_now(),
            }),
            &refs,
        );
    }
}
