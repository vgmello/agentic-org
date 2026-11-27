//! Chief of Staff `suggestion create|list|inspect|withdraw` commands (`TASK-80.4`).

use org_command::{Ack, Answer, Command, Example, Field, IfMatch, Kind, Refusal, Request, Type};
use org_proto::tess::{
    SuggestionActionResult, SuggestionCreate, SuggestionCreateParams, SuggestionCreateResult,
    SuggestionInspect, SuggestionInspectParams, SuggestionInspectResult, SuggestionList,
    SuggestionListParams, SuggestionListResult, SuggestionStatus, SuggestionWithdraw,
    SuggestionWithdrawParams,
};
use org_proto::ErrorCode;
use serde_json::{Map, Value};

use super::ui::UI_BLOCK_FIELDS;
use super::{TESS_ERRORS, TESS_WRITE_ERRORS};
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{
    call_rpc, committed_answer, encode, opt_str, opt_string, opt_u64, require_str,
};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport};

const STATUS_ENUM: &[&str] = &["pending", "withdrawn", "dismissed", "expired"];

pub(super) const SUGGESTION_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Suggestion UUIDv7"),
    Field::required(
        "status",
        Type::Enum(STATUS_ENUM),
        "pending, withdrawn, dismissed, or expired",
    ),
    Field::required("command", Type::String, "Canonical target agent-os command"),
    Field::required("call", Type::String, "Target office RPC method"),
    Field::required("input", Type::Json, "Validated target command input object"),
    Field::required("summary", Type::String, "Human-facing confirmation summary"),
    Field::optional("rationale", Type::String, "Optional proposal rationale"),
    Field::optional(
        "expected_revision",
        Type::Integer,
        "Pinned expected target entity revision",
    ),
    Field::required(
        "created_by_member_id",
        Type::String,
        "Chief of Staff member UUIDv7",
    ),
    Field::required("for_caller_id", Type::String, "Target human caller UUIDv7"),
    Field::required(
        "preflight_ok",
        Type::Boolean,
        "Server preflight validation result",
    ),
    Field::optional("preflight_note", Type::String, "Server preflight note"),
    Field::required("created_at", Type::String, "RFC 3339 creation timestamp"),
    Field::required("expires_at", Type::String, "RFC 3339 expiry timestamp"),
    Field::optional("resolved_at", Type::String, "RFC 3339 resolution timestamp"),
    Field::optional(
        "close_reason",
        Type::String,
        "Withdrawal or dismissal reason",
    ),
];

const SUGGESTION_CREATE_INPUT: &[Field] = &[
    Field::required(
        "command",
        Type::String,
        "Target agent-os command (for example \"task create\" or \"team update\")",
    ),
    Field::required("summary", Type::String, "Concise human-facing summary"),
    Field::optional(
        "input",
        Type::Json,
        "JSON object matching the target command's input schema",
    ),
    Field::optional("rationale", Type::String, "Why this change is proposed"),
    Field::optional(
        "expected_revision",
        Type::Integer,
        "Expected target entity revision to pin before confirmation",
    ),
    Field::optional(
        "ttl_seconds",
        Type::Integer,
        "Proposal expiry in seconds (30 to 86400; default 900)",
    ),
];

const SUGGESTION_CREATE_OUTPUT: &[Field] = &[
    Field::required(
        "suggestion",
        Type::Object(SUGGESTION_FIELDS),
        "Validated proposal record",
    ),
    Field::required(
        "ui_block",
        Type::Object(UI_BLOCK_FIELDS),
        "Verified UI confirmation card block",
    ),
    Field::optional(
        "dry_run",
        Type::Boolean,
        "True when preflighted without storing",
    ),
];

const SUGGESTION_LIST_INPUT: &[Field] = &[Field::optional(
    "status",
    Type::Enum(STATUS_ENUM),
    "Filter by suggestion status",
)];

const SUGGESTION_INSPECT_INPUT: &[Field] = &[Field::required(
    "suggestion",
    Type::String,
    "Suggestion UUIDv7",
)];

const SUGGESTION_INSPECT_OUTPUT: &[Field] = &[
    Field::required(
        "suggestion",
        Type::Object(SUGGESTION_FIELDS),
        "Stored proposal record",
    ),
    Field::required(
        "ui_block",
        Type::Object(UI_BLOCK_FIELDS),
        "Verified UI presentation block",
    ),
];

const SUGGESTION_WITHDRAW_INPUT: &[Field] = &[
    Field::required("suggestion", Type::String, "Pending suggestion UUIDv7"),
    Field::optional(
        "reason",
        Type::String,
        "Reason for withdrawing the proposal",
    ),
];

const SUGGESTION_WITHDRAW_OUTPUT: &[Field] = &[Field::required(
    "suggestion",
    Type::Object(SUGGESTION_FIELDS),
    "Updated proposal record",
)];

pub(super) const COMMANDS: &[Command] = &[
    Command {
        resource: "suggestion",
        verb: "create",
        summary: "Preflight and propose a mutation for caller confirmation",
        call: "suggestion.create",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: SUGGESTION_CREATE_INPUT,
        content_field: None,
        output: SUGGESTION_CREATE_OUTPUT,
        errors: TESS_WRITE_ERRORS,
        example: Example {
            input: r#"{"command":"task create","input":{"title":"Review release"},"summary":"Create release review task"}"#,
            output: r#"{"suggestion":{"id":"0193a5c0-0000-7000-8000-000000000901","status":"pending","command":"task create","call":"tasks.create","input":{"title":"Review release"},"summary":"Create release review task","created_by_member_id":"0193a5c0-0000-7000-8000-000000000010","for_caller_id":"0193a5c0-0000-7000-8000-000000000002","preflight_ok":true,"created_at":"2026-11-27T00:00:00.000Z","expires_at":"2026-11-27T00:15:00.000Z"},"ui_block":{"title":"Create release review task","suggestion_id":"0193a5c0-0000-7000-8000-000000000901"}}"#,
        },
    },
    Command {
        resource: "suggestion",
        verb: "list",
        summary: "List caller-scoped mutation proposals",
        call: "suggestion.list",
        kind: Kind::List,
        input: SUGGESTION_LIST_INPUT,
        content_field: None,
        output: SUGGESTION_FIELDS,
        errors: TESS_ERRORS,
        example: Example {
            input: "{}",
            output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000901","status":"pending","command":"task create","call":"tasks.create","input":{"title":"Review release"},"summary":"Create release review task","created_by_member_id":"0193a5c0-0000-7000-8000-000000000010","for_caller_id":"0193a5c0-0000-7000-8000-000000000002","preflight_ok":true,"created_at":"2026-11-27T00:00:00.000Z","expires_at":"2026-11-27T00:15:00.000Z"}]"#,
        },
    },
    Command {
        resource: "suggestion",
        verb: "inspect",
        summary: "Inspect one caller-scoped mutation proposal",
        call: "suggestion.inspect",
        kind: Kind::Read,
        input: SUGGESTION_INSPECT_INPUT,
        content_field: None,
        output: SUGGESTION_INSPECT_OUTPUT,
        errors: TESS_ERRORS,
        example: Example {
            input: r#"{"suggestion":"0193a5c0-0000-7000-8000-000000000901"}"#,
            output: r#"{"suggestion":{"id":"0193a5c0-0000-7000-8000-000000000901","status":"pending","command":"task create","call":"tasks.create","input":{"title":"Review release"},"summary":"Create release review task","created_by_member_id":"0193a5c0-0000-7000-8000-000000000010","for_caller_id":"0193a5c0-0000-7000-8000-000000000002","preflight_ok":true,"created_at":"2026-11-27T00:00:00.000Z","expires_at":"2026-11-27T00:15:00.000Z"},"ui_block":{"title":"Create release review task","suggestion_id":"0193a5c0-0000-7000-8000-000000000901"}}"#,
        },
    },
    Command {
        resource: "suggestion",
        verb: "withdraw",
        summary: "Withdraw a pending mutation proposal",
        call: "suggestion.withdraw",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: SUGGESTION_WITHDRAW_INPUT,
        content_field: None,
        output: SUGGESTION_WITHDRAW_OUTPUT,
        errors: TESS_WRITE_ERRORS,
        example: Example {
            input: r#"{"suggestion":"0193a5c0-0000-7000-8000-000000000901","reason":"Superseded"}"#,
            output: r#"{"suggestion":{"id":"0193a5c0-0000-7000-8000-000000000901","status":"withdrawn","command":"task create","call":"tasks.create","input":{"title":"Review release"},"summary":"Create release review task","created_by_member_id":"0193a5c0-0000-7000-8000-000000000010","for_caller_id":"0193a5c0-0000-7000-8000-000000000002","preflight_ok":true,"created_at":"2026-11-27T00:00:00.000Z","expires_at":"2026-11-27T00:15:00.000Z","resolved_at":"2026-11-27T00:01:00.000Z","close_reason":"Superseded"}}"#,
        },
    },
];

pub(super) const CUES: &[CommandCue] = &[
    (
        "suggestion",
        "create",
        "proposing a preflighted durable change for caller confirmation instead of mutating state directly.",
    ),
    (
        "suggestion",
        "list",
        "listing pending or resolved proposals scoped to the current caller.",
    ),
    (
        "suggestion",
        "inspect",
        "reading the validated payload, revision pin, and status of one caller-scoped proposal.",
    ),
    (
        "suggestion",
        "withdraw",
        "cancelling a pending proposal before the caller confirms it.",
    ),
];

fn parse_status(raw: Option<&str>) -> Result<Option<SuggestionStatus>, Refusal> {
    match raw {
        None => Ok(None),
        Some("pending") => Ok(Some(SuggestionStatus::Pending)),
        Some("withdrawn") => Ok(Some(SuggestionStatus::Withdrawn)),
        Some("dismissed") => Ok(Some(SuggestionStatus::Dismissed)),
        Some("expired") => Ok(Some(SuggestionStatus::Expired)),
        Some(other) => Err(Refusal::new(
            ErrorCode::InvalidRequest,
            format!("unknown suggestion status `{other}`"),
            "Pass --status pending, withdrawn, dismissed, or expired.",
        )),
    }
}

fn read_input_object(request: &Request) -> Result<Map<String, Value>, Refusal> {
    match request.input.get("input") {
        None | Some(Value::Null) => Ok(Map::new()),
        Some(Value::Object(map)) => Ok(map.clone()),
        Some(_) => Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "`input` must be a JSON object matching the target command's input schema.",
            "Pass target fields inside `--json '{\"input\":{...}}'`.",
        )),
    }
}

pub(super) fn create_suggestion<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = SuggestionCreateParams {
        command: require_str(request, "command")?.to_owned(),
        input: read_input_object(request)?,
        summary: require_str(request, "summary")?.to_owned(),
        rationale: opt_string(request, "rationale"),
        expected_revision: opt_u64(request, "expected_revision")?,
        ttl_seconds: opt_u64(request, "ttl_seconds")?,
        dry_run: request.dry_run,
    };
    let res: SuggestionCreateResult = call_rpc::<SuggestionCreate, _>(transport, &params)?;
    let data = encode(&res, "suggestion.create")?;
    if request.dry_run {
        Ok(Answer::completed(data))
    } else {
        Ok(committed_answer(request, data))
    }
}

pub(super) fn list_suggestions<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    parse_cursor(request.cursor.as_deref())?;
    let params = SuggestionListParams {
        status: parse_status(opt_str(request, "status"))?,
        limit: None,
    };
    let res: SuggestionListResult = call_rpc::<SuggestionList, _>(transport, &params)?;
    let items = match encode(&res.suggestions, "suggestion.list")? {
        Value::Array(items) => items,
        _ => Vec::new(),
    };
    paginate(request, items)
}

pub(super) fn inspect_suggestion<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = SuggestionInspectParams {
        suggestion_id: require_str(request, "suggestion")?.to_owned(),
    };
    let res: SuggestionInspectResult = call_rpc::<SuggestionInspect, _>(transport, &params)?;
    Ok(Answer::completed(encode(&res, "suggestion.inspect")?))
}

pub(super) fn withdraw_suggestion<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = SuggestionWithdrawParams {
        suggestion_id: require_str(request, "suggestion")?.to_owned(),
        reason: opt_string(request, "reason"),
        dry_run: request.dry_run,
    };
    let res: SuggestionActionResult = call_rpc::<SuggestionWithdraw, _>(transport, &params)?;
    let data = encode(&res, "suggestion.withdraw")?;
    if request.dry_run {
        Ok(Answer::completed(data))
    } else {
        Ok(committed_answer(request, data))
    }
}
