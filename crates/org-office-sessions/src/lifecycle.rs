//! Autonomous session lifecycle orchestration for `TASK-47` (`C6` required-ask suspend/resume,
//! `C8` peer-ask/handoff/mention/answer wake-ups with hourly limit & mutual-deadlock breaker,
//! `C10` always-available presence & human/task auto-start).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use org_office::directory::MemberRuntime;
use org_office::organization_family::records::{Employee, EmployeeKind, EmployeeStatus};
use org_office::Office;
use org_proto::directory::{MemberPresence, RecipientPresence};
use org_proto::sessions::SessionState;

use crate::binding::RunBindingRegistry;
use crate::credentials::{now_ms, rfc3339_now};
use crate::manager::{SessionManager, SessionRegistry};
use crate::pump::{OpenRequiredAsksHook, PumpCommand, SuspendedHook, TurnEndedHook};
use crate::recovery::SessionStore;
use crate::wake::{
    compose_coalesced_wake_objective, compose_required_ask_resume_objective, PendingAskAnswer,
    PendingWakeItem, PendingWakeQueueRecord, SuspendedAskEntry, SuspendedStateRecord,
    WakeHoldReason, WakeKind, WakeOutcome, WakeQuotaRecord,
};

struct DispatchWakeGuard {
    set: Arc<Mutex<HashSet<String>>>,
    employee_id: String,
}

impl DispatchWakeGuard {
    fn try_acquire(set: &Arc<Mutex<HashSet<String>>>, employee_id: &str) -> Option<Self> {
        let mut guard = set.lock().unwrap_or_else(|p| p.into_inner());
        if guard.insert(employee_id.to_owned()) {
            Some(Self {
                set: Arc::clone(set),
                employee_id: employee_id.to_owned(),
            })
        } else {
            None
        }
    }
}

impl Drop for DispatchWakeGuard {
    fn drop(&mut self) {
        let mut guard = self.set.lock().unwrap_or_else(|p| p.into_inner());
        guard.remove(&self.employee_id);
    }
}

impl SessionManager {
    /// Records an open required ask created by an active agent so turn-end or grace expiry
    /// suspends the session cleanly (`ASK-3`, `ASK-11`, `ASK-12`).
    pub fn on_required_ask_opened(&self, employee_id: &str, entry: SuspendedAskEntry) {
        let _lock = self
            .lifecycle_lock
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        insert_open_required_ask(&self.open_required_asks, employee_id, entry);
    }

    /// Arms the required-ask suspension grace period after decision 5 wind-down notice (`ASK-3`).
    /// Only arms when `employee_id` actually owns `ask_id` as an open required ask.
    pub fn on_required_ask_wait_expired(&self, employee_id: &str, ask_id: &str) {
        let _lock = self
            .lifecycle_lock
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let owns_ask = self
            .open_required_asks
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(employee_id)
            .is_some_and(|list| list.iter().any(|a| a.ask_id == ask_id));
        if !owns_ask {
            return;
        }
        let deadline = Instant::now() + self.config.suspend_grace;
        let sent = {
            let reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
            reg.live.get(employee_id).is_some_and(|handle| {
                let state = handle.state.lock().unwrap_or_else(|p| p.into_inner());
                let active = matches!(
                    state.state,
                    SessionState::Starting
                        | SessionState::Running
                        | SessionState::Idle
                        | SessionState::WaitingApproval
                );
                drop(state);
                active && handle.tx.send(PumpCommand::ArmSuspend { deadline }).is_ok()
            })
        };
        if !sent {
            let asks = take_open_required_asks(&self.open_required_asks, employee_id);
            if !asks.is_empty() {
                let persisted = self.store.read_active_turn(employee_id).ok().flatten();
                let session_id = persisted
                    .as_ref()
                    .map(|r| r.session_id.clone())
                    .unwrap_or_else(|| format!("sess-{employee_id}"));
                let rsid = persisted.and_then(|r| r.runtime_session_id);
                let tokens = self.bindings.active_tokens();
                let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
                persist_suspended_record(
                    &self.store,
                    employee_id,
                    asks,
                    Vec::new(),
                    session_id,
                    rsid,
                    &refs,
                );
            }
        }
    }

    /// Handles an answer to an ask: resumes a suspended asker (`ASK-5..9`, `ASK-11..12`), ignores
    /// a currently running turn (`ADR-0023` d6), or wakes an idle asker (`WAKE-7`).
    #[allow(clippy::too_many_arguments)]
    pub fn on_ask_answered(
        &self,
        asker_id: &str,
        ask_id: &str,
        question_summary: &str,
        answer_summary: &str,
        answered_by_id: &str,
        answered_by_name: &str,
        answered_by_person: bool,
        asked_by_subagent: bool,
    ) -> WakeOutcome {
        let suspended_resume = {
            let _lock = self
                .lifecycle_lock
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            remove_open_required_ask(&self.open_required_asks, asker_id, ask_id);
            self.store
                .read_suspended_state(asker_id)
                .ok()
                .flatten()
                .and_then(|mut susp| {
                    let pos = susp.asks.iter().position(|a| a.ask_id == ask_id)?;
                    let ask_entry = susp.asks.remove(pos);
                    let answer = PendingAskAnswer {
                        ask_id: ask_id.to_owned(),
                        question_summary: if ask_entry.question_summary.is_empty() {
                            question_summary.to_owned()
                        } else {
                            ask_entry.question_summary
                        },
                        answer_summary: answer_summary.to_owned(),
                        answered_by: answered_by_name.to_owned(),
                        answered_at: rfc3339_now(),
                        asked_by_subagent: ask_entry.asked_by_subagent || asked_by_subagent,
                    };
                    if !susp
                        .pending_resume_answers
                        .iter()
                        .any(|a| a.ask_id == answer.ask_id)
                    {
                        susp.pending_resume_answers.push(answer);
                    }
                    susp.updated_at = rfc3339_now();
                    let tokens = self.bindings.active_tokens();
                    let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
                    let _ = self.store.write_suspended_state_redacted(&susp, &refs);
                    Some(susp)
                })
        };
        if suspended_resume.is_some() {
            return self.try_resume_suspended(asker_id);
        }
        if self.has_live_active_turn(asker_id) {
            return WakeOutcome::Ignored;
        }
        let item = PendingWakeItem {
            item_id: format!("answer:{ask_id}"),
            kind: WakeKind::AskAnswer,
            sender_id: answered_by_id.to_owned(),
            sender_name: answered_by_name.to_owned(),
            sender_is_person: answered_by_person,
            conversation_id: None,
            source_id: ask_id.to_owned(),
            summary: format!("Question: {question_summary}\nAnswer: {answer_summary}"),
            enqueued_at_ms: now_ms(),
        };
        self.enqueue_wake(asker_id, item)
    }

    /// Removes a withdrawn ask from suspended state and queued pending wakes (`ADR-0030` d3).
    pub fn on_ask_withdrawn(&self, asker_id: &str, ask_id: &str) {
        let _lock = self
            .lifecycle_lock
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        remove_open_required_ask(&self.open_required_asks, asker_id, ask_id);
        if let Ok(Some(mut susp)) = self.store.read_suspended_state(asker_id) {
            susp.asks.retain(|a| a.ask_id != ask_id);
            if susp.asks.is_empty() && susp.pending_resume_answers.is_empty() {
                let _ = self.store.clear_suspended_state(asker_id);
            } else {
                susp.updated_at = rfc3339_now();
                let tokens = self.bindings.active_tokens();
                let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
                let _ = self.store.write_suspended_state_redacted(&susp, &refs);
            }
        }
        self.prune_pending_wake_source_locked(ask_id);
    }

    /// Enqueues or immediately dispatches an autonomous wake item for `target_employee_id`
    /// (`WAKE-1..7`, `AVAIL-1..6`, `LIVE-1..8`).
    pub fn enqueue_wake(&self, target_employee_id: &str, item: PendingWakeItem) -> WakeOutcome {
        let Some(employee) = self.load_agent_employee(target_employee_id) else {
            return WakeOutcome::Ignored;
        };
        if employee.status == EmployeeStatus::Inactive {
            self.on_agent_offboarded(target_employee_id);
            return WakeOutcome::Ignored;
        }
        {
            let _lock = self
                .lifecycle_lock
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            let mut queue = self
                .store
                .read_pending_wakes(target_employee_id)
                .ok()
                .flatten()
                .unwrap_or_else(|| PendingWakeQueueRecord::empty(target_employee_id));
            queue.push_dedup(item);
            let tokens = self.bindings.active_tokens();
            let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
            let _ = self.store.write_pending_wakes_redacted(&queue, &refs);
        }
        self.process_due_wakes_for(target_employee_id)
    }

    /// Wakes an idle agent on task assignment from either a person or another agent (`AVAIL-2`).
    pub fn on_task_assigned(
        &self,
        assignee_id: &str,
        actor_id: &str,
        actor_name: &str,
        actor_is_person: bool,
        task_id: &str,
        task_title: &str,
    ) -> WakeOutcome {
        let item = PendingWakeItem {
            item_id: format!("task:{task_id}:{assignee_id}"),
            kind: WakeKind::TaskAssignment,
            sender_id: actor_id.to_owned(),
            sender_name: actor_name.to_owned(),
            sender_is_person: actor_is_person,
            conversation_id: None,
            source_id: task_id.to_owned(),
            summary: format!("Task `{task_id}` assigned to you: {task_title}"),
            enqueued_at_ms: now_ms(),
        };
        self.enqueue_wake(assignee_id, item)
    }

    /// Sets or clears an office-wide budget hold (`ASK-8`, `LIVE-3`). Clearing retries held starts.
    pub fn set_budget_hold(&self, reason: Option<String>) {
        let cleared = reason.is_none();
        *self.budget_hold.lock().unwrap_or_else(|p| p.into_inner()) = reason;
        if cleared {
            self.process_due_wakes();
        }
    }

    /// Returns `true` when an office-wide budget hold is currently active (`ADR-0024` d13).
    pub fn has_budget_hold(&self) -> bool {
        self.budget_hold
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
    }

    /// Called when a paused agent is resumed (`ADR-0030` d3).
    pub fn on_agent_resumed(&self, employee_id: &str) {
        let _ = self.process_due_wakes_for(employee_id);
    }

    /// Called when an agent is offboarded: discards pending starts & suspended state (`ADR-0030` d3).
    pub fn on_agent_offboarded(&self, employee_id: &str) {
        let _ = self.stop_session(employee_id, "offboarded");
        let _lock = self
            .lifecycle_lock
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        self.open_required_asks
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(employee_id);
        let _ = self.store.clear_pending_wakes(employee_id);
        let _ = self.store.clear_suspended_state(employee_id);
    }

    /// Scans all session folders and retries any held resumes or due pending wakes (`ASK-6`, `LIVE-3`, `LIVE-4`).
    pub fn process_due_wakes(&self) {
        let Ok(ids) = self.store.list_employee_ids() else {
            return;
        };
        for emp_id in ids {
            let _ = self.process_due_wakes_for(&emp_id);
        }
    }

    /// Retries any held required-ask resume, mutual-deadlock breaker, or pending wake queue for `employee_id`.
    pub fn process_due_wakes_for(&self, employee_id: &str) -> WakeOutcome {
        let Some(employee) = self.load_agent_employee(employee_id) else {
            return WakeOutcome::Ignored;
        };
        if employee.status == EmployeeStatus::Inactive {
            self.on_agent_offboarded(employee_id);
            return WakeOutcome::Ignored;
        }
        if let Ok(Some(susp)) = self.store.read_suspended_state(employee_id) {
            if !susp.pending_resume_answers.is_empty() {
                return self.try_resume_suspended(employee_id);
            }
            return self.try_dispatch_deadlock_breaker(&employee, susp);
        }
        self.try_dispatch_pending_queue(&employee)
    }

    /// Returns live runtime presence for `employee_id` (`ASK-4`, `AVAIL-4`, `AVAIL-5`).
    pub fn member_runtime_presence(&self, employee_id: &str) -> Option<MemberRuntime> {
        if let Some(live) = self.live_pump_presence(employee_id) {
            return Some(live);
        }
        if let Ok(Some(susp)) = self.store.read_suspended_state(employee_id) {
            return Some(MemberRuntime {
                presence: MemberPresence::WaitingForAnswer,
                last_seen_at: Some(susp.suspended_since.clone()),
                status: Some(susp.waiting_status_line()),
                status_expires_at: None,
                current_task_id: None,
            });
        }
        if let Ok(Some(rec)) = self.store.read_active_turn(employee_id) {
            if rec.stall_locked || rec.recovery_required {
                let reason = if rec.stall_locked {
                    "Unable to run: stall-locked"
                } else {
                    "Unable to run: recovery required"
                };
                return Some(MemberRuntime {
                    presence: MemberPresence::UnableToRun,
                    last_seen_at: Some(rec.updated_at),
                    status: Some(reason.to_owned()),
                    status_expires_at: None,
                    current_task_id: None,
                });
            }
        }
        None
    }

    /// Computes `AVAIL-5` recipient presence summaries for a set of member IDs (agents + people).
    pub fn recipient_presence_for(&self, member_ids: &[String]) -> Vec<RecipientPresence> {
        let mut out = Vec::with_capacity(member_ids.len());
        for id in member_ids {
            let Ok(Some(emp)) = self.office.load_employee(id) else {
                continue;
            };
            let (presence, detail, away_since) = if emp.kind == EmployeeKind::Agent {
                if emp.status == EmployeeStatus::Paused {
                    (MemberPresence::Paused, Some("Paused".to_owned()), None)
                } else if let Some(rt) = self.member_runtime_presence(id) {
                    (rt.presence, rt.status, rt.last_seen_at)
                } else {
                    (MemberPresence::Available, None, None)
                }
            } else if let Some(rt) = self.office.member_runtime(id) {
                let away = (rt.presence == MemberPresence::Offline)
                    .then(|| rt.last_seen_at.clone())
                    .flatten();
                (rt.presence, rt.status, away)
            } else {
                (MemberPresence::Offline, Some("Away".to_owned()), None)
            };
            out.push(RecipientPresence {
                member_id: id.clone(),
                presence,
                detail,
                away_since,
            });
        }
        out
    }

    pub(crate) fn build_pump_hooks(&self) -> (OpenRequiredAsksHook, TurnEndedHook, SuspendedHook) {
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
        let lock_s = Arc::clone(&self.lifecycle_lock);
        let bindings_s = self.bindings.clone();
        let on_suspended: SuspendedHook = Arc::new(
            move |emp_id: &str,
                  reaped_bg: Vec<String>,
                  session_id: Option<String>,
                  rsid: Option<String>| {
                let _guard = lock_s.lock().unwrap_or_else(|p| p.into_inner());
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

        let drain_ctx = TurnEndDrainCtx {
            store: self.store.clone(),
            registry: Arc::clone(&self.registry),
            budget_hold: Arc::clone(&self.budget_hold),
            lifecycle_lock: Arc::clone(&self.lifecycle_lock),
            dispatching_wakes: Arc::clone(&self.dispatching_wakes),
            bindings: self.bindings.clone(),
            office: Arc::clone(&self.office),
            hourly_limit: self.config.hourly_wake_limit,
            window_ms: self.config.wake_window.as_millis() as u64,
        };
        let on_turn_ended: TurnEndedHook = Arc::new(move |emp_id: &str| {
            drain_pending_wakes_into_live_pump(emp_id, &drain_ctx);
        });

        (has_open, on_turn_ended, on_suspended)
    }

    fn load_agent_employee(&self, employee_id: &str) -> Option<Employee> {
        self.office
            .load_employee(employee_id)
            .ok()
            .flatten()
            .filter(|e| e.kind == EmployeeKind::Agent)
    }

    fn has_live_active_turn(&self, employee_id: &str) -> bool {
        let reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        if reg.starting.contains(employee_id) {
            return true;
        }
        if let Some(handle) = reg.live.get(employee_id) {
            let state = handle.state.lock().unwrap_or_else(|p| p.into_inner());
            return matches!(
                state.state,
                SessionState::Starting | SessionState::Running | SessionState::WaitingApproval
            );
        }
        // Only treat an unmanaged socket binding (no SessionManager pump) as active (`High #1`).
        self.bindings.has_active(employee_id)
    }

    fn live_pump_presence(&self, employee_id: &str) -> Option<MemberRuntime> {
        let reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        let handle = reg.live.get(employee_id)?;
        let state = handle.state.lock().unwrap_or_else(|p| p.into_inner());
        let presence = match state.state {
            SessionState::Starting | SessionState::Running => MemberPresence::Busy,
            SessionState::WaitingApproval => MemberPresence::NeedsYou,
            SessionState::StallLocked | SessionState::RecoveryRequired => {
                MemberPresence::UnableToRun
            }
            _ => return None,
        };
        Some(MemberRuntime {
            presence,
            last_seen_at: state.suspended_since.clone(),
            status: None,
            status_expires_at: None,
            current_task_id: None,
        })
    }

    fn queue_hold_reason(
        &self,
        employee: &Employee,
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
        if self.has_live_active_turn(employee.id.as_str()) {
            return Some((WakeHoldReason::Busy, None));
        }
        if self.is_budget_held() {
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

    fn is_budget_held(&self) -> bool {
        self.budget_hold
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
    }

    fn try_dispatch_deadlock_breaker(
        &self,
        employee: &Employee,
        susp: SuspendedStateRecord,
    ) -> WakeOutcome {
        let emp_id = employee.id.as_str();
        let Some(_dispatch_guard) = DispatchWakeGuard::try_acquire(&self.dispatching_wakes, emp_id)
        else {
            return WakeOutcome::Queued {
                reason: WakeHoldReason::Busy,
            };
        };
        let ts = now_ms();
        let (matching, held_since) = {
            let _lock = self
                .lifecycle_lock
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            let Ok(Some(mut queue)) = self.store.read_pending_wakes(emp_id) else {
                return WakeOutcome::Ignored;
            };
            if queue.items.is_empty() {
                let _ = self.store.clear_pending_wakes(emp_id);
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
            if let Some((reason, retry_after)) =
                self.queue_hold_reason(employee, person_triggered, ts)
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

    fn run_deadlock_breaker_launch(
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
            let _lock = self
                .lifecycle_lock
                .lock()
                .unwrap_or_else(|p| p.into_inner());
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
                let _lock = self
                    .lifecycle_lock
                    .lock()
                    .unwrap_or_else(|p| p.into_inner());
                if !person_triggered {
                    self.record_quota_start_locked(emp_id, ts);
                }
                let claimed: HashSet<String> = matching.into_iter().map(|i| i.item_id).collect();
                self.remove_claimed_wakes_locked(emp_id, &claimed);
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

    fn try_dispatch_pending_queue(&self, employee: &Employee) -> WakeOutcome {
        let emp_id = employee.id.as_str();
        let Some(_dispatch_guard) = DispatchWakeGuard::try_acquire(&self.dispatching_wakes, emp_id)
        else {
            return WakeOutcome::Queued {
                reason: WakeHoldReason::Busy,
            };
        };
        let ts = now_ms();
        let queue = {
            let _lock = self
                .lifecycle_lock
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            let Ok(Some(mut queue)) = self.store.read_pending_wakes(emp_id) else {
                return WakeOutcome::Ignored;
            };
            if queue.items.is_empty() {
                let _ = self.store.clear_pending_wakes(emp_id);
                return WakeOutcome::Ignored;
            }
            if let Some((reason, retry_after)) =
                self.queue_hold_reason(employee, queue.has_person_item(), ts)
            {
                let first = queue.items[0].clone();
                let changed = queue.hold_reason != Some(reason);
                queue.set_hold(reason, ts, retry_after);
                self.write_wakes_redacted(&queue);
                if changed {
                    self.record_wake_held(emp_id, &first, reason, retry_after);
                }
                return WakeOutcome::Queued { reason };
            }
            queue
        };
        self.launch_claimed_wakes(employee, queue, ts)
    }

    fn launch_claimed_wakes(
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
                let _lock = self
                    .lifecycle_lock
                    .lock()
                    .unwrap_or_else(|p| p.into_inner());
                if !person_triggered {
                    self.record_quota_start_locked(emp_id, ts);
                }
                let claimed: HashSet<String> = queue.items.into_iter().map(|i| i.item_id).collect();
                self.remove_claimed_wakes_locked(emp_id, &claimed);
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

    fn try_resume_suspended(&self, employee_id: &str) -> WakeOutcome {
        let Some(employee) = self.load_agent_employee(employee_id) else {
            return WakeOutcome::Ignored;
        };
        let Some(_dispatch_guard) =
            DispatchWakeGuard::try_acquire(&self.dispatching_wakes, employee_id)
        else {
            return WakeOutcome::Queued {
                reason: WakeHoldReason::Busy,
            };
        };
        let ts = now_ms();
        let susp = {
            let _lock = self
                .lifecycle_lock
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            let Ok(Some(mut susp)) = self.store.read_suspended_state(employee_id) else {
                return WakeOutcome::Ignored;
            };
            if susp.pending_resume_answers.is_empty() {
                return WakeOutcome::Ignored;
            }
            let hold = if employee.status == EmployeeStatus::Paused {
                Some(WakeHoldReason::Paused)
            } else if self.is_budget_held() {
                Some(WakeHoldReason::BudgetHold)
            } else {
                None
            };
            if let Some(reason) = hold {
                if susp.hold_reason != Some(reason) {
                    susp.held_since_ms = Some(ts);
                }
                susp.hold_reason = Some(reason);
                susp.updated_at = rfc3339_now();
                self.write_suspended_redacted(&susp);
                return WakeOutcome::Queued { reason };
            }
            for open in &susp.asks {
                insert_open_required_ask(&self.open_required_asks, employee_id, open.clone());
            }
            susp
        };
        self.launch_suspended_resume(&employee, susp, ts)
    }

    fn launch_suspended_resume(
        &self,
        employee: &Employee,
        susp: SuspendedStateRecord,
        ts: u64,
    ) -> WakeOutcome {
        let emp_id = employee.id.as_str();
        let objective = compose_required_ask_resume_objective(
            &susp.pending_resume_answers,
            &susp.reaped_background_tasks,
            &susp.asks,
        );
        let waited_ms = susp.held_since_ms.map(|since| ts.saturating_sub(since));
        let claimed_ask_ids: HashSet<String> = susp
            .pending_resume_answers
            .iter()
            .map(|a| a.ask_id.clone())
            .collect();
        let trigger = serde_json::json!({
            "type": "turn_trigger",
            "kind": "resume",
            "ask_ids": susp.pending_resume_answers.iter().map(|a| &a.ask_id).collect::<Vec<_>>(),
            "held_reason": susp.hold_reason.map(|r| r.as_str()),
            "waited_ms": waited_ms,
            "at": rfc3339_now(),
        });
        match self.spawn_or_send_autonomous(
            employee,
            objective,
            susp.runtime_session_id.as_deref(),
            None,
            trigger,
        ) {
            Ok(_) => {
                let _lock = self
                    .lifecycle_lock
                    .lock()
                    .unwrap_or_else(|p| p.into_inner());
                if let Ok(Some(mut latest)) = self.store.read_suspended_state(emp_id) {
                    latest
                        .pending_resume_answers
                        .retain(|a| !claimed_ask_ids.contains(&a.ask_id));
                    if latest.pending_resume_answers.is_empty() {
                        let _ = self.store.clear_suspended_state(emp_id);
                    } else {
                        latest.updated_at = rfc3339_now();
                        self.write_suspended_redacted(&latest);
                    }
                }
                WakeOutcome::Started
            }
            Err(_) => {
                let _lock = self
                    .lifecycle_lock
                    .lock()
                    .unwrap_or_else(|p| p.into_inner());
                if let Ok(Some(mut latest)) = self.store.read_suspended_state(emp_id) {
                    latest.hold_reason = Some(WakeHoldReason::AdmissionRefused);
                    latest.held_since_ms.get_or_insert(ts);
                    latest.updated_at = rfc3339_now();
                    self.write_suspended_redacted(&latest);
                }
                WakeOutcome::Queued {
                    reason: WakeHoldReason::AdmissionRefused,
                }
            }
        }
    }

    fn record_quota_start_locked(&self, emp_id: &str, ts: u64) {
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

    fn remove_claimed_wakes_locked(&self, emp_id: &str, claimed: &HashSet<String>) {
        let Ok(Some(mut queue)) = self.store.read_pending_wakes(emp_id) else {
            return;
        };
        queue.items.retain(|i| !claimed.contains(&i.item_id));
        if queue.items.is_empty() {
            let _ = self.store.clear_pending_wakes(emp_id);
        } else {
            queue.updated_at = rfc3339_now();
            self.write_wakes_redacted(&queue);
        }
    }

    fn mark_queue_hold_locked(
        &self,
        emp_id: &str,
        reason: WakeHoldReason,
        ts: u64,
        retry_after: Option<u64>,
    ) {
        let _lock = self
            .lifecycle_lock
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if let Ok(Some(mut queue)) = self.store.read_pending_wakes(emp_id) {
            queue.set_hold(reason, ts, retry_after);
            self.write_wakes_redacted(&queue);
        }
    }

    fn write_wakes_redacted(&self, queue: &PendingWakeQueueRecord) {
        let tokens = self.bindings.active_tokens();
        let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
        let _ = self.store.write_pending_wakes_redacted(queue, &refs);
    }

    fn write_suspended_redacted(&self, susp: &SuspendedStateRecord) {
        let tokens = self.bindings.active_tokens();
        let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
        let _ = self.store.write_suspended_state_redacted(susp, &refs);
    }

    fn record_wake_held(
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

    fn prune_pending_wake_source_locked(&self, source_id: &str) {
        let Ok(ids) = self.store.list_employee_ids() else {
            return;
        };
        for emp_id in ids {
            if let Ok(Some(mut queue)) = self.store.read_pending_wakes(&emp_id) {
                let before = queue.items.len();
                queue.items.retain(|i| i.source_id != source_id);
                if queue.items.len() != before {
                    if queue.items.is_empty() {
                        let _ = self.store.clear_pending_wakes(&emp_id);
                    } else {
                        queue.updated_at = rfc3339_now();
                        self.write_wakes_redacted(&queue);
                    }
                }
            }
        }
    }
}

struct TurnEndDrainCtx {
    store: SessionStore,
    registry: Arc<Mutex<SessionRegistry>>,
    budget_hold: Arc<Mutex<Option<String>>>,
    lifecycle_lock: Arc<Mutex<()>>,
    dispatching_wakes: Arc<Mutex<HashSet<String>>>,
    bindings: RunBindingRegistry,
    office: Arc<Office>,
    hourly_limit: usize,
    window_ms: u64,
}

fn insert_open_required_ask(
    map: &Mutex<HashMap<String, Vec<SuspendedAskEntry>>>,
    employee_id: &str,
    entry: SuspendedAskEntry,
) {
    let mut guard = map.lock().unwrap_or_else(|p| p.into_inner());
    let list = guard.entry(employee_id.to_owned()).or_default();
    if !list.iter().any(|existing| existing.ask_id == entry.ask_id) {
        list.push(entry);
    }
}

fn take_open_required_asks(
    map: &Mutex<HashMap<String, Vec<SuspendedAskEntry>>>,
    employee_id: &str,
) -> Vec<SuspendedAskEntry> {
    map.lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(employee_id)
        .unwrap_or_default()
}

fn remove_open_required_ask(
    map: &Mutex<HashMap<String, Vec<SuspendedAskEntry>>>,
    employee_id: &str,
    ask_id: &str,
) {
    let mut guard = map.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(list) = guard.get_mut(employee_id) {
        list.retain(|a| a.ask_id != ask_id);
        if list.is_empty() {
            guard.remove(employee_id);
        }
    }
}

fn persist_suspended_record(
    store: &SessionStore,
    employee_id: &str,
    asks: Vec<SuspendedAskEntry>,
    reaped_bg: Vec<String>,
    session_id: String,
    runtime_session_id: Option<String>,
    active_secrets: &[&str],
) {
    if asks.is_empty() {
        return;
    }
    let now = rfc3339_now();
    let mut record = store
        .read_suspended_state(employee_id)
        .ok()
        .flatten()
        .unwrap_or_else(|| SuspendedStateRecord {
            version: 1,
            employee_id: employee_id.to_owned(),
            session_id: session_id.clone(),
            runtime_session_id: runtime_session_id.clone(),
            suspended_since: now.clone(),
            asks: Vec::new(),
            reaped_background_tasks: Vec::new(),
            pending_resume_answers: Vec::new(),
            hold_reason: None,
            held_since_ms: None,
            updated_at: now.clone(),
        });
    if runtime_session_id.is_some() {
        record.runtime_session_id = runtime_session_id;
    }
    for ask in asks {
        if !record
            .asks
            .iter()
            .any(|existing| existing.ask_id == ask.ask_id)
        {
            record.asks.push(ask);
        }
    }
    for bg in reaped_bg {
        if !record.reaped_background_tasks.contains(&bg) {
            record.reaped_background_tasks.push(bg);
        }
    }
    record.updated_at = now;
    let _ = store.write_suspended_state_redacted(&record, active_secrets);
}

fn drain_pending_wakes_into_live_pump(employee_id: &str, ctx: &TurnEndDrainCtx) {
    let Some(_dispatch_guard) = DispatchWakeGuard::try_acquire(&ctx.dispatching_wakes, employee_id)
    else {
        return;
    };
    let _lock = ctx.lifecycle_lock.lock().unwrap_or_else(|p| p.into_inner());
    let Ok(Some(mut queue)) = ctx.store.read_pending_wakes(employee_id) else {
        return;
    };
    if queue.items.is_empty() {
        let _ = ctx.store.clear_pending_wakes(employee_id);
        return;
    }
    if ctx
        .store
        .read_suspended_state(employee_id)
        .ok()
        .flatten()
        .is_some()
    {
        return;
    }
    let Some(emp) = ctx.office.load_employee(employee_id).ok().flatten() else {
        return;
    };
    if emp.status != EmployeeStatus::Active {
        return;
    }
    let ts = now_ms();
    let tokens = ctx.bindings.active_tokens();
    let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
    if ctx
        .budget_hold
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .is_some()
    {
        queue.set_hold(WakeHoldReason::BudgetHold, ts, None);
        let _ = ctx.store.write_pending_wakes_redacted(&queue, &refs);
        return;
    }
    let person_triggered = queue.has_person_item();
    let mut quota = ctx
        .store
        .read_wake_quota(employee_id)
        .ok()
        .flatten()
        .unwrap_or_else(|| WakeQuotaRecord::empty(employee_id));
    if !person_triggered {
        if let Err(retry_after) = quota.check_limit(ts, ctx.window_ms, ctx.hourly_limit) {
            queue.set_hold(WakeHoldReason::HourlyLimit, ts, Some(retry_after));
            let _ = ctx.store.write_pending_wakes_redacted(&queue, &refs);
            return;
        }
    }
    let objective = compose_coalesced_wake_objective(&queue.items, &[]);
    let sent = {
        let reg = ctx.registry.lock().unwrap_or_else(|p| p.into_inner());
        reg.live
            .get(employee_id)
            .is_some_and(|handle| handle.tx.send(PumpCommand::SendTurn(objective)).is_ok())
    };
    if sent {
        if !person_triggered {
            quota.record_start(ts, ctx.window_ms);
            let _ = ctx.store.write_wake_quota(&quota);
        }
        let waited_ms = queue.held_since_ms.map(|since| ts.saturating_sub(since));
        let first = &queue.items[0];
        let _ = ctx.store.append_history(
            employee_id,
            &serde_json::json!({
                "type": "turn_trigger",
                "kind": first.kind.as_str(),
                "sender_id": first.sender_id,
                "source_id": first.source_id,
                "coalesced_count": queue.items.len(),
                "held_reason": queue.hold_reason.map(|r| r.as_str()),
                "waited_ms": waited_ms,
                "at": rfc3339_now(),
            }),
            &refs,
        );
        let _ = ctx.store.clear_pending_wakes(employee_id);
    }
}

/// Spawns a lightweight background wake-retry thread that sleeps in short 50ms slices so
/// office shutdown is never blocked (`Low #15`, `WAKE-4`, `LIVE-3`).
pub fn spawn_wake_retry_loop(
    manager: Arc<SessionManager>,
    stopping: Arc<std::sync::atomic::AtomicBool>,
    interval: Duration,
) -> Option<std::thread::JoinHandle<()>> {
    let slice = Duration::from_millis(50);
    std::thread::Builder::new()
        .name("org-wake-retry".into())
        .spawn(move || {
            while !stopping.load(std::sync::atomic::Ordering::SeqCst) {
                manager.process_due_wakes();
                let mut slept = Duration::ZERO;
                while slept < interval && !stopping.load(std::sync::atomic::Ordering::SeqCst) {
                    let step = slice.min(interval.saturating_sub(slept));
                    std::thread::sleep(step);
                    slept += step;
                }
            }
        })
        .ok()
}
