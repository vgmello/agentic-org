//! The built-in Chief of Staff's sessions (`TASK-80.1`): one partition per caller
//! (`tess:ask:<caller_id>`), so a member's turn never lands in another member's conversation,
//! and the caller her run acts for, recorded server-side in its run binding for the
//! `caller ∩ tess` check on every call she makes.
//!
//! The office keeps one live process per agent, and that process serves one partition, named on
//! its live handle. Whether a turn may use it is decided in the **same** `registry` critical
//! section that reserves the start slot or picks the warm send ([`partition_switch_reason`],
//! called from `decide_start_or_send`), so no second caller can slip between the check and the
//! reservation:
//!
//! - a turn for the partition the live process serves goes to it warm;
//! - a turn for another partition (or one due to rotate) stops an idle process, after recording
//!   which conversation it held, and starts the new partition's process resuming that
//!   partition's own conversation;
//! - a process busy with another partition's turn, or a start already in flight, is never
//!   interrupted: the request is refused with `conflict` before anything changes.
//!
//! The caller a run acts for travels with its launch (the partition key on the planned launch,
//! the run binding, the live handle, the pump and any failover of the run): there is no
//! out-of-band "next run" record another start could overwrite or take.

use org_office::organization_family::records::{BuiltinKey, Employee};
use org_proto::sessions::SessionState;
use org_proto::{ApiError, ErrorCode};

use super::{LiveSessionHandle, SessionManager};
use crate::partition::{PartitionKey, PartitionLedger};
use crate::pump::SessionRuntimeState;

/// The reason a partition switch gives the process it stops.
pub(super) const SWITCH_REASON: &str = "chief_of_staff_partition_switch";

impl SessionManager {
    /// The partition a turn of `employee` acting for `caller_id` (`None`: for nobody) runs in;
    /// `None` for anyone but the Chief of Staff, whose turns are not partitioned.
    pub(crate) fn turn_partition(
        employee: &Employee,
        caller_id: Option<&str>,
    ) -> Option<PartitionKey> {
        (employee.builtin_key == Some(BuiltinKey::ChiefOfStaff))
            .then(|| PartitionKey::for_caller(caller_id))
    }
}

/// Why the Chief of Staff's live process must stop before a turn of partition `key`, decided
/// under the `registry` lock (and `state`, the handle's state lock) by the start that is about
/// to reserve the slot: `None` when the process already serves `key` and needs no rotation.
/// A process that is not idle is never stopped for another partition: `Conflict`, with
/// nothing changed. Before a switch, the conversation the process holds is recorded for its
/// own partition, so that partition's next turn resumes it.
pub(super) fn partition_switch_reason(
    partitions: &PartitionLedger,
    emp_id: &str,
    handle: &LiveSessionHandle,
    state: &SessionRuntimeState,
    key: &PartitionKey,
) -> Result<Option<&'static str>, ApiError> {
    let serving = handle.partition.as_ref();
    if serving == Some(key) && !partitions.needs_rotation(emp_id, key) {
        return Ok(None);
    }
    if !matches!(
        state.state,
        SessionState::Idle | SessionState::Stopped | SessionState::Suspended
    ) {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "the Chief of Staff is answering another request; try again shortly",
        ));
    }
    if let (Some(serving), Some(conversation)) = (serving, state.runtime_session_id.as_deref()) {
        partitions.record_runtime_session(emp_id, serving, conversation);
    }
    Ok(Some(SWITCH_REASON))
}
