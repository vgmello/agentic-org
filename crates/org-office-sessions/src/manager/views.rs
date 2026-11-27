//! Session views for agents without a live pump, overlaid with durable wake and suspension state.

use std::path::PathBuf;

use org_office::organization_family::records::PermissionPreset;
use org_proto::sessions::{SessionState, SessionView};
use org_proto::ApiError;

use crate::instructions::SAME_USER_ISOLATION_NOTICE;
use crate::recovery::{ActiveTurnRecord, SessionStore};

use super::SessionManager;

impl SessionManager {
    pub(super) fn idle_view_for(&self, employee_id: &str) -> Result<SessionView, ApiError> {
        if let Some(mut last) = self
            .last_states
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(employee_id)
            .cloned()
        {
            overlay_durable_wake_state(&self.store, employee_id, &mut last);
            return Ok(last);
        }
        let (preset, sandbox_enabled) = self
            .load_preset_and_sandbox(employee_id)
            .unwrap_or((PermissionPreset::SafeAsk, true));
        Ok(build_idle_session_view(
            &self.store,
            employee_id,
            &self.office.info().office_id,
            self.office.agent_private_home_path(employee_id),
            preset,
            sandbox_enabled,
        ))
    }
}

pub(super) fn overlay_durable_wake_state(
    store: &SessionStore,
    employee_id: &str,
    view: &mut SessionView,
) {
    let suspended = store.read_suspended_state(employee_id).ok().flatten();
    let pending_wakes = store.read_pending_wakes(employee_id).ok().flatten();
    let blocked = view.stall_locked || view.recovery_required;
    if let Some(ref susp) = suspended {
        if !blocked
            && !matches!(
                view.state,
                SessionState::Starting | SessionState::Running | SessionState::WaitingApproval
            )
        {
            view.state = SessionState::Suspended;
        }
        let (w_id, w_name) = susp.primary_waiting_target();
        view.suspended_on_asks = susp.asks.iter().map(|a| a.ask_id.clone()).collect();
        view.waiting_for_member_id = w_id;
        view.waiting_for_name = w_name;
        view.suspended_since = Some(susp.suspended_since.clone());
        if view.runtime_session_id.is_none() {
            view.runtime_session_id = susp.runtime_session_id.clone();
        }
    } else {
        view.suspended_on_asks.clear();
        view.waiting_for_member_id = None;
        view.waiting_for_name = None;
        view.suspended_since = None;
    }
    let (count, reason) = extract_pending_wake_fields(pending_wakes.as_ref(), suspended.as_ref());
    view.pending_wake_count = count;
    view.pending_wake_reason = reason;
}

pub(super) fn build_idle_session_view(
    store: &SessionStore,
    employee_id: &str,
    office_id: &str,
    private_home: PathBuf,
    preset: PermissionPreset,
    sandbox_enabled: bool,
) -> SessionView {
    let suspended = store.read_suspended_state(employee_id).ok().flatten();
    let pending_wakes = store.read_pending_wakes(employee_id).ok().flatten();
    let persisted = store.read_active_turn(employee_id).ok().flatten();
    let (mut state, consecutive_stalls, stall_locked, recovery_required, run_id, session_id, rsid) =
        extract_persisted_turn(persisted);
    let (suspended_on_asks, waiting_for_member_id, waiting_for_name, suspended_since) =
        extract_suspended_fields(
            suspended.as_ref(),
            stall_locked || recovery_required,
            &mut state,
        );
    let (pending_wake_count, pending_wake_reason) =
        extract_pending_wake_fields(pending_wakes.as_ref(), suspended.as_ref());
    SessionView {
        employee_id: employee_id.to_owned(),
        office_id: office_id.to_owned(),
        state,
        run_id,
        session_id,
        runtime_id: None,
        runtime_session_id: rsid.or_else(|| suspended.and_then(|s| s.runtime_session_id)),
        private_home: private_home.display().to_string(),
        same_user_isolation_notice: SAME_USER_ISOLATION_NOTICE.to_owned(),
        permission_preset: preset.as_str().to_owned(),
        sandbox_enabled,
        consecutive_stalls,
        stall_locked,
        recovery_required,
        last_reply: None,
        last_error: None,
        background_tasks: Vec::new(),
        pending_approvals: Vec::new(),
        usage: Default::default(),
        suspended_on_asks,
        waiting_for_member_id,
        waiting_for_name,
        suspended_since,
        pending_wake_count,
        pending_wake_reason,
        activity_phase: None,
        active_tool_name: None,
        active_tool_started_at: None,
        supports_steer: false,
    }
}

pub(super) fn extract_persisted_turn(
    persisted: Option<ActiveTurnRecord>,
) -> (
    SessionState,
    u32,
    bool,
    bool,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    let Some(rec) = persisted else {
        return (SessionState::Idle, 0, false, false, None, None, None);
    };
    let state = if rec.stall_locked {
        SessionState::StallLocked
    } else if rec.recovery_required {
        SessionState::RecoveryRequired
    } else {
        SessionState::Idle
    };
    (
        state,
        rec.consecutive_stalls,
        rec.stall_locked,
        rec.recovery_required,
        Some(rec.run_id),
        Some(rec.session_id),
        rec.runtime_session_id,
    )
}

pub(super) fn extract_suspended_fields(
    suspended: Option<&crate::wake::SuspendedStateRecord>,
    blocked: bool,
    state: &mut SessionState,
) -> (Vec<String>, Option<String>, Option<String>, Option<String>) {
    let Some(susp) = suspended else {
        return (Vec::new(), None, None, None);
    };
    if !blocked {
        *state = SessionState::Suspended;
    }
    let (w_id, w_name) = susp.primary_waiting_target();
    (
        susp.asks.iter().map(|a| a.ask_id.clone()).collect(),
        w_id,
        w_name,
        Some(susp.suspended_since.clone()),
    )
}

pub(super) fn extract_pending_wake_fields(
    pending_wakes: Option<&crate::wake::PendingWakeQueueRecord>,
    suspended: Option<&crate::wake::SuspendedStateRecord>,
) -> (u32, Option<String>) {
    match pending_wakes {
        Some(q) if !q.items.is_empty() => (
            q.items.len() as u32,
            q.hold_reason.map(|r| r.as_str().to_owned()),
        ),
        _ => (
            0,
            suspended.and_then(|s| s.hold_reason.map(|r| r.as_str().to_owned())),
        ),
    }
}
