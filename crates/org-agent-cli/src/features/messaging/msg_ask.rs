//! Asks: asking, withdrawing, answering and accepting, with their outcome fields.

use org_command::{
    Ack, Answer, Command, Example, Field, IfMatch, Kind, Refusal, Request, Status, Type,
};
use org_proto::ask::{
    AskStatus, ChatAnswerResult, ChatAskResult, MsgWithdrawResult, MAX_SHORT_HOLD_MS,
};
use org_proto::attachments::MESSAGE_ATTACHMENTS_MAX;
use org_proto::ErrorCode;
use serde_json::{json, Map, Value};

use super::msg_ask_input::questions;
use super::schema::{RECIPIENT_PRESENCE_FIELD, STRING_TYPE, WOKE_AGENTS_FIELD, WRITE_ERRORS};
use super::{insert_delivery, message_id_for, with_ack, AskOutcomeRef};
use crate::runtime::domain::CommandCue;
use crate::runtime::file_transfer::resolve_attach_ids;
use crate::runtime::input::{
    decode_err, insert_opt, opt_bool, opt_str, opt_str_list, opt_u64, reject_dry_run, require_str,
};
use crate::runtime::transport::OfficeTransport;

const ASK_OPTION_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Unique option ID"),
    Field::required("label", Type::String, "Short option label"),
    Field::optional(
        "description",
        Type::String,
        "Optional one-line option description",
    ),
];

const ASK_OPTION_TYPE: Type = Type::Object(ASK_OPTION_FIELDS);

const ASK_QUESTION_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Unique question ID"),
    Field::required(
        "question_type",
        Type::Enum(&["choice", "yes_no", "free_text"]),
        "Question component type",
    ),
    Field::required("prompt", Type::String, "Question prompt text"),
    Field::optional(
        "options",
        Type::List(&ASK_OPTION_TYPE),
        "Choice options (1..=6)",
    ),
    Field::optional(
        "allow_multiple",
        Type::Boolean,
        "Multi-select checkboxes for choice questions",
    ),
    Field::optional(
        "default_value",
        Type::String,
        "Optional default option ID or yes/no",
    ),
    Field::optional(
        "optional",
        Type::Boolean,
        "Whether the question may be skipped",
    ),
];

const ASK_QUESTION_TYPE: Type = Type::Object(ASK_QUESTION_FIELDS);

const MSG_ASK_INPUT: &[Field] = &[
    Field::required(
        "to",
        Type::String,
        "Recipient member ID, handle, owner, or #channel",
    ),
    Field::optional(
        "body",
        Type::String,
        "Optional intro body (defaults to first question prompt)",
    ),
    Field::optional("subject", Type::String, "Optional short subject"),
    Field::optional(
        "questions",
        Type::List(&ASK_QUESTION_TYPE),
        "1 to 8 typed questions (choice, yes_no, free_text); or ask one with --prompt",
    ),
    Field::optional(
        "prompt",
        Type::String,
        "One question's text, instead of a questions array",
    ),
    Field::optional(
        "question_type",
        Type::Enum(&["choice", "yes_no", "free_text"]),
        "With --prompt: the question's type (default choice with --option, else free_text)",
    ),
    Field::optional(
        "option",
        Type::List(&STRING_TYPE),
        "With --prompt: one choice, as <id> or <id>=<label> (repeat for each)",
    ),
    Field::optional(
        "default_value",
        Type::String,
        "With --prompt: the default option ID, or yes or no",
    ),
    Field::optional(
        "required",
        Type::Boolean,
        "True when the agent cannot continue without the answer (ADR-0020)",
    ),
    Field::optional(
        "wait_ms",
        Type::Integer,
        "Total bounded wait duration in ms (default 600000, max 3600000)",
    ),
    Field::optional(
        "hold_ms",
        Type::Integer,
        "Short-hold duration for this command in ms (capped at 90000)",
    ),
    Field::optional(
        "blocks",
        Type::String,
        "Optional work ref blocked by this ask",
    ),
    Field::optional(
        "approval",
        Type::Boolean,
        "True for a person-only approval card",
    ),
    Field::optional(
        "addressed_to",
        Type::String,
        "Optional explicit responder member ID or handle inside a channel",
    ),
    Field::optional(
        "attach",
        Type::List(&STRING_TYPE),
        "Local file path to upload and attach (repeatable)",
    ),
    Field::optional(
        "attachment_id",
        Type::List(&STRING_TYPE),
        "Pre-uploaded attachment UUIDv7 to attach (repeatable)",
    ),
];

const MSG_ASK_OUTPUT: &[Field] = &[
    Field::required(
        "status",
        Type::Enum(&[
            "answered",
            "waiting",
            "timed_out",
            "required_pending",
            "withdrawn",
        ]),
        "Current ask status after the short hold",
    ),
    Field::required("ask_id", Type::String, "Ask message UUIDv7"),
    Field::required("conversation_id", Type::String, "Conversation UUIDv7"),
    Field::required("thread_id", Type::String, "Thread root message ID"),
    Field::required("local_seq", Type::Integer, "Local office sequence"),
    Field::required("origin_seq", Type::Integer, "Origin office sequence"),
    Field::optional(
        "wait_until",
        Type::String,
        "RFC 3339 UTC bounded wait deadline",
    ),
    Field::optional(
        "remaining_ms",
        Type::Integer,
        "Milliseconds left before wait_until",
    ),
    Field::optional(
        "answer",
        Type::String,
        "Recorded answer JSON when status is answered",
    ),
    Field::optional(
        "watched",
        Type::Boolean,
        "Whether the channel question watcher opened a watch (ADR-0024)",
    ),
    Field::optional(
        "watch_not_started_reason",
        Type::String,
        "Why a channel watch was not opened (e.g. no_watching_model, watch_limit_reached)",
    ),
    Field::optional(
        "withdrawal",
        Type::String,
        "Withdrawal JSON when status is withdrawn",
    ),
    Field::optional(
        "defaults",
        Type::String,
        "Default answers JSON on non-required timeout (never populated for required asks)",
    ),
    Field::optional(
        "required_notice",
        Type::String,
        "Wind-down notice when a required ask reaches required_pending",
    ),
    RECIPIENT_PRESENCE_FIELD,
    WOKE_AGENTS_FIELD,
];

const MSG_WITHDRAW_INPUT: &[Field] = &[
    Field::required("id", Type::String, "Ask message UUIDv7 to withdraw"),
    Field::optional("reason", Type::String, "Optional withdrawal reason"),
];

const MSG_WITHDRAW_OUTPUT: &[Field] = &[
    Field::required("ask_id", Type::String, "Withdrawn ask UUIDv7"),
    Field::required("conversation_id", Type::String, "Conversation UUIDv7"),
    Field::required("by", Type::String, "Employee UUIDv7 who withdrew the ask"),
    Field::required("at", Type::String, "RFC 3339 UTC timestamp"),
    Field::optional("reason", Type::String, "Optional withdrawal reason"),
];

const MSG_ACCEPT_INPUT: &[Field] = &[
    Field::required("id", Type::String, "Handoff message ID to accept"),
    Field::optional("reason", Type::String, "Optional acceptance note"),
];

const MSG_ACCEPT_OUTPUT: &[Field] = &[
    Field::required("id", Type::String, "Accepted handoff message ID"),
    Field::required("accepted_by", Type::String, "Accepting employee UUIDv7"),
    Field::required("accepted_at", Type::String, "RFC 3339 UTC timestamp"),
    Field::optional("reason", Type::String, "Optional acceptance note"),
];

/// The `msg ask`, `msg withdraw` and `msg accept` rows.
pub(super) const COMMANDS: &[Command] = &[
    Command {
        resource: "msg",
        verb: "ask",
        summary: "Send structured ask (bounded wait)",
        call: "chat.ask",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: MSG_ASK_INPUT,
        content_field: None,
        output: MSG_ASK_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"to":"owner","prompt":"Target environment?","question_type":"choice","option":["dev=Dev","prod=Prod"],"default_value":"dev"}"#,
            output: r#"{"status":"waiting","ask_id":"0193a5c0-0000-7000-8000-000000000105","conversation_id":"0193a5c0-0000-7000-8000-000000000100","thread_id":"0193a5c0-0000-7000-8000-000000000105","local_seq":5,"origin_seq":5,"wait_until":"2026-11-21T12:10:00.000Z","remaining_ms":510000,"recipient_presence":[{"member_id":"0193a5c0-0000-7000-8000-000000000002","presence":"online"}],"woke_agents":[]}"#,
        },
    },
    Command {
        resource: "msg",
        verb: "withdraw",
        summary: "Withdraw open structured ask",
        call: "msg.withdraw",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: MSG_WITHDRAW_INPUT,
        content_field: None,
        output: MSG_WITHDRAW_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"id":"0193a5c0-0000-7000-8000-000000000105","reason":"No longer needed"}"#,
            output: r#"{"ask_id":"0193a5c0-0000-7000-8000-000000000105","conversation_id":"0193a5c0-0000-7000-8000-000000000100","by":"0193a5c0-0000-7000-8000-000000000004","at":"2026-11-21T12:03:00.000Z","reason":"No longer needed"}"#,
        },
    },
    Command {
        resource: "msg",
        verb: "accept",
        summary: "Accept a handoff addressed to you",
        call: "msg.accept",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: MSG_ACCEPT_INPUT,
        content_field: None,
        output: MSG_ACCEPT_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"id":"0193a5c0-0000-7000-8000-000000000101","reason":"Taking ownership"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000101","accepted_by":"0193a5c0-0000-7000-8000-000000000004","accepted_at":"2026-11-21T12:02:00.000Z","reason":"Taking ownership"}"#,
        },
    },
];

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
    let attach = opt_str_list(request, "attach").unwrap_or_default();
    let att_ids = opt_str_list(request, "attachment_id").unwrap_or_default();
    let resolved_ids = resolve_attach_ids(transport, &attach, &att_ids, MESSAGE_ATTACHMENTS_MAX)?;
    let mut params = Map::new();
    params.insert("id".into(), Value::String(id));
    params.insert("to".into(), Value::String(to.to_owned()));
    params.insert("questions".into(), questions);
    if !resolved_ids.is_empty() {
        params.insert("attachment_ids".into(), json!(resolved_ids));
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
    if let Some(wait_ms) = opt_u64(request, "wait_ms")? {
        params.insert("wait_ms".into(), Value::from(wait_ms));
    }
    let hold_ms = opt_u64(request, "hold_ms")?
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
    let attach = opt_str_list(request, "attach").unwrap_or_default();
    let att_ids = opt_str_list(request, "attachment_id").unwrap_or_default();
    let resolved_ids = resolve_attach_ids(transport, &attach, &att_ids, MESSAGE_ATTACHMENTS_MAX)?;
    if !resolved_ids.is_empty() {
        let mut combined: Vec<String> = params
            .get("attachment_ids")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        combined.extend(resolved_ids);
        params.insert("attachment_ids".into(), json!(combined));
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
