//! Dispatch for B6 `msg`, `channel`, and `status` commands over [`OfficeTransport`] (`TASK-45`).

use org_command::{Ack, Answer, Refusal, Request};
use org_proto::ask::{
    AskStatus, ChatAnswerResult, ChatAskResult, ChatWaitAskResult, MsgWithdrawResult,
    MAX_SHORT_HOLD_MS,
};
use org_proto::chat::{ChatChannelsResult, ChatInboxResult, ChatMessagesResult, ChatSendResult};
use org_proto::directory::StatusSetResult;
use org_proto::msg::{HeaderPage, MsgReadResult, MsgWaitResult, SendResult, MAX_WAIT_MS};
use org_proto::ErrorCode;
use serde_json::{json, Map, Value};

use crate::dispatch::OfficeTransport;
use crate::keys::try_new_uuid_v7;

/// Attempts to execute a B6 `msg`, `channel`, or `status` command.
pub fn try_execute<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    match request.command.resource {
        "msg" => try_msg(transport, request),
        "channel" => try_channel(transport, request),
        "status" => try_status(transport, request),
        _ => None,
    }
}

fn try_msg<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    match request.command.verb {
        "send" => Some(write_msg_send(transport, request)),
        "ask" => Some(write_msg_ask(transport, request)),
        "withdraw" => Some(write_msg_withdraw(transport, request)),
        "reply" => Some(write_msg_reply(transport, request)),
        "accept" => Some(write_msg_accept(transport, request)),
        "inbox" => Some(read_msg_inbox(transport, request)),
        "read" => Some(read_msg_read(transport, request)),
        "list" => Some(read_msg_list(transport, request)),
        "done" => Some(write_msg_done(transport, request)),
        "defer" => Some(write_msg_defer(transport, request)),
        "wait" => Some(read_msg_wait(transport, request)),
        _ => None,
    }
}

fn try_channel<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    match request.command.verb {
        "list" => Some(read_channel_list(transport, request)),
        "join" => Some(write_channel_membership(transport, request, true)),
        "leave" => Some(write_channel_membership(transport, request, false)),
        _ => None,
    }
}

fn try_status<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    match request.command.verb {
        "set" => Some(write_status(transport, request, false)),
        "clear" => Some(write_status(transport, request, true)),
        _ => None,
    }
}

fn message_id_for(request: &Request) -> Result<String, Refusal> {
    if let Some(key) = &request.idempotency_key {
        if org_proto::is_uuid_v7(key) {
            return Ok(key.clone());
        }
    }
    try_new_uuid_v7().ok_or_else(|| {
        Refusal::new(
            ErrorCode::Unavailable,
            "Could not mint a canonical UUIDv7 message ID.",
            "Retry the command or supply a UUIDv7 --idempotency-key.",
        )
    })
}

fn write_msg_send<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let to = require_str(request, "to")?;
    let body = require_str(request, "body")?;
    let id = message_id_for(request)?;
    let mut params = Map::new();
    params.insert("id".into(), Value::String(id));
    params.insert("to".into(), Value::String(to.to_owned()));
    params.insert("body".into(), Value::String(body.to_owned()));
    params.insert(
        "kind".into(),
        Value::String(opt_str(request, "kind").unwrap_or("msg").to_owned()),
    );
    insert_opt(&mut params, "priority", opt_str(request, "priority"));
    insert_opt(&mut params, "subject", opt_str(request, "subject"));
    insert_opt(&mut params, "reply_to", opt_str(request, "reply_to"));
    insert_opt(&mut params, "reason", opt_str(request, "reason"));
    if opt_bool(request, "also_in_channel").unwrap_or(false) {
        params.insert("also_in_channel".into(), Value::Bool(true));
    }
    let raw = transport.call_method("chat.send", Value::Object(params))?;
    let res: ChatSendResult = serde_json::from_value(raw).map_err(|_| decode_err("chat.send"))?;
    let val = json!({
        "id": res.id,
        "conversation_id": res.conversation_id,
        "thread_id": res.thread_id,
        "local_seq": res.local_seq,
        "origin_seq": res.origin_seq,
        "mentions": res.mentions,
        "unmatched_mentions": res.unmatched_mentions,
    });
    Ok(with_ack(Answer::completed(val), request))
}

struct AskOutcomeRef<'a> {
    status: AskStatus,
    wait_until: Option<&'a str>,
    remaining_ms: Option<u64>,
    answer: Option<&'a org_proto::ask::AskAnswerRecord>,
    withdrawal: Option<&'a org_proto::ask::AskWithdrawalRecord>,
    defaults: &'a [org_proto::ask::AskDefaultEntry],
    required_notice: Option<&'a str>,
}

fn insert_ask_outcome_fields(obj: &mut Map<String, Value>, outcome: AskOutcomeRef<'_>) {
    obj.insert(
        "status".into(),
        serde_json::to_value(outcome.status).unwrap_or(Value::String("waiting".into())),
    );
    insert_opt(obj, "wait_until", outcome.wait_until);
    if let Some(rem) = outcome.remaining_ms {
        obj.insert("remaining_ms".into(), Value::from(rem));
    }
    if let Some(ans) = outcome.answer {
        obj.insert(
            "answer".into(),
            Value::String(serde_json::to_string(ans).unwrap_or_default()),
        );
    }
    if let Some(wd) = outcome.withdrawal {
        obj.insert(
            "withdrawal".into(),
            Value::String(serde_json::to_string(wd).unwrap_or_default()),
        );
    }
    if !outcome.defaults.is_empty() {
        obj.insert(
            "defaults".into(),
            Value::String(serde_json::to_string(outcome.defaults).unwrap_or_default()),
        );
    }
    insert_opt(obj, "required_notice", outcome.required_notice);
}

fn write_msg_ask<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let to = require_str(request, "to")?;
    let Some(questions) = request.input.get("questions") else {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "Missing required `questions` array.",
            "Pass `--json` with 1..=8 typed questions.",
        ));
    };
    let id = message_id_for(request)?;
    let mut params = Map::new();
    params.insert("id".into(), Value::String(id));
    params.insert("to".into(), Value::String(to.to_owned()));
    params.insert("questions".into(), questions.clone());
    insert_opt(&mut params, "body", opt_str(request, "body"));
    insert_opt(&mut params, "subject", opt_str(request, "subject"));
    insert_opt(&mut params, "blocks", opt_str(request, "blocks"));
    insert_opt(
        &mut params,
        "addressed_to",
        opt_str(request, "addressed_to"),
    );
    if opt_bool(request, "required").unwrap_or(false) {
        params.insert("required".into(), Value::Bool(true));
    }
    if opt_bool(request, "approval").unwrap_or(false) {
        params.insert("approval".into(), Value::Bool(true));
    }
    if let Some(wait_ms) = opt_non_neg_u64(request, "wait_ms")? {
        params.insert("wait_ms".into(), Value::from(wait_ms));
    }
    let hold_ms = opt_non_neg_u64(request, "hold_ms")?
        .unwrap_or(MAX_SHORT_HOLD_MS)
        .min(MAX_SHORT_HOLD_MS);
    params.insert("hold_ms".into(), Value::from(hold_ms));
    let timeout = std::time::Duration::from_millis(hold_ms) + org_client::DEFAULT_TIMEOUT;
    let raw = transport.call_method_with_timeout("chat.ask", Value::Object(params), timeout)?;
    let res: ChatAskResult = serde_json::from_value(raw).map_err(|_| decode_err("chat.ask"))?;
    let mut obj = Map::new();
    obj.insert("ask_id".into(), Value::String(res.ask_id));
    obj.insert("conversation_id".into(), Value::String(res.conversation_id));
    obj.insert("thread_id".into(), Value::String(res.thread_id));
    obj.insert("local_seq".into(), Value::from(res.local_seq));
    obj.insert("origin_seq".into(), Value::from(res.origin_seq));
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
    Ok(with_ack(Answer::completed(Value::Object(obj)), request))
}

fn write_msg_withdraw<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let ask_id = require_str(request, "id")?;
    let mut params = Map::new();
    params.insert("ask_id".into(), Value::String(ask_id.to_owned()));
    insert_opt(&mut params, "reason", opt_str(request, "reason"));
    let raw = transport.call_method("msg.withdraw", Value::Object(params))?;
    let res: MsgWithdrawResult =
        serde_json::from_value(raw).map_err(|_| decode_err("msg.withdraw"))?;
    let mut obj = Map::new();
    obj.insert("ask_id".into(), Value::String(res.ask_id));
    obj.insert("conversation_id".into(), Value::String(res.conversation_id));
    obj.insert("by".into(), Value::String(res.withdrawal.by));
    obj.insert("at".into(), Value::String(res.withdrawal.at));
    insert_opt(&mut obj, "reason", res.withdrawal.reason.as_deref());
    Ok(with_ack(Answer::completed(Value::Object(obj)), request))
}

fn write_msg_answer<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    ask_id: &str,
    answer_raw: &str,
) -> Result<Answer, Refusal> {
    let parsed: Value = serde_json::from_str(answer_raw).map_err(|_| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            "`--answer` must be valid JSON (an array of QuestionAnswer or an answer object).",
            "Pass e.g. `[{\"question_id\":\"q1\",\"option_ids\":[\"dev\"]}]`.",
        )
    })?;
    let mut params = Map::new();
    match parsed {
        Value::Array(arr) => {
            params.insert("answers".into(), Value::Array(arr));
        }
        Value::Object(map) => {
            if let Some(existing) = map.get("ask_id") {
                if existing.as_str() != Some(ask_id) {
                    return Err(Refusal::new(
                        ErrorCode::InvalidRequest,
                        "`--answer` contains a conflicting or invalid `ask_id`.",
                        "Omit `ask_id` inside `--answer` or match `--id`.",
                    ));
                }
            }
            for (k, v) in map {
                params.insert(k, v);
            }
        }
        _ => {
            return Err(Refusal::new(
                ErrorCode::InvalidRequest,
                "`--answer` must be a JSON array or object.",
                "Pass `[{\"question_id\":\"q1\",\"option_ids\":[\"yes\"]}]`.",
            ));
        }
    }
    params.insert("ask_id".into(), Value::String(ask_id.to_owned()));
    let raw = transport.call_method("chat.answer", Value::Object(params))?;
    let res: ChatAnswerResult =
        serde_json::from_value(raw).map_err(|_| decode_err("chat.answer"))?;
    let mut obj = Map::new();
    obj.insert("id".into(), Value::String(res.ask_id));
    obj.insert("conversation_id".into(), Value::String(res.conversation_id));
    obj.insert(
        "answer".into(),
        Value::String(serde_json::to_string(&res.answer).unwrap_or_default()),
    );
    Ok(with_ack(Answer::completed(Value::Object(obj)), request))
}

fn write_msg_reply<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let to_id = require_str(request, "id")?;
    if let Some(answer_raw) = opt_str(request, "answer") {
        return write_msg_answer(transport, request, to_id, answer_raw);
    }
    let body = require_str(request, "body")?;
    let id = message_id_for(request)?;
    let mut params = Map::new();
    params.insert("id".into(), Value::String(id));
    params.insert("to_id".into(), Value::String(to_id.to_owned()));
    params.insert("body".into(), Value::String(body.to_owned()));
    insert_opt(&mut params, "subject", opt_str(request, "subject"));
    insert_opt(&mut params, "priority", opt_str(request, "priority"));
    let raw = transport.call_method("msg.reply", Value::Object(params))?;
    let res: SendResult = serde_json::from_value(raw).map_err(|_| decode_err("msg.reply"))?;
    Ok(with_ack(
        Answer::completed(json!({
            "id": res.id,
            "thread_id": res.thread_id,
            "local_seq": res.local_seq,
            "origin_seq": res.origin_seq,
        })),
        request,
    ))
}

fn write_msg_accept<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let id = require_str(request, "id")?;
    let mut params = Map::new();
    params.insert("id".into(), Value::String(id.to_owned()));
    insert_opt(&mut params, "reason", opt_str(request, "reason"));
    let raw = transport.call_method("msg.accept", Value::Object(params))?;
    Ok(with_ack(Answer::completed(raw), request))
}

fn read_msg_inbox<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let mut params = Map::new();
    if opt_bool(request, "all").unwrap_or(false) {
        params.insert("include_all".into(), Value::Bool(true));
    }
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

fn read_msg_read<T: OfficeTransport + ?Sized>(
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

fn read_msg_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let conversation = opt_str(request, "conversation").or_else(|| opt_str(request, "channel"));
    let thread = opt_str(request, "thread");
    let participant = opt_str(request, "participant");
    if conversation.is_some() || (thread.is_some() && participant.is_none()) {
        let mut params = Map::new();
        insert_opt(&mut params, "conversation", conversation);
        insert_opt(&mut params, "thread_id", thread);
        if let Some(limit) = request.limit {
            params.insert("limit".into(), Value::from(limit));
        }
        if let Some(cursor) = parse_u64_cursor(request.cursor.as_deref())? {
            params.insert("cursor".into(), Value::from(cursor));
        }
        let raw = transport.call_method("chat.messages", Value::Object(params))?;
        let res: ChatMessagesResult =
            serde_json::from_value(raw).map_err(|_| decode_err("chat.messages"))?;
        let items: Vec<Value> = res
            .messages
            .into_iter()
            .map(|m| {
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
                o.insert("body".into(), Value::String(m.body));
                o.insert("created_at".into(), Value::String(m.created_at));
                Value::Object(o)
            })
            .collect();
        let mut answer = Answer::completed(Value::Array(items));
        answer.next_cursor = res.next_cursor.map(|c| c.to_string());
        return Ok(answer);
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

fn write_msg_done<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let id = require_str(request, "id")?;
    let mut params = Map::new();
    params.insert("id".into(), Value::String(id.to_owned()));
    insert_opt(&mut params, "note", opt_str(request, "note"));
    let raw = transport.call_method("msg.done", Value::Object(params))?;
    let res: org_proto::msg::MsgDoneResult =
        serde_json::from_value(raw).map_err(|_| decode_err("msg.done"))?;
    Ok(with_ack(
        Answer::completed(json!({
            "id": res.id,
            "state": "done",
            "resolved_at": res.resolved_at,
        })),
        request,
    ))
}

fn write_msg_defer<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let id = require_str(request, "id")?;
    let until = match (opt_str(request, "until"), opt_bool(request, "after_task")) {
        (Some(at), _) => org_proto::msg::SnoozeTarget::Until { at: at.to_owned() },
        (None, Some(true)) => org_proto::msg::SnoozeTarget::AfterTask,
        _ => {
            return Err(Refusal::new(
                ErrorCode::InvalidRequest,
                "Pass either `--until <rfc3339>` or `--after-task` to defer a message.",
                "Run `agent-os msg defer --help` for usage.",
            ));
        }
    };
    let wire = org_proto::msg::MsgDeferParams {
        id: id.to_owned(),
        until,
        note: opt_str(request, "note").map(str::to_owned),
    };
    let params = serde_json::to_value(&wire).map_err(|_| decode_err("msg.defer"))?;
    let raw = transport.call_method("msg.defer", params)?;
    let res: org_proto::msg::MsgDeferResult =
        serde_json::from_value(raw).map_err(|_| decode_err("msg.defer"))?;
    let mut obj = Map::new();
    obj.insert("id".into(), Value::String(res.id));
    obj.insert("state".into(), Value::String("snoozed".into()));
    obj.insert("triage_gen".into(), Value::from(res.triage_gen));
    insert_opt(&mut obj, "snoozed_until", res.snoozed_until.as_deref());
    obj.insert(
        "snooze_after_task".into(),
        Value::Bool(res.snooze_after_task),
    );
    Ok(with_ack(Answer::completed(Value::Object(obj)), request))
}

fn read_msg_wait_ask<T: OfficeTransport + ?Sized>(
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
    Ok(Answer::completed(Value::Object(obj)))
}

fn read_msg_wait<T: OfficeTransport + ?Sized>(
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

fn read_channel_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let include_archived = opt_bool(request, "include_archived").unwrap_or(false);
    let raw = transport.call_method(
        "chat.channels",
        json!({ "include_archived": include_archived }),
    )?;
    let res: ChatChannelsResult =
        serde_json::from_value(raw).map_err(|_| decode_err("chat.channels"))?;
    let items: Vec<Value> = res
        .channels
        .into_iter()
        .map(|c| {
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
            Value::Object(o)
        })
        .collect();
    Ok(Answer::completed(Value::Array(items)))
}

fn write_channel_membership<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    join: bool,
) -> Result<Answer, Refusal> {
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

fn write_status<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    clear: bool,
) -> Result<Answer, Refusal> {
    let mut params = Map::new();
    if clear {
        params.insert("text".into(), Value::Null);
    } else {
        let text = require_str(request, "text")?;
        params.insert("text".into(), Value::String(text.to_owned()));
        if let Some(ttl_ms) = opt_non_neg_u64(request, "ttl_ms")? {
            params.insert("ttl_ms".into(), Value::from(ttl_ms));
        }
    }
    let raw = transport.call_method("status.set", Value::Object(params))?;
    let res: StatusSetResult = serde_json::from_value(raw).map_err(|_| decode_err("status.set"))?;
    let mut obj = Map::new();
    insert_opt(&mut obj, "status", res.status.as_deref());
    insert_opt(&mut obj, "status_expires_at", res.expires_at.as_deref());
    Ok(with_ack(Answer::completed(Value::Object(obj)), request))
}

fn with_ack(mut answer: Answer, request: &Request) -> Answer {
    answer.ack = Some(if request.durable {
        Ack::Durable
    } else {
        Ack::Committed
    });
    answer
}

fn require_str<'a>(request: &'a Request, name: &str) -> Result<&'a str, Refusal> {
    opt_str(request, name).ok_or_else(|| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            format!("Missing required field `{name}`."),
            "Pass the required flag or include the field in --json.",
        )
    })
}

fn opt_str<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request.input.get(name).and_then(Value::as_str)
}

fn opt_bool(request: &Request, name: &str) -> Option<bool> {
    request.input.get(name).and_then(Value::as_bool)
}

fn opt_non_neg_u64(request: &Request, name: &str) -> Result<Option<u64>, Refusal> {
    let Some(val) = request.input.get(name) else {
        return Ok(None);
    };
    if val.is_null() {
        return Ok(None);
    }
    val.as_u64().map(Some).ok_or_else(|| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            format!("`{name}` must be a non-negative integer."),
            "Pass a non-negative millisecond value.",
        )
    })
}

fn insert_opt(map: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(v) = value {
        map.insert(key.into(), Value::String(v.to_owned()));
    }
}

fn parse_u64_cursor(cursor: Option<&str>) -> Result<Option<u64>, Refusal> {
    let Some(raw) = cursor else {
        return Ok(None);
    };
    raw.parse::<u64>().map(Some).map_err(|_| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            "Cursor must be a non-negative integer.",
            "Pass the exact `next_cursor` returned by the previous list call.",
        )
    })
}

fn decode_err(method: &str) -> Refusal {
    Refusal::new(
        ErrorCode::Unavailable,
        format!("The office returned an unreadable `{method}` response."),
        "Retry the command or check the office version.",
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::{json, Value};

    use crate::dispatch::{OfficeTransport, TransportOffice};

    struct RecordingTransport {
        last_method: Option<String>,
        last_timeout: Option<Duration>,
        response: Value,
    }

    impl OfficeTransport for RecordingTransport {
        fn call_method(
            &mut self,
            method: &str,
            _params: Value,
        ) -> Result<Value, org_command::Refusal> {
            self.last_method = Some(method.to_owned());
            self.last_timeout = None;
            Ok(self.response.clone())
        }

        fn call_method_with_timeout(
            &mut self,
            method: &str,
            _params: Value,
            timeout: Duration,
        ) -> Result<Value, org_command::Refusal> {
            self.last_method = Some(method.to_owned());
            self.last_timeout = Some(timeout);
            Ok(self.response.clone())
        }
    }

    #[test]
    fn msg_ask_and_wait_ask_pass_timeout_budget_above_short_hold() {
        let mut t = TransportOffice(RecordingTransport {
            last_method: None,
            last_timeout: None,
            response: json!({
                "status": "waiting",
                "ask_id": "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa",
                "conversation_id": "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071bb",
                "thread_id": "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa",
                "local_seq": 1,
                "origin_seq": 1
            }),
        });
        let ask_json = json!({
            "to": "owner",
            "questions": [{ "id": "q1", "question_type": "yes_no", "prompt": "Proceed?" }]
        });
        let out = crate::execute_with_office(
            "agent-os",
            &[
                "msg".into(),
                "ask".into(),
                "--json".into(),
                ask_json.to_string(),
            ],
            &mut t,
        );
        assert_eq!(out.exit_code, 0, "{}", out.stdout);
        assert_eq!(t.0.last_method.as_deref(), Some("chat.ask"));
        assert!(
            t.0.last_timeout.unwrap() >= Duration::from_secs(100),
            "expected >= 100s timeout budget, got {:?}",
            t.0.last_timeout
        );

        let wait_out = crate::execute_with_office(
            "agent-os",
            &[
                "msg".into(),
                "wait".into(),
                "--ask".into(),
                "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa".into(),
            ],
            &mut t,
        );
        assert_eq!(wait_out.exit_code, 0, "{}", wait_out.stdout);
        assert_eq!(t.0.last_method.as_deref(), Some("chat.wait_ask"));
        assert!(t.0.last_timeout.unwrap() >= Duration::from_secs(100));
    }

    #[test]
    fn negative_wait_ms_and_conflicting_answer_ask_id_are_refused() {
        let mut t = TransportOffice(RecordingTransport {
            last_method: None,
            last_timeout: None,
            response: json!({}),
        });
        let neg_json = json!({
            "to": "owner",
            "wait_ms": -5,
            "questions": [{ "id": "q1", "question_type": "yes_no", "prompt": "Proceed?" }]
        });
        let neg_out = crate::execute_with_office(
            "agent-os",
            &[
                "msg".into(),
                "ask".into(),
                "--json".into(),
                neg_json.to_string(),
            ],
            &mut t,
        );
        assert_ne!(neg_out.exit_code, 0, "{}", neg_out.stdout);

        let conflict_out = crate::execute_with_office(
            "agent-os",
            &[
                "msg".into(),
                "reply".into(),
                "--id".into(),
                "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa".into(),
                "--answer".into(),
                json!({ "ask_id": "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071ff", "option_id": "yes" })
                    .to_string(),
            ],
            &mut t,
        );
        assert_ne!(conflict_out.exit_code, 0, "{}", conflict_out.stdout);
    }
}
