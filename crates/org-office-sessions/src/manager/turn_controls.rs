//! A person's live turn controls (`TASK-103`): interrupt an agent's running turn
//! (`member.session_interrupt`) or steer it mid-turn (`member.session_steer`), both keeping the
//! warm process alive. The pump does the work ([`crate::pump::PumpCommand::InterruptTurn`],
//! [`crate::pump::PumpCommand::SteerTurn`]); these only find the live session, refuse one that
//! is locked or ending, and hand the request over.
//!
//! Callers authorize first ([`super::rpc`]): the owner or an active person, and only the owner
//! on the Chief of Staff's sessions. Agents are refused.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use org_proto::sessions::{SessionState, SessionView, MAX_STEER_TEXT_BYTES};
use org_proto::{ApiError, ErrorCode};

use crate::credentials::{now_ms, random_hex};
use crate::pump::{queue_steer, PumpCommand, SessionRuntimeState, SteerRequest};
use crate::wake::{compose_person_steer, PendingWakeItem, WakeKind};

use super::SessionManager;

type LiveTarget = (Arc<Mutex<SessionRuntimeState>>, Sender<PumpCommand>);

impl SessionManager {
    /// Interrupts `employee_id`'s running turn: the turn stops, the process lives on, turns
    /// queued behind it still run, and the interrupted turn never fails over. An idle live
    /// session is left as it is. `Conflict` when the agent has no live session (or it is
    /// locked or ending).
    pub fn interrupt_session(&self, employee_id: &str) -> Result<SessionView, ApiError> {
        self.require_agent(employee_id)?;
        let (state, tx) = self.live_turn_target(employee_id, "interrupt")?;
        if tx.send(PumpCommand::InterruptTurn).is_err() {
            return Err(session_ended());
        }
        let view = state.lock().unwrap_or_else(|p| p.into_inner()).to_view();
        Ok(view)
    }

    /// Steers `employee_id`'s running turn with `text` from `caller_id` (a person): delivered
    /// natively where the runtime steers, otherwise the turn is interrupted and the steer runs
    /// next, ahead of queued turns; an idle live session runs it as its next turn.
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
        validate_steer_text(text)?;
        self.require_agent(employee_id)?;
        let request = self.compose_steer(caller_id, hq_attested_owner, text)?;
        let (state, tx) = self.live_turn_target(employee_id, "steer")?;
        if !queue_steer(&state, &tx, request) {
            return Err(session_ended());
        }
        let view = state.lock().unwrap_or_else(|p| p.into_inner()).to_view();
        Ok(view)
    }

    /// The live session a control goes to, refused when there is none or it is locked or
    /// ending. The registry lock is released before the state lock is taken.
    fn live_turn_target(&self, employee_id: &str, verb: &str) -> Result<LiveTarget, ApiError> {
        let (state, tx) = {
            let reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
            let Some(handle) = reg.live.get(employee_id) else {
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    format!("the agent has no live session to {verb}"),
                ));
            };
            (Arc::clone(&handle.state), handle.tx.clone())
        };
        {
            let st = state.lock().unwrap_or_else(|p| p.into_inner());
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
        }
        Ok((state, tx))
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
