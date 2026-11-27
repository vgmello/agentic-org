//! Asks: asking, withdrawing, answering and accepting, with their outcome fields.

use org_command::{Answer, Refusal, Request, Status};
use org_proto::ask::{
    AskStatus, ChatAnswerResult, ChatAskResult, MsgWithdrawResult, MAX_SHORT_HOLD_MS,
};
use org_proto::attachments::MESSAGE_ATTACHMENTS_MAX;
use org_proto::ErrorCode;
use serde_json::{json, Map, Value};

use super::msg_ask_input::questions;
use super::{insert_delivery, message_id_for, with_ack, AskOutcomeRef};
use crate::runtime::domain::CommandCue;
use crate::runtime::file_transfer::resolve_attach_ids;
use crate::runtime::input::{
    decode_err, insert_opt, opt_bool, opt_str, opt_str_list, opt_u64, reject_dry_run, require_str,
};
use crate::runtime::transport::OfficeTransport;

/// `Use when:` guidance of each `msg` command here.
pub(super) const CUES: &[CommandCue] = &[
    ("msg", "accept", "explicitly claiming ownership of a handoff message addressed to you."),
    ("msg", "ask", "asking typed questions (choice/yes_no/free_text) or requesting approval with bounded wait instead of free-form `msg send`."),
    ("msg", "withdraw", "cancelling an open structured ask you sent when the answer is no longer needed."),
];

pub(super) fn insert_ask_outcome_fields(obj: &mut Map<String, Value>, outcome: AskOutcomeRef<'_>) {
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

/// `meta.status` of an ask's outcome, so `meta` and `data.status` never disagree: an answered
/// ask completed; one still open after the short hold is `waiting`; a required one the agent
/// must wind down for is `suspended`; a timeout and a withdrawal say so.
pub(super) fn ask_meta_status(status: AskStatus) -> Status {
    match status {
        AskStatus::Answered => Status::Completed,
        AskStatus::TimedOut => Status::TimedOut,
        AskStatus::RequiredPending => Status::Suspended,
        AskStatus::Withdrawn => Status::Withdrawn,
        _ => Status::Waiting,
    }
}

pub(super) fn write_msg_ask<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    reject_dry_run(request)?;
    let to = require_str(request, "to")?;
    let questions = questions(request)?;
    let id = message_id_for(request)?;
    let wait_ms = opt_u64(request, "wait_ms")?;
    let hold_ms = opt_u64(request, "hold_ms")?
        .unwrap_or(MAX_SHORT_HOLD_MS)
        .min(MAX_SHORT_HOLD_MS);
    let attach = opt_str_list(request, "attach").unwrap_or_default();
    let att_ids = opt_str_list(request, "attachment_id").unwrap_or_default();
    let resolved = resolve_attach_ids(transport, &attach, &att_ids, MESSAGE_ATTACHMENTS_MAX)?;
    let mut params = Map::new();
    params.insert("id".into(), Value::String(id));
    params.insert("to".into(), Value::String(to.to_owned()));
    params.insert("questions".into(), questions);
    if !resolved.ids.is_empty() {
        params.insert("attachment_ids".into(), json!(resolved.ids));
    }
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
    if let Some(wait_ms) = wait_ms {
        params.insert("wait_ms".into(), Value::from(wait_ms));
    }
    params.insert("hold_ms".into(), Value::from(hold_ms));
    let timeout = std::time::Duration::from_millis(hold_ms) + org_client::DEFAULT_TIMEOUT;
    let raw = match transport.call_method_with_timeout("chat.ask", Value::Object(params), timeout) {
        Ok(raw) => raw,
        Err(err) => {
            resolved.abort_staged(transport);
            return Err(err);
        }
    };
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
    if let Some(watched) = res.watched {
        obj.insert("watched".into(), Value::Bool(watched));
    }
    insert_opt(
        &mut obj,
        "watch_not_started_reason",
        res.watch_not_started_reason.as_deref(),
    );
    insert_delivery(&mut obj, &res.recipient_presence, &res.woke_agents);
    let mut answer = with_ack(Answer::completed(Value::Object(obj)), request);
    answer.status = ask_meta_status(res.status);
    Ok(answer)
}

pub(super) fn write_msg_withdraw<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    reject_dry_run(request)?;
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

pub(super) fn write_msg_answer<T: OfficeTransport + ?Sized>(
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
    let existing_att_ids = existing_answer_attachment_ids(&params)?;
    let attach = opt_str_list(request, "attach").unwrap_or_default();
    let att_ids = opt_str_list(request, "attachment_id").unwrap_or_default();
    let total_att = existing_att_ids
        .len()
        .saturating_add(attach.len())
        .saturating_add(att_ids.len());
    if total_att > MESSAGE_ATTACHMENTS_MAX {
        return Err(Refusal::new(
            ErrorCode::TooLarge,
            format!(
                "At most {MESSAGE_ATTACHMENTS_MAX} attachments are allowed here (got {total_att})."
            ),
            "Attach fewer files and retry.",
        ));
    }
    let remaining_cap = MESSAGE_ATTACHMENTS_MAX.saturating_sub(existing_att_ids.len());
    let resolved = resolve_attach_ids(transport, &attach, &att_ids, remaining_cap)?;
    if !resolved.ids.is_empty() {
        let mut combined = existing_att_ids;
        combined.extend(resolved.ids.iter().cloned());
        params.insert("attachment_ids".into(), json!(combined));
    }
    params.insert("ask_id".into(), Value::String(ask_id.to_owned()));
    let raw = match transport.call_method("chat.answer", Value::Object(params)) {
        Ok(raw) => raw,
        Err(err) => {
            resolved.abort_staged(transport);
            return Err(err);
        }
    };
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

fn existing_answer_attachment_ids(params: &Map<String, Value>) -> Result<Vec<String>, Refusal> {
    match params.get("attachment_ids") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(arr)) => {
            let mut ids = Vec::with_capacity(arr.len());
            for item in arr {
                let Some(id) = item.as_str() else {
                    return Err(Refusal::new(
                        ErrorCode::InvalidRequest,
                        "`--answer` `attachment_ids` entries must be strings.",
                        "Pass an array of attachment UUIDv7 strings in `attachment_ids`.",
                    ));
                };
                ids.push(id.to_owned());
            }
            Ok(ids)
        }
        Some(_) => Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "`--answer` `attachment_ids` must be an array of attachment UUIDv7 strings.",
            "Pass an array of attachment UUIDv7 strings or omit `attachment_ids`.",
        )),
    }
}

pub(super) fn write_msg_accept<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    reject_dry_run(request)?;
    let id = require_str(request, "id")?;
    let mut params = Map::new();
    params.insert("id".into(), Value::String(id.to_owned()));
    insert_opt(&mut params, "reason", opt_str(request, "reason"));
    let raw = transport.call_method("msg.accept", Value::Object(params))?;
    Ok(with_ack(Answer::completed(raw), request))
}
