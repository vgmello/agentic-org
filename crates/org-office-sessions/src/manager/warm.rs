//! Warm-handle turn dispatch and warm-route permit acquisition or reroute.

use super::*;

pub(super) struct LaunchRouteOpts<'a> {
    pub(super) resume_runtime_session_id: Option<&'a str>,
    pub(super) task_ref: Option<&'a str>,
    pub(super) is_failover_switch: bool,
    pub(super) oauth_refresh_attempted: bool,
    pub(super) excluded_connections: HashSet<String>,
    pub(super) tried_connections: Vec<String>,
}

pub(super) struct WarmHandleTurn<'a> {
    pub(super) emp_id: &'a str,
    pub(super) task_ref: Option<&'a str>,
    pub(super) tx: Sender<PumpCommand>,
    pub(super) state: Arc<Mutex<SessionRuntimeState>>,
    pub(super) conn_id: Option<String>,
    pub(super) route_digest: String,
    pub(super) active_permit: Arc<Mutex<Option<(String, u64)>>>,
    pub(super) text: String,
}

pub(super) fn dispatch_warm_handle_turn(
    mgr: &SessionManager,
    turn: WarmHandleTurn<'_>,
) -> Result<(Option<SessionView>, Option<LiveSessionHandle>), ApiError> {
    // Caller already reserved `reg.starting` for `turn.emp_id`. Release it on warm success or
    // error, and keep it reserved only when transferring to cold-start (`Ok((None, old))`).
    let slot_guard = StartSlotGuard {
        registry: &mgr.registry,
        employee_id: turn.emp_id.to_owned(),
    };
    let needs_permit = turn.conn_id.is_some()
        && turn
            .active_permit
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_none();
    if !needs_permit {
        if turn.tx.send(PumpCommand::SendTurn(turn.text)).is_ok() {
            let view = turn
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .to_view();
            return Ok((Some(view), None));
        }
        let old = mgr
            .registry
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .live
            .remove(turn.emp_id);
        std::mem::forget(slot_guard);
        return Ok((None, old));
    }
    let cid = turn.conn_id.as_deref().unwrap_or_default();
    let digest = (!turn.route_digest.is_empty()).then_some(turn.route_digest.as_str());
    let granted =
        match acquire_or_reroute_warm_permit(mgr, cid, turn.emp_id, turn.task_ref, digest)? {
            Ok(p) => p,
            Err(old) => {
                std::mem::forget(slot_guard);
                return Ok((None, old));
            }
        };
    let mut pguard = PermitGuard::new(&mgr.office, mgr.config.hq_permits.as_deref(), Some(granted));
    let mut installed_pid: Option<String> = None;
    {
        let mut slot = turn.active_permit.lock().unwrap_or_else(|p| p.into_inner());
        if slot.is_none() {
            if let Some(pair) = pguard.take() {
                installed_pid = Some(pair.0.clone());
                *slot = Some(pair);
            }
        }
    }
    if turn.tx.send(PumpCommand::SendTurn(turn.text)).is_ok() {
        let view = turn
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .to_view();
        return Ok((Some(view), None));
    }
    if let Some(ref pid) = installed_pid {
        let mut slot = turn.active_permit.lock().unwrap_or_else(|p| p.into_inner());
        if slot.as_ref().is_some_and(|(cur, _)| cur == pid) {
            let _ = slot.take();
            drop(slot);
            crate::router::release_turn_permit_any(
                &mgr.office,
                mgr.config.hq_permits.as_deref(),
                pid,
            );
        }
    }
    let old = mgr
        .registry
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .live
        .remove(turn.emp_id);
    std::mem::forget(slot_guard);
    Ok((None, old))
}

pub(super) fn acquire_or_reroute_warm_permit(
    mgr: &SessionManager,
    cid: &str,
    emp_id: &str,
    task_ref: Option<&str>,
    route_digest: Option<&str>,
) -> Result<Result<(String, u64), Option<LiveSessionHandle>>, ApiError> {
    match crate::router::acquire_warm_permit_for_office(
        &mgr.office,
        mgr.config.hq_permits.as_deref(),
        cid,
        emp_id,
        task_ref,
        route_digest,
    ) {
        crate::router::WarmPermitOutcome::Granted(p) => Ok(Ok(p)),
        crate::router::WarmPermitOutcome::Refused(err) => Err(err),
        crate::router::WarmPermitOutcome::Unavailable {
            multi_connection: false,
            reason,
        } => Err(ApiError::new(
            ErrorCode::Conflict,
            format!("POOL_EXHAUSTED: [{reason}]"),
        )),
        crate::router::WarmPermitOutcome::StaleWarm
        | crate::router::WarmPermitOutcome::Unavailable {
            multi_connection: true,
            ..
        } => {
            let old = mgr
                .registry
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .live
                .remove(emp_id);
            if let Some(ref handle) = old {
                let _ = handle.tx.send(PumpCommand::Stop {
                    reason: "pool_reroute".into(),
                });
            }
            Ok(Err(old))
        }
    }
}

pub(super) fn warm_handle_stop_reason(
    office: &Office,
    emp_id: &str,
    handle: &LiveSessionHandle,
    requested_workspace: Option<&str>,
    state: SessionState,
    bwrap_topology_stale: bool,
) -> Option<&'static str> {
    if handle.workspace.as_deref() != requested_workspace {
        return Some("workspace_changed");
    }
    if state != SessionState::Idle {
        return None;
    }
    if bwrap_topology_stale {
        return Some("bwrap_sibling_topology_changed");
    }
    if crate::host_auth::is_oauth_expiring(handle.oauth_expires_at_ms, crate::credentials::now_ms())
    {
        return Some("oauth_token_expiring");
    }
    if handle.connection_id.is_none()
        && office.load_agent_execution(emp_id).ok().flatten().is_some()
    {
        return Some("warm_route_stale");
    }
    None
}
