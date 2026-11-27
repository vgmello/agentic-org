//! A person's live turn controls (`TASK-103`): interrupt an agent's running turn
//! (`member.session_interrupt`) or steer it mid-turn (`member.session_steer`), both keeping the
//! warm process alive. The pump does the work ([`crate::pump::PumpCommand::InterruptTurn`],
//! [`crate::pump::PumpCommand::SteerTurn`]); these only find the live session, refuse one that
//! is locked or ending, and hand the request over.
//!
//! Interrupt callers authorize first ([`super::rpc`]): the owner or an active person, and only
//! the owner on the Chief of Staff's sessions. Agents are refused. A steer carries its caller,
//! so [`SessionManager::steer_session`] checks the same itself (one choke-point for every
//! caller), and on the Chief of Staff it reaches only a turn of the caller's own partition
//! (`tess:ask:<caller_id>`): never another member's conversation or `tess:wake`.

use std::sync::MutexGuard;

use org_proto::sessions::{SessionState, SessionView, MAX_STEER_TEXT_BYTES};
use org_proto::{ApiError, ErrorCode};

use crate::credentials::{now_ms, random_hex};
use crate::partition::PartitionKey;
use crate::pump::{queue_steer, PumpCommand, SteerRequest};
use crate::wake::{compose_person_steer, PendingWakeItem, WakeKind};

use super::{LiveSessionHandle, SessionManager, SessionRegistry};

impl SessionManager {
    /// Interrupts `employee_id`'s running turn: the turn stops, the process lives on, turns
    /// queued behind it still run, and the interrupted turn never fails over. An idle live
    /// session is left as it is. `Conflict` when the agent has no live session (or it is
    /// locked or ending).
    pub fn interrupt_session(&self, employee_id: &str) -> Result<SessionView, ApiError> {
        self.require_agent(employee_id)?;
        let reg = self.lock_registry();
        let handle = self.live_turn_target(&reg, employee_id, "interrupt", None)?;
        if handle.tx.send(PumpCommand::InterruptTurn).is_err() {
            return Err(session_ended());
        }
        let view = handle
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .to_view();
        Ok(view)
    }

    /// Interrupts `employee_id`'s running turn ONLY when the live handle is currently bound to
    /// `partition`, checked atomically under `self.registry` (zero cross-caller TOCTOU for Ask
    /// Tess `tess.turn_cancel`). Returns `Ok(true)` if an interrupt was sent, `Ok(false)` when no
    /// live session for `partition` is active.
    pub fn interrupt_partition_session(
        &self,
        employee_id: &str,
        partition: &PartitionKey,
    ) -> Result<bool, ApiError> {
        self.require_agent(employee_id)?;
        let reg = self.lock_registry();
        let Some(handle) = reg.live.get(employee_id) else {
            return Ok(false);
        };
        if handle.partition.as_ref() != Some(partition) {
            return Ok(false);
        }
        let target = match self.live_turn_target(&reg, employee_id, "interrupt", Some(partition)) {
            Ok(target) => target,
            Err(err) if err.code == ErrorCode::Conflict => return Ok(false),
            Err(err) => return Err(err),
        };
        if target.tx.send(PumpCommand::InterruptTurn).is_err() {
            return Ok(false);
        }
        Ok(true)
    }

    /// Whether `employee_id` currently has a live or starting session bound to `partition` that
    /// has not reached a terminal stopped/locked state. Used by Ask Tess event polling to detect
    /// crashed or externally stopped sessions without stranding SSE streams.
    pub fn is_partition_session_active(&self, employee_id: &str, partition: &PartitionKey) -> bool {
        let reg = self.lock_registry();
        if let Some(handle) = reg.live.get(employee_id) {
            if handle.partition.as_ref() != Some(partition) {
                return false;
            }
            let st = handle.state.lock().unwrap_or_else(|p| p.into_inner());
            return !matches!(
                st.state,
                SessionState::Stopped
                    | SessionState::Suspended
                    | SessionState::StallLocked
                    | SessionState::RecoveryRequired
            );
        }
        reg.starting.contains(employee_id)
    }

    /// Steers `employee_id`'s running turn with `text` from `caller_id` (the owner or an active
    /// person; on the Chief of Staff, the owner, and only into the caller's own partition):
    /// delivered natively where the runtime steers, otherwise the turn is interrupted and the
    /// steer runs next, ahead of queued turns; an idle live session runs it as its next turn.
    pub fn steer_session(
        &self,
        caller_id: &str,
        employee_id: &str,
        text: &str,
    ) -> Result<SessionView, ApiError> {
        self.steer_session_inner(caller_id, false, employee_id, text)
    }

    pub(super) fn steer_session_inner(
        &self,
        caller_id: &str,
        hq_attested_owner: bool,
        employee_id: &str,
        text: &str,
    ) -> Result<SessionView, ApiError> {
        self.authorize_owner_or_person_inner(caller_id, hq_attested_owner)?;
        self.authorize_chief_of_staff_target_inner(caller_id, hq_attested_owner, employee_id)?;
        validate_steer_text(text)?;
        let employee = self.require_agent(employee_id)?;
        let partition = Self::turn_partition(&employee, Some(caller_id));
        let request = self.compose_steer(caller_id, hq_attested_owner, text)?;
        // Checked and queued in one `registry` critical section, like a warm send
        // (`decide_start_or_send`): a partition switch can't slip between the two.
        let reg = self.lock_registry();
        let handle = self.live_turn_target(&reg, employee_id, "steer", partition.as_ref())?;
        if !queue_steer(&handle.state, &handle.tx, request) {
            return Err(session_ended());
        }
        let view = handle
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .to_view();
        Ok(view)
    }

    fn lock_registry(&self) -> MutexGuard<'_, SessionRegistry> {
        self.registry.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The live session a control goes to, under the caller's `registry` lock (its state lock
    /// taken after it, the order `decide_start_or_send` takes them). Refused (`Conflict`) when
    /// there is none, it is locked or ending, or, for a control bound to the Chief of Staff
    /// partition `partition`, the process serves another partition or is idle with `partition`
    /// due to rotate (its next turn starts a fresh process).
    fn live_turn_target<'r>(
        &self,
        reg: &'r SessionRegistry,
        employee_id: &str,
        verb: &str,
        partition: Option<&PartitionKey>,
    ) -> Result<&'r LiveSessionHandle, ApiError> {
        let Some(handle) = reg.live.get(employee_id) else {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                format!("the agent has no live session to {verb}"),
            ));
        };
        let st = handle.state.lock().unwrap_or_else(|p| p.into_inner());
        if st.recovery_required
            || matches!(
                st.state,
                SessionState::StallLocked
                    | SessionState::RecoveryRequired
                    | SessionState::Stopping
                    | SessionState::Stopped
                    | SessionState::Suspended
            )
        {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                format!("the agent's session is locked or ending and takes no {verb}"),
            ));
        }
        if let Some(expected) = partition {
            let rotating = st.state == SessionState::Idle
                && self.partitions.needs_rotation(employee_id, expected);
            if handle.partition.as_ref() != Some(expected) || rotating {
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    format!("the Chief of Staff is not in your conversation; no {verb} sent"),
                ));
            }
        }
        Ok(handle)
    }

    /// The steer composed under the caller's verified identity (`sender_is_person`).
    fn compose_steer(
        &self,
        caller_id: &str,
        hq_attested_owner: bool,
        text: &str,
    ) -> Result<SteerRequest, ApiError> {
        let caller = self
            .office
            .load_employee(caller_id)
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not read caller"))?;
        let is_owner = hq_attested_owner || caller_id == self.office.owner_member_id();
        let (name, handle) = caller.map_or_else(
            || (caller_id.to_owned(), None),
            |e| (e.name, Some(e.handle)),
        );
        let delivery_id = format!(
            "steer-{}",
            random_hex(8).map_err(|_| ApiError::new(
                ErrorCode::Unavailable,
                "could not allocate a steer id"
            ))?
        );
        let sender = PendingWakeItem {
            item_id: delivery_id.clone(),
            kind: WakeKind::DirectMessage,
            sender_id: caller_id.to_owned(),
            sender_name: name,
            sender_handle: handle,
            sender_relation: Some(if is_owner { "owner" } else { "person" }.to_owned()),
            sender_is_person: true,
            conversation_id: None,
            source_id: delivery_id.clone(),
            summary: text.to_owned(),
            attachment_ids: Vec::new(),
            enqueued_at_ms: now_ms(),
        };
        let (notice, turn) = compose_person_steer(&sender);
        Ok(SteerRequest {
            delivery_id,
            notice,
            turn,
        })
    }
}

/// A steer is one non-blank message body of at most [`MAX_STEER_TEXT_BYTES`].
fn validate_steer_text(text: &str) -> Result<(), ApiError> {
    if text.trim().is_empty() {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "a steer needs some text",
        ));
    }
    if text.len() > MAX_STEER_TEXT_BYTES {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!("a steer is at most {MAX_STEER_TEXT_BYTES} bytes"),
        ));
    }
    Ok(())
}

fn session_ended() -> ApiError {
    ApiError::new(
        ErrorCode::Conflict,
        "the agent's session ended; start it again",
    )
}
