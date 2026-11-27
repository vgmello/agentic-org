//! Chief of Staff `suggestion create|list|inspect|withdraw` commands (`TASK-80.4`).

use org_command::{Answer, Refusal, Request};
use org_proto::tess::{
    SuggestionActionResult, SuggestionCreate, SuggestionCreateParams, SuggestionCreateResult,
    SuggestionInspect, SuggestionInspectParams, SuggestionInspectResult, SuggestionList,
    SuggestionListParams, SuggestionListResult, SuggestionStatus, SuggestionWithdraw,
    SuggestionWithdrawParams,
};
use org_proto::ErrorCode;
use serde_json::{Map, Value};

use crate::runtime::domain::CommandCue;
use crate::runtime::input::{
    call_rpc, committed_answer, encode, opt_str, opt_string, opt_u64, require_str,
};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport};

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
