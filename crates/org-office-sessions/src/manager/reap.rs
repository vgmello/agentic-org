//! The single fail-closed teardown choke-point for a removed live session handle (`AE-07`,
//! `TASK-166`, rules §2): stop, crash, timeout, pre-pump abort, sibling refresh, recovery clear,
//! pool failover and shutdown all reap through [`reap_live_handle`].

use org_proto::sessions::SessionState;
use org_proto::{ApiError, ErrorCode};

use crate::pump::PumpCommand;

use super::{LiveSessionHandle, SessionManager};

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
