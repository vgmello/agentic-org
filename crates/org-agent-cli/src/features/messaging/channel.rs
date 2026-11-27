//! `channel` commands: listing, membership, read markers and summaries.

use org_command::{Answer, Refusal, Request};
use org_proto::chat::{
    ChatChannelCreateResult, ChatChannelsResult, ChatMarkReadResult, ConversationSummary,
};
use org_proto::ErrorCode;
use serde_json::{json, Map, Value};

use super::with_ack;
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{
    decode_err, entity_id, insert_opt, opt_bool, opt_str, opt_str_list, opt_u64, reject_dry_run,
    require_str,
};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport};

/// `Use when:` guidance of each `channel` command.
pub(super) const CUES: &[CommandCue] = &[
    ("channel", "create", "creating a new manual `#channel` with optional initial members."),
    ("channel", "join", "subscribing to a manual `#channel` so you can read and post in it."),
    ("channel", "leave", "leaving a manual `#channel` you no longer need to follow."),
    ("channel", "list", "discovering available `#channels`, whether you are a member, and how many messages are new since your read marker (`--unread-only`)."),
    ("channel", "mark-read", "recording that you have caught up on a `#channel` (to `--up-to` or its latest message) so `--since-read` starts after it."),
    ("channel", "summarize", "answering a question from recent `#channel` history, or digesting what is new since your read marker (`--since-read`), without dumping every message."),
];

pub(super) fn try_channel<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    match request.command.verb {
        "create" => Some(write_channel_create(transport, request)),
        "list" => Some(read_channel_list(transport, request)),
        "join" => Some(write_channel_membership(transport, request, true)),
        "leave" => Some(write_channel_membership(transport, request, false)),
        "mark-read" => Some(write_channel_mark_read(transport, request)),
        "summarize" => Some(read_channel_summarize(transport, request)),
        _ => None,
    }
}

fn channel_summary_value(c: ConversationSummary) -> Value {
    let mut o = Map::new();
    o.insert("id".into(), Value::String(c.id));
    o.insert(
        "kind".into(),
        serde_json::to_value(c.kind).unwrap_or(Value::String("channel".into())),
    );
    insert_opt(&mut o, "name", c.name.as_deref());
    if let Some(scope) = c.auto_scope {
        o.insert(
            "auto_scope".into(),
            serde_json::to_value(scope).unwrap_or(Value::Null),
        );
    }
    o.insert("archived".into(), Value::Bool(c.archived));
    let active_count = c.members.iter().filter(|m| m.left_at.is_none()).count();
    o.insert("member_count".into(), Value::from(active_count));
    o.insert("is_member".into(), Value::Bool(c.is_member));
    o.insert("new_message_count".into(), Value::from(c.new_message_count));
    o.insert("unread_count".into(), Value::from(c.unread_count));
    insert_opt(
        &mut o,
        "last_read_message_id",
        c.last_read_message_id.as_deref(),
    );
    insert_opt(&mut o, "last_read_at", c.last_read_at.as_deref());
    insert_opt(&mut o, "last_message_id", c.last_message_id.as_deref());
    insert_opt(&mut o, "last_message_at", c.last_message_at.as_deref());
    Value::Object(o)
}

pub(super) fn write_channel_create<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    reject_dry_run(request)?;
    let name = require_str(request, "name")?;
    let id = entity_id(request, Some("channel_id"), "chat.channel_create")?;
    let mut params = Map::new();
    params.insert("id".into(), Value::String(id));
    params.insert("name".into(), Value::String(name.to_owned()));
    if let Some(members) = opt_str_list(request, "members") {
        params.insert("members".into(), json!(members));
    }
    let raw = transport.call_method("chat.channel_create", Value::Object(params))?;
    let res: ChatChannelCreateResult =
        serde_json::from_value(raw).map_err(|_| decode_err("chat.channel_create"))?;
    let mut answer = Answer::completed(channel_summary_value(res.channel));
    answer.operation_id = request.idempotency_key.clone();
    Ok(with_ack(answer, request))
}

pub(super) fn read_channel_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    parse_cursor(request.cursor.as_deref())?;
    let include_archived = opt_bool(request, "include_archived").unwrap_or(false);
    let unread_only = opt_bool(request, "unread_only").unwrap_or(false);
    let raw = transport.call_method(
        "chat.channels",
        json!({ "include_archived": include_archived, "unread_only": unread_only }),
    )?;
    let res: ChatChannelsResult =
        serde_json::from_value(raw).map_err(|_| decode_err("chat.channels"))?;
    let items: Vec<Value> = res
        .channels
        .into_iter()
        .map(channel_summary_value)
        .collect();
    paginate(request, items)
}

pub(super) fn write_channel_membership<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    join: bool,
) -> Result<Answer, Refusal> {
    reject_dry_run(request)?;
    let channel = require_str(request, "channel")?;
    let method = if join {
        "chat.channel_join"
    } else {
        "chat.channel_leave"
    };
    let raw = transport.call_method(method, json!({ "channel": channel }))?;
    let res: org_proto::chat::ChatChannelMembershipResult =
        serde_json::from_value(raw).map_err(|_| decode_err(method))?;
    let mut obj = Map::new();
    obj.insert("conversation_id".into(), Value::String(res.channel.id));
    insert_opt(&mut obj, "name", res.channel.name.as_deref());
    obj.insert("is_member".into(), Value::Bool(res.joined));
    Ok(with_ack(Answer::completed(Value::Object(obj)), request))
}

/// `channel mark-read`: moves the caller's read marker forward to `--up-to`, or to the latest
/// message. Never backward, and inbox items are left as they are (`TASK-99` rung 2).
pub(super) fn write_channel_mark_read<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let channel = require_str(request, "channel")?;
    let mut params = Map::new();
    params.insert("conversation_id".into(), Value::String(channel.to_owned()));
    insert_opt(
        &mut params,
        "last_read_message_id",
        opt_str(request, "up_to"),
    );
    if request.dry_run {
        params.insert("dry_run".into(), Value::Bool(true));
    }
    let raw = transport.call_method("chat.mark_read", Value::Object(params))?;
    let res: ChatMarkReadResult =
        serde_json::from_value(raw).map_err(|_| decode_err("chat.mark_read"))?;
    let mut obj = Map::new();
    obj.insert("conversation_id".into(), Value::String(res.conversation_id));
    insert_opt(
        &mut obj,
        "last_read_message_id",
        res.last_read_message_id.as_deref(),
    );
    obj.insert("read_at".into(), Value::String(res.read_at));
    obj.insert("advanced".into(), Value::Bool(res.advanced));
    let answer = Answer::completed(Value::Object(obj));
    Ok(if request.dry_run {
        answer
    } else {
        with_ack(answer, request)
    })
}

pub(super) fn read_channel_summarize<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let channel = require_str(request, "channel")?;
    let since_read = opt_bool(request, "since_read").unwrap_or(false);
    let question = opt_str(request, "question").filter(|q| !q.trim().is_empty());
    if question.is_none() && !since_read {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "`channel summarize` needs --question, or --since-read for a catch-up digest.",
            "Ask with --question <text>, or pass --since-read to digest what is new since your read marker.",
        ));
    }
    let mut params = Map::new();
    params.insert("channel".into(), Value::String(channel.to_owned()));
    insert_opt(&mut params, "question", question);
    if since_read {
        params.insert("since_read".into(), Value::Bool(true));
    }
    if let Some(limit) = opt_u64(request, "max_messages")? {
        params.insert("limit".into(), Value::from(limit.clamp(1, 200)));
    }
    let timeout = std::time::Duration::from_secs(75);
    let raw = transport.call_method_with_timeout(
        "chat.channel_summarize",
        Value::Object(params),
        timeout,
    )?;
    let res: org_proto::chat::ChatChannelSummarizeResult =
        serde_json::from_value(raw).map_err(|_| decode_err("chat.channel_summarize"))?;
    let mut obj = Map::new();
    obj.insert("conversation_id".into(), Value::String(res.conversation_id));
    obj.insert("summary".into(), Value::String(res.summary));
    obj.insert(
        "message_ids".into(),
        Value::Array(res.message_ids.into_iter().map(Value::String).collect()),
    );
    obj.insert("derived_notice".into(), Value::String(res.derived_notice));
    insert_opt(&mut obj, "usage_record_id", res.usage_record_id.as_deref());
    if let Some(count) = res.new_message_count {
        obj.insert("new_message_count".into(), Value::from(count));
    }
    if let Some(remaining) = res.remaining_unread {
        obj.insert("remaining_unread".into(), Value::from(remaining));
    }
    insert_opt(
        &mut obj,
        "through_message_id",
        res.through_message_id.as_deref(),
    );
    Ok(Answer::completed(Value::Object(obj)))
}
