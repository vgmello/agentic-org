//! Declarative rows for `agent inspect|update|move|pause|resume|offboard|execution-get|execution-set|readiness`.

use org_command::{Ack, Command, Example, Field, IfMatch, Kind, Type};

use super::schema::{EFFORT_TYPE, READ_ERRORS, WRITE_ERRORS};
use crate::shared::{DIRECTORY_CARD_FIELDS, ENTITY_OP_OUTPUT, MEMBER_INSPECT_INPUT, STRING_TYPE};

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

const AGENT_READINESS_INPUT: &[Field] = &[
    Field::required("member", Type::String, "Agent employee UUIDv7"),
    Field::optional(
        "task_id",
        Type::String,
        "Optional task UUIDv7 for budget check",
    ),
];

const AGENT_READINESS_CANDIDATE_FIELDS: &[Field] = &[
    Field::required("connection_id", Type::String, "Provider connection UUIDv7"),
    Field::required("priority", Type::Integer, "Failover priority order"),
    Field::required(
        "eligible",
        Type::Boolean,
        "Whether candidate is currently ready",
    ),
    Field::optional("skip_reason", Type::String, "Why candidate is skipped"),
    Field::optional("cooldown_until", Type::String, "Cooldown expiry timestamp"),
    Field::optional("driver_model_id", Type::String, "Driver-specific model ID"),
];
const AGENT_READINESS_CANDIDATE_TYPE: Type = Type::Object(AGENT_READINESS_CANDIDATE_FIELDS);

const AGENT_READINESS_OUTPUT: &[Field] = &[
    Field::required("employee_id", Type::String, "Agent employee UUIDv7"),
    Field::required("ready", Type::Boolean, "Whether agent can start a turn now"),
    Field::required("status", Type::String, "Readiness diagnostic code"),
    Field::optional(
        "model_profile_id",
        Type::String,
        "Configured model profile UUIDv7",
    ),
    Field::optional("model_id", Type::String, "Configured model identifier"),
    Field::required(
        "candidates",
        Type::List(&AGENT_READINESS_CANDIDATE_TYPE),
        "Evaluated provider connection candidates",
    ),
    Field::optional(
        "earliest_cooldown_until",
        Type::String,
        "Earliest cooldown expiry if pool is exhausted",
    ),
];

/// The `agent` rows of the directory and lifecycle.
pub const AGENT_COMMANDS: &[Command] = &[
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
    Command {
        resource: "agent",
        verb: "readiness",
        summary: "Check agent turn readiness and provider candidates",
        call: "member.agent_readiness",
        kind: Kind::Read,
        input: AGENT_READINESS_INPUT,
        content_field: None,
        output: AGENT_READINESS_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"member":"0193a5c0-0000-7000-8000-000000000051"}"#,
            output: r#"{"employee_id":"0193a5c0-0000-7000-8000-000000000051","ready":true,"status":"READY","model_profile_id":"0193a5c0-0000-7000-8000-000000000060","model_id":"sonnet","candidates":[]}"#,
        },
    },
];
