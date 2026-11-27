//! Read-only snapshot of live pending tool approvals and change-notification hook registration
//! for the owner Inbox (`TASK-102`).

use std::sync::Arc;

use org_proto::sessions::{PendingApprovalView, SessionState};
use org_proto::ApiError;

use crate::redact::redact_text;

use super::SessionManager;

/// Callback invoked outside state locks whenever any live session's pending tool approvals
/// change (arrival, owner decision, runtime cancellation, interrupt, stall, stop, or exit).
pub type ApprovalChangeHook = Arc<dyn Fn() + Send + Sync>;

/// Safe, redacted metadata snapshot of one pending tool approval in a live supervised session.
/// Carries only metadata (never raw tool arguments, environment tokens, or private transcripts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingToolApproval {
    pub employee_id: String,
    pub office_id: String,
    pub run_id: Option<String>,
    pub approval_id: String,
    pub tool: String,
    pub escalation: String,
    pub reason: Option<String>,
    pub requested_at: String,
}

impl SessionManager {
    /// Registers an office-local observer notified whenever any live session's pending tool
    /// approvals arrive, resolve, cancel, or clear on session teardown (`TASK-102`).
    pub fn set_approval_change_hook(&self, hook: ApprovalChangeHook) {
        *self
            .approval_change_hook
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(hook);
    }

    pub(crate) fn notify_approval_changed(&self) {
        let hook = self
            .approval_change_hook
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Some(cb) = hook {
            cb();
        }
    }

    /// Returns a lock-free-of-I/O, redacted snapshot of all pending tool approvals across live
    /// sessions currently in [`SessionState::WaitingApproval`]. Strictly enforces owner-only
    /// caller authorization, matching [`SessionManager::decide_approval`].
    pub fn pending_approvals_for(
        &self,
        caller_id: &str,
    ) -> Result<Vec<PendingToolApproval>, ApiError> {
        self.authorize_owner_only(caller_id)?;
        let handles: Vec<(
            String,
            Arc<std::sync::Mutex<crate::pump::SessionRuntimeState>>,
        )> = {
            let reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
            reg.live
                .iter()
                .map(|(emp_id, handle)| (emp_id.clone(), Arc::clone(&handle.state)))
                .collect()
        };
        if handles.is_empty() {
            return Ok(Vec::new());
        }
        let tokens = self.bindings.active_tokens();
        let token_refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
        let mut approvals = Vec::new();
        for (employee_id, state) in handles {
            let Some((office_id, run_id, pending)) = snapshot_waiting_approvals(&state) else {
                continue;
            };
            for view in pending {
                approvals.push(to_pending_tool_approval(
                    &employee_id,
                    &office_id,
                    run_id.as_deref(),
                    view,
                    &token_refs,
                ));
            }
        }
        approvals.sort_by(|a, b| {
            a.employee_id
                .cmp(&b.employee_id)
                .then_with(|| a.approval_id.cmp(&b.approval_id))
        });
        Ok(approvals)
    }
}

fn snapshot_waiting_approvals(
    state: &std::sync::Mutex<crate::pump::SessionRuntimeState>,
) -> Option<(String, Option<String>, Vec<PendingApprovalView>)> {
    let guard = state.lock().unwrap_or_else(|p| p.into_inner());
    if guard.state != SessionState::WaitingApproval || guard.pending_approvals.is_empty() {
        return None;
    }
    Some((
        guard.office_id.clone(),
        guard.run_id.clone(),
        guard.pending_approvals.clone(),
    ))
}

fn to_pending_tool_approval(
    employee_id: &str,
    office_id: &str,
    run_id: Option<&str>,
    view: PendingApprovalView,
    active_tokens: &[&str],
) -> PendingToolApproval {
    PendingToolApproval {
        employee_id: employee_id.to_owned(),
        office_id: office_id.to_owned(),
        run_id: run_id.map(str::to_owned),
        approval_id: view.approval_id,
        tool: redact_text(&view.tool, active_tokens),
        escalation: view.escalation,
        reason: view
            .reason
            .as_deref()
            .map(|reason| redact_text(reason, active_tokens)),
        requested_at: view.requested_at,
    }
}
