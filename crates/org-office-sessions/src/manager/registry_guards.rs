//! Registry admission checks shared by session start, stop, and recovery clear.

use org_proto::sessions::SessionState;
use org_proto::{ApiError, ErrorCode};

use crate::pump::SessionRuntimeState;

use super::SessionRegistry;

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
