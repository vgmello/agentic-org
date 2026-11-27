//! Accepting a handoff request.

use org_proto::chat::{MsgAcceptParams, MsgAcceptResult};
use org_proto::msg::{json_string_len, MessageLifecycleState, MAX_NOTE_BYTES};
use org_proto::ErrorCode;
use org_store::{Ack, Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};

use crate::messaging::{
    ports::{MessagingRead, MessagingWrite},
    records::{self, MessageRecord, ParticipantAnswerFile, RecipientStateRecord, SCHEMA_VERSION},
};

use super::recipients::upsert_participant_snapshot;
use super::{put_or_create_file, refusal};

fn load_handoff_target(
    read: &impl MessagingRead,
    caller: &Id,
    params: &MsgAcceptParams,
) -> Result<(MessageRecord, RecipientStateRecord), Refusal> {
    if let Some(reason) = &params.reason {
        if json_string_len(reason) > MAX_NOTE_BYTES {
            return Err(refusal(ErrorCode::TooLarge, "reason exceeds size limit"));
        }
    }
    let msg = read
        .message(&params.id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read message"))?
        .ok_or_else(|| refusal(ErrorCode::NotFound, "message not found"))?;
    let state = read
        .recipient_state(&msg.id, caller)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read recipient state"))?
        .ok_or_else(|| {
            refusal(
                ErrorCode::NotFound,
                "handoff message is not addressed to caller",
            )
        })?;
    if msg.kind != "handoff" {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            "msg.accept only applies to handoff messages",
        ));
    }
    Ok((msg, state))
}

pub fn decide_accept_handoff<E>(
    uow: &UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    caller: &Id,
    params: &MsgAcceptParams,
    org_seq: u64,
    now_utc: &str,
) -> Result<(Change, MsgAcceptResult), Refusal>
where
    E: Engine,
    E::Tx: MessagingWrite,
{
    let (msg, mut updated_state) = load_handoff_target(uow.view(), caller, params)?;
    if let Some(existing) = uow
        .view()
        .answer_for_message(&msg.id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read acceptance"))?
    {
        if existing.accepted_by.as_ref() == Some(caller) {
            return Ok((
                Change::new(Ack::Committed),
                MsgAcceptResult {
                    id: msg.id,
                    accepted_by: caller.as_str().to_owned(),
                    accepted_at: existing.accepted_at.unwrap_or_else(|| now_utc.to_owned()),
                    reason: existing.reason,
                },
            ));
        }
        return Err(refusal(
            ErrorCode::Conflict,
            "handoff was already accepted by another member",
        ));
    }
    let answer = ParticipantAnswerFile {
        schema_version: SCHEMA_VERSION,
        revision: 1,
        organization_id: here.organization_id.clone(),
        conversation_id: msg.conversation_id.clone(),
        employee_id: caller.clone(),
        message_id: msg.id.clone(),
        accepted_by: Some(caller.clone()),
        accepted_at: Some(now_utc.to_owned()),
        reason: params.reason.clone(),
        answer: None,
        attachments: Vec::new(),
    };
    let ans_path = records::participant_answer_path(&msg.conversation_id, caller, &msg.id);
    let change = put_or_create_file(
        uow,
        Change::new(Ack::Committed),
        ans_path,
        records::encode_json(&answer),
    )?;
    updated_state.state = MessageLifecycleState::Read;
    if updated_state.read_at.is_none() {
        updated_state.read_at = Some(now_utc.to_owned());
    }
    updated_state.updated_at = now_utc.to_owned();
    updated_state.reason = params.reason.clone();
    updated_state.org_sequence = org_seq;
    let change = upsert_participant_snapshot(
        uow.view(),
        &here.organization_id,
        &msg.conversation_id,
        caller,
        updated_state,
        change,
    )?;
    Ok((
        change,
        MsgAcceptResult {
            id: msg.id,
            accepted_by: caller.as_str().to_owned(),
            accepted_at: now_utc.to_owned(),
            reason: params.reason.clone(),
        },
    ))
}
