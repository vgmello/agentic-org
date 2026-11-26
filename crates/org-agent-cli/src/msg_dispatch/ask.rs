//! Asks: asking, withdrawing, answering and accepting, with their outcome fields.

use super::*;

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

pub(super) fn write_msg_ask<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    reject_dry_run(request)?;
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
    if let Some(watched) = res.watched {
        obj.insert("watched".into(), Value::Bool(watched));
    }
    insert_opt(
        &mut obj,
        "watch_not_started_reason",
        res.watch_not_started_reason.as_deref(),
    );
    Ok(with_ack(Answer::completed(Value::Object(obj)), request))
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
