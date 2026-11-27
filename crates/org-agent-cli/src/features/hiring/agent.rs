//! `agent inspect|update|move|pause|resume|offboard|execution-get|execution-set`: an agent's
//! directory card, profile, placement, lifecycle and execution settings.

use org_command::{Ack, Answer, Command, Example, Field, IfMatch, Kind, Request, Type};
use org_proto::hiring::AgentExecutionRecord;
use serde_json::{json, Map};

use super::schema::{EFFORT_TYPE, READ_ERRORS, STRING_TYPE, WRITE_ERRORS};
use crate::runtime::card::{inspect_member, DIRECTORY_CARD_FIELDS, MEMBER_INSPECT_INPUT};
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{
    copy_opt_bool, copy_opt_str, decode_err, encode, opt_bool, require_if_match, require_str,
    send_entity_write, ENTITY_OP_OUTPUT,
};
use crate::runtime::transport::{OfficeTransport, Refusal};

const AGENT_UPDATE_INPUT: &[Field] = &[
    Field::required("member", Type::String, "Agent employee UUIDv7"),
    Field::optional("name", Type::String, "Updated agent display name"),
    Field::optional("role_id", Type::String, "Catalog role UUIDv7"),
    Field::optional("clear_role_id", Type::Boolean, "Clear catalog role"),
    Field::optional(
        "reports_to_member_id",
        Type::String,
        "Manager employee UUIDv7",
    ),
    Field::optional("clear_reports_to", Type::Boolean, "Clear reporting manager"),
    Field::optional("title", Type::String, "Updated title"),
    Field::optional("clear_title", Type::Boolean, "Clear title"),
    Field::optional("focus", Type::String, "Updated focus"),
    Field::optional("clear_focus", Type::Boolean, "Clear focus"),
    Field::optional("role_text", Type::String, "Updated ad-hoc role text"),
    Field::optional("clear_role_text", Type::Boolean, "Clear ad-hoc role text"),
    Field::optional("title_in_team", Type::String, "Updated title in home team"),
    Field::optional(
        "clear_title_in_team",
        Type::Boolean,
        "Clear title in home team",
    ),
    Field::optional("body", Type::String, "Updated profile markdown body"),
];

const AGENT_MOVE_INPUT: &[Field] = &[
    Field::required("member", Type::String, "Agent employee UUIDv7"),
    Field::required("to_team_id", Type::String, "Destination home team UUIDv7"),
];

const AGENT_LIFECYCLE_INPUT: &[Field] = &[Field::required(
    "member",
    Type::String,
    "Agent employee UUIDv7",
)];

const AGENT_EXECUTION_OUTPUT: &[Field] = &[
    Field::required("employee_id", Type::String, "Agent employee UUIDv7"),
    Field::required("model_profile_id", Type::String, "Model profile UUIDv7"),
    Field::required("model_id", Type::String, "Model identifier"),
    Field::required(
        "tools_config",
        Type::List(&STRING_TYPE),
        "Enabled tool names",
    ),
    Field::required("permission_preset", Type::String, "Permission preset"),
    Field::optional(
        "effort",
        EFFORT_TYPE,
        "Reasoning effort; absent when the runtime default applies",
    ),
    Field::required("enabled", Type::Boolean, "Whether execution is enabled"),
    Field::required(
        "system_prompt_markdown",
        Type::String,
        "Agent system prompt",
    ),
    Field::required("revision", Type::Integer, "Record revision"),
];

const AGENT_EXECUTION_SET_INPUT: &[Field] = &[
    Field::required("member", Type::String, "Agent employee UUIDv7"),
    Field::required("model_profile_id", Type::String, "Model profile UUIDv7"),
    Field::required("model_id", Type::String, "Model identifier"),
    Field::optional(
        "permission_preset",
        Type::Enum(&[
            "safe_ask",
            "read_only",
            "workspace_edits",
            "autonomous_sandboxed",
        ]),
        "Execution permission preset (full replacement; defaults to safe_ask when omitted)",
    ),
    Field::optional(
        "effort",
        EFFORT_TYPE,
        "Reasoning effort (full replacement; omitted resets to the runtime default)",
    ),
    Field::optional("enabled", Type::Boolean, "Enable or disable execution"),
    Field::required(
        "system_prompt_markdown",
        Type::String,
        "Agent system prompt markdown",
    ),
];

/// The `agent` rows of the directory and lifecycle.
pub(super) const COMMANDS: &[Command] = &[
    Command {
        resource: "agent",
        verb: "inspect",
        summary: "Inspect agent directory card",
        call: "who",
        kind: Kind::Read,
        input: MEMBER_INSPECT_INPUT,
        content_field: None,
        output: DIRECTORY_CARD_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"member":"hr"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000003","name":"HR","handle":"hr","kind":"agent","owner":false,"agent_status":"active","role_title":"HR Lead","presence":"offline","teams":[{"id":"0193a5c0-0000-7000-8000-000000000001","name":"Core"}]}"#,
        },
    },
    Command {
        resource: "agent",
        verb: "update",
        summary: "Update agent profile or manager",
        call: "member.agent_update",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: AGENT_UPDATE_INPUT,
        content_field: Some("body"),
        output: ENTITY_OP_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"member":"0193a5c0-0000-7000-8000-000000000051","title":"Staff Engineer"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000051","revision":2,"generation":6}"#,
        },
    },
    Command {
        resource: "agent",
        verb: "move",
        summary: "Move agent to another team",
        call: "member.agent_move",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: AGENT_MOVE_INPUT,
        content_field: None,
        output: ENTITY_OP_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"member":"0193a5c0-0000-7000-8000-000000000051","to_team_id":"0193a5c0-0000-7000-8000-000000000010"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000051","revision":3,"generation":7}"#,
        },
    },
    Command {
        resource: "agent",
        verb: "pause",
        summary: "Pause agent",
        call: "member.agent_set_status",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: AGENT_LIFECYCLE_INPUT,
        content_field: None,
        output: ENTITY_OP_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"member":"0193a5c0-0000-7000-8000-000000000051"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000051","revision":4,"generation":8}"#,
        },
    },
    Command {
        resource: "agent",
        verb: "resume",
        summary: "Resume paused agent",
        call: "member.agent_set_status",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: AGENT_LIFECYCLE_INPUT,
        content_field: None,
        output: ENTITY_OP_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"member":"0193a5c0-0000-7000-8000-000000000051"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000051","revision":5,"generation":9}"#,
        },
    },
    Command {
        resource: "agent",
        verb: "offboard",
        summary: "Offboard agent",
        call: "member.agent_offboard",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: AGENT_LIFECYCLE_INPUT,
        content_field: None,
        output: ENTITY_OP_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"member":"0193a5c0-0000-7000-8000-000000000051"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000051","revision":6,"generation":10}"#,
        },
    },
    Command {
        resource: "agent",
        verb: "execution-get",
        summary: "Inspect agent execution configuration",
        call: "member.agent_execution_get",
        kind: Kind::Read,
        input: AGENT_LIFECYCLE_INPUT,
        content_field: None,
        output: AGENT_EXECUTION_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"member":"0193a5c0-0000-7000-8000-000000000051"}"#,
            output: r#"{"employee_id":"0193a5c0-0000-7000-8000-000000000051","model_profile_id":"0193a5c0-0000-7000-8000-000000000060","model_id":"sonnet","tools_config":[],"permission_preset":"autonomous_sandboxed","effort":"xhigh","enabled":true,"system_prompt_markdown":"You are an engineer.","revision":1}"#,
        },
    },
    Command {
        resource: "agent",
        verb: "execution-set",
        summary: "Configure agent model profile, preset, effort, and prompt",
        call: "member.agent_execution_set",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Optional,
        },
        input: AGENT_EXECUTION_SET_INPUT,
        content_field: Some("system_prompt_markdown"),
        output: ENTITY_OP_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"member":"0193a5c0-0000-7000-8000-000000000051","model_profile_id":"0193a5c0-0000-7000-8000-000000000060","model_id":"sonnet","permission_preset":"autonomous_sandboxed","effort":"xhigh","system_prompt_markdown":"You are an engineer."}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000051","revision":1,"generation":10}"#,
        },
    },
];

/// `Use when:` guidance of each `agent` command here.
pub(super) const CUES: &[CommandCue] = &[
    ("agent", "execution-get", "inspecting an agent's model profile, model ID, permission preset, reasoning effort, and standing system prompt."),
    ("agent", "execution-set", "configuring or updating an agent's model profile, model ID, permission preset, reasoning effort, and system prompt."),
    ("agent", "inspect", "looking up one agent's directory card, role, lifecycle status, and live presence."),
    ("agent", "move", "re-assigning an agent to a different home team (requires `--if-match`)."),
    ("agent", "offboard", "permanently offboarding an agent and ending its active work (requires `--if-match`)."),
    ("agent", "pause", "temporarily suspending an active agent without offboarding it (requires `--if-match`)."),
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
