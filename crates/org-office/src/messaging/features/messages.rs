//! Reading messages: message views, wire headers and channel history for a caller.

use org_proto::chat::{
    ChatMessageView, ChatMessagesParams, ChatMessagesResult, ConversationKind,
    HandoffAcceptanceView, ReactionView,
};
use org_proto::msg::{
    is_urgent, json_string_len, MessageLifecycleState, DEFAULT_PAGE_LIMIT, KIND_MSG, KIND_REPLY,
    MAX_PAGE_LIMIT,
};
use org_proto::{ApiError, ErrorCode};
use org_store::Id;

use crate::messaging::{
    ports::MessagingRead,
    records::{ConversationRecord, MessageRecord, RecipientStateRecord},
};

use super::auto_channels::effective_auto_members;
use super::conversations::{all_conversations_including_auto, find_channel};

fn resolve_messages_target(
    read: &impl MessagingRead,
    owner: &Id,
    caller: &Id,
    params: &ChatMessagesParams,
    now_utc: &str,
) -> Result<ConversationRecord, ApiError> {
    let all = all_conversations_including_auto(read, now_utc)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to load conversations"))?;
    let conv = if let Some(ref_str) = &params.conversation {
        if let Ok(cid) = Id::parse(ref_str.trim()) {
            all.iter().find(|c| c.id == cid).cloned()
        } else {
            find_channel(&all, ref_str)
        }
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "conversation not found"))?
    } else if let Some(tid) = &params.thread_id {
        let msg = read
            .message(tid)
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read thread"))?
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "thread not found"))?;
        all.iter()
            .find(|c| c.id == msg.conversation_id)
            .cloned()
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "conversation not found"))?
    } else {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "pass --conversation or --thread",
        ));
    };
    let active = effective_auto_members(read, &conv, owner)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to resolve members"))?;
    let allowed =
        conv.kind == ConversationKind::Channel || active.contains(caller) || caller == owner;
    if !allowed {
        return Err(ApiError::new(ErrorCode::NotFound, "conversation not found"));
    }
    Ok(conv)
}

fn build_chat_message_view(
    read: &impl MessagingRead,
    m: MessageRecord,
    thread_id: String,
    local_seq: u64,
) -> Result<ChatMessageView, ApiError> {
    let reactions = read
        .reactions_for_message(&m.id)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read reactions"))?
        .into_iter()
        .map(|r| ReactionView {
            employee_id: r.employee_id.as_str().to_owned(),
            reaction: r.reaction,
            created_at: r.created_at,
        })
        .collect();
    let answer_file = read
        .answer_for_message(&m.id)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read acceptance"))?;
    let acceptance = answer_file.as_ref().and_then(|a| {
        Some(HandoffAcceptanceView {
            accepted_by: a.accepted_by.as_ref()?.as_str().to_owned(),
            accepted_at: a.accepted_at.clone()?,
            reason: a.reason.clone(),
        })
    });
    let (answer, withdrawal) = answer_file
        .as_ref()
        .map(crate::messaging::ask_card::decode_ask_answer_file)
        .unwrap_or((None, None));
    let mentions = m
        .metadata
        .get("mentions")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Ok(ChatMessageView {
        id: m.id,
        conversation_id: m.conversation_id.as_str().to_owned(),
        thread_id,
        reply_to: m.reply_to_message_id,
        also_in_channel: m.also_in_channel,
        sender_kind: m.sender_kind.as_str().to_owned(),
        sender_employee_id: m.sender_employee_id.map(|i| i.as_str().to_owned()),
        sender_display_name: m.sender_display_name,
        kind: m.kind,
        priority: m.priority,
        subject: m.subject,
        body: m.body_markdown,
        meta: m.metadata,
        created_at: m.created_at,
        origin_sequence: local_seq,
        mentions,
        reactions,
        acceptance,
        answer,
        withdrawal,
    })
}

pub fn list_chat_messages(
    read: &impl MessagingRead,
    owner: &Id,
    caller: &Id,
    params: &ChatMessagesParams,
    now_utc: &str,
) -> Result<ChatMessagesResult, ApiError> {
    let limit = params.limit.unwrap_or(DEFAULT_PAGE_LIMIT);
    if limit == 0 || limit > MAX_PAGE_LIMIT {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "limit must be 1..=100",
        ));
    }
    let conv = resolve_messages_target(read, owner, caller, params, now_utc)?;
    let raw_msgs = read
        .messages_in_conversation(&conv.id)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read messages"))?;
    let mut views = Vec::new();
    for m in raw_msgs {
        let thread_id = m.thread_root_id.clone().unwrap_or_else(|| m.id.clone());
        if let Some(tid) = &params.thread_id {
            if thread_id != *tid {
                continue;
            }
        }
        let local_seq = read
            .message_local_seq(&m.id)
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read local_seq"))?
            .unwrap_or(m.origin_sequence);
        if let Some(cursor) = params.cursor {
            if local_seq >= cursor {
                continue;
            }
        }
        views.push(build_chat_message_view(read, m, thread_id, local_seq)?);
    }
    let more = views.len() > limit as usize;
    if more {
        let skip = views.len() - limit as usize;
        views = views.split_off(skip);
    }
    let next_cursor = if more {
        views.first().map(|m| m.origin_sequence)
    } else {
        None
    };
    Ok(ChatMessagesResult {
        conversation_id: conv.id.as_str().to_owned(),
        messages: views,
        next_cursor,
    })
}

/// Converts a `MessageRecord` + optional `RecipientStateRecord` into an `org_proto::msg::MessageHeader`.
pub fn to_wire_header(
    msg: &MessageRecord,
    state: Option<&RecipientStateRecord>,
    local_seq: u64,
) -> org_proto::msg::MessageHeader {
    let from_member_id = msg
        .sender_employee_id
        .as_ref()
        .map(|i| i.as_str().to_owned())
        .unwrap_or_else(|| org_proto::msg::OFFICE_SENDER.to_owned());
    let to_member_id = state
        .map(|s| s.employee_id.as_str().to_owned())
        .unwrap_or_else(|| msg.conversation_id.as_str().to_owned());
    let to_display_name = state
        .and_then(|s| s.recipient_display_name.clone())
        .unwrap_or_else(|| to_member_id.clone());
    let meta_bytes = if msg.metadata.is_empty() {
        0
    } else {
        serde_json::to_vec(&msg.metadata)
            .map(|b| b.len() as u64)
            .unwrap_or(0)
    };
    org_proto::msg::MessageHeader {
        id: msg.id.clone(),
        thread_id: msg.thread_root_id.clone().unwrap_or_else(|| msg.id.clone()),
        reply_to: msg.reply_to_message_id.clone(),
        from_member_id,
        from_display_name: msg.sender_display_name.clone(),
        to_member_id,
        to_display_name,
        kind: msg.kind.clone(),
        priority: msg.priority,
        urgent: is_urgent(&msg.kind, msg.priority),
        subject: msg.subject.clone(),
        created_at: msg.created_at.clone(),
        expires_at: msg.expires_at.clone(),
        ttl_ms: msg.ttl_ms,
        body_bytes: json_string_len(&msg.body_markdown) as u64,
        meta_bytes,
        local_seq,
        state: state.map_or(MessageLifecycleState::Unread, |s| s.state),
        triage_gen: state.map_or(0, |s| s.triage_generation),
        snoozed_until: state.and_then(|s| s.snoozed_until.clone()),
        snooze_after_task: state.is_some_and(|s| s.snooze_after_task),
        snooze_note: state.and_then(|s| s.snooze_note.clone()),
        read_at: state.and_then(|s| s.read_at.clone()),
        expired_waiting_person: state.is_some_and(|s| s.expired_waiting_person),
    }
}

/// Ensures `msg.kind == KIND_REPLY` or valid DM reply helper for `org-mailbox` integration.
pub fn is_reply_kind(kind: &str) -> bool {
    kind == KIND_REPLY || kind == KIND_MSG
}

/// Returns the chronological latest `limit` (capped at 200) messages in a channel that `caller`
/// is permitted to read — identical visibility to `chat.messages` / `msg list` (`WATCH-10`).
pub fn channel_messages_for_caller(
    read: &impl MessagingRead,
    owner: &Id,
    caller: &Id,
    channel_ref: &str,
    limit: usize,
    now_utc: &str,
) -> Result<(ConversationRecord, Vec<MessageRecord>), ApiError> {
    let params = ChatMessagesParams {
        conversation: Some(channel_ref.to_owned()),
        thread_id: None,
        cursor: None,
        limit: Some(100),
    };
    let conv = resolve_messages_target(read, owner, caller, &params, now_utc)?;
    if conv.kind != ConversationKind::Channel {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "channel summarize only operates on channels",
        ));
    }
    let mut msgs = read
        .messages_in_conversation(&conv.id)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read channel messages"))?;
    let cap = limit.clamp(1, 200);
    if msgs.len() > cap {
        let skip = msgs.len() - cap;
        msgs = msgs.split_off(skip);
    }
    Ok((conv, msgs))
}
