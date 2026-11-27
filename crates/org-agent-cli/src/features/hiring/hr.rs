//! `hr list|inspect|hire`: agent hire requests, drafted, confirmed or withdrawn.

use org_command::{Answer, Request};
use org_proto::hiring::{HireOpResult, HireProposalWire, HireRequestRecord, HrHireListResult};
use org_proto::ErrorCode;
use serde_json::{json, Map, Value};

use crate::runtime::domain::CommandCue;
use crate::runtime::input::{
    decode_err, encode, entity_id, hiring_command_id, opt_bool, opt_str, require_if_match,
    require_str, revised_answer,
};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport, Refusal};

/// `Use when:` guidance of each `hr` command.
pub(super) const CUES: &[CommandCue] = &[
    ("hr", "hire", "drafting (`--draft`), confirming (`--id`), directly hiring, or withdrawing (`--id --withdraw`) an agent hire request."),
    ("hr", "inspect", "checking one hire request's proposal details, state, or failed provisioning step by UUIDv7."),
    ("hr", "list", "reviewing pending, hired, failed, or withdrawn HR hire requests (optionally filtered by team or state)."),
];

/// Runs an `hr` command.
pub(super) fn dispatch<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    Some(match request.command.verb {
        "list" => read_hr_list(transport, request),
        "inspect" => read_hr_inspect(transport, request),
        "hire" => write_hr_hire(transport, request),
        _ => return None,
    })
}

fn read_hr_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    parse_cursor(request.cursor.as_deref())?;
    let mut params = Map::new();
    if let Some(team_id) = opt_str(request, "team_id") {
        params.insert("team_id".into(), json!(team_id));
    }
    if let Some(state) = opt_str(request, "state") {
        params.insert("state".into(), json!(state));
    }
    let raw = transport.call_method("hr.hire.list", Value::Object(params))?;
    let listed: HrHireListResult =
        serde_json::from_value(raw).map_err(|_| decode_err("hr.hire.list"))?;
    let items = serde_json::to_value(&listed.hires)
        .ok()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();
    paginate(request, items)
}

fn read_hr_inspect<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let request_id = require_str(request, "request_id")?;
    let raw = transport.call_method("hr.hire.inspect", json!({ "request_id": request_id }))?;
    let record: HireRequestRecord =
        serde_json::from_value(raw).map_err(|_| decode_err("hr.hire.inspect"))?;
    Ok(Answer::completed(encode(&record, "hr.hire.inspect")?))
}

fn write_hr_hire<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let is_draft = opt_bool(request, "draft").unwrap_or(false);
    let is_withdraw = opt_bool(request, "withdraw").unwrap_or(false);
    if let Some(existing_id) = opt_str(request, "id") {
        if is_draft {
            return Err(Refusal::new(
                ErrorCode::InvalidRequest,
                "--draft cannot be combined with --id.",
                "Pass --id <request_id> --if-match <rev> to confirm or withdraw an existing request.",
            ));
        }
        return if is_withdraw {
            write_hr_withdraw(transport, request, existing_id)
        } else {
            write_hr_confirm(transport, request, existing_id)
        };
    }
    if is_withdraw {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "--withdraw requires --id <request_id>.",
            "Pass --id <request_id> --if-match <rev> --withdraw --closed-reason <reason>.",
        ));
    }
    write_hr_new(transport, request, is_draft)
}

fn write_hr_new<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    is_draft: bool,
) -> Result<Answer, Refusal> {
    let team = require_str(request, "team")?;
    let reason = require_str(request, "reason")?;
    let proposal = build_proposal(request)?;
    let request_id = entity_id(request, Some("request_id"), "hr.hire.request")?;
    let mut params = Map::new();
    params.insert("request_id".into(), json!(request_id));
    params.insert("home_team_id".into(), json!(team));
    if let Some(role_id) = opt_str(request, "role_id") {
        params.insert("role_id".into(), json!(role_id));
    }
    params.insert("proposal".into(), encode(&proposal, "hr.hire")?);
    params.insert("reason".into(), json!(reason));
    if is_draft {
        send_hire_write(transport, request, "hr.hire.draft", params)
    } else {
        let employee_id = entity_id(request, Some("employee_id"), "hr.hire.employee")?;
        params.insert("employee_id".into(), json!(employee_id));
        send_hire_write(transport, request, "hr.hire", params)
    }
}

fn write_hr_confirm<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    request_id: &str,
) -> Result<Answer, Refusal> {
    let expected = require_if_match(request)?;
    let employee_id = entity_id(request, Some("employee_id"), "hr.hire.confirm.employee")?;
    let mut params = Map::new();
    params.insert("request_id".into(), json!(request_id));
    params.insert("employee_id".into(), json!(employee_id));
    params.insert("expected_version".into(), json!(expected));
    send_hire_write(transport, request, "hr.hire.confirm", params)
}

fn write_hr_withdraw<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    request_id: &str,
) -> Result<Answer, Refusal> {
    let expected = require_if_match(request)?;
    let closed_reason = opt_str(request, "closed_reason")
        .or_else(|| opt_str(request, "reason"))
        .ok_or_else(|| {
            Refusal::new(
                ErrorCode::InvalidRequest,
                "Missing required --closed-reason flag.",
                "Pass --closed-reason <text> when withdrawing a hire request.",
            )
        })?;
    let mut params = Map::new();
    params.insert("request_id".into(), json!(request_id));
    params.insert("expected_version".into(), json!(expected));
    params.insert("closed_reason".into(), json!(closed_reason));
    send_hire_write(transport, request, "hr.hire.withdraw", params)
}

fn build_proposal(request: &Request) -> Result<HireProposalWire, Refusal> {
    Ok(HireProposalWire {
        name: require_str(request, "name")?.to_owned(),
        handle: require_str(request, "handle")?.to_owned(),
        title: opt_str(request, "title").map(str::to_owned),
        focus: opt_str(request, "focus").map(str::to_owned),
        role_text: opt_str(request, "role_text").map(str::to_owned),
        title_in_team: opt_str(request, "title_in_team").map(str::to_owned),
        reports_to_employee_id: opt_str(request, "reports_to_member_id").map(str::to_owned),
        office_id: opt_str(request, "office_id").map(str::to_owned),
        model_profile_id: opt_str(request, "model_profile_id").map(str::to_owned),
        model_id: opt_str(request, "model_id").map(str::to_owned),
        permission_preset: opt_str(request, "permission_preset").map(str::to_owned),
        effort: opt_str(request, "effort").map(str::to_owned),
        tools_config: Vec::new(),
        system_prompt_markdown: opt_str(request, "system_prompt_markdown").map(str::to_owned),
        body: opt_str(request, "body").map(str::to_owned),
    })
}

/// Sends a hire-request write whose reply is the office's [`HireOpResult`].
fn send_hire_write<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    method: &str,
    mut params: Map<String, Value>,
) -> Result<Answer, Refusal> {
    params.insert("command_id".into(), json!(hiring_command_id(request)?));
    let raw = transport.call_method(method, Value::Object(params))?;
    let out: HireOpResult = serde_json::from_value(raw).map_err(|_| decode_err(method))?;
    Ok(revised_answer(request, encode(&out, method)?, out.revision))
}
