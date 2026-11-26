//! Pool failover relaunch, host Claude subscription refresh, and recovery wakes.

use std::collections::HashSet;
use std::path::PathBuf;

use org_office::organization_family::records::{EmployeeKind, EmployeeStatus, PermissionPreset};
use org_proto::sessions::{MemberSessionStartParams, SessionState};
use org_proto::ApiError;

use crate::credentials::rfc3339_now;
use crate::pump::FailoverRequest;

use super::views::build_idle_session_view;
use super::warm::LaunchRouteOpts;
use super::{
    is_admissions_closed, next_entropy_hex, set_waiting_messages_count, LiveSessionHandle,
    SessionManager, StartSlotGuard,
};

impl SessionManager {
    /// Handles §7 connection failover after a pre-side-effect `rate_limited`, `quota_exhausted`,
    /// `auth_expired`, or `model_unavailable` turn failure (`doc-88` §7).
    pub(crate) fn failover_relaunch(&self, req: FailoverRequest) {
        run_failover_relaunch(self, req);
    }
}

pub(crate) fn is_host_claude_subscription_refresh_eligible(
    mgr: &SessionManager,
    connection_id: &str,
) -> Option<PathBuf> {
    let conn = mgr.office.load_provider_connection(connection_id).ok()??;
    if conn.auth_mode != org_office::organization_family::records::ProviderAuthMode::SubscriptionCli
    {
        return None;
    }
    let adapter = mgr.config.runtimes.resolve(&conn.runtime_driver).ok()?;
    if adapter.id().as_str() != "claude" {
        return None;
    }
    if mgr
        .enrollment
        .vault()
        .load_unwrapped(conn.id.as_str())
        .ok()
        .flatten()
        .is_some()
    {
        return None;
    }
    mgr.config
        .runtimes
        .resolve_binary(adapter.as_ref(), None)
        .ok()
}

pub(super) fn try_refresh_host_claude_subscription(
    mgr: &SessionManager,
    req: &FailoverRequest,
) -> bool {
    if req.reason != "auth_expired" || req.oauth_refresh_attempted {
        return false;
    }
    let Some(binary) = is_host_claude_subscription_refresh_eligible(mgr, &req.failed_connection_id)
    else {
        return false;
    };
    crate::host_auth::refresh_host_claude_oauth_reactive(&binary, req.oauth_token_fp.as_deref())
}

pub(super) fn record_failover_connection_health(mgr: &SessionManager, req: &FailoverRequest) {
    match req.reason {
        "rate_limited" | "quota_exhausted" => {
            let _ = mgr.office.record_connection_cooldown_internal(
                &req.failed_connection_id,
                None,
                req.rate_limit_resets_at.as_deref(),
                None,
            );
            crate::router::report_cooldown_any(
                &mgr.office,
                mgr.config.hq_permits.as_deref(),
                &req.failed_connection_id,
                req.rate_limit_resets_at.as_deref(),
                Some(req.reason),
            );
        }
        "auth_expired" => {
            let _ = mgr
                .office
                .mark_connection_auth_expired_internal(&req.failed_connection_id, req.reason);
        }
        _ => {}
    }
}

/// Outcome of claiming a failed run's start slot for a pool failover relaunch.
pub(super) enum FailoverSlot {
    /// The failed run's handle (when still registered) was removed and the start slot reserved.
    Reserved(Option<Box<LiveSessionHandle>>),
    /// Admissions are closed or the run was explicitly stopped: the turn is intentionally dropped.
    Abandoned,
    /// A concurrent lifecycle operation holds the start slot (a recovery clear, stop, start, or
    /// warm recycle), the handle needs recovery, or a newer run replaced it: the relaunch cannot
    /// happen now, so the turn must be requeued rather than dropped.
    Contended,
}

pub(super) fn take_failed_live_handle(mgr: &SessionManager, req: &FailoverRequest) -> FailoverSlot {
    let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
    if reg.draining || reg.shutting_down || reg.stop_epoch(&req.employee_id) != req.stop_epoch {
        return FailoverSlot::Abandoned;
    }
    if reg.starting.contains(&req.employee_id) {
        return FailoverSlot::Contended;
    }
    if let Some(existing) = reg.live.get(&req.employee_id) {
        let st = existing.state.lock().unwrap_or_else(|p| p.into_inner());
        if st.recovery_required
            || st.state == SessionState::RecoveryRequired
            || st.run_id.as_deref() != Some(req.failed_run_id.as_str())
        {
            return FailoverSlot::Contended;
        }
    }
    let old = reg.live.remove(&req.employee_id);
    reg.starting.insert(req.employee_id.clone());
    FailoverSlot::Reserved(old.map(Box::new))
}

pub(crate) fn run_failover_relaunch(mgr: &SessionManager, req: FailoverRequest) {
    if is_admissions_closed(mgr) {
        return;
    }
    let is_stale_recycle = req.reason == "warm_route_stale";
    let refreshed_same_conn = !is_stale_recycle && try_refresh_host_claude_subscription(mgr, &req);
    if !is_stale_recycle && !refreshed_same_conn {
        record_failover_connection_health(mgr, &req);
    }
    let dead_handle = match take_failed_live_handle(mgr, &req) {
        FailoverSlot::Reserved(dead_handle) => dead_handle.map(|handle| *handle),
        FailoverSlot::Abandoned => return,
        FailoverSlot::Contended => {
            requeue_contended_failover(mgr, &req);
            return;
        }
    };
    let slot = StartSlotGuard::new(&mgr.registry, req.employee_id.clone());
    if let Some(old) = dead_handle {
        if super::reap::reap_live_handle(
            mgr,
            &req.employee_id,
            old,
            Some("pool_failover"),
            super::reap::ReapMode::FailClosedRestore {
                conflict_message: format!(
                    "could not verify teardown of failed session `{}`; resolve recovery first",
                    req.employee_id
                ),
            },
        )
        .is_err()
        {
            return;
        }
    }
    let Ok(employee) = mgr.require_active_agent(&req.employee_id) else {
        return;
    };
    let Ok(cmd_suffix) = next_entropy_hex(8) else {
        return;
    };
    let params = MemberSessionStartParams {
        command_id: format!("failover-{cmd_suffix}"),
        employee_id: req.employee_id.clone(),
        instruction: Some(req.objective.clone()),
        workspace: req.workspace.clone(),
    };
    let opts = failover_route_opts(&req, refreshed_same_conn);
    let objective = mgr.prepend_watch_notices(employee.id.as_str(), req.objective.clone());
    let launch_res = mgr
        .plan_session_launch(&employee, &params, opts)
        .and_then(|planned| mgr.issue_and_spawn_launch(objective, planned))
        .and_then(|prepared| mgr.install_live_session(prepared, slot));
    if let Err(err) = launch_res {
        record_failover_exhausted(mgr, &req, &err);
    }
}

/// The route options of a failover relaunch: the failed run's task, epoch and partition (so the
/// relaunch acts for exactly the caller the failed run acted for, `TASK-80.1`), with the tried
/// connections excluded unless the run is only being recycled.
pub(crate) fn failover_route_opts(
    req: &FailoverRequest,
    refreshed_same_conn: bool,
) -> LaunchRouteOpts<'_> {
    let is_stale_recycle = req.reason == "warm_route_stale";
    let mut excluded: HashSet<String> = if is_stale_recycle {
        HashSet::new()
    } else {
        req.tried_connections.iter().cloned().collect()
    };
    if refreshed_same_conn {
        excluded.remove(&req.failed_connection_id);
    }
    LaunchRouteOpts {
        resume_runtime_session_id: None,
        task_ref: req.task_ref.as_deref(),
        is_failover_switch: !is_stale_recycle,
        oauth_refresh_attempted: req.oauth_refresh_attempted || refreshed_same_conn,
        excluded_connections: excluded,
        tried_connections: if is_stale_recycle {
            Vec::new()
        } else {
            req.tried_connections.clone()
        },
        // `take_failed_live_handle` reserved the slot only while the epoch still matched, so a
        // stop from here on makes the relaunch abort at registration (`TASK-190`).
        start_stop_epoch: req.stop_epoch,
        partition: req.partition.clone(),
    }
}

/// Persists a failover turn that lost its relaunch to a concurrent lifecycle operation as a
/// durable `Recovery` wake (every failover reason, not only `warm_route_stale`), so the wake
/// retry loop or the contending operation's own wake pass runs it once the slot frees.
///
/// Nothing is queued once admissions close or the agent is definitively retired (offboarded or no
/// longer an agent). Pause and offboard already abandon the failover through their explicit
/// `stop_session` ([`FailoverSlot::Abandoned`]); a transient admission refusal (a satellite
/// awaiting its HQ snapshot, an unreadable record) keeps the turn, never drops it.
pub(crate) fn requeue_contended_failover(mgr: &SessionManager, req: &FailoverRequest) {
    if is_failover_abandoned(mgr, req) || is_retired_agent(mgr, &req.employee_id) {
        return;
    }
    let note = serde_json::json!({
        "type": "pool_failover_requeued",
        "failed_connection_id": req.failed_connection_id,
        "reason": req.reason,
        "at": rfc3339_now(),
    });
    let _ = mgr.store.append_history(&req.employee_id, &note, &[]);
    enqueue_recovery_wake(mgr, req, None);
}

/// `true` once admissions closed or an explicit stop moved the employee's stop epoch past the
/// failed run's: the failover is intentionally dropped.
fn is_failover_abandoned(mgr: &SessionManager, req: &FailoverRequest) -> bool {
    let reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
    reg.draining || reg.shutting_down || reg.stop_epoch(&req.employee_id) != req.stop_epoch
}

/// `true` only when the employee record says the agent will never run again here.
fn is_retired_agent(mgr: &SessionManager, employee_id: &str) -> bool {
    matches!(
        mgr.office.load_employee(employee_id),
        Ok(Some(emp)) if emp.kind != EmployeeKind::Agent || emp.status == EmployeeStatus::Inactive
    )
}

pub(super) fn record_failover_exhausted(
    mgr: &SessionManager,
    req: &FailoverRequest,
    err: &ApiError,
) {
    if is_failover_abandoned(mgr, req) {
        // Explicitly stopped (pause, offboard, owner stop) or shutting down while relaunching:
        // the turn is dropped on purpose, so nothing is recorded or requeued (`TASK-190`).
        return;
    }
    let note = serde_json::json!({
        "type": "pool_failover_exhausted",
        "failed_connection_id": req.failed_connection_id,
        "reason": req.reason,
        "error": err.message,
        "at": rfc3339_now(),
    });
    let _ = mgr.store.append_history(&req.employee_id, &note, &[]);
    let mut last_view = mgr.idle_view_for(&req.employee_id).unwrap_or_else(|_| {
        build_idle_session_view(
            &mgr.store,
            &req.employee_id,
            &mgr.office.info().office_id,
            mgr.office.agent_private_home_path(&req.employee_id),
            PermissionPreset::SafeAsk,
            true,
        )
    });
    last_view.last_error = Some(err.message.clone());
    mgr.last_states
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(req.employee_id.clone(), last_view);
    // Queue a Recovery wake so the exhausted turn automatically retries when cooldown/capacity clears.
    enqueue_recovery_wake(
        mgr,
        req,
        Some(crate::wake::WakeHoldReason::AdmissionRefused),
    );
}

pub(super) fn enqueue_recovery_wake(
    mgr: &SessionManager,
    req: &FailoverRequest,
    hold: Option<crate::wake::WakeHoldReason>,
) {
    // A Chief of Staff turn acting for a member carries that member's request: the shared wake
    // queue runs in her `tess:wake` partition for nobody, so it never takes the turn. The member
    // sees the failure on the session and asks again (`TASK-80.1`).
    if req.acting_for_caller_id().is_some() {
        let note = serde_json::json!({
            "type": "pool_failover_not_requeued",
            "reason": "delegated_chief_of_staff_turn",
            "failed_connection_id": req.failed_connection_id,
            "at": rfc3339_now(),
        });
        let _ = mgr.store.append_history(&req.employee_id, &note, &[]);
        return;
    }
    let ts = crate::credentials::now_ms();
    let _lock = mgr.employee_locks.lock(&req.employee_id);
    {
        let reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
        if reg.draining || reg.shutting_down || reg.stop_epoch(&req.employee_id) != req.stop_epoch {
            return;
        }
    }
    let mut queue = mgr
        .store
        .read_pending_wakes(&req.employee_id)
        .ok()
        .flatten()
        .unwrap_or_else(|| crate::wake::PendingWakeQueueRecord::empty(&req.employee_id));
    queue.push_dedup(crate::wake::PendingWakeItem {
        item_id: format!(
            "failover:{}:{}:{}",
            req.reason, req.failed_connection_id, ts
        ),
        kind: crate::wake::WakeKind::Recovery,
        sender_id: mgr.office.owner_member_id().to_owned(),
        sender_name: "Office Supervisor".into(),
        sender_handle: None,
        sender_relation: Some("office_supervisor".into()),
        sender_is_person: true,
        conversation_id: None,
        source_id: req.failed_connection_id.clone(),
        summary: req.objective.clone(),
        enqueued_at_ms: ts,
    });
    if let Some(reason) = hold {
        queue.set_hold(reason, ts, None);
    }
    let tokens = mgr.bindings.active_tokens();
    let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
    let _ = mgr.store.write_pending_wakes_redacted(&queue, &refs);
    set_waiting_messages_count(&mgr.waiting_messages, &req.employee_id, queue.items.len());
}
