//! `msg send|reply|done|defer|unsnooze`: sending and replying to messages, answering an ask,
//! and triaging the inbox.

use org_command::{Ack, Answer, Command, Example, IfMatch, Kind, Refusal, Request};
use org_proto::attachments::MESSAGE_ATTACHMENTS_MAX;
use org_proto::chat::ChatSendResult;
use org_proto::msg::SendResult;
use org_proto::ErrorCode;
use serde_json::{json, Map, Value};

use super::msg_ask::write_msg_answer;
use super::schema::{
    MSG_DEFER_INPUT, MSG_DEFER_OUTPUT, MSG_DONE_INPUT, MSG_DONE_OUTPUT, MSG_REPLY_INPUT,
    MSG_REPLY_OUTPUT, MSG_SEND_INPUT, MSG_SEND_OUTPUT, MSG_UNSNOOZE_INPUT, MSG_UNSNOOZE_OUTPUT,
    WRITE_ERRORS,
};
use super::{insert_delivery, message_id_for, with_ack};
use crate::runtime::domain::CommandCue;
use crate::runtime::file_transfer::resolve_attach_ids;
use crate::runtime::input::{
    decode_err, insert_opt, opt_bool, opt_str, opt_str_list, reject_dry_run, require_str,
};
use crate::runtime::transport::OfficeTransport;

/// The `msg send`, `msg reply`, `msg done`, `msg defer` and `msg unsnooze` rows.
pub(super) const COMMANDS: &[Command] = &[
    Command {
        resource: "msg",
        verb: "send",
        summary: "Send DM or #channel message",
        call: "chat.send",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: MSG_SEND_INPUT,
        content_field: Some("body"),
        output: MSG_SEND_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r##"{"to":"#general","body":"Status update for @ada"}"##,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000101","conversation_id":"0193a5c0-0000-7000-8000-000000000100","thread_id":"0193a5c0-0000-7000-8000-000000000101","local_seq":1,"origin_seq":1,"mentions":[{"employee_id":"0193a5c0-0000-7000-8000-000000000004","handle":"ada","display_name":"Ada"}],"unmatched_mentions":[],"recipient_presence":[{"member_id":"0193a5c0-0000-7000-8000-000000000004","presence":"asleep"}],"woke_agents":["0193a5c0-0000-7000-8000-000000000004"]}"#,
        },
    },
    Command {
        resource: "msg",
        verb: "reply",
        summary: "Reply to message or ask",
        call: "msg.reply",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: MSG_REPLY_INPUT,
        content_field: Some("body"),
        output: MSG_REPLY_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"id":"0193a5c0-0000-7000-8000-000000000101","body":"On it."}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000102","thread_id":"0193a5c0-0000-7000-8000-000000000101","local_seq":2,"origin_seq":2}"#,
        },
    },
    Command {
        resource: "msg",
        verb: "done",
        summary: "Mark inbox message done",
        call: "msg.done",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: MSG_DONE_INPUT,
        content_field: None,
        output: MSG_DONE_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"id":"0193a5c0-0000-7000-8000-000000000101"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000101","state":"done","resolved_at":"2026-11-21T12:03:00.000Z"}"#,
        },
    },
    Command {
        resource: "msg",
        verb: "defer",
        summary: "Snooze or defer a message",
        call: "msg.defer",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: MSG_DEFER_INPUT,
        content_field: None,
        output: MSG_DEFER_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"id":"0193a5c0-0000-7000-8000-000000000101","after_task":true}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000101","state":"snoozed","triage_gen":1,"snooze_after_task":true}"#,
        },
    },
    Command {
        resource: "msg",
        verb: "unsnooze",
        summary: "Un-snooze a message early",
        call: "msg.unsnooze",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: MSG_UNSNOOZE_INPUT,
        content_field: None,
        output: MSG_UNSNOOZE_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"id":"0193a5c0-0000-7000-8000-000000000101"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000101","state":"unread","triage_gen":2}"#,
        },
    },
];

/// `Use when:` guidance of each `msg` command here.
pub(super) const CUES: &[CommandCue] = &[
    ("msg", "defer", "snoozing an inbox message until a timestamp (`--until`) or until your current task finishes (`--after-task`)."),
    ("msg", "done", "marking an inbox message handled so it leaves your unread queue."),
    ("msg", "reply", "replying in an existing message thread or submitting structured `--answer` JSON to an open ask."),
    ("msg", "send", "sending a direct message (DM to a handle/ID/owner), `#channel` post, handoff, or announcement."),
    ("msg", "unsnooze", "pulling a previously snoozed message back to unread before its wake trigger fires."),
];

pub(super) fn write_msg_send<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    reject_dry_run(request)?;
    let to = require_str(request, "to")?;
    let body = opt_str(request, "body").unwrap_or("");
    let attach = opt_str_list(request, "attach").unwrap_or_default();
    let att_ids = opt_str_list(request, "attachment_id").unwrap_or_default();
    if body.trim().is_empty() && attach.is_empty() && att_ids.is_empty() {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "`msg send` requires `--body` or at least one `--attach` / `--attachment-id`.",
            "Pass `--body <text>` (or `--input-file <path>`) or attach a file with `--attach <path>`.",
        ));
    }
    let id = message_id_for(request)?;
    let resolved_ids = resolve_attach_ids(transport, &attach, &att_ids, MESSAGE_ATTACHMENTS_MAX)?;
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
    if !resolved_ids.is_empty() {
        params.insert("attachment_ids".into(), json!(resolved_ids));
    }
    let raw = transport.call_method("chat.send", Value::Object(params))?;
    let res: ChatSendResult = serde_json::from_value(raw).map_err(|_| decode_err("chat.send"))?;
    let mut obj = Map::new();
    obj.insert("id".into(), json!(res.id));
    obj.insert("conversation_id".into(), json!(res.conversation_id));
    obj.insert("thread_id".into(), json!(res.thread_id));
    obj.insert("local_seq".into(), json!(res.local_seq));
    obj.insert("origin_seq".into(), json!(res.origin_seq));
    obj.insert("mentions".into(), json!(res.mentions));
    obj.insert("unmatched_mentions".into(), json!(res.unmatched_mentions));
    insert_delivery(&mut obj, &res.recipient_presence, &res.woke_agents);
    Ok(with_ack(Answer::completed(Value::Object(obj)), request))
}

pub(super) fn write_msg_reply<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    reject_dry_run(request)?;
    let to_id = require_str(request, "id")?;
    if let Some(answer_raw) = opt_str(request, "answer") {
        // An answer to an ask carries no text of its own: a flag it would drop is refused.
        if let Some(flag) = ["body", "subject", "priority"]
            .into_iter()
            .find(|name| request.input.contains_key(*name))
        {
            return Err(Refusal::new(
                ErrorCode::InvalidRequest,
                format!("--answer cannot be combined with --{flag}: an answer to an ask carries no text, subject or priority."),
                "Answer the ask with --id and --answer alone, or reply in its thread with --body and no --answer.",
            ));
        }
        return write_msg_answer(transport, request, to_id, answer_raw);
    }
    let attach = opt_str_list(request, "attach").unwrap_or_default();
    let att_ids = opt_str_list(request, "attachment_id").unwrap_or_default();
    let body = match opt_str(request, "body") {
        Some(text) => text,
        None if !attach.is_empty() || !att_ids.is_empty() => "",
        None => {
            return Err(Refusal::new(
                ErrorCode::InvalidRequest,
                "`msg reply` needs --body, --attach/--attachment-id, or --answer to answer an ask.",
                "Pass --body <text> (or --input-file <path>) to reply, --attach <path> to attach a file, or --answer <json> to answer an ask.",
            ));
        }
    };
    let id = message_id_for(request)?;
    let resolved_ids = resolve_attach_ids(transport, &attach, &att_ids, MESSAGE_ATTACHMENTS_MAX)?;
    let mut params = Map::new();
    params.insert("id".into(), Value::String(id));
    params.insert("to_id".into(), Value::String(to_id.to_owned()));
    params.insert("body".into(), Value::String(body.to_owned()));
    insert_opt(&mut params, "subject", opt_str(request, "subject"));
    insert_opt(&mut params, "priority", opt_str(request, "priority"));
    if !resolved_ids.is_empty() {
        params.insert("attachment_ids".into(), json!(resolved_ids));
    }
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

pub(super) fn write_msg_done<T: OfficeTransport + ?Sized>(
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

pub(super) fn write_msg_defer<T: OfficeTransport + ?Sized>(
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

pub(super) fn write_msg_unsnooze<T: OfficeTransport + ?Sized>(
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
