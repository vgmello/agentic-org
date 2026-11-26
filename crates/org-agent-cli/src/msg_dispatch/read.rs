//! Message reads: inbox, one message, list and waits.

use org_command::{Answer, Refusal, Request};
use org_proto::ask::{AskStatus, ChatWaitAskResult, MAX_SHORT_HOLD_MS};
use org_proto::chat::{ChatInboxResult, ChatMessageView, ChatMessagesResult};
use org_proto::msg::{HeaderPage, MsgReadResult, MsgWaitResult, MAX_WAIT_MS};
use org_proto::ErrorCode;
use serde_json::{json, Map, Value};

use super::ask::{ask_meta_status, insert_ask_outcome_fields};
use super::{
    decode_err, insert_opt, opt_bool, opt_non_neg_u64, opt_str, parse_u64_cursor, AskOutcomeRef,
};
use crate::transport::OfficeTransport;

pub(super) fn read_msg_inbox<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let mut params = Map::new();
    if opt_bool(request, "all").unwrap_or(false) {
        params.insert("include_all".into(), Value::Bool(true));
    }
    insert_opt(&mut params, "state", opt_str(request, "state"));
    insert_opt(&mut params, "from", opt_str(request, "from"));
    insert_opt(&mut params, "thread_id", opt_str(request, "thread"));
    insert_opt(&mut params, "because", opt_str(request, "because"));
    if let Some(limit) = request.limit {
        params.insert("limit".into(), Value::from(limit));
    }
    if let Some(cursor) = parse_u64_cursor(request.cursor.as_deref())? {
        params.insert("cursor".into(), Value::from(cursor));
    }
    let raw = transport.call_method("chat.inbox", Value::Object(params))?;
    let res: ChatInboxResult = serde_json::from_value(raw).map_err(|_| decode_err("chat.inbox"))?;
    let items: Vec<Value> = res
        .items
        .into_iter()
        .map(|it| serde_json::to_value(it).unwrap_or(Value::Null))
        .collect();
    let mut answer = Answer::completed(Value::Array(items));
    answer.next_cursor = res.next_cursor.map(|c| c.to_string());
    Ok(answer)
}

pub(super) fn read_msg_read<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let mut params = Map::new();
    insert_opt(&mut params, "id", opt_str(request, "id"));
    let raw = transport.call_method("msg.read", Value::Object(params))?;
    let res: MsgReadResult = serde_json::from_value(raw).map_err(|_| decode_err("msg.read"))?;
    let Some(msg) = res.message else {
        return Ok(Answer::completed(json!({})));
    };
    let mut obj = Map::new();
    obj.insert("id".into(), Value::String(msg.header.id));
    obj.insert("thread_id".into(), Value::String(msg.header.thread_id));
    insert_opt(&mut obj, "reply_to", msg.header.reply_to.as_deref());
    obj.insert(
        "from_member_id".into(),
        Value::String(msg.header.from_member_id),
    );
    obj.insert(
        "from_display_name".into(),
        Value::String(msg.header.from_display_name),
    );
    obj.insert(
        "to_member_id".into(),
        Value::String(msg.header.to_member_id),
    );
    obj.insert("kind".into(), Value::String(msg.header.kind));
    obj.insert(
        "priority".into(),
        serde_json::to_value(msg.header.priority).unwrap_or(Value::String("normal".into())),
    );
    insert_opt(&mut obj, "subject", msg.header.subject.as_deref());
    obj.insert("body".into(), Value::String(msg.body));
    obj.insert(
        "state".into(),
        serde_json::to_value(msg.header.state).unwrap_or(Value::String("read".into())),
    );
    obj.insert("created_at".into(), Value::String(msg.header.created_at));
    Ok(Answer::completed(Value::Object(obj)))
}

pub(super) fn read_msg_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let conversation = list_conversation(request)?;
    let thread = opt_str(request, "thread");
    let participant = opt_str(request, "participant");
    if conversation.is_some() || (thread.is_some() && participant.is_none()) {
        return read_conversation_messages(transport, request, conversation, thread);
    }
    let mut params = Map::new();
    insert_opt(&mut params, "participant", participant);
    insert_opt(&mut params, "thread_id", thread);
    if opt_bool(request, "unread_only").unwrap_or(false) {
        params.insert("unread_only".into(), Value::Bool(true));
    }
    if let Some(limit) = request.limit {
        params.insert("limit".into(), Value::from(limit));
    }
    if let Some(cursor) = parse_u64_cursor(request.cursor.as_deref())? {
        params.insert("cursor".into(), Value::from(cursor));
    }
    let raw = transport.call_method("msg.list", Value::Object(params))?;
    let page: HeaderPage = serde_json::from_value(raw).map_err(|_| decode_err("msg.list"))?;
    let items: Vec<Value> = page
        .headers
        .into_iter()
        .map(|h| {
            let mut o = Map::new();
            o.insert("id".into(), Value::String(h.id));
            o.insert("thread_id".into(), Value::String(h.thread_id));
            insert_opt(&mut o, "reply_to", h.reply_to.as_deref());
            o.insert("from_member_id".into(), Value::String(h.from_member_id));
            o.insert(
                "from_display_name".into(),
                Value::String(h.from_display_name),
            );
            o.insert("kind".into(), Value::String(h.kind));
            o.insert(
                "priority".into(),
                serde_json::to_value(h.priority).unwrap_or(Value::String("normal".into())),
            );
            insert_opt(&mut o, "subject", h.subject.as_deref());
            o.insert("created_at".into(), Value::String(h.created_at));
            Value::Object(o)
        })
        .collect();
    let mut answer = Answer::completed(Value::Array(items));
    answer.next_cursor = page.next_cursor.map(|c| c.to_string());
    Ok(answer)
}

/// A channel, conversation or thread read over `chat.messages`, plain (newest page, backward
/// cursor) or delta (`--since-read` / `--after-id`: oldest first, `meta.next_cursor` is the
/// message the next page starts after and `meta.total` counts every match after the anchor).
fn read_conversation_messages<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    conversation: Option<&str>,
    thread: Option<&str>,
) -> Result<Answer, Refusal> {
    let since_read = opt_bool(request, "since_read").unwrap_or(false);
    let after_id = opt_str(request, "after_id");
    let delta = since_read || after_id.is_some();
    let mut params = Map::new();
    insert_opt(&mut params, "conversation", conversation);
    insert_opt(&mut params, "thread_id", thread);
    if let Some(limit) = request.limit {
        params.insert("limit".into(), Value::from(limit));
    }
    for flag in ["since_read", "top_level_only", "headers_only"] {
        if opt_bool(request, flag).unwrap_or(false) {
            params.insert(flag.into(), Value::Bool(true));
        }
    }
    if delta {
        // `--cursor` only picks the page (`page_after_id`); `--after-id` stays the read's anchor,
        // so `meta.total` holds from page to page and no page reaches behind the anchor.
        let page = request.cursor.as_deref();
        if page.is_some_and(|cursor| !org_proto::is_uuid_v7(cursor)) {
            return Err(refuse_list(
                "In a --since-read or --after-id read, --cursor is the message ID `meta.next_cursor` gave.",
            ));
        }
        insert_opt(&mut params, "after_id", after_id);
        insert_opt(&mut params, "page_after_id", page);
    } else if let Some(cursor) = parse_u64_cursor(request.cursor.as_deref())? {
        params.insert("cursor".into(), Value::from(cursor));
    }
    let raw = transport.call_method("chat.messages", Value::Object(params))?;
    let res: ChatMessagesResult =
        serde_json::from_value(raw).map_err(|_| decode_err("chat.messages"))?;
    let items: Vec<Value> = res.messages.into_iter().map(message_item).collect();
    let mut answer = Answer::completed(Value::Array(items));
    answer.next_cursor = if delta {
        res.next_after_id
    } else {
        res.next_cursor.map(|c| c.to_string())
    };
    answer.total = res.total;
    Ok(answer)
}

fn message_item(m: ChatMessageView) -> Value {
    let mut o = Map::new();
    o.insert("id".into(), Value::String(m.id));
    o.insert("thread_id".into(), Value::String(m.thread_id));
    insert_opt(&mut o, "reply_to", m.reply_to.as_deref());
    o.insert(
        "from_member_id".into(),
        Value::String(
            m.sender_employee_id
                .unwrap_or_else(|| org_proto::msg::OFFICE_SENDER.to_owned()),
        ),
    );
    o.insert(
        "from_display_name".into(),
        Value::String(m.sender_display_name),
    );
    o.insert("kind".into(), Value::String(m.kind));
    o.insert(
        "priority".into(),
        serde_json::to_value(m.priority).unwrap_or(Value::String("normal".into())),
    );
    insert_opt(&mut o, "subject", m.subject.as_deref());
    insert_opt(&mut o, "body", m.body.as_deref());
    insert_opt(&mut o, "snippet", m.snippet.as_deref());
    if let Some(bytes) = m.body_bytes {
        o.insert("body_bytes".into(), Value::from(bytes));
    }
    o.insert("created_at".into(), Value::String(m.created_at));
    if m.reply_count > 0 {
        o.insert("reply_count".into(), Value::from(m.reply_count));
    }
    insert_opt(&mut o, "latest_reply_at", m.latest_reply_at.as_deref());
    if m.attachment_count > 0 {
        o.insert("attachment_count".into(), Value::from(m.attachment_count));
    }
    Value::Object(o)
}

const LIST_HINT: &str = "Read a channel or conversation with --channel or --conversation (and --thread); catch up with --since-read, then `channel mark-read --channel <channel>`; read a DM history with --participant (and --thread, --unread-only).";

fn refuse_list(message: &str) -> Refusal {
    Refusal::new(ErrorCode::InvalidRequest, message.to_owned(), LIST_HINT)
}

/// The conversation `msg list` reads, refusing flag combinations one of whose flags the list
/// would otherwise drop: `--conversation` with `--channel`; `--participant` or `--unread-only`
/// with a conversation, a channel or a thread alone (only a DM history by `--participant`
/// filters by participant and unread state); the catch-up flags (`--since-read`, `--after-id`,
/// `--top-level-only`, `--headers-only`) on a DM history, whether named by `--participant` or
/// read for want of a channel, conversation or thread; and `--since-read` with `--after-id`.
fn list_conversation(request: &Request) -> Result<Option<&str>, Refusal> {
    let conversation = opt_str(request, "conversation");
    let channel = opt_str(request, "channel");
    let participant = opt_str(request, "participant").is_some();
    let flag = |name: &str| opt_bool(request, name).unwrap_or(false);
    if conversation.is_some() && channel.is_some() {
        return Err(refuse_list(
            "--conversation and --channel cannot be given together.",
        ));
    }
    let target = conversation.or(channel);
    if target.is_some() && participant {
        return Err(refuse_list(
            "--participant cannot be combined with --conversation or --channel.",
        ));
    }
    let by_thread_alone = opt_str(request, "thread").is_some() && !participant;
    if flag("unread_only") && (target.is_some() || by_thread_alone) {
        return Err(refuse_list("--unread-only reads a DM history and needs --participant; in a channel or conversation, --since-read lists what is new since your read marker."));
    }
    let after_id = opt_str(request, "after_id");
    if flag("since_read") && after_id.is_some() {
        return Err(refuse_list("--since-read and --after-id cannot be given together: --since-read starts at your read marker, --after-id at a message you name (page on with --cursor <meta.next_cursor>)."));
    }
    if flag("since_read") && target.is_none() {
        return Err(refuse_list(
            "--since-read needs --channel or --conversation: a read marker belongs to one conversation.",
        ));
    }
    if after_id.is_some_and(|anchor| !org_proto::is_uuid_v7(anchor)) {
        return Err(refuse_list(
            "--after-id must be a message ID (a canonical lowercase UUIDv7).",
        ));
    }
    if flag("top_level_only") && opt_str(request, "thread").is_some() {
        return Err(refuse_list(
            "--top-level-only lists thread roots and cannot be combined with --thread, which lists one thread's replies.",
        ));
    }
    let catchup = ["since_read", "top_level_only", "headers_only"]
        .into_iter()
        .any(flag)
        || after_id.is_some();
    if catchup && participant {
        return Err(refuse_list("--since-read, --after-id, --top-level-only and --headers-only read a channel or conversation, not a DM history by --participant."));
    }
    if catchup && target.is_none() && !by_thread_alone {
        return Err(refuse_list("--after-id, --top-level-only and --headers-only need --channel, --conversation or --thread: without one, `msg list` reads your DM history, which they do not apply to."));
    }
    Ok(target)
}

pub(super) fn read_msg_wait_ask<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    ask_id: &str,
) -> Result<Answer, Refusal> {
    let mut params = Map::new();
    params.insert("ask_id".into(), Value::String(ask_id.to_owned()));
    let hold_ms = opt_non_neg_u64(request, "timeout_ms")?
        .unwrap_or(MAX_SHORT_HOLD_MS)
        .min(MAX_SHORT_HOLD_MS);
    params.insert("hold_ms".into(), Value::from(hold_ms));
    let timeout = std::time::Duration::from_millis(hold_ms) + org_client::DEFAULT_TIMEOUT;
    let raw =
        transport.call_method_with_timeout("chat.wait_ask", Value::Object(params), timeout)?;
    let res: ChatWaitAskResult =
        serde_json::from_value(raw).map_err(|_| decode_err("chat.wait_ask"))?;
    let status = ask_meta_status(res.status);
    let mut obj = Map::new();
    let woken = !matches!(res.status, AskStatus::Waiting);
    obj.insert("woken".into(), Value::Bool(woken));
    obj.insert("ask_id".into(), Value::String(res.ask_id));
    insert_ask_outcome_fields(
        &mut obj,
        AskOutcomeRef {
            status: res.status,
            wait_until: res.wait_until.as_deref(),
            remaining_ms: res.remaining_ms,
            answer: res.answer.as_ref(),
            withdrawal: res.withdrawal.as_ref(),
            defaults: &res.defaults,
            required_notice: res.required_notice.as_deref(),
        },
    );
    let mut answer = Answer::completed(Value::Object(obj));
    answer.status = status;
    Ok(answer)
}

pub(super) fn read_msg_wait<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    if let Some(ask_id) = opt_str(request, "ask") {
        return read_msg_wait_ask(transport, request, ask_id);
    }
    let mut params = Map::new();
    let wait_ms = if let Some(ms) = opt_non_neg_u64(request, "timeout_ms")? {
        let clamped = ms.min(MAX_WAIT_MS);
        params.insert("timeout_ms".into(), Value::from(clamped));
        clamped
    } else {
        MAX_WAIT_MS
    };
    insert_opt(&mut params, "from", opt_str(request, "from"));
    if opt_bool(request, "urgent_only").unwrap_or(false) {
        params.insert("urgent_only".into(), Value::Bool(true));
    }
    let timeout = std::time::Duration::from_millis(wait_ms) + org_client::DEFAULT_TIMEOUT;
    let raw = transport.call_method_with_timeout("msg.wait", Value::Object(params), timeout)?;
    let res: MsgWaitResult = serde_json::from_value(raw).map_err(|_| decode_err("msg.wait"))?;
    let mut obj = Map::new();
    match res.message {
        Some(msg) => {
            obj.insert("woken".into(), Value::Bool(true));
            obj.insert("id".into(), Value::String(msg.header.id));
            obj.insert("thread_id".into(), Value::String(msg.header.thread_id));
            obj.insert(
                "from_member_id".into(),
                Value::String(msg.header.from_member_id),
            );
            insert_opt(&mut obj, "subject", msg.header.subject.as_deref());
            obj.insert("body".into(), Value::String(msg.body));
        }
        None => {
            obj.insert("woken".into(), Value::Bool(false));
        }
    }
    Ok(Answer::completed(Value::Object(obj)))
}
