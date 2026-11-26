//! The chat inbox: matching, ordering and paging of inbox items.

use org_proto::chat::{ChatInboxItem, ChatInboxParams, ChatInboxResult};
use org_proto::msg::{is_urgent, MessageLifecycleState, DEFAULT_PAGE_LIMIT, MAX_PAGE_LIMIT};
use org_proto::{ApiError, ErrorCode};
use org_store::Id;

use crate::messaging::{ports::MessagingRead, records::RecipientStateRecord};

use super::resolve_employee;

fn matches_inbox_state(state: MessageLifecycleState, params: &ChatInboxParams) -> bool {
    match params.state {
        Some(wanted) => state == wanted,
        None => params.include_all || state == MessageLifecycleState::Unread,
    }
}

fn inbox_item_if_matching(
    read: &impl MessagingRead,
    st: RecipientStateRecord,
    params: &ChatInboxParams,
    from_filter: Option<&Id>,
) -> Result<Option<ChatInboxItem>, ApiError> {
    if !matches_inbox_state(st.state, params) {
        return Ok(None);
    }
    if params.because.is_some_and(|b| st.delivered_because != b) {
        return Ok(None);
    }
    let Some(msg) = read
        .message(&st.message_id)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read message"))?
    else {
        return Ok(None);
    };
    if from_filter.is_some_and(|fid| msg.sender_employee_id.as_ref() != Some(fid)) {
        return Ok(None);
    }
    let thread_id = msg.thread_root_id.clone().unwrap_or_else(|| msg.id.clone());
    if params.thread_id.as_deref().is_some_and(|t| thread_id != t) {
        return Ok(None);
    }
    let seq = read
        .message_local_seq(&msg.id)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read local_seq"))?
        .unwrap_or(msg.origin_sequence);
    if params.state != Some(MessageLifecycleState::Snoozed)
        && params.cursor.is_some_and(|c| seq >= c)
    {
        return Ok(None);
    }
    Ok(Some(ChatInboxItem {
        id: msg.id,
        conversation_id: msg.conversation_id.as_str().to_owned(),
        thread_id,
        reply_to: msg.reply_to_message_id,
        from_member_id: msg
            .sender_employee_id
            .map(|i| i.as_str().to_owned())
            .unwrap_or_else(|| "office".into()),
        from_display_name: msg.sender_display_name,
        kind: msg.kind.clone(),
        priority: msg.priority,
        urgent: is_urgent(&msg.kind, msg.priority),
        subject: msg.subject,
        created_at: msg.created_at,
        state: st.state,
        triage_gen: st.triage_generation,
        snoozed_until: st.snoozed_until,
        snooze_after_task: st.snooze_after_task,
        snooze_note: st.snooze_note,
        read_at: st.read_at,
        delivered_because: st.delivered_because,
        related_message_ids: st.related_message_ids,
        reason: st.reason,
        local_seq: seq,
    }))
}

fn sort_chat_inbox_items(items: &mut [ChatInboxItem], state: Option<MessageLifecycleState>) {
    if state == Some(MessageLifecycleState::Snoozed) {
        items.sort_by(compare_snoozed_inbox_items);
    } else {
        items.sort_by(|a, b| b.local_seq.cmp(&a.local_seq).then_with(|| b.id.cmp(&a.id)));
    }
}

fn compare_snoozed_inbox_items(a: &ChatInboxItem, b: &ChatInboxItem) -> std::cmp::Ordering {
    match (&a.snoozed_until, &b.snoozed_until) {
        (Some(x), Some(y)) => {
            let cmp = match (
                crate::time::parse_rfc3339_ms(x),
                crate::time::parse_rfc3339_ms(y),
            ) {
                (Some(xm), Some(ym)) => xm.cmp(&ym),
                _ => x.cmp(y),
            };
            cmp.then_with(|| b.local_seq.cmp(&a.local_seq))
                .then_with(|| b.id.cmp(&a.id))
        }
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => b.local_seq.cmp(&a.local_seq).then_with(|| b.id.cmp(&a.id)),
    }
}

fn paginate_chat_inbox(
    mut items: Vec<ChatInboxItem>,
    params: &ChatInboxParams,
    limit: usize,
) -> ChatInboxResult {
    sort_chat_inbox_items(&mut items, params.state);
    if params.state == Some(MessageLifecycleState::Snoozed) {
        let offset = usize::try_from(params.cursor.unwrap_or(0)).unwrap_or(usize::MAX);
        if offset >= items.len() {
            items.clear();
        } else {
            items.drain(..offset);
        }
        let more = items.len() > limit;
        items.truncate(limit);
        let next_cursor = more.then(|| (offset + items.len()) as u64);
        return ChatInboxResult { items, next_cursor };
    }
    let more = items.len() > limit;
    items.truncate(limit);
    let next_cursor = if more {
        items.last().map(|i| i.local_seq)
    } else {
        None
    };
    ChatInboxResult { items, next_cursor }
}

pub fn list_chat_inbox(
    read: &impl MessagingRead,
    owner: &Id,
    caller: &Id,
    params: &ChatInboxParams,
) -> Result<ChatInboxResult, ApiError> {
    if params.state == Some(MessageLifecycleState::Unknown) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "unknown message state filter",
        ));
    }
    let limit = params.limit.unwrap_or(DEFAULT_PAGE_LIMIT);
    if limit == 0 || limit > MAX_PAGE_LIMIT {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "limit must be 1..=100",
        ));
    }
    let employees = read
        .employees()
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read employees"))?;
    let from_filter = match &params.from {
        Some(f) => Some(resolve_employee(&employees, owner, f)?.id.clone()),
        None => None,
    };
    let states = read
        .recipient_states_for_employee(caller)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read inbox"))?;
    let mut items = Vec::new();
    for st in states {
        if let Some(item) = inbox_item_if_matching(read, st, params, from_filter.as_ref())? {
            items.push(item);
        }
    }
    Ok(paginate_chat_inbox(items, params, limit as usize))
}
