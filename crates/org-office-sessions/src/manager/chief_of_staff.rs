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
use org_proto::chat::ConversationKind;
use org_proto::sessions::SessionState;
use org_proto::{ApiError, ErrorCode};

use super::{LiveSessionHandle, SessionManager};
use crate::partition::{PartitionKey, PartitionLedger};
use crate::pump::SessionRuntimeState;
use crate::wake::{PendingWakeItem, WakeKind};

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

    /// The partition a queued wake `item` of `employee` runs in; `None` for anyone but the
    /// Chief of Staff. What one sender addressed to her (a DM, a hand-off, an ask), and anything
    /// else from a private conversation ([`Self::is_private_wake`]), runs in that sender's own
    /// partition: a person's `tess:ask:<id>`, acting for them, or anyone else's `tess:dm:<id>`,
    /// acting for nobody. Everything else (channel mentions and answers, assignments, recovery)
    /// runs in `tess:wake`, which so never holds a sender's private message.
    pub(crate) fn wake_partition(
        &self,
        employee: &Employee,
        item: &PendingWakeItem,
    ) -> Option<PartitionKey> {
        Self::turn_partition(employee, None)?;
        let addressed = matches!(
            item.kind,
            WakeKind::DirectMessage | WakeKind::Handoff | WakeKind::Ask
        );
        Some(if !addressed && !self.is_private_wake(item) {
            PartitionKey::tess_wake()
        } else if item.sender_is_person {
            PartitionKey::tess_ask(&item.sender_id)
        } else {
            PartitionKey::tess_dm(&item.sender_id)
        })
    }

    /// Whether wake `item` comes from a conversation only its members see: a DM or group DM
    /// (an `@mention` or an answer in one included). An answer from a conversation the office
    /// cannot name, and any wake whose conversation cannot be read, counts as private (fail
    /// closed): only a channel's answer reaches `tess:wake`.
    fn is_private_wake(&self, item: &PendingWakeItem) -> bool {
        let answer = matches!(item.kind, WakeKind::AskAnswer | WakeKind::WatcherAnswer);
        let Some(conversation) = item.conversation_id.as_deref() else {
            return answer;
        };
        match self.office.conversation_kind(conversation) {
            Ok(Some(kind)) => kind != ConversationKind::Channel,
            Ok(None) => answer,
            Err(_) => true,
        }
    }

    /// Claims of `items` only those of the first item's partition, and names it: items of
    /// different partitions never share a turn, and the rest stay queued for their own. Anyone
    /// but the Chief of Staff claims them all.
    pub(crate) fn claim_one_partition(
        &self,
        employee: &Employee,
        items: Vec<PendingWakeItem>,
    ) -> (Vec<PendingWakeItem>, Option<PartitionKey>) {
        let Some(partition) = items
            .first()
            .and_then(|first| self.wake_partition(employee, first))
        else {
            return (items, None);
        };
        let claimed = items
            .into_iter()
            .filter(|item| self.wake_partition(employee, item).as_ref() == Some(&partition))
            .collect();
        (claimed, Some(partition))
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
