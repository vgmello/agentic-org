//! Registry admission checks shared by session start, stop, and recovery clear.

use std::sync::Mutex;

use org_proto::sessions::SessionState;
use org_proto::{ApiError, ErrorCode};

use crate::pump::SessionRuntimeState;

use super::SessionRegistry;

/// The refusal of a start, warm send or relaunch that an explicit stop (pause, offboard, owner
/// stop) overtook after its start slot was reserved (`TASK-190`).
pub(super) const STOPPED_WHILE_STARTING: &str = "agent session was stopped while it was starting";

pub(super) fn stopped_while_starting() -> ApiError {
    ApiError::new(ErrorCode::Conflict, STOPPED_WHILE_STARTING)
}

/// `true` once an explicit stop moved `emp_id`'s stop epoch past `start_stop_epoch`.
pub(super) fn stopped_since(
    registry: &Mutex<SessionRegistry>,
    emp_id: &str,
    start_stop_epoch: u64,
) -> bool {
    registry
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .stop_epoch(emp_id)
        != start_stop_epoch
}

/// An early fence that skips route admission and spawning once a stop overtook the start.
/// Registration re-checks the epoch atomically, so this is never the only check.
pub(super) fn ensure_not_stopped_since(
    registry: &Mutex<SessionRegistry>,
    emp_id: &str,
    start_stop_epoch: u64,
) -> Result<(), ApiError> {
    if stopped_since(registry, emp_id, start_stop_epoch) {
        return Err(stopped_while_starting());
    }
    Ok(())
}

pub(super) fn check_registry_startable(
    reg: &SessionRegistry,
    emp_id: &str,
) -> Result<(), ApiError> {
    if reg.draining || reg.shutting_down {
        return Err(ApiError::new(
            ErrorCode::Unavailable,
            "office session manager is shutting down",
        ));
    }
    if reg.starting.contains(emp_id) {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "agent session start is already in progress",
        ));
    }
    Ok(())
}

pub(super) fn ensure_live_handle_not_recovery_required(
    state: &SessionRuntimeState,
) -> Result<(), ApiError> {
    if state.recovery_required || state.state == SessionState::RecoveryRequired {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "agent session requires manual recovery before starting",
        ));
    }
    Ok(())
}

pub(super) struct BgSpawnGuard<'a> {
    pub(super) registry: &'a Mutex<SessionRegistry>,
}

impl Drop for BgSpawnGuard<'_> {
    fn drop(&mut self) {
        let mut reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        reg.bg_spawning = reg.bg_spawning.saturating_sub(1);
    }
}

pub(crate) struct StartSlotGuard<'a> {
    registry: &'a Mutex<SessionRegistry>,
    employee_id: String,
    armed: bool,
}

impl<'a> StartSlotGuard<'a> {
    pub(crate) fn new(registry: &'a Mutex<SessionRegistry>, employee_id: String) -> Self {
        Self {
            registry,
            employee_id,
            armed: true,
        }
    }

    /// Releases the slot inside the `registry` critical section the caller already holds, so
    /// registering a handle and releasing its start slot are one atomic step (`TASK-190`).
    pub(crate) fn release_locked(mut self, reg: &mut SessionRegistry) {
        reg.starting.remove(&self.employee_id);
        self.armed = false;
    }

    /// Keeps the slot reserved for the next owner (a warm send handing over to a cold start).
    pub(crate) fn keep_reserved(mut self) {
        self.armed = false;
    }
}

impl Drop for StartSlotGuard<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        reg.starting.remove(&self.employee_id);
    }
}
