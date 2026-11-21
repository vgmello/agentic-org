//! Dispatch for C2 HR hiring, role catalog, permission, team limit, and agent lifecycle
//! commands (`TASK-41`, `ADR-0029`, `CLI-14`).

use org_command::{Ack, Answer, Refusal, Request};
use org_proto::hiring::{
    EntityOpResult, HireOpResult, HireProposalWire, HireRequestRecord, HrHireListResult,
    RoleListResult, RoleRecord,
};
use org_proto::ErrorCode;
use serde_json::{json, Map, Value};

use crate::dispatch::{derive_uuid_v7, parse_cursor, OfficeTransport};
use crate::keys::UuidV7KeySource;
use org_command::KeySource;

/// Dispatches a C2 hiring/role/agent command, returning `None` if not handled here.
pub(crate) fn try_execute<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    let result = match (request.command.resource, request.command.verb) {
        ("role", "list") => read_role_list(transport, request),
        ("role", "inspect") => read_role_inspect(transport, request),
        ("role", "create") => write_role_create(transport, request),
        ("role", "update") => write_role_update(transport, request),
        ("hr", "list") => read_hr_list(transport, request),
        ("hr", "inspect") => read_hr_inspect(transport, request),
        ("hr", "hire") => write_hr_hire(transport, request),
        ("agent", "update") => write_agent_update(transport, request),
        ("agent", "move") => write_agent_move(transport, request),
        ("agent", "pause") => write_agent_status(transport, request, true),
        ("agent", "resume") => write_agent_status(transport, request, false),
        ("agent", "offboard") => write_agent_offboard(transport, request),
        ("member", "set-hiring") => write_member_set_may_hire(transport, request),
        ("team", "set-limit") => write_team_set_max_members(transport, request),
        _ => return None,
    };
    Some(result)
}

fn read_role_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let offset = parse_cursor(request.cursor.as_deref())?;
    let raw = transport.call_method("role.list", json!({}))?;
    let listed: RoleListResult =
        serde_json::from_value(raw).map_err(|_| decode_err("role.list"))?;
    let items = serde_json::to_value(&listed.roles)
        .ok()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();
    Ok(paginate_values(items, offset, request.limit))
}

fn read_role_inspect<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let role = require_str(request, "role")?;
    let raw = transport.call_method("role.inspect", json!({ "role": role }))?;
    let record: RoleRecord = serde_json::from_value(raw).map_err(|_| decode_err("role.inspect"))?;
    Ok(Answer::completed(
        serde_json::to_value(record).expect("RoleRecord serializes"),
    ))
}

fn read_hr_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let offset = parse_cursor(request.cursor.as_deref())?;
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
    Ok(paginate_values(items, offset, request.limit))
}

fn read_hr_inspect<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let request_id = require_str(request, "request_id")?;
    let raw = transport.call_method("hr.hire.inspect", json!({ "request_id": request_id }))?;
    let record: HireRequestRecord =
        serde_json::from_value(raw).map_err(|_| decode_err("hr.hire.inspect"))?;
    Ok(Answer::completed(
        serde_json::to_value(record).expect("HireRequestRecord serializes"),
    ))
}

fn write_role_create<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let name = require_str(request, "name")?;
    let role_id = entity_id(request, "role_id", "role.create")?;
    let mut params = Map::new();
    params.insert("role_id".into(), json!(role_id));
    params.insert("name".into(), json!(name));
    if let Some(key) = opt_str(request, "key") {
        params.insert("key".into(), json!(key));
    }
    if let Some(body) = opt_str(request, "body") {
        params.insert("body".into(), json!(body));
    }
    send_entity_write(transport, request, "role.create", params)
}

fn write_role_update<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let role_id = require_str(request, "role")?;
    let expected = require_if_match(request)?;
    let mut params = Map::new();
    params.insert("role_id".into(), json!(role_id));
    params.insert("expected_version".into(), json!(expected));
    copy_opt_str(request, &mut params, "name", "name");
    copy_opt_str(request, &mut params, "key", "key");
    copy_opt_bool(request, &mut params, "clear_key", "clear_key");
    copy_opt_str(request, &mut params, "body", "body");
    send_entity_write(transport, request, "role.update", params)
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
    let request_id = entity_id(request, "request_id", "hr.hire.request")?;
    let mut params = Map::new();
    params.insert("request_id".into(), json!(request_id));
    params.insert("home_team_id".into(), json!(team));
    if let Some(role_id) = opt_str(request, "role_id") {
        params.insert("role_id".into(), json!(role_id));
    }
    params.insert(
        "proposal".into(),
        serde_json::to_value(proposal).expect("HireProposalWire serializes"),
    );
    params.insert("reason".into(), json!(reason));
    if is_draft {
        send_hire_write(transport, request, "hr.hire.draft", params)
    } else {
        let employee_id = entity_id(request, "employee_id", "hr.hire.employee")?;
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
    let employee_id = entity_id(request, "employee_id", "hr.hire.confirm.employee")?;
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
        tools_config: Vec::new(),
        system_prompt_markdown: opt_str(request, "system_prompt_markdown").map(str::to_owned),
        body: opt_str(request, "body").map(str::to_owned),
    })
}

fn write_agent_update<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let member_id = require_str(request, "member")?;
    let expected = require_if_match(request)?;
    let mut params = Map::new();
    params.insert("member_id".into(), json!(member_id));
    params.insert("expected_version".into(), json!(expected));
    for field in [
        "name",
        "role_id",
        "reports_to_member_id",
        "title",
        "focus",
        "role_text",
        "title_in_team",
        "body",
    ] {
        copy_opt_str(request, &mut params, field, field);
    }
    for flag in [
        "clear_role_id",
        "clear_reports_to",
        "clear_title",
        "clear_focus",
        "clear_role_text",
        "clear_title_in_team",
    ] {
        copy_opt_bool(request, &mut params, flag, flag);
    }
    send_entity_write(transport, request, "member.agent_update", params)
}

fn write_agent_move<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let member_id = require_str(request, "member")?;
    let to_team_id = require_str(request, "to_team_id")?;
    let expected = require_if_match(request)?;
    let mut params = Map::new();
    params.insert("member_id".into(), json!(member_id));
    params.insert("expected_version".into(), json!(expected));
    params.insert("to_team_id".into(), json!(to_team_id));
    send_entity_write(transport, request, "member.agent_move", params)
}

fn write_agent_status<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    paused: bool,
) -> Result<Answer, Refusal> {
    let member_id = require_str(request, "member")?;
    let expected = require_if_match(request)?;
    let mut params = Map::new();
    params.insert("member_id".into(), json!(member_id));
    params.insert("expected_version".into(), json!(expected));
    params.insert("paused".into(), json!(paused));
    send_entity_write(transport, request, "member.agent_set_status", params)
}

fn write_agent_offboard<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let member_id = require_str(request, "member")?;
    let expected = require_if_match(request)?;
    let mut params = Map::new();
    params.insert("member_id".into(), json!(member_id));
    params.insert("expected_version".into(), json!(expected));
    send_entity_write(transport, request, "member.agent_offboard", params)
}

fn write_member_set_may_hire<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let member_id = require_str(request, "member")?;
    let may_hire = opt_bool(request, "may_hire").ok_or_else(|| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            "Missing required --may-hire flag.",
            "Pass --may-hire true or --may-hire false.",
        )
    })?;
    let expected = require_if_match(request)?;
    let mut params = Map::new();
    params.insert("member_id".into(), json!(member_id));
    params.insert("expected_version".into(), json!(expected));
    params.insert("may_hire".into(), json!(may_hire));
    send_entity_write(transport, request, "member.set_may_hire", params)
}

fn write_team_set_max_members<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let team_id = require_str(request, "team")?;
    let max_members = request
        .input
        .get("max_members")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            Refusal::new(
                ErrorCode::InvalidRequest,
                "Missing required --max-members integer flag.",
                "Pass --max-members <1..=100>.",
            )
        })?;
    let expected = require_if_match(request)?;
    let mut params = Map::new();
    params.insert("team_id".into(), json!(team_id));
    params.insert("expected_version".into(), json!(expected));
    params.insert("max_members".into(), json!(max_members));
    send_entity_write(transport, request, "team.set_max_members", params)
}

fn prepare_write_params(request: &Request, params: &mut Map<String, Value>) -> Result<(), Refusal> {
    if request.dry_run {
        return Err(Refusal::new(
            ErrorCode::Unsupported,
            "Dry-run is not supported for this command yet.",
            "Remove --dry-run to run this command.",
        ));
    }
    let key = request.idempotency_key.as_deref().ok_or_else(|| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            "Write is missing an idempotency key.",
            "Pass --idempotency-key <uuidv7> or let the CLI mint one.",
        )
    })?;
    params.insert("command_id".into(), json!(key));
    Ok(())
}

fn send_entity_write<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    method: &str,
    mut params: Map<String, Value>,
) -> Result<Answer, Refusal> {
    prepare_write_params(request, &mut params)?;
    let raw = transport.call_method(method, Value::Object(params))?;
    let out: EntityOpResult = serde_json::from_value(raw).map_err(|_| decode_err(method))?;
    Ok(finalize_write_answer(
        request,
        serde_json::to_value(&out).expect("EntityOpResult serializes"),
        out.revision,
    ))
}

fn send_hire_write<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    method: &str,
    mut params: Map<String, Value>,
) -> Result<Answer, Refusal> {
    prepare_write_params(request, &mut params)?;
    let raw = transport.call_method(method, Value::Object(params))?;
    let out: HireOpResult = serde_json::from_value(raw).map_err(|_| decode_err(method))?;
    Ok(finalize_write_answer(
        request,
        serde_json::to_value(&out).expect("HireOpResult serializes"),
        out.revision,
    ))
}

fn finalize_write_answer(request: &Request, data: Value, revision: u64) -> Answer {
    let mut answer = Answer::completed(data);
    answer.operation_id = request.idempotency_key.clone();
    answer.revision = Some(revision);
    answer.ack = Some(if request.durable {
        Ack::Durable
    } else {
        Ack::Committed
    });
    answer
}

fn paginate_values(items: Vec<Value>, offset: usize, limit: Option<u32>) -> Answer {
    let total = items.len();
    let cap = limit.unwrap_or(org_command::DEFAULT_LIMIT) as usize;
    let slice: Vec<Value> = items.into_iter().skip(offset).take(cap).collect();
    let next = offset.saturating_add(slice.len());
    let mut answer = Answer::completed(Value::Array(slice));
    if next < total {
        answer.next_cursor = Some(next.to_string());
    }
    answer
}

fn entity_id(request: &Request, field: &str, domain: &str) -> Result<String, Refusal> {
    if let Some(explicit) = opt_str(request, field) {
        return Ok(explicit.to_owned());
    }
    if let Some(key) = &request.idempotency_key {
        Ok(derive_uuid_v7(domain, key))
    } else {
        let mut keys = UuidV7KeySource;
        Ok(keys.new_key())
    }
}

fn require_str<'a>(request: &'a Request, field: &str) -> Result<&'a str, Refusal> {
    request
        .input
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Refusal::new(
                ErrorCode::InvalidRequest,
                format!("Missing required --{} flag.", field.replace('_', "-")),
                format!("Pass --{} <value>.", field.replace('_', "-")),
            )
        })
}

fn opt_str<'a>(request: &'a Request, field: &str) -> Option<&'a str> {
    request.input.get(field).and_then(Value::as_str)
}

fn opt_bool(request: &Request, field: &str) -> Option<bool> {
    request.input.get(field).and_then(Value::as_bool)
}

fn copy_opt_str(request: &Request, params: &mut Map<String, Value>, src: &str, dst: &str) {
    if let Some(val) = opt_str(request, src) {
        params.insert(dst.into(), json!(val));
    }
}

fn copy_opt_bool(request: &Request, params: &mut Map<String, Value>, src: &str, dst: &str) {
    if let Some(val) = opt_bool(request, src) {
        params.insert(dst.into(), json!(val));
    }
}

fn require_if_match(request: &Request) -> Result<u64, Refusal> {
    request.if_match.ok_or_else(|| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            "This update requires --if-match <revision>.",
            "Inspect the current record and pass its revision with --if-match.",
        )
    })
}

fn decode_err(method: &str) -> Refusal {
    Refusal::new(
        ErrorCode::Unavailable,
        format!("The office returned an unreadable `{method}` reply."),
        "Retry the command or check the office version.",
    )
}
