//! Warm-handle turn dispatch and warm-route permit acquisition or reroute.

use std::collections::HashSet;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use org_proto::sessions::{SessionState, SessionView};
use org_proto::{ApiError, ErrorCode};

use crate::pump::{PumpCommand, SessionRuntimeState};

use super::launch::StartReservation;
use super::reap::{reap_live_handle, ReapMode};
use super::{LiveSessionHandle, PermitGuard, SessionManager, StartSlotGuard};

pub(super) struct LaunchRouteOpts<'a> {
    pub(super) resume_runtime_session_id: Option<&'a str>,
    pub(super) task_ref: Option<&'a str>,
    pub(super) is_failover_switch: bool,
    pub(super) oauth_refresh_attempted: bool,
    pub(super) excluded_connections: HashSet<String>,
    pub(super) tried_connections: Vec<String>,
    /// The explicit-stop epoch when the start slot was reserved (`TASK-190`).
    pub(super) start_stop_epoch: u64,
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
    pub(super) start_stop_epoch: u64,
}

/// Results of a warm handle's disk and SQLite freshness checks, run with no lock held.
#[derive(Debug, Clone, Copy)]
pub(super) struct WarmFreshness {
    /// A sibling appeared that this Linux `bwrap` session does not mask.
    pub(super) bwrap_topology_stale: bool,
    /// The handle has no pool connection but the agent now has an execution config.
    pub(super) route_stale: bool,
}

pub(super) fn dispatch_warm_handle_turn(
    mgr: &SessionManager,
    turn: WarmHandleTurn<'_>,
) -> Result<StartReservation, ApiError> {
    // Caller already reserved `reg.starting` for `turn.emp_id`. Release it on warm success or
    // error, and keep it reserved only when transferring to a cold start.
    let slot_guard = StartSlotGuard::new(&mgr.registry, turn.emp_id.to_owned());
    // Both `registry` and the handle's state lock are released here: the watch-notice callback
    // may do I/O and must never run under them (`TASK-190`, rules §3.4).
    let text = mgr.prepend_watch_notices(turn.emp_id, turn.text);
    let cold = |old: Option<LiveSessionHandle>| StartReservation {
        live_view: None,
        finished_handle: old,
        start_stop_epoch: turn.start_stop_epoch,
    };
    let needs_permit = turn.conn_id.is_some()
        && turn
            .active_permit
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_none();
    if !needs_permit {
        if turn.tx.send(PumpCommand::SendTurn(text)).is_ok() {
            return finish_warm_send(
                mgr,
                &turn.state,
                turn.emp_id,
                turn.start_stop_epoch,
                slot_guard,
            );
        }
        let old = mgr
            .registry
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .live
            .remove(turn.emp_id);
        slot_guard.keep_reserved();
        return Ok(cold(old));
    }
    let cid = turn.conn_id.as_deref().unwrap_or_default();
    let digest = (!turn.route_digest.is_empty()).then_some(turn.route_digest.as_str());
    let granted = match acquire_or_reroute_warm_permit(mgr, cid, turn.emp_id, turn.task_ref, digest)
    {
        Ok(Ok(p)) => p,
        Ok(Err(old)) => {
            slot_guard.keep_reserved();
            return Ok(cold(old));
        }
        Err(err) => {
            release_warm_slot(
                mgr,
                &turn.state,
                turn.emp_id,
                turn.start_stop_epoch,
                slot_guard,
            )?;
            return Err(err);
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
    if turn.tx.send(PumpCommand::SendTurn(text)).is_ok() {
        return finish_warm_send(
            mgr,
            &turn.state,
            turn.emp_id,
            turn.start_stop_epoch,
            slot_guard,
        );
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
    slot_guard.keep_reserved();
    Ok(cold(old))
}

/// Releases a delivered warm turn's start slot and reports the handle's view.
fn finish_warm_send(
    mgr: &SessionManager,
    state: &Arc<Mutex<SessionRuntimeState>>,
    emp_id: &str,
    start_stop_epoch: u64,
    slot_guard: StartSlotGuard<'_>,
) -> Result<StartReservation, ApiError> {
    let reaped = release_warm_slot(mgr, state, emp_id, start_stop_epoch, slot_guard)?;
    let view = reaped.unwrap_or_else(|| state.lock().unwrap_or_else(|p| p.into_inner()).to_view());
    Ok(StartReservation {
        live_view: Some(view),
        finished_handle: None,
        start_stop_epoch,
    })
}

/// Releases a warm send's start slot in the same critical section that checks the explicit-stop
/// epoch. A stop (pause, offboard, owner stop) refused while the slot was held still wins: the
/// handle is removed and reaped, with the slot held through the reap, and its final view is
/// returned (`TASK-190`).
fn release_warm_slot(
    mgr: &SessionManager,
    state: &Arc<Mutex<SessionRuntimeState>>,
    emp_id: &str,
    start_stop_epoch: u64,
    slot_guard: StartSlotGuard<'_>,
) -> Result<Option<SessionView>, ApiError> {
    let stopped = {
        let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
        if reg.stop_epoch(emp_id) == start_stop_epoch {
            slot_guard.release_locked(&mut reg);
            return Ok(None);
        }
        let same_handle = reg
            .live
            .get(emp_id)
            .is_some_and(|h| Arc::ptr_eq(&h.state, state));
        same_handle.then(|| reg.live.remove(emp_id)).flatten()
    };
    let Some(handle) = stopped else {
        return Ok(None);
    };
    let reaped = reap_live_handle(
        mgr,
        emp_id,
        handle,
        Some("explicit_stop_during_turn_delivery"),
        ReapMode::FailClosedRestore {
            conflict_message: format!(
                "could not verify teardown of session `{emp_id}` stopped during turn delivery; resolve recovery first"
            ),
        },
    )?;
    drop(slot_guard);
    Ok(Some(reaped.view))
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

/// Why a warm handle must be recycled before this start, decided under the `registry` lock
/// from facts only: the disk and SQLite checks arrive precomputed in `freshness`.
pub(super) fn warm_handle_stop_reason(
    handle: &LiveSessionHandle,
    requested_workspace: Option<&str>,
    state: SessionState,
    freshness: WarmFreshness,
) -> Option<&'static str> {
    if handle.workspace.as_deref() != requested_workspace {
        return Some("workspace_changed");
    }
    if state != SessionState::Idle {
        return None;
    }
    if freshness.bwrap_topology_stale {
        return Some("bwrap_sibling_topology_changed");
    }
    if crate::host_auth::is_oauth_expiring(handle.oauth_expires_at_ms, crate::credentials::now_ms())
    {
        return Some("oauth_token_expiring");
    }
    if freshness.route_stale {
        return Some("warm_route_stale");
    }
    None
}
