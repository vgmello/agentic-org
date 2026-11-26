//! Autonomous session lifecycle orchestration for `TASK-47` (`C6` required-ask suspend/resume,
//! `C8` peer-ask/handoff/mention/answer wake-ups with hourly limit & mutual-deadlock breaker,
//! `C10` always-available presence & human/task auto-start).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use org_office::directory::MemberRuntime;
use org_office::organization_family::records::{Employee, EmployeeKind, EmployeeStatus};
use org_office::Office;
use org_proto::directory::{MemberPresence, RecipientPresence};
use org_proto::sessions::SessionState;

use crate::credentials::now_ms;
use crate::manager::{set_waiting_messages_count, SessionManager};
use crate::pump::PumpCommand;
use crate::wake::{PendingWakeItem, PendingWakeQueueRecord, WakeKind, WakeOutcome};

mod asks;
mod drain;
mod hooks;
mod usage;
mod wake_queue;

#[cfg(test)]
pub(crate) use drain::{
    drain_into_live_pump_with_acquirer, recycle_stale_warm_handle_in, WarmPermitAcquirer,
};
use usage::flush_pending_usage;

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
    /// `true` while an office-wide or employee/task budget hold blocks new autonomous turns.
    fn is_budget_held_for(&self, employee_id: &str, task_id: Option<&str>) -> bool {
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

    /// Enqueues or immediately dispatches an autonomous wake item for `target_employee_id`
    /// (`WAKE-1..7`, `AVAIL-1..6`, `LIVE-1..8`).
    pub fn enqueue_wake(&self, target_employee_id: &str, item: PendingWakeItem) -> WakeOutcome {
        self.enqueue_wake_durable(target_employee_id, item)
            .unwrap_or(WakeOutcome::Ignored)
    }

    /// Persists a wake item durably before dispatching, returning `Err` if durable persistence
    /// fails so cross-office wake relays never acknowledge an unpersisted wake (`TASK-52` #14).
    pub fn enqueue_wake_durable(
        &self,
        target_employee_id: &str,
        mut item: PendingWakeItem,
    ) -> Result<WakeOutcome, String> {
        let Some(employee) = self.load_agent_employee(target_employee_id) else {
            return Ok(WakeOutcome::Ignored);
        };
        if employee.status == EmployeeStatus::Inactive {
            self.on_agent_offboarded(target_employee_id);
            return Ok(WakeOutcome::Ignored);
        }
        self.enrich_wake_provenance(&employee, &mut item);
        {
            let _lock = self.employee_locks.lock(target_employee_id);
            let mut queue = self
                .store
                .read_pending_wakes(target_employee_id)
                .map_err(|e| format!("read pending wakes failed: {e}"))?
                .unwrap_or_else(|| PendingWakeQueueRecord::empty(target_employee_id));
            queue.push_dedup(item);
            let tokens = self.bindings.active_tokens();
            let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
            self.store
                .write_pending_wakes_redacted(&queue, &refs)
                .map_err(|e| format!("write pending wakes failed: {e}"))?;
            set_waiting_messages_count(
                &self.waiting_messages,
                target_employee_id,
                queue.items.len(),
            );
        }
        Ok(self.process_due_wakes_for(target_employee_id))
    }

    /// Populates office-verified `sender_handle`, canonical `sender_name`/`sender_is_person`, and
    /// reporting-chain `sender_relation` badge (`doc-78`, `doc-101` §4C) unconditionally from
    /// office state so untrusted callers cannot forge provenance.
    fn enrich_wake_provenance(&self, target: &Employee, item: &mut PendingWakeItem) {
        let sender_emp = self.office.load_employee(&item.sender_id).ok().flatten();
        if let Some(emp) = sender_emp.as_ref() {
            item.sender_name = emp.name.clone();
            item.sender_is_person = emp.kind == EmployeeKind::Person;
            item.sender_handle = (!emp.handle.is_empty()).then(|| emp.handle.clone());
        } else {
            item.sender_handle = None;
        }
        let relation = if item.kind == WakeKind::Recovery
            && item.sender_relation.as_deref() == Some("office_supervisor")
        {
            "office_supervisor"
        } else {
            resolve_verified_sender_relation(&self.office, target, item, sender_emp.as_ref())
        };
        item.sender_relation = Some(relation.to_owned());
    }

    fn notify_live_pump_pending_wakes(&self, employee_id: &str) {
        let reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(handle) = reg.live.get(employee_id).filter(|h| h.supports_mid_turn) {
            let _ = handle.tx.send(PumpCommand::NotifyPendingWakes);
        }
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
            sender_handle: None,
            sender_relation: None,
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

    /// Retries any agents held on a budget hold after a raise, clear, extension, or reset (`L3`, `ASK-8`).
    pub fn retry_budget_held(&self) {
        self.process_due_wakes();
    }

    /// Called when a paused agent is resumed (`ADR-0030` d3).
    pub fn on_agent_resumed(&self, employee_id: &str) {
        let _ = self.process_due_wakes_for(employee_id);
    }

    /// Called when an agent is offboarded: discards pending starts & suspended state (`ADR-0030` d3).
    pub fn on_agent_offboarded(&self, employee_id: &str) {
        let _ = self.stop_session(employee_id, "offboarded");
        let _lock = self.employee_locks.lock(employee_id);
        self.open_required_asks
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(employee_id);
        self.mid_turn_rates
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(employee_id);
        let _ = self.store.clear_pending_wakes(employee_id);
        let _ = self.store.clear_suspended_state(employee_id);
        set_waiting_messages_count(&self.waiting_messages, employee_id, 0);
    }

    /// Scans all session folders, flushes any deferred session usage, and retries any held resumes
    /// or due pending wakes (`ASK-6`, `LIVE-3`, `LIVE-4`).
    pub fn process_due_wakes(&self) {
        let Ok(ids) = self.store.list_employee_ids() else {
            return;
        };
        for emp_id in ids {
            let _ = flush_pending_usage(&self.office, &self.store, &emp_id);
            let _ = self.process_due_wakes_for(&emp_id);
        }
    }

    /// Flushes all deferred/overflow session usage across every known employee into office
    /// accounting (called on clean office shutdown after all live pumps have stopped). Returns
    /// the number of usage records still unpersisted after the flush attempt.
    pub fn flush_all_pending_usage(&self) -> usize {
        let Ok(ids) = self.store.list_employee_ids() else {
            return 0;
        };
        let mut remaining = 0usize;
        for emp_id in ids {
            let _ = flush_pending_usage(&self.office, &self.store, &emp_id);
            let overflow = self.store.pending_usage_overflow_count(&emp_id);
            let pending = self
                .store
                .read_pending_usage(&emp_id)
                .map(|v| v.len())
                .unwrap_or(0);
            if overflow > 0 || pending > 0 {
                remaining = remaining.saturating_add(overflow).saturating_add(pending);
                eprintln!(
                    "org-office-sessions: warning: {emp_id} still has {pending} pending and {overflow} overflow usage record(s) after shutdown flush"
                );
            }
        }
        remaining
    }

    /// Retries any held required-ask resume, mutual-deadlock breaker, or pending wake queue for `employee_id`.
    pub fn process_due_wakes_for(&self, employee_id: &str) -> WakeOutcome {
        if crate::manager::is_admissions_closed(self) {
            return WakeOutcome::Ignored;
        }
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

    fn load_agent_employee(&self, employee_id: &str) -> Option<Employee> {
        if self
            .office
            .sync_status()
            .is_ok_and(|s| s.initial_snapshot_required)
        {
            return None;
        }
        let emp = self
            .office
            .load_employee(employee_id)
            .ok()
            .flatten()
            .filter(|e| e.kind == EmployeeKind::Agent)?;
        // Enforce placement at session admission so a sending office can never start a session
        // for an agent placed on a different office (`TASK-52` P6-#1).
        if self.office.is_multi_office().unwrap_or(false)
            && org_office::resolve_employee_office(&self.office, employee_id)
                .is_ok_and(|oid| oid != self.office.info().office_id)
        {
            return None;
        }
        Some(emp)
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
}

fn sender_in_reports_to_chain(office: &Office, target: &Employee, sender_id: &str) -> bool {
    let mut visited = HashSet::new();
    visited.insert(target.id.as_str().to_owned());
    let mut cursor = target
        .reports_to_employee_id
        .as_ref()
        .map(|id| id.as_str().to_owned());
    while let Some(mgr_id) = cursor {
        if mgr_id == sender_id {
            return true;
        }
        if !visited.insert(mgr_id.clone()) {
            break;
        }
        cursor = office
            .load_employee(&mgr_id)
            .ok()
            .flatten()
            .and_then(|e| e.reports_to_employee_id)
            .map(|id| id.as_str().to_owned());
    }
    false
}

fn sender_in_ancestor_team_leads(office: &Office, target: &Employee, sender_id: &str) -> bool {
    let Ok(snap) = office.org_snapshot() else {
        return false;
    };
    let by_id: HashMap<&str, (&Option<String>, &Option<String>)> = snap
        .teams
        .iter()
        .map(|t| (t.id.as_str(), (&t.lead_member_id, &t.owner_team_id)))
        .collect();
    let mut visited = HashSet::new();
    let mut cursor = Some(target.home_team_id.as_str());
    while let Some(team_id) = cursor {
        if !visited.insert(team_id) {
            break;
        }
        let Some((lead, parent)) = by_id.get(team_id) else {
            break;
        };
        if lead.as_deref() == Some(sender_id) {
            return true;
        }
        cursor = parent.as_deref();
    }
    false
}

fn resolve_verified_sender_relation(
    office: &Office,
    target: &Employee,
    item: &PendingWakeItem,
    sender_emp: Option<&Employee>,
) -> &'static str {
    if item.sender_id == office.owner_member_id() {
        return "owner";
    }
    if sender_in_reports_to_chain(office, target, &item.sender_id) {
        return "reports_to";
    }
    if sender_in_ancestor_team_leads(office, target, &item.sender_id) {
        return "team_lead";
    }
    if item.sender_is_person || sender_emp.is_some_and(|e| e.kind == EmployeeKind::Person) {
        "person"
    } else {
        "peer"
    }
}

/// Spawns a lightweight background wake-retry thread that sleeps in short 50ms slices so
/// office shutdown is never blocked (`Low #15`, `WAKE-4`, `LIVE-3`).
pub fn spawn_wake_retry_loop(
    manager: Arc<SessionManager>,
    stopping: Arc<std::sync::atomic::AtomicBool>,
    interval: Duration,
) -> Option<std::thread::JoinHandle<()>> {
    SessionManager::attach_self(&manager);
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
