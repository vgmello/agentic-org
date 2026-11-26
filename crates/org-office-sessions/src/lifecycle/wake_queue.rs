//! Durable pending-wake queue dispatch: hold reasons, the mutual-deadlock breaker, and claimed
//! wake launches (`C8`, `WAKE-1..7`, `LIVE-1..8`).

use super::*;

impl SessionManager {
    pub(super) fn queue_hold_reason(
        &self,
        employee: &Employee,
        task_id: Option<&str>,
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
        if self.has_live_active_turn(employee.id.as_str()) {
            return Some((WakeHoldReason::Busy, None));
        }
        if self.is_budget_held_for(employee.id.as_str(), task_id) {
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

    pub(super) fn is_budget_held_for(&self, employee_id: &str, task_id: Option<&str>) -> bool {
        if self
            .budget_hold
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
        {
            return true;
        }
        self.office
            .active_budget_holds(Some(employee_id), task_id)
            .map(|holds| !holds.is_empty())
            .unwrap_or(false)
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
            let person_triggered = matching.iter().any(|i| i.sender_is_person);
            let task_ref = matching
                .iter()
                .find(|i| i.kind == WakeKind::TaskAssignment)
                .map(|i| i.source_id.as_str())
                .filter(|s| !s.is_empty());
            if let Some((reason, retry_after)) =
                self.queue_hold_reason(employee, task_ref, person_triggered, ts)
            {
                queue.set_hold(reason, ts, retry_after);
                self.write_wakes_redacted(&queue);
                self.record_wake_held(emp_id, &matching[0], reason, retry_after);
                return WakeOutcome::Queued { reason };
            }
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
        let person_triggered = matching.iter().any(|i| i.sender_is_person);
        {
            let _lock = self.employee_locks.lock(emp_id);
            for open in &susp.asks {
                insert_open_required_ask(&self.open_required_asks, emp_id, open.clone());
            }
        }
        let objective = compose_coalesced_wake_objective(&matching, &susp.asks);
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
        let task_ref = matching
            .iter()
            .find(|i| i.kind == WakeKind::TaskAssignment)
            .map(|i| i.source_id.as_str())
            .filter(|s| !s.is_empty());
        match self.spawn_or_send_autonomous(
            employee,
            objective,
            susp.runtime_session_id.as_deref(),
            task_ref,
            trigger,
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
            Err(_) => {
                self.mark_queue_hold_locked(emp_id, WakeHoldReason::AdmissionRefused, ts, None);
                WakeOutcome::Queued {
                    reason: WakeHoldReason::AdmissionRefused,
                }
            }
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
            let task_ref = queue
                .items
                .iter()
                .find(|i| i.kind == WakeKind::TaskAssignment)
                .map(|i| i.source_id.as_str())
                .filter(|s| !s.is_empty());
            if let Some((reason, retry_after)) =
                self.queue_hold_reason(employee, task_ref, queue.has_person_item(), ts)
            {
                let first = queue.items[0].clone();
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
            queue
        };
        self.launch_claimed_wakes(employee, queue, ts)
    }

    pub(super) fn launch_claimed_wakes(
        &self,
        employee: &Employee,
        queue: PendingWakeQueueRecord,
        ts: u64,
    ) -> WakeOutcome {
        let emp_id = employee.id.as_str();
        let person_triggered = queue.has_person_item();
        let objective = compose_coalesced_wake_objective(&queue.items, &[]);
        let waited_ms = queue.held_since_ms.map(|since| ts.saturating_sub(since));
        let first = &queue.items[0];
        let wants_resume = queue
            .items
            .iter()
            .any(|i| matches!(i.kind, WakeKind::AskAnswer | WakeKind::WatcherAnswer));
        let resume_rsid = wants_resume
            .then(|| {
                self.store
                    .read_active_turn(emp_id)
                    .ok()
                    .flatten()
                    .and_then(|r| r.runtime_session_id)
            })
            .flatten();
        let trigger = serde_json::json!({
            "type": "turn_trigger",
            "kind": first.kind.as_str(),
            "sender_id": first.sender_id,
            "source_id": first.source_id,
            "coalesced_count": queue.items.len(),
            "held_reason": queue.hold_reason.map(|r| r.as_str()),
            "waited_ms": waited_ms,
            "at": rfc3339_now(),
        });
        let task_ref = queue
            .items
            .iter()
            .find(|i| i.kind == WakeKind::TaskAssignment)
            .map(|i| i.source_id.as_str())
            .filter(|s| !s.is_empty());
        match self.spawn_or_send_autonomous(
            employee,
            objective,
            resume_rsid.as_deref(),
            task_ref,
            trigger,
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
            Err(_) => {
                self.mark_queue_hold_locked(emp_id, WakeHoldReason::AdmissionRefused, ts, None);
                WakeOutcome::Queued {
                    reason: WakeHoldReason::AdmissionRefused,
                }
            }
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
