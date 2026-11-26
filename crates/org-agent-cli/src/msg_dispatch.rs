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

mod ask;
mod channel;
mod read;

use ask::{
    insert_ask_outcome_fields, write_msg_accept, write_msg_answer, write_msg_ask,
    write_msg_withdraw,
};
use channel::try_channel;
use read::{read_msg_inbox, read_msg_list, read_msg_read, read_msg_wait};

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

fn reject_dry_run(request: &Request) -> Result<(), Refusal> {
    if request.dry_run {
        return Err(Refusal::new(
            ErrorCode::Unsupported,
            format!(
                "`--dry-run` is not supported on `{} {}` yet.",
                request.command.resource, request.command.verb
            ),
            "Re-run without `--dry-run` to apply this change.",
        ));
    }
    Ok(())
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
        "unsnooze" => Some(write_msg_unsnooze(transport, request)),
        "wait" => Some(read_msg_wait(transport, request)),
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
    reject_dry_run(request)?;
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

fn write_msg_reply<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    reject_dry_run(request)?;
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

fn write_msg_done<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    reject_dry_run(request)?;
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
    reject_dry_run(request)?;
    let id = require_str(request, "id")?;
    let until = match (opt_str(request, "until"), opt_bool(request, "after_task")) {
        (Some(_), Some(true)) => {
            return Err(Refusal::new(
                ErrorCode::InvalidRequest,
                "Pass either `--until <rfc3339>` or `--after-task`, not both.",
                "Run `agent-os msg defer --help` for usage.",
            ));
        }
        (Some(at), _) => {
            let canonical = org_proto::canonicalize_rfc3339_utc(at).ok_or_else(|| {
                Refusal::new(
                    ErrorCode::InvalidRequest,
                    "`--until` must be a valid RFC 3339 timestamp.",
                    "Example: `2026-11-23T09:00:00Z` or `2026-11-23T11:00:00+02:00`.",
                )
            })?;
            org_proto::msg::SnoozeTarget::Until { at: canonical }
        }
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

fn write_msg_unsnooze<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    reject_dry_run(request)?;
    let id = require_str(request, "id")?;
    let wire = org_proto::msg::MsgUnsnoozeParams { id: id.to_owned() };
    let params = serde_json::to_value(&wire).map_err(|_| decode_err("msg.unsnooze"))?;
    let raw = transport.call_method("msg.unsnooze", params)?;
    let res: org_proto::msg::MsgUnsnoozeResult =
        serde_json::from_value(raw).map_err(|_| decode_err("msg.unsnooze"))?;
    Ok(with_ack(
        Answer::completed(json!({
            "id": res.id,
            "state": res.state,
            "triage_gen": res.triage_gen,
        })),
        request,
    ))
}

fn write_status<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    clear: bool,
) -> Result<Answer, Refusal> {
    reject_dry_run(request)?;
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
mod tests;
