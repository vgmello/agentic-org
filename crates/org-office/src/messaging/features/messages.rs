//! Reading messages: message views, wire headers and channel history for a caller.

use org_proto::chat::{
    ChatMessageView, ChatMessagesParams, ChatMessagesResult, ConversationKind,
    HandoffAcceptanceView, ReactionView,
};
use org_proto::msg::{
    is_urgent, json_string_len, MessageLifecycleState, Meta, DEFAULT_PAGE_LIMIT, KIND_MSG,
    KIND_REPLY, MAX_PAGE_LIMIT,
};
use org_proto::{ApiError, ErrorCode};
use org_store::Id;

use crate::messaging::{
    ports::MessagingRead,
    records::{ConversationRecord, MessageRecord, RecipientStateRecord},
};

use super::catchup::{delta_bounds, is_top_level, sent_by, snippet, ThreadStats};
use super::conversations::{all_conversations_including_auto, find_channel};
use crate::messaging::access::may_read;

/// Resolves a conversation UUIDv7, `#channel` or channel name the caller may read.
pub(super) fn resolve_conversation_ref(
    read: &impl MessagingRead,
    owner: &Id,
    caller: &Id,
    reference: &str,
    now_utc: &str,
) -> Result<ConversationRecord, ApiError> {
    let all = all_conversations_including_auto(read, now_utc)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to load conversations"))?;
    let trimmed = reference.trim();
    let by_id = Id::parse(trimmed)
        .ok()
        .and_then(|cid| all.iter().find(|c| c.id == cid).cloned());
    let conv = by_id
        .or_else(|| find_channel(&all, trimmed))
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "conversation not found"))?;
    require_read_access(read, owner, caller, conv)
}

fn require_read_access(
    read: &impl MessagingRead,
    owner: &Id,
    caller: &Id,
    conv: ConversationRecord,
) -> Result<ConversationRecord, ApiError> {
    let allowed = may_read(read, owner, caller, &conv)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to resolve members"))?;
    if !allowed {
        return Err(ApiError::new(ErrorCode::NotFound, "conversation not found"));
    }
    Ok(conv)
}

fn resolve_messages_target(
    read: &impl MessagingRead,
    owner: &Id,
    caller: &Id,
    params: &ChatMessagesParams,
    now_utc: &str,
) -> Result<ConversationRecord, ApiError> {
    if let Some(ref_str) = &params.conversation {
        return resolve_conversation_ref(read, owner, caller, ref_str, now_utc);
    }
    let Some(tid) = &params.thread_id else {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "pass --conversation or --thread",
        ));
    };
    let msg = read
        .message(tid)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read thread"))?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "thread not found"))?;
    let all = all_conversations_including_auto(read, now_utc)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to load conversations"))?;
    let conv = all
        .into_iter()
        .find(|c| c.id == msg.conversation_id)
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "conversation not found"))?;
    require_read_access(read, owner, caller, conv)
}

/// Refuses flag combinations a catch-up read cannot honor (`TASK-99` rung 2).
fn check_read_shape(params: &ChatMessagesParams) -> Result<(), ApiError> {
    let invalid = |msg: &str| Err(ApiError::new(ErrorCode::InvalidRequest, msg));
    if params.since_read && params.conversation.is_none() {
        return invalid("since_read needs a conversation or channel");
    }
    for (field, anchor) in [
        ("after_id", &params.after_id),
        ("page_after_id", &params.page_after_id),
    ] {
        if anchor
            .as_deref()
            .is_some_and(|id| !org_proto::is_uuid_v7(id))
        {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                format!("{field} must be a canonical lowercase UUIDv7 message ID"),
            ));
        }
    }
    if params.page_after_id.is_some() && !params.is_delta() {
        return invalid("page_after_id pages a delta read: pass since_read or after_id with it");
    }
    if params.is_delta() && params.cursor.is_some() {
        return invalid(
            "a delta read pages forward with page_after_id = next_after_id, not cursor",
        );
    }
    if params.top_level_only && params.thread_id.is_some() {
        return invalid("top_level_only lists thread roots; a thread read lists its replies");
    }
    Ok(())
}

/// How one message is shown: full, or headers-only with a snippet.
#[derive(Clone, Copy)]
struct ViewShape<'a> {
    headers_only: bool,
    threads: &'a ThreadStats,
}

fn build_chat_message_view(
    read: &impl MessagingRead,
    m: MessageRecord,
    thread_id: String,
    local_seq: u64,
    shape: ViewShape<'_>,
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
    let (reply_count, latest_reply_at) = shape.threads.of_root(&m.id);
    let (body, snippet_text, body_bytes, meta) = if shape.headers_only {
        let bytes = m.body_markdown.len() as u64;
        (
            None,
            Some(snippet(&m.body_markdown)),
            Some(bytes),
            Meta::new(),
        )
    } else {
        (Some(m.body_markdown), None, None, m.metadata)
    };
    let attachments = crate::messaging::files::metas(
        &m.attachments,
        &m.created_at,
        m.sender_employee_id.as_ref(),
    );
    Ok(ChatMessageView {
        attachment_count: m.attachments.len() as u64,
        attachments,
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
        body,
        snippet: snippet_text,
        body_bytes,
        meta,
        created_at: m.created_at,
        origin_sequence: local_seq,
        reply_count,
        latest_reply_at,
        mentions,
        reactions,
        acceptance,
        answer,
        withdrawal,
    })
}

/// One message a read may answer, before its view is built.
struct Candidate {
    msg: MessageRecord,
    thread_id: String,
    /// Looked up only when needed: for a backward cursor, or once the message is on the page.
    local_seq: Option<u64>,
}

fn local_seq_of(read: &impl MessagingRead, m: &MessageRecord) -> Result<u64, ApiError> {
    Ok(read
        .message_local_seq(&m.id)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read local_seq"))?
        .unwrap_or(m.origin_sequence))
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
    check_read_shape(params)?;
    let conv = resolve_messages_target(read, owner, caller, params, now_utc)?;
    let raw_msgs = read
        .messages_in_conversation(&conv.id)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read messages"))?;
    let threads = ThreadStats::of(&raw_msgs);
    let (start, page_start) = if params.is_delta() {
        delta_bounds(read, &raw_msgs, &conv.id, caller, params)?
    } else {
        (0, 0)
    };
    // A delta read counts every match from its anchor; earlier pages' matches are not kept.
    let mut total = 0u64;
    let mut candidates = Vec::new();
    for (index, m) in raw_msgs.into_iter().enumerate().skip(start) {
        let thread_id = m.thread_root_id.clone().unwrap_or_else(|| m.id.clone());
        let in_thread = params
            .thread_id
            .as_ref()
            .is_none_or(|tid| *tid == thread_id);
        let shown = !params.top_level_only || is_top_level(&m);
        let has = crate::messaging::files::admits(params.attachment_filter(), &m.attachments);
        if !in_thread || !shown || !has || (params.since_read && sent_by(&m, caller)) {
            continue;
        }
        total += 1;
        if index < page_start {
            continue;
        }
        let local_seq = match params.cursor {
            Some(cursor) => {
                let seq = local_seq_of(read, &m)?;
                if seq >= cursor {
                    continue;
                }
                Some(seq)
            }
            None => None,
        };
        candidates.push(Candidate {
            msg: m,
            thread_id,
            local_seq,
        });
    }
    let page = Page::of(
        candidates,
        limit as usize,
        params.is_delta().then_some(total),
    );
    let shape = ViewShape {
        headers_only: params.headers_only,
        threads: &threads,
    };
    let mut views = Vec::with_capacity(page.items.len());
    for c in page.items {
        let local_seq = match c.local_seq {
            Some(seq) => seq,
            None => local_seq_of(read, &c.msg)?,
        };
        views.push(build_chat_message_view(
            read,
            c.msg,
            c.thread_id,
            local_seq,
            shape,
        )?);
    }
    // A plain read with older messages pages back from its oldest message's `local_seq`.
    let next_cursor = page
        .older
        .then(|| views.first().map(|v| v.origin_sequence))
        .flatten();
    Ok(ChatMessagesResult {
        conversation_id: conv.id.as_str().to_owned(),
        messages: views,
        next_cursor,
        next_after_id: page.next_after_id,
        total: page.total,
    })
}

/// One page of candidates: a delta read (with its `total` matches across every page) keeps the
/// oldest `limit` and pages forward; a plain read keeps the newest `limit` and pages backward
/// by `local_seq`.
struct Page {
    items: Vec<Candidate>,
    /// A plain read left older messages behind.
    older: bool,
    next_after_id: Option<String>,
    total: Option<u64>,
}

impl Page {
    fn of(mut items: Vec<Candidate>, limit: usize, delta_total: Option<u64>) -> Self {
        let more = items.len() > limit;
        if let Some(total) = delta_total {
            items.truncate(limit);
            let next_after_id = more
                .then(|| items.last().map(|c| c.msg.id.clone()))
                .flatten();
            return Self {
                items,
                older: false,
                next_after_id,
                total: Some(total),
            };
        }
        if more {
            items = items.split_off(items.len() - limit);
        }
        Self {
            items,
            older: more,
            next_after_id: None,
            total: None,
        }
    }
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
    let conv = resolve_conversation_ref(read, owner, caller, channel_ref, now_utc)?;
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
