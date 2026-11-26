//! Warm-handle turn dispatch and warm-route permit acquisition or reroute.

use std::collections::HashSet;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use org_proto::sessions::{SessionState, SessionView};
use org_proto::{ApiError, ErrorCode};

use crate::pump::{PumpCommand, SessionRuntimeState};

use super::reap::{reap_live_handle, ReapMode};
use super::registry_guards::{stopped_since, stopped_while_starting};
use super::{LiveSessionHandle, PermitGuard, SessionManager, StartSlotGuard};

/// The outcome of reserving a start: a live or warm-send view, or a reserved start slot with
/// the handle it replaces and the explicit-stop epoch at reservation (`TASK-190`).
pub(super) struct StartReservation {
    pub(super) live_view: Option<SessionView>,
    pub(super) finished_handle: Option<LiveSessionHandle>,
    pub(super) start_stop_epoch: u64,
}

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

/// What became of a warm turn at the fenced send.
enum FencedSend<'a> {
    /// The turn reached the pump and the start slot was released.
    Delivered,
    /// The pump is gone: its handle was removed and the slot stays reserved for a cold start.
    PumpGone(Option<LiveSessionHandle>),
    /// An explicit stop overtook the turn: nothing was sent and the slot is still held.
    Stopped(StartSlotGuard<'a>),
}

/// Sends a turn to a warm handle. An explicit stop (pause, offboard, owner stop) that arrives
/// while this send holds the start slot always wins: no turn reaches the child, the handle is
/// reaped and the caller gets `Conflict`, so a paused autonomous wake keeps its durable items
/// (`TASK-190`).
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
    if stopped_since(&mgr.registry, turn.emp_id, turn.start_stop_epoch) {
        return Err(abort_stopped_warm_turn(
            mgr,
            &turn.state,
            turn.emp_id,
            slot_guard,
        ));
    }
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
    let mut installed_pid = None;
    if needs_permit {
        let cid = turn.conn_id.as_deref().unwrap_or_default();
        let digest = (!turn.route_digest.is_empty()).then_some(turn.route_digest.as_str());
        match acquire_or_reroute_warm_permit(mgr, cid, turn.emp_id, turn.task_ref, digest) {
            Ok(Ok(granted)) => {
                installed_pid = install_warm_permit(mgr, &turn.active_permit, granted)
            }
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
        }
    }
    match send_turn_fenced(
        mgr,
        turn.emp_id,
        &turn.tx,
        turn.start_stop_epoch,
        text,
        slot_guard,
    ) {
        FencedSend::Delivered => Ok(StartReservation {
            live_view: Some(
                turn.state
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .to_view(),
            ),
            finished_handle: None,
            start_stop_epoch: turn.start_stop_epoch,
        }),
        FencedSend::PumpGone(old) => {
            release_installed_permit(mgr, &turn.active_permit, installed_pid.as_deref());
            Ok(cold(old))
        }
        FencedSend::Stopped(slot_guard) => {
            release_installed_permit(mgr, &turn.active_permit, installed_pid.as_deref());
            Err(abort_stopped_warm_turn(
                mgr,
                &turn.state,
                turn.emp_id,
                slot_guard,
            ))
        }
    }
}

/// Checks the stop epoch, sends the turn and releases the start slot in one `registry` critical
/// section, so a stop either lands first (and no turn reaches the child) or finds no start in
/// flight and stops the session itself. `Sender::send` on the pump's unbounded channel never
/// blocks, so no slow work runs under the lock (rules §3.4).
fn send_turn_fenced<'a>(
    mgr: &'a SessionManager,
    emp_id: &str,
    tx: &Sender<PumpCommand>,
    start_stop_epoch: u64,
    text: String,
    slot_guard: StartSlotGuard<'a>,
) -> FencedSend<'a> {
    let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
    if reg.stop_epoch(emp_id) != start_stop_epoch {
        drop(reg);
        return FencedSend::Stopped(slot_guard);
    }
    if tx.send(PumpCommand::SendTurn(text)).is_ok() {
        slot_guard.release_locked(&mut reg);
        return FencedSend::Delivered;
    }
    let old = reg.live.remove(emp_id);
    slot_guard.keep_reserved();
    FencedSend::PumpGone(old)
}

/// Installs a granted warm-route permit as the handle's active permit, or returns it to the
/// pool when one is already active, and reports the permit id this call installed.
fn install_warm_permit(
    mgr: &SessionManager,
    active_permit: &Mutex<Option<(String, u64)>>,
    granted: (String, u64),
) -> Option<String> {
    let mut pguard = PermitGuard::new(&mgr.office, mgr.config.hq_permits.as_deref(), Some(granted));
    let mut slot = active_permit.lock().unwrap_or_else(|p| p.into_inner());
    if slot.is_some() {
        drop(slot);
        // `pguard` returns the unused permit when it drops, with no lock held.
        return None;
    }
    let pair = pguard.take()?;
    let pid = pair.0.clone();
    *slot = Some(pair);
    Some(pid)
}

/// Returns a permit this turn installed but never used to the pool.
fn release_installed_permit(
    mgr: &SessionManager,
    active_permit: &Mutex<Option<(String, u64)>>,
    installed_pid: Option<&str>,
) {
    let Some(pid) = installed_pid else {
        return;
    };
    let mut slot = active_permit.lock().unwrap_or_else(|p| p.into_inner());
    if slot.as_ref().is_some_and(|(cur, _)| cur == pid) {
        let _ = slot.take();
        drop(slot);
        crate::router::release_turn_permit_any(&mgr.office, mgr.config.hq_permits.as_deref(), pid);
    }
}

/// Releases a warm turn's start slot when no explicit stop moved the epoch, in the same critical
/// section that checks it; otherwise reaps the stopped handle and refuses with `Conflict`.
fn release_warm_slot(
    mgr: &SessionManager,
    state: &Arc<Mutex<SessionRuntimeState>>,
    emp_id: &str,
    start_stop_epoch: u64,
    slot_guard: StartSlotGuard<'_>,
) -> Result<(), ApiError> {
    {
        let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
        if reg.stop_epoch(emp_id) == start_stop_epoch {
            slot_guard.release_locked(&mut reg);
            return Ok(());
        }
    }
    Err(abort_stopped_warm_turn(mgr, state, emp_id, slot_guard))
}

/// Reaps the warm handle that a stop (pause, offboard, owner stop) overtook while this turn held
/// the start slot, holding the slot through the reap, and returns the refusal. The stop was
/// refused while the slot was held, so the turn finishes it here (`TASK-190`).
fn abort_stopped_warm_turn(
    mgr: &SessionManager,
    state: &Arc<Mutex<SessionRuntimeState>>,
    emp_id: &str,
    slot_guard: StartSlotGuard<'_>,
) -> ApiError {
    let stopped = {
        let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
        let same_handle = reg
            .live
            .get(emp_id)
            .is_some_and(|h| Arc::ptr_eq(&h.state, state));
        same_handle.then(|| reg.live.remove(emp_id)).flatten()
    };
    if let Some(handle) = stopped {
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
        );
        if let Err(err) = reaped {
            return err;
        }
    }
    drop(slot_guard);
    stopped_while_starting()
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
