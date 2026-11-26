//! The built-in Chief of Staff's sessions (`TASK-80.1`): one partition per caller
//! (`tess:ask:<caller_id>`), so a member's turn never lands in another member's conversation,
//! and the caller her run acts for, recorded server-side in its run binding for the
//! `caller ∩ tess` check on every call she makes.
//!
//! The office keeps one live process per agent. A turn for the partition the live process
//! already serves goes to it warm; a turn for another partition (or a partition due to rotate)
//! first stops the idle process, after recording which conversation it held, and starts the
//! new partition's process resuming that partition's own conversation. A process busy with
//! another caller's turn is never interrupted: the request is refused with `conflict`.

use org_office::organization_family::records::{BuiltinKey, Employee};
use org_proto::sessions::SessionState;
use org_proto::{ApiError, ErrorCode};

use super::SessionManager;
use crate::partition::{PartitionKey, PartitionTurn};

/// The reason a partition switch gives the process it stops.
const SWITCH_REASON: &str = "chief_of_staff_partition_switch";

impl SessionManager {
    /// Enters the partition of a turn of `employee` acting for `caller_id` (`None`: for
    /// nobody). Answers `None` for anyone but the Chief of Staff. For her: stops an idle
    /// process that serves another partition (or one due to rotate), names the caller the next
    /// run acts for, counts the turn, and answers the conversation a cold start resumes.
    pub(crate) fn enter_partition(
        &self,
        employee: &Employee,
        caller_id: Option<&str>,
    ) -> Result<Option<PartitionTurn>, ApiError> {
        if employee.builtin_key != Some(BuiltinKey::ChiefOfStaff) {
            return Ok(None);
        }
        let emp_id = employee.id.as_str();
        let key = PartitionKey::for_caller(caller_id);
        if let Some((state, runtime_session_id)) = self.live_partition_snapshot(emp_id) {
            let serving =
                PartitionKey::for_caller(self.bindings.acting_for_caller_id(emp_id).as_deref());
            let switching = serving != key || self.partitions.needs_rotation(emp_id, &key);
            if switching {
                if !matches!(state, SessionState::Idle | SessionState::Stopped) {
                    return Err(ApiError::new(
                        ErrorCode::Conflict,
                        "the Chief of Staff is answering another request; try again shortly",
                    ));
                }
                if let Some(conversation) = runtime_session_id {
                    self.partitions
                        .record_runtime_session(emp_id, &serving, &conversation);
                }
                self.stop_session(emp_id, SWITCH_REASON)?;
            }
        }
        // Taken by the next run's binding alone: a warm turn keeps its run's caller, which is
        // this caller, since a switch stopped any other.
        self.bindings.delegate_next_run(emp_id, caller_id);
        Ok(Some(self.partitions.begin_turn(emp_id, &key)))
    }

    /// The live process's state and runtime conversation, read under a brief lock.
    fn live_partition_snapshot(&self, emp_id: &str) -> Option<(SessionState, Option<String>)> {
        let reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        let handle = reg.live.get(emp_id)?;
        let state = handle.state.lock().unwrap_or_else(|p| p.into_inner());
        Some((state.state, state.runtime_session_id.clone()))
    }
}
