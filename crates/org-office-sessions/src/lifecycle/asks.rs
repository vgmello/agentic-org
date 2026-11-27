//! Required asks: suspension on turn end or grace expiry, answers, withdrawal, and resume
//! (`C6`, `ASK-3..12`, `ADR-0030` d3).

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use org_office::organization_family::records::{Employee, EmployeeStatus};
use org_proto::sessions::SessionState;

use crate::binding::RunBindingRegistry;
use crate::credentials::{now_ms, rfc3339_now};
use crate::manager::{set_waiting_messages_count, SessionManager};
use crate::pump::PumpCommand;
use crate::recovery::SessionStore;
use crate::wake::{
    compose_required_ask_resume_objective, PendingAskAnswer, PendingWakeItem, SuspendedAskEntry,
    SuspendedStateRecord, WakeHoldReason, WakeKind, WakeOutcome,
};

use super::DispatchWakeGuard;

impl SessionManager {
    /// Records an open required ask created by an active agent so turn-end or grace expiry
    /// suspends the session cleanly (`ASK-3`, `ASK-11`, `ASK-12`). The built-in Chief of Staff
    /// is never suspended on an ask (`TASK-80.1`): one process serves every caller's partition,
    /// so suspending it for one caller's question would hold every other caller, and resuming
    /// it would carry that caller's answer outside their partition. Her answer arrives as an
    /// ordinary `AskAnswer` wake in the answering member's partition instead.
    pub fn on_required_ask_opened(&self, employee_id: &str, entry: SuspendedAskEntry) {
        if self.office.is_chief_of_staff(employee_id) {
            return;
        }
        let _lock = self.employee_locks.lock(employee_id);
        insert_open_required_ask(&self.open_required_asks, employee_id, entry);
    }

    /// Arms the required-ask suspension grace period after decision 5 wind-down notice (`ASK-3`).
    /// Only arms when `employee_id` actually owns `ask_id` as an open required ask.
    pub fn on_required_ask_wait_expired(&self, employee_id: &str, ask_id: &str) {
        let _lock = self.employee_locks.lock(employee_id);
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

    /// Handles an answer to an ask: resumes a suspended asker (`ASK-5..9`, `ASK-11..12`), skips
    /// duplicate delivery only when an active `msg ask`/`msg wait` waiter already received the
    /// answer inline (`ADR-0023` d6), or enqueues an `AskAnswer` wake for mid-turn or next-turn
    /// delivery (`TASK-115` / `doc-101` §4A).
    #[expect(
        clippy::too_many_arguments,
        reason = "public ask-answered hook takes explicit asker, ask, question/answer summaries, responder, subagent flag, and active-waiter flag"
    )]
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
        satisfied_active_waiter: bool,
    ) -> WakeOutcome {
        self.on_ask_answered_durable(
            asker_id,
            ask_id,
            question_summary,
            answer_summary,
            (answered_by_id, answered_by_name, answered_by_person),
            asked_by_subagent,
            satisfied_active_waiter,
        )
        .unwrap_or(WakeOutcome::Ignored)
    }

    /// Durable variant of `on_ask_answered` that propagates suspended-state and pending-wake
    /// persistence failures so cross-office federation only commits `delivered: true` after
    /// resume intent is durably recorded (`TASK-52` P7-#4, `TASK-115`).
    #[expect(
        clippy::too_many_arguments,
        reason = "durable ask-answered hook takes explicit asker, ask, summaries, responder, subagent flag, and active-waiter flag"
    )]
    pub fn on_ask_answered_durable(
        &self,
        asker_id: &str,
        ask_id: &str,
        question_summary: &str,
        answer_summary: &str,
        responder: (&str, &str, bool),
        asked_by_subagent: bool,
        satisfied_active_waiter: bool,
    ) -> Result<WakeOutcome, String> {
        let (answered_by_id, answered_by_name, answered_by_person) = responder;
        let suspended_resume = {
            let _lock = self.employee_locks.lock(asker_id);
            remove_open_required_ask(&self.open_required_asks, asker_id, ask_id);
            record_suspended_ask_answer_locked(
                &self.store,
                &self.bindings,
                asker_id,
                ask_id,
                question_summary,
                answer_summary,
                (answered_by_name, asked_by_subagent),
            )?
        };
        if suspended_resume {
            return Ok(self.resume_suspended_with_retry(asker_id));
        }
        if satisfied_active_waiter {
            return Ok(WakeOutcome::Ignored);
        }
        let item = PendingWakeItem {
            item_id: format!("answer:{ask_id}"),
            kind: WakeKind::AskAnswer,
            sender_id: answered_by_id.to_owned(),
            sender_name: answered_by_name.to_owned(),
            sender_handle: None,
            sender_relation: None,
            sender_is_person: answered_by_person,
            // The ask's conversation, so an answer given in a DM never reaches the Chief of
            // Staff's shared wake partition (`TASK-80.1`); unknown here (an ask of another
            // office), it counts as private.
            conversation_id: self.office.message_conversation_id(ask_id).ok().flatten(),
            source_id: ask_id.to_owned(),
            summary: format!("Question: {question_summary}\nAnswer: {answer_summary}"),
            enqueued_at_ms: now_ms(),
        };
        self.enqueue_wake_durable(asker_id, item)
    }

    pub(super) fn resume_suspended_with_retry(&self, asker_id: &str) -> WakeOutcome {
        for _ in 0..10 {
            let outcome = self.try_resume_suspended(asker_id);
            if outcome
                != (WakeOutcome::Queued {
                    reason: WakeHoldReason::Busy,
                })
            {
                return outcome;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        self.try_resume_suspended(asker_id)
    }

    /// Removes a withdrawn ask from suspended state and queued pending wakes (`ADR-0030` d3).
    pub fn on_ask_withdrawn(&self, asker_id: &str, ask_id: &str) {
        let _ = self.on_ask_withdrawn_durable(asker_id, ask_id);
    }

    /// Durably clears a withdrawn ask from suspended state and queued pending wakes, propagating
    /// storage errors so cross-office withdrawal delivery only acknowledges after cleanup is
    /// persisted (`TASK-52` P13-#4).
    pub fn on_ask_withdrawn_durable(&self, asker_id: &str, ask_id: &str) -> Result<(), String> {
        {
            let _lock = self.employee_locks.lock(asker_id);
            remove_open_required_ask(&self.open_required_asks, asker_id, ask_id);
            let maybe_susp = self
                .store
                .read_suspended_state(asker_id)
                .map_err(|e| format!("failed reading suspended state: {e}"))?;
            if let Some(mut susp) = maybe_susp {
                susp.asks.retain(|a| a.ask_id != ask_id);
                if susp.asks.is_empty() && susp.pending_resume_answers.is_empty() {
                    self.store
                        .clear_suspended_state(asker_id)
                        .map_err(|e| format!("failed clearing suspended state: {e}"))?;
                } else {
                    susp.updated_at = rfc3339_now();
                    let tokens = self.bindings.active_tokens();
                    let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
                    self.store
                        .write_suspended_state_redacted(&susp, &refs)
                        .map_err(|e| format!("failed writing suspended state: {e}"))?;
                }
            }
        }
        // The asker's lock is released first: the sweep below takes each employee's lock in turn
        // and never nests two employee locks (`TASK-170`).
        self.prune_pending_wake_source_durable(ask_id)
    }

    pub(super) fn try_resume_suspended(&self, employee_id: &str) -> WakeOutcome {
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
            let _lock = self.employee_locks.lock(employee_id);
            let Ok(Some(mut susp)) = self.store.read_suspended_state(employee_id) else {
                return WakeOutcome::Ignored;
            };
            if susp.pending_resume_answers.is_empty() {
                return WakeOutcome::Ignored;
            }
            let hold = if employee.status == EmployeeStatus::Paused {
                Some(WakeHoldReason::Paused)
            } else if self.is_budget_held_for(employee_id, &[]) {
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

    pub(super) fn launch_suspended_resume(
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
            None,
        ) {
            Ok(_) => {
                let _lock = self.employee_locks.lock(emp_id);
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
                let _lock = self.employee_locks.lock(emp_id);
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

    pub(super) fn write_suspended_redacted(&self, susp: &SuspendedStateRecord) {
        let tokens = self.bindings.active_tokens();
        let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
        let _ = self.store.write_suspended_state_redacted(susp, &refs);
    }

    pub(super) fn prune_pending_wake_source_durable(&self, source_id: &str) -> Result<(), String> {
        let ids = self
            .store
            .list_employee_ids()
            .map_err(|e| format!("failed listing wake employees: {e}"))?;
        let tokens = self.bindings.active_tokens();
        let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
        for emp_id in ids {
            let _lock = self.employee_locks.lock(&emp_id);
            let Some(mut queue) = self
                .store
                .read_pending_wakes(&emp_id)
                .map_err(|e| format!("failed reading pending wakes: {e}"))?
            else {
                continue;
            };
            let before = queue.items.len();
            queue.items.retain(|i| i.source_id != source_id);
            if queue.items.len() == before {
                continue;
            }
            if queue.items.is_empty() && queue.seen_item_ids.is_empty() {
                self.store
                    .clear_pending_wakes(&emp_id)
                    .map_err(|e| format!("failed clearing pending wakes: {e}"))?;
                set_waiting_messages_count(&self.waiting_messages, &emp_id, 0);
                continue;
            }
            if queue.items.is_empty() {
                queue.hold_reason = None;
                queue.held_since_ms = None;
                queue.retry_after_ms = None;
            }
            queue.updated_at = rfc3339_now();
            self.store
                .write_pending_wakes_redacted(&queue, &refs)
                .map_err(|e| format!("failed writing pending wakes: {e}"))?;
            set_waiting_messages_count(&self.waiting_messages, &emp_id, queue.items.len());
        }
        Ok(())
    }
}

pub(super) fn record_suspended_ask_answer_locked(
    store: &SessionStore,
    bindings: &RunBindingRegistry,
    asker_id: &str,
    ask_id: &str,
    question_summary: &str,
    answer_summary: &str,
    responder: (&str, bool),
) -> Result<bool, String> {
    let (answered_by_name, asked_by_subagent) = responder;
    let Some(mut susp) = store
        .read_suspended_state(asker_id)
        .map_err(|e| format!("failed reading suspended state: {e}"))?
    else {
        return Ok(false);
    };
    let Some(pos) = susp.asks.iter().position(|a| a.ask_id == ask_id) else {
        return Ok(false);
    };
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
    let tokens = bindings.active_tokens();
    let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
    store
        .write_suspended_state_redacted(&susp, &refs)
        .map_err(|e| format!("failed persisting suspended ask answer: {e}"))?;
    Ok(true)
}

pub(super) fn insert_open_required_ask(
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

pub(super) fn take_open_required_asks(
    map: &Mutex<HashMap<String, Vec<SuspendedAskEntry>>>,
    employee_id: &str,
) -> Vec<SuspendedAskEntry> {
    map.lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(employee_id)
        .unwrap_or_default()
}

pub(super) fn remove_open_required_ask(
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

pub(super) fn persist_suspended_record(
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
