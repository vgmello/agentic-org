//! The single fail-closed teardown choke-point for a removed live session handle (`AE-07`,
//! `TASK-166`, rules §2): stop, crash, timeout, pre-pump abort, sibling refresh, recovery clear,
//! pool failover and shutdown all reap through [`reap_live_handle`].

use std::sync::{Arc, Mutex};

use org_proto::sessions::SessionState;
use org_proto::{ApiError, ErrorCode};

use crate::pump::PumpCommand;

use super::{spawn_bg_worker_reserved, LiveSessionHandle, SessionManager, StartSlotGuard};

pub(crate) struct ReapedHandle {
    pub(crate) view: org_proto::sessions::SessionView,
    pub(crate) permit_released: bool,
}

pub(crate) enum ReapMode {
    /// Normal start/stop/recycle/sibling-reap: restore unrecovered handle into `reg.live`
    /// (preserving `sandbox_plan` with `thread: None`) and return `Err(Conflict)`.
    FailClosedRestore { conflict_message: String },
    /// Daemon `stop_all` / `shutdown` drain: persist durable `recovery_required = true` and mark
    /// permit `release_pending`, but do not re-insert into `reg.live` so shutdown converges.
    ShutdownDrain,
}

/// Single fail-closed choke-point for stopping/joining any removed `LiveSessionHandle`.
///
/// Caller MUST hold `StartSlotGuard` for `employee_id` across this call when operating under
/// `ReapMode::FailClosedRestore` so no concurrent launch can race while teardown is verified
/// (`AE-07`, `TASK-166`).
pub(crate) fn reap_live_handle(
    mgr: &SessionManager,
    employee_id: &str,
    mut handle: LiveSessionHandle,
    stop_reason: Option<&str>,
    mode: ReapMode,
) -> Result<ReapedHandle, ApiError> {
    mgr.bindings.revoke(employee_id, None);
    mgr.hibernation.clear_live_tracking(employee_id);
    if let Some(reason) = stop_reason {
        let _ = handle.tx.send(PumpCommand::Stop {
            reason: reason.to_owned(),
        });
    }
    let joined_ok = handle
        .thread
        .take()
        .is_none_or(|thread| thread.join().is_ok());
    let (needs_recovery, run_id, session_id, runtime_session_id, view) = {
        let mut st = handle.state.lock().unwrap_or_else(|p| p.into_inner());
        let needs_recovery =
            !joined_ok || st.recovery_required || st.state == SessionState::RecoveryRequired;
        if needs_recovery {
            st.recovery_required = true;
            st.state = SessionState::RecoveryRequired;
        }
        (
            needs_recovery,
            st.run_id.clone(),
            st.session_id.clone(),
            st.runtime_session_id.clone(),
            st.to_view(),
        )
    };
    let mut permit_released = false;
    if let Some((pid, _)) = handle
        .active_permit
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
    {
        if !needs_recovery {
            crate::router::release_turn_permit_any(
                &mgr.office,
                mgr.config.hq_permits.as_deref(),
                &pid,
            );
            permit_released = true;
        } else {
            let _ = mgr.office.mark_turn_permit_release_pending(&pid);
        }
    }
    mgr.last_states
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(employee_id.to_owned(), view.clone());
    if needs_recovery {
        // Upsert durable recovery state even if `active-turn.v1.json` does not yet exist (`AE-07`).
        let _ = mgr.store.persist_recovery_required(
            employee_id,
            run_id.as_deref(),
            session_id.as_deref(),
            runtime_session_id.as_deref(),
        );
        match mode {
            ReapMode::FailClosedRestore { conflict_message } => {
                // Teardown was NOT verified: restore the unrecovered handle into `reg.live`
                // (preserving `sandbox_plan`, with `thread: None`) before `StartSlotGuard`
                // drops so subsequent retries continue to fail closed until manual recovery clears it.
                mgr.registry
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .live
                    .insert(employee_id.to_owned(), handle);
                return Err(ApiError::new(ErrorCode::Conflict, conflict_message));
            }
            ReapMode::ShutdownDrain => {
                return Ok(ReapedHandle {
                    view,
                    permit_released: false,
                });
            }
        }
    }
    Ok(ReapedHandle {
        view,
        permit_released,
    })
}

/// Removes an idle, permit-free, matching-`run_id` warm handle from `mgr.registry`, stops its
/// pump, reaps it fail-closed on a tracked background worker, and triggers `process_due_wakes_for`
/// once teardown is verified (`TASK-166`, `TASK-160`).
pub(crate) fn recycle_stale_warm_handle_in(
    mgr: &Arc<SessionManager>,
    employee_id: &str,
    expected_run_id: &str,
    reason: &str,
) -> bool {
    let removed = {
        let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
        if reg.draining || reg.shutting_down || reg.starting.contains(employee_id) {
            return false;
        }
        let still_recyclable = reg.live.get(employee_id).is_some_and(|handle| {
            let permit_free = handle
                .active_permit
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .is_none();
            let st = handle.state.lock().unwrap_or_else(|p| p.into_inner());
            st.run_id.as_deref() == Some(expected_run_id)
                && st.state == SessionState::Idle
                && !st.recovery_required
                && permit_free
        });
        if !still_recyclable {
            return false;
        }
        let Some(handle) = reg.live.remove(employee_id) else {
            return false;
        };
        reg.starting.insert(employee_id.to_owned());
        reg.bg_workers.retain(|h| !h.is_finished());
        reg.bg_spawning += 1;
        let _ = handle.tx.send(PumpCommand::Stop {
            reason: reason.to_owned(),
        });
        handle
    };
    let emp = employee_id.to_owned();
    let slot = Arc::new(Mutex::new(Some(removed)));
    let worker_slot = Arc::clone(&slot);
    let spawned = spawn_bg_worker_reserved(mgr, "org-warm-recycle", move |m| {
        let mut reaped_cleanly = true;
        {
            let _start_guard = StartSlotGuard::new(&m.registry, emp.clone());
            if let Some(old) = worker_slot.lock().unwrap_or_else(|p| p.into_inner()).take() {
                reaped_cleanly = reap_live_handle(
                    &m,
                    &emp,
                    old,
                    None,
                    ReapMode::FailClosedRestore {
                        conflict_message: format!(
                            "could not verify teardown of recycled warm session `{emp}`; resolve recovery first"
                        ),
                    },
                )
                .is_ok();
            }
        }
        if reaped_cleanly {
            let _ = m.process_due_wakes_for(&emp);
        }
    });
    if !spawned {
        let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(old) = slot.lock().unwrap_or_else(|p| p.into_inner()).take() {
            reg.live.insert(employee_id.to_owned(), old);
        }
        reg.starting.remove(employee_id);
        return false;
    }
    true
}
