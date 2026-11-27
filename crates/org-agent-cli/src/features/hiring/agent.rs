//! `agent inspect|update|move|pause|resume|offboard|execution-get|execution-set`: an agent's
//! directory card, profile, placement, lifecycle and execution settings.

use org_command::{Answer, Request};
use org_proto::hiring::AgentExecutionRecord;
use org_proto::providers::AgentReadinessResult;
use serde_json::{json, Map};

use crate::runtime::card::inspect_member;
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{
    copy_opt_bool, copy_opt_str, decode_err, encode, opt_bool, require_if_match, require_str,
    send_entity_write,
};
use crate::runtime::transport::{OfficeTransport, Refusal};

/// `Use when:` guidance of each `agent` command here.
pub(super) const CUES: &[CommandCue] = &[
    ("agent", "execution-get", "inspecting an agent's model profile, model ID, permission preset, reasoning effort, and standing system prompt."),
    ("agent", "execution-set", "configuring or updating an agent's model profile, model ID, permission preset, reasoning effort, and system prompt."),
    ("agent", "inspect", "looking up one agent's directory card, role, lifecycle status, and live presence."),
    ("agent", "move", "re-assigning an agent to a different home team (requires `--if-match`)."),
    ("agent", "offboard", "permanently offboarding an agent and ending its active work (requires `--if-match`)."),
    ("agent", "pause", "temporarily suspending an active agent without offboarding it (requires `--if-match`)."),
    ("agent", "readiness", "diagnosing whether an agent is ready to take a turn and why candidate provider connections are eligible or skipped."),
    ("agent", "resume", "re-activating a paused agent so it can take turns again (requires `--if-match`)."),
    ("agent", "update", "editing an agent's profile fields, role, reporting manager, or instructions body (requires `--if-match`)."),
];

/// Runs an `agent` directory or lifecycle command.
pub(super) fn dispatch<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    Some(match request.command.verb {
        "inspect" => inspect_member(transport, request, "agent"),
        "update" => write_agent_update(transport, request),
        "move" => write_agent_move(transport, request),
        "pause" => write_agent_status(transport, request, true),
        "resume" => write_agent_status(transport, request, false),
        "offboard" => write_agent_offboard(transport, request),
        "execution-get" => read_agent_execution_get(transport, request),
        "execution-set" => write_agent_execution_set(transport, request),
        "readiness" => read_agent_readiness(transport, request),
        _ => return None,
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

fn read_agent_execution_get<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let employee_id = require_str(request, "member")?;
    let raw = transport.call_method(
        "member.agent_execution_get",
        json!({ "employee_id": employee_id }),
    )?;
    let record: AgentExecutionRecord =
        serde_json::from_value(raw).map_err(|_| decode_err("member.agent_execution_get"))?;
    Ok(Answer::completed(encode(
        &record,
        "member.agent_execution_get",
    )?))
}

fn write_agent_execution_set<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let employee_id = require_str(request, "member")?;
    let model_profile_id = require_str(request, "model_profile_id")?;
    let model_id = require_str(request, "model_id")?;
    let system_prompt_markdown = require_str(request, "system_prompt_markdown")?;
    let mut params = Map::new();
    params.insert("employee_id".into(), json!(employee_id));
    if let Some(expected) = request.if_match {
        params.insert("expected_version".into(), json!(expected));
    }
    params.insert("model_profile_id".into(), json!(model_profile_id));
    params.insert("model_id".into(), json!(model_id));
    copy_opt_str(
        request,
        &mut params,
        "permission_preset",
        "permission_preset",
    );
    copy_opt_str(request, &mut params, "effort", "effort");
    if let Some(enabled) = opt_bool(request, "enabled") {
        params.insert("enabled".into(), json!(enabled));
    }
    params.insert(
        "system_prompt_markdown".into(),
        json!(system_prompt_markdown),
    );
    send_entity_write(transport, request, "member.agent_execution_set", params)
}

fn read_agent_readiness<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let employee_id = require_str(request, "member")?;
    let mut params = Map::new();
    params.insert("employee_id".into(), json!(employee_id));
    copy_opt_str(request, &mut params, "task_id", "task_id");
    let raw = transport.call_method("member.agent_readiness", serde_json::Value::Object(params))?;
    let record: AgentReadinessResult =
        serde_json::from_value(raw).map_err(|_| decode_err("member.agent_readiness"))?;
    Ok(Answer::completed(encode(
        &record,
        "member.agent_readiness",
    )?))
}
