//! Direct RPC translators and response shapers for `msg` and `channel` apply commands.

use org_proto::ask::{ChatAskResult, MsgWithdrawResult, MAX_SHORT_HOLD_MS};
use org_proto::chat::{
    ChatChannelCreateResult, ChatChannelMembershipResult, ChatMarkReadResult, ChatSendResult,
    ConversationSummary,
};
use org_proto::msg::{
    MsgDeferParams, MsgDeferResult, MsgDoneResult, MsgUnsnoozeParams, MsgUnsnoozeResult,
    SnoozeTarget,
};
use org_proto::ApiError;
use serde_json::{json, Map, Value};

use super::{
    insert_opt, invalid, opt_bool, opt_str, opt_str_list, opt_u64, require_str, unreadable,
    OutputShape, PreparedRpc,
};

const SCALAR_ASK_FIELDS: [&str; 4] = ["prompt", "question_type", "option", "default_value"];

pub(super) fn try_prepare(
    next_id: &mut impl FnMut() -> String,
    resource: &str,
    verb: &str,
    input: &Map<String, Value>,
) -> Option<Result<PreparedRpc, ApiError>> {
    match (resource, verb) {
        ("msg", "send") => Some(prep_msg_send(next_id, input)),
        ("msg", "ask") => Some(prep_msg_ask(next_id, input)),
        ("msg", "withdraw") => Some(prep_msg_withdraw(input)),
        ("msg", "accept") => Some(prep_msg_accept(input)),
        ("msg", "done") => Some(prep_msg_done(input)),
        ("msg", "defer") => Some(prep_msg_defer(input)),
        ("msg", "unsnooze") => Some(prep_msg_unsnooze(input)),
        ("channel", "create") => Some(prep_channel_create(next_id, input)),
        ("channel", "join") => Some(prep_channel_membership(input, true)),
        ("channel", "leave") => Some(prep_channel_membership(input, false)),
        ("channel", "mark-read") => Some(prep_channel_mark_read(input)),
        _ => None,
    }
}

fn prep_msg_send(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
) -> Result<PreparedRpc, ApiError> {
    let to = require_str(input, "to")?;
    let body = opt_str(input, "body").unwrap_or("");
    let att_ids = opt_str_list(input, "attachment_id").unwrap_or_default();
    if body.trim().is_empty() && att_ids.is_empty() {
        return Err(invalid("`msg send` requires `body` or `attachment_id`"));
    }
    let mut params = Map::new();
    params.insert("id".into(), Value::String(next_id()));
    params.insert("to".into(), Value::String(to.to_owned()));
    params.insert("body".into(), Value::String(body.to_owned()));
    params.insert(
        "kind".into(),
        Value::String(opt_str(input, "kind").unwrap_or("msg").to_owned()),
    );
    insert_opt(&mut params, "priority", opt_str(input, "priority"));
    insert_opt(&mut params, "subject", opt_str(input, "subject"));
    insert_opt(&mut params, "reply_to", opt_str(input, "reply_to"));
    insert_opt(&mut params, "reason", opt_str(input, "reason"));
    if opt_bool(input, "also_in_channel").unwrap_or(false) {
        params.insert("also_in_channel".into(), Value::Bool(true));
    }
    if !att_ids.is_empty() {
        params.insert("attachment_ids".into(), json!(att_ids));
    }
    Ok(PreparedRpc {
        method: "chat.send",
        params: Value::Object(params),
        shape: OutputShape::MsgSend,
    })
}

fn parse_ask_questions(input: &Map<String, Value>) -> Result<Value, ApiError> {
    let has_scalar = SCALAR_ASK_FIELDS.iter().any(|k| input.contains_key(*k));
    match (input.get("questions"), has_scalar) {
        (Some(_), true) => Err(invalid(
            "scalar question fields cannot be combined with `questions`",
        )),
        (Some(q), false) => Ok(q.clone()),
        (None, false) => Err(invalid("`msg ask` requires `prompt` or `questions`")),
        (None, true) => build_single_question(input),
    }
}

fn build_single_question(input: &Map<String, Value>) -> Result<Value, ApiError> {
    let prompt = opt_str(input, "prompt").ok_or_else(|| {
        invalid("`question_type`, `option`, and `default_value` require `prompt`")
    })?;
    let options: Vec<&str> = input
        .get("option")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let qtype = match opt_str(input, "question_type") {
        Some(kind) => kind,
        None if options.is_empty() => "free_text",
        None => "choice",
    };
    match (qtype, options.is_empty()) {
        ("choice", true) => {
            return Err(invalid("a choice question requires at least one `option`"))
        }
        ("yes_no" | "free_text", false) => {
            return Err(invalid(format!(
                "`option` is invalid for `{qtype}` questions"
            )))
        }
        _ => {}
    }
    let mut question = Map::new();
    question.insert("id".into(), json!("q1"));
    question.insert("question_type".into(), json!(qtype));
    question.insert("prompt".into(), json!(prompt));
    if !options.is_empty() {
        let mut parsed_opts = Vec::with_capacity(options.len());
        for raw in options {
            let (id, label) = raw.split_once('=').unwrap_or((raw, raw));
            let (id, label) = (id.trim(), label.trim());
            if id.is_empty() || label.is_empty() {
                return Err(invalid(format!(
                    "option `{raw}` needs a non-empty ID and label"
                )));
            }
            parsed_opts.push(json!({"id": id, "label": label}));
        }
        question.insert("options".into(), Value::Array(parsed_opts));
    }
    insert_opt(
        &mut question,
        "default_value",
        opt_str(input, "default_value"),
    );
    Ok(Value::Array(vec![Value::Object(question)]))
}

fn prep_msg_ask(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
) -> Result<PreparedRpc, ApiError> {
    let to = require_str(input, "to")?;
    let questions = parse_ask_questions(input)?;
    let wait_ms = opt_u64(input, "wait_ms")?;
    let hold_ms = opt_u64(input, "hold_ms")?
        .unwrap_or(MAX_SHORT_HOLD_MS)
        .min(MAX_SHORT_HOLD_MS);
    let att_ids = opt_str_list(input, "attachment_id").unwrap_or_default();
    let mut params = Map::new();
    params.insert("id".into(), Value::String(next_id()));
    params.insert("to".into(), Value::String(to.to_owned()));
    params.insert("questions".into(), questions);
    if !att_ids.is_empty() {
        params.insert("attachment_ids".into(), json!(att_ids));
    }
    insert_opt(&mut params, "body", opt_str(input, "body"));
    insert_opt(&mut params, "subject", opt_str(input, "subject"));
    insert_opt(&mut params, "blocks", opt_str(input, "blocks"));
    insert_opt(&mut params, "addressed_to", opt_str(input, "addressed_to"));
    if opt_bool(input, "required").unwrap_or(false) {
        params.insert("required".into(), Value::Bool(true));
    }
    if opt_bool(input, "approval").unwrap_or(false) {
        params.insert("approval".into(), Value::Bool(true));
    }
    if let Some(wait_ms) = wait_ms {
        params.insert("wait_ms".into(), Value::from(wait_ms));
    }
    params.insert("hold_ms".into(), Value::from(hold_ms));
    Ok(PreparedRpc {
        method: "chat.ask",
        params: Value::Object(params),
        shape: OutputShape::MsgAsk,
    })
}

fn prep_msg_withdraw(input: &Map<String, Value>) -> Result<PreparedRpc, ApiError> {
    let ask_id = require_str(input, "id")?;
    let mut params = Map::new();
    params.insert("ask_id".into(), Value::String(ask_id.to_owned()));
    insert_opt(&mut params, "reason", opt_str(input, "reason"));
    Ok(PreparedRpc {
        method: "msg.withdraw",
        params: Value::Object(params),
        shape: OutputShape::MsgWithdraw,
    })
}

fn prep_msg_accept(input: &Map<String, Value>) -> Result<PreparedRpc, ApiError> {
    let id = require_str(input, "id")?;
    let mut params = Map::new();
    params.insert("id".into(), Value::String(id.to_owned()));
    insert_opt(&mut params, "reason", opt_str(input, "reason"));
    Ok(PreparedRpc {
        method: "msg.accept",
        params: Value::Object(params),
        shape: OutputShape::Passthrough,
    })
}

fn prep_msg_done(input: &Map<String, Value>) -> Result<PreparedRpc, ApiError> {
    let id = require_str(input, "id")?;
    let mut params = Map::new();
    params.insert("id".into(), Value::String(id.to_owned()));
    insert_opt(&mut params, "note", opt_str(input, "note"));
    Ok(PreparedRpc {
        method: "msg.done",
        params: Value::Object(params),
        shape: OutputShape::MsgDone,
    })
}

fn prep_msg_defer(input: &Map<String, Value>) -> Result<PreparedRpc, ApiError> {
    let id = require_str(input, "id")?;
    let until = match (opt_str(input, "until"), opt_bool(input, "after_task")) {
        (Some(_), Some(true)) => {
            return Err(invalid("pass either `until` or `after_task`, not both"))
        }
        (Some(at), _) => {
            let canonical = org_proto::canonicalize_rfc3339_utc(at)
                .ok_or_else(|| invalid("`until` must be a valid RFC 3339 timestamp"))?;
            SnoozeTarget::Until { at: canonical }
        }
        (None, Some(true)) => SnoozeTarget::AfterTask,
        _ => {
            return Err(invalid(
                "pass either `until` or `after_task` to defer a message",
            ))
        }
    };
    let wire = MsgDeferParams {
        id: id.to_owned(),
        until,
        note: opt_str(input, "note").map(str::to_owned),
    };
    Ok(PreparedRpc {
        method: "msg.defer",
        params: json!(wire),
        shape: OutputShape::MsgDefer,
    })
}

fn prep_msg_unsnooze(input: &Map<String, Value>) -> Result<PreparedRpc, ApiError> {
    let id = require_str(input, "id")?.to_owned();
    Ok(PreparedRpc {
        method: "msg.unsnooze",
        params: json!(MsgUnsnoozeParams { id }),
        shape: OutputShape::MsgUnsnooze,
    })
}

fn prep_channel_create(
    next_id: &mut impl FnMut() -> String,
    input: &Map<String, Value>,
) -> Result<PreparedRpc, ApiError> {
    let name = require_str(input, "name")?;
    let id = opt_str(input, "channel_id")
        .map(str::to_owned)
        .unwrap_or_else(next_id);
    let mut params = Map::new();
    params.insert("id".into(), Value::String(id));
    params.insert("name".into(), Value::String(name.to_owned()));
    if let Some(members) = opt_str_list(input, "members") {
        params.insert("members".into(), json!(members));
    }
    Ok(PreparedRpc {
        method: "chat.channel_create",
        params: Value::Object(params),
        shape: OutputShape::ChannelCreate,
    })
}

fn prep_channel_membership(
    input: &Map<String, Value>,
    join: bool,
) -> Result<PreparedRpc, ApiError> {
    let channel = require_str(input, "channel")?;
    Ok(PreparedRpc {
        method: if join {
            "chat.channel_join"
        } else {
            "chat.channel_leave"
        },
        params: json!({ "channel": channel }),
        shape: OutputShape::ChannelMembership,
    })
}

fn prep_channel_mark_read(input: &Map<String, Value>) -> Result<PreparedRpc, ApiError> {
    let channel = require_str(input, "channel")?;
    let mut params = Map::new();
    params.insert("conversation_id".into(), Value::String(channel.to_owned()));
    insert_opt(&mut params, "last_read_message_id", opt_str(input, "up_to"));
    Ok(PreparedRpc {
        method: "chat.mark_read",
        params: Value::Object(params),
        shape: OutputShape::ChannelMarkRead,
    })
}

// --- Response shapers matching CLI output contract ---

pub(super) fn shape_msg_send(raw: Value) -> Result<Value, ApiError> {
    let res: ChatSendResult = serde_json::from_value(raw).map_err(|_| unreadable("chat.send"))?;
    Ok(json!({
        "id": res.id,
        "conversation_id": res.conversation_id,
        "thread_id": res.thread_id,
        "local_seq": res.local_seq,
        "origin_seq": res.origin_seq,
        "mentions": res.mentions,
        "unmatched_mentions": res.unmatched_mentions,
        "recipient_presence": res.recipient_presence,
        "woke_agents": res.woke_agents,
    }))
}

pub(super) fn shape_msg_ask(raw: Value) -> Result<Value, ApiError> {
    let res: ChatAskResult = serde_json::from_value(raw).map_err(|_| unreadable("chat.ask"))?;
    let mut obj = Map::new();
    obj.insert("ask_id".into(), Value::String(res.ask_id));
    obj.insert("conversation_id".into(), Value::String(res.conversation_id));
    obj.insert("thread_id".into(), Value::String(res.thread_id));
    obj.insert("local_seq".into(), Value::from(res.local_seq));
    obj.insert("origin_seq".into(), Value::from(res.origin_seq));
    obj.insert(
        "status".into(),
        serde_json::to_value(res.status).unwrap_or(Value::String("waiting".into())),
    );
    insert_opt(&mut obj, "wait_until", res.wait_until.as_deref());
    if let Some(rem) = res.remaining_ms {
        obj.insert("remaining_ms".into(), Value::from(rem));
    }
    if let Some(ans) = res.answer.as_ref() {
        obj.insert(
            "answer".into(),
            Value::String(serde_json::to_string(ans).unwrap_or_default()),
        );
    }
    if let Some(wd) = res.withdrawal.as_ref() {
        obj.insert(
            "withdrawal".into(),
            Value::String(serde_json::to_string(wd).unwrap_or_default()),
        );
    }
    if !res.defaults.is_empty() {
        obj.insert(
            "defaults".into(),
            Value::String(serde_json::to_string(&res.defaults).unwrap_or_default()),
        );
    }
    insert_opt(&mut obj, "required_notice", res.required_notice.as_deref());
    if let Some(watched) = res.watched {
        obj.insert("watched".into(), Value::Bool(watched));
    }
    insert_opt(
        &mut obj,
        "watch_not_started_reason",
        res.watch_not_started_reason.as_deref(),
    );
    obj.insert("recipient_presence".into(), json!(res.recipient_presence));
    obj.insert("woke_agents".into(), json!(res.woke_agents));
    Ok(Value::Object(obj))
}

pub(super) fn shape_msg_withdraw(raw: Value) -> Result<Value, ApiError> {
    let res: MsgWithdrawResult =
        serde_json::from_value(raw).map_err(|_| unreadable("msg.withdraw"))?;
    let mut obj = Map::new();
    obj.insert("ask_id".into(), Value::String(res.ask_id));
    obj.insert("conversation_id".into(), Value::String(res.conversation_id));
    obj.insert("by".into(), Value::String(res.withdrawal.by));
    obj.insert("at".into(), Value::String(res.withdrawal.at));
    insert_opt(&mut obj, "reason", res.withdrawal.reason.as_deref());
    Ok(Value::Object(obj))
}

pub(super) fn shape_msg_done(raw: Value) -> Result<Value, ApiError> {
    let res: MsgDoneResult = serde_json::from_value(raw).map_err(|_| unreadable("msg.done"))?;
    Ok(json!({
        "id": res.id,
        "state": "done",
        "resolved_at": res.resolved_at,
    }))
}

pub(super) fn shape_msg_defer(raw: Value) -> Result<Value, ApiError> {
    let res: MsgDeferResult = serde_json::from_value(raw).map_err(|_| unreadable("msg.defer"))?;
    let mut obj = Map::new();
    obj.insert("id".into(), Value::String(res.id));
    obj.insert("state".into(), Value::String("snoozed".into()));
    obj.insert("triage_gen".into(), Value::from(res.triage_gen));
    insert_opt(&mut obj, "snoozed_until", res.snoozed_until.as_deref());
    obj.insert(
        "snooze_after_task".into(),
        Value::Bool(res.snooze_after_task),
    );
    Ok(Value::Object(obj))
}

pub(super) fn shape_msg_unsnooze(raw: Value) -> Result<Value, ApiError> {
    let res: MsgUnsnoozeResult =
        serde_json::from_value(raw).map_err(|_| unreadable("msg.unsnooze"))?;
    Ok(json!({
        "id": res.id,
        "state": res.state,
        "triage_gen": res.triage_gen,
    }))
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

pub(super) fn shape_channel_create(raw: Value) -> Result<Value, ApiError> {
    let res: ChatChannelCreateResult =
        serde_json::from_value(raw).map_err(|_| unreadable("chat.channel_create"))?;
    Ok(channel_summary_value(res.channel))
}

pub(super) fn shape_channel_membership(method: &str, raw: Value) -> Result<Value, ApiError> {
    let res: ChatChannelMembershipResult =
        serde_json::from_value(raw).map_err(|_| unreadable(method))?;
    let mut obj = Map::new();
    obj.insert("conversation_id".into(), Value::String(res.channel.id));
    insert_opt(&mut obj, "name", res.channel.name.as_deref());
    obj.insert("is_member".into(), Value::Bool(res.joined));
    Ok(Value::Object(obj))
}

pub(super) fn shape_channel_mark_read(raw: Value) -> Result<Value, ApiError> {
    let res: ChatMarkReadResult =
        serde_json::from_value(raw).map_err(|_| unreadable("chat.mark_read"))?;
    let mut obj = Map::new();
    obj.insert("conversation_id".into(), Value::String(res.conversation_id));
    insert_opt(
        &mut obj,
        "last_read_message_id",
        res.last_read_message_id.as_deref(),
    );
    obj.insert("read_at".into(), Value::String(res.read_at));
    obj.insert("advanced".into(), Value::Bool(res.advanced));
    Ok(Value::Object(obj))
}
