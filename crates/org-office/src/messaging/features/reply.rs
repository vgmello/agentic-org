//! Replying to a message: the reply target, the DM peer check and marking the parent read.

use crate::messaging::{
    ports::{MessagingRead, MessagingWrite},
    records::{ConversationRecord, SenderKind},
};
use crate::organization::records::Employee;
use org_proto::chat::{ChatSendParams, ConversationKind};
use org_proto::msg::{
    json_string_len, MessageLifecycleState, MsgReplyParams, KIND_DELIVERY_FAILED, KIND_REPLY,
    MAX_ID_BYTES,
};
use org_proto::ErrorCode;
use org_store::{Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};

use super::channels::effective_auto_members;
use super::conversations::all_conversations_including_auto;
use super::recipients::upsert_participant_snapshot;
use super::send::{decide_chat_send, ChatSendCommitted};
use super::{is_active_employee, refusal};

fn ensure_dm_peer_active(
    conv: &ConversationRecord,
    caller: &Id,
    employees: &[Employee],
) -> Result<(), Refusal> {
    if conv.kind != ConversationKind::Dm {
        return Ok(());
    }
    let mut has_active_peer = false;
    for m in conv
        .memberships
        .iter()
        .filter(|m| m.left_at.is_none() && m.employee_id != *caller)
    {
        if !employees
            .iter()
            .any(|e| e.id == m.employee_id && is_active_employee(e))
        {
            return Err(refusal(
                ErrorCode::UnknownRecipient,
                "recipient is not active",
            ));
        }
        has_active_peer = true;
    }
    if !has_active_peer {
        return Err(refusal(
            ErrorCode::UnknownRecipient,
            "recipient is not active",
        ));
    }
    Ok(())
}

fn resolve_reply_target_conv(
    view: &impl MessagingRead,
    owner: &Id,
    caller: &Id,
    to_id: &str,
    now_utc: &str,
) -> Result<ConversationRecord, Refusal> {
    if json_string_len(to_id) > MAX_ID_BYTES {
        return Err(refusal(
            ErrorCode::TooLarge,
            "reply target id exceeds size limit",
        ));
    }
    let parent = view
        .message(to_id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read reply target"))?
        .ok_or_else(|| refusal(ErrorCode::NotFound, "message not found"))?;
    let conv = match view
        .conversation(&parent.conversation_id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read conversation"))?
    {
        Some(conv) => conv,
        None => all_conversations_including_auto(view, now_utc)
            .map_err(|_| refusal(ErrorCode::StorageError, "failed to load conversations"))?
            .into_iter()
            .find(|c| c.id == parent.conversation_id)
            .ok_or_else(|| refusal(ErrorCode::NotFound, "message not found"))?,
    };
    let employees = view
        .employees()
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read employees"))?;
    if !employees
        .iter()
        .any(|e| e.id == *caller && is_active_employee(e))
    {
        return Err(refusal(
            ErrorCode::NotBound,
            "sender is not an active employee",
        ));
    }
    let active_members = effective_auto_members(view, &conv, owner)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to resolve members"))?;
    if !active_members.contains(caller) && *caller != *owner {
        return Err(refusal(ErrorCode::NotFound, "message not found"));
    }
    if parent.kind == KIND_DELIVERY_FAILED || parent.sender_kind == SenderKind::Office {
        return Err(refusal(
            ErrorCode::InvalidRequest,
            "cannot reply to an office or delivery_failed notice",
        ));
    }
    ensure_dm_peer_active(&conv, caller, &employees)?;
    Ok(conv)
}

fn mark_reply_parent_read(
    view: &impl MessagingRead,
    org_id: &Id,
    target: (&Id, &Id, &str),
    org_seq: u64,
    now_utc: &str,
    change: Change,
) -> Result<Change, Refusal> {
    let (conv_id, caller, to_id) = target;
    let Some(mut parent_state) = view
        .recipient_state(to_id, caller)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read recipient state"))?
    else {
        return Ok(change);
    };
    if !matches!(
        parent_state.state,
        MessageLifecycleState::Unread | MessageLifecycleState::Snoozed
    ) {
        return Ok(change);
    }
    parent_state.state = MessageLifecycleState::Read;
    parent_state.snoozed_until = None;
    parent_state.snooze_after_task = false;
    parent_state.snooze_note = None;
    parent_state.triaged_at = Some(now_utc.to_owned());
    if parent_state.read_at.is_none() {
        parent_state.read_at = Some(now_utc.to_owned());
    }
    parent_state.updated_at = now_utc.to_owned();
    parent_state.org_sequence = org_seq;
    upsert_participant_snapshot(view, org_id, conv_id, caller, parent_state, change)
}

/// Native conversation-aware `msg.reply`: resolves `params.to_id` in the caller's visible
/// conversation (DM, GroupDM, or Channel), commits a threaded reply through [`decide_chat_send`]
/// (enforcing HR access, mentions, idempotency, and cross-office relay), and atomically marks the
/// answered message `Read` for `caller` when it was `Unread` or `Snoozed`.
pub fn decide_chat_reply<E>(
    uow: &UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    caller: &Id,
    params: &MsgReplyParams,
    org_seq: u64,
    now_utc: &str,
) -> Result<(Change, ChatSendCommitted), Refusal>
where
    E: Engine,
    E::Tx: MessagingWrite,
{
    let conv = resolve_reply_target_conv(uow.view(), owner, caller, &params.to_id, now_utc)?;
    let send_params = ChatSendParams {
        id: params.id.clone(),
        to: conv.id.as_str().to_owned(),
        kind: KIND_REPLY.to_owned(),
        priority: params.priority,
        subject: params.subject.clone(),
        body: params.body.clone(),
        meta: params.meta.clone(),
        reply_to: Some(params.to_id.clone()),
        also_in_channel: false,
        reason: None,
    };
    let (mut change, committed) =
        decide_chat_send(uow, here, owner, caller, &send_params, org_seq, now_utc)?;
    if committed.inserted {
        change = mark_reply_parent_read(
            uow.view(),
            &here.organization_id,
            (&conv.id, caller, &params.to_id),
            org_seq,
            now_utc,
            change,
        )?;
    }
    Ok((change, committed))
}
