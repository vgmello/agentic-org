//! C2 HR hiring, role catalog, team member limits, `may_hire` permission, and agent lifecycle
//! rows on the shared `org-command` grammar (`TASK-41`, `ADR-0029`, `CLI-14`).

use org_command::{Ack, Command, Example, Field, IfMatch, Kind, Type};
use org_proto::ErrorCode;

const STRING_TYPE: Type = Type::String;

const READ_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotFound,
    ErrorCode::NotAuthorized,
    ErrorCode::Unavailable,
];

const WRITE_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::InvalidName,
    ErrorCode::AlreadyExists,
    ErrorCode::NotFound,
    ErrorCode::NotEmpty,
    ErrorCode::Conflict,
    ErrorCode::OperationReused,
    ErrorCode::NotAuthorized,
    ErrorCode::Unavailable,
];

const ROLE_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Role UUIDv7"),
    Field::optional("key", Type::String, "Optional lowercase role key"),
    Field::required("name", Type::String, "Role name"),
    Field::required("body", Type::String, "Role description markdown"),
    Field::required("revision", Type::Integer, "Record revision"),
    Field::optional("created_at", Type::String, "Creation timestamp"),
    Field::optional("updated_at", Type::String, "Last update timestamp"),
];

const ENTITY_OP_OUTPUT: &[Field] = &[
    Field::required("id", Type::String, "Target entity UUIDv7"),
    Field::required("revision", Type::Integer, "Committed record revision"),
    Field::required("generation", Type::Integer, "Organization store generation"),
];

const HIRE_OP_OUTPUT: &[Field] = &[
    Field::required("request_id", Type::String, "Hire request UUIDv7"),
    Field::required(
        "state",
        Type::Enum(&["drafting", "hiring", "hired", "failed", "withdrawn"]),
        "Current hire request state",
    ),
    Field::optional("employee_id", Type::String, "Hired agent employee UUIDv7"),
    Field::optional("failed_step", Type::String, "Provisioning step that failed"),
    Field::required("revision", Type::Integer, "Hire request revision"),
    Field::required("generation", Type::Integer, "Organization store generation"),
];

const HIRE_PROPOSAL_FIELDS: &[Field] = &[
    Field::required("name", Type::String, "Agent display name"),
    Field::required("handle", Type::String, "Agent handle"),
    Field::optional("title", Type::String, "Agent title"),
    Field::optional("focus", Type::String, "Agent focus"),
    Field::optional("role_text", Type::String, "Ad-hoc role text"),
    Field::optional("title_in_team", Type::String, "Title in home team"),
    Field::optional(
        "reports_to_employee_id",
        Type::String,
        "Manager employee UUIDv7",
    ),
    Field::optional("office_id", Type::String, "Assigned office UUIDv7"),
    Field::optional("model_profile_id", Type::String, "Model profile UUIDv7"),
    Field::optional("model_id", Type::String, "Model identifier"),
    Field::optional(
        "permission_preset",
        Type::String,
        "Execution permission preset",
    ),
    Field::optional(
        "tools_config",
        Type::List(&STRING_TYPE),
        "Enabled tool names",
    ),
    Field::optional(
        "system_prompt_markdown",
        Type::String,
        "Agent system prompt",
    ),
    Field::optional("body", Type::String, "Agent profile markdown body"),
];

const HIRE_REQUEST_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Hire request UUIDv7"),
    Field::required("revision", Type::Integer, "Hire request revision"),
    Field::required(
        "state",
        Type::Enum(&["drafting", "hiring", "hired", "failed", "withdrawn"]),
        "Hire request state",
    ),
    Field::required(
        "requested_by_employee_id",
        Type::String,
        "Requester employee UUIDv7",
    ),
    Field::required("home_team_id", Type::String, "Destination home team UUIDv7"),
    Field::optional("role_id", Type::String, "Catalog role UUIDv7"),
    Field::required(
        "proposal",
        Type::Object(HIRE_PROPOSAL_FIELDS),
        "Hire proposal",
    ),
    Field::required("reason", Type::String, "Reason for hiring"),
    Field::optional("confirmed_at", Type::String, "Confirmation timestamp"),
    Field::optional("closed_reason", Type::String, "Withdrawal reason"),
    Field::optional("employee_id", Type::String, "Created agent employee UUIDv7"),
    Field::optional("failed_step", Type::String, "Failed provisioning step"),
    Field::required("created_at", Type::String, "Creation timestamp"),
    Field::required("updated_at", Type::String, "Last update timestamp"),
];

const ROLE_INSPECT_INPUT: &[Field] = &[Field::required(
    "role",
    Type::String,
    "Role UUIDv7, key, or unique name",
)];

const ROLE_CREATE_INPUT: &[Field] = &[
    Field::required("name", Type::String, "Role display name"),
    Field::optional("key", Type::String, "Optional lowercase slug key"),
    Field::optional("body", Type::String, "Role description markdown"),
    Field::optional("role_id", Type::String, "Caller-supplied role UUIDv7"),
];

const ROLE_UPDATE_INPUT: &[Field] = &[
    Field::required("role", Type::String, "Role UUIDv7"),
    Field::optional("name", Type::String, "Updated role name"),
    Field::optional("key", Type::String, "Updated lowercase role key"),
    Field::optional("clear_key", Type::Boolean, "Clear the role key"),
    Field::optional("body", Type::String, "Updated role description markdown"),
];

const HR_LIST_INPUT: &[Field] = &[
    Field::optional("team_id", Type::String, "Filter by home team UUIDv7"),
    Field::optional(
        "state",
        Type::Enum(&["drafting", "hiring", "hired", "failed", "withdrawn"]),
        "Filter by hire state",
    ),
];

const HR_INSPECT_INPUT: &[Field] = &[Field::required(
    "request_id",
    Type::String,
    "Hire request UUIDv7",
)];

const HR_HIRE_INPUT: &[Field] = &[
    Field::optional(
        "draft",
        Type::Boolean,
        "Create only a drafting hire request without provisioning",
    ),
    Field::optional(
        "id",
        Type::String,
        "Existing hire request UUIDv7 to confirm or withdraw",
    ),
    Field::optional(
        "withdraw",
        Type::Boolean,
        "Withdraw the hire request named by --id",
    ),
    Field::optional("team", Type::String, "Home team UUIDv7 for the new agent"),
    Field::optional("name", Type::String, "Agent display name"),
    Field::optional("handle", Type::String, "Agent addressing handle"),
    Field::optional("role_id", Type::String, "Catalog role UUIDv7"),
    Field::optional("reason", Type::String, "Reason for hiring"),
    Field::optional("title", Type::String, "Agent title"),
    Field::optional("focus", Type::String, "Agent focus"),
    Field::optional("role_text", Type::String, "Ad-hoc role text"),
    Field::optional("title_in_team", Type::String, "Title in home team"),
    Field::optional(
        "reports_to_member_id",
        Type::String,
        "Manager employee UUIDv7",
    ),
    Field::optional("office_id", Type::String, "Assigned office UUIDv7"),
    Field::optional("model_profile_id", Type::String, "Model profile UUIDv7"),
    Field::optional("model_id", Type::String, "Model identifier"),
    Field::optional(
        "permission_preset",
        Type::String,
        "Execution permission preset",
    ),
    Field::optional(
        "system_prompt_markdown",
        Type::String,
        "Agent system prompt markdown",
    ),
    Field::optional("body", Type::String, "Agent profile markdown body"),
    Field::optional(
        "request_id",
        Type::String,
        "Caller-supplied hire request UUIDv7",
    ),
    Field::optional(
        "employee_id",
        Type::String,
        "Caller-supplied agent employee UUIDv7",
    ),
    Field::optional(
        "closed_reason",
        Type::String,
        "Reason when withdrawing a request",
    ),
];

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
        "Execution permission preset",
    ),
    Field::optional("enabled", Type::Boolean, "Enable or disable execution"),
    Field::required(
        "system_prompt_markdown",
        Type::String,
        "Agent system prompt markdown",
    ),
];

const MEMBER_SET_MAY_HIRE_INPUT: &[Field] = &[
    Field::required("member", Type::String, "Employee UUIDv7"),
    Field::required(
        "may_hire",
        Type::Boolean,
        "Whether the employee may hire agents",
    ),
];

const TEAM_SET_MAX_MEMBERS_INPUT: &[Field] = &[
    Field::required("team", Type::String, "Team UUIDv7"),
    Field::required(
        "max_members",
        Type::Integer,
        "Maximum active team members (1..=100)",
    ),
];

/// C2 HR hiring, role catalog, permission, team limit, and agent lifecycle commands.
pub const HIRING_COMMANDS: &[Command] = &[
    Command {
        resource: "role",
        verb: "list",
        summary: "List catalog roles",
        call: "role.list",
        kind: Kind::List,
        input: &[],
        content_field: None,
        output: ROLE_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000040","key":"backend-engineer","name":"Backend Engineer","body":"Builds backend services.","revision":1}]"#,
        },
    },
    Command {
        resource: "role",
        verb: "inspect",
        summary: "Inspect a catalog role",
        call: "role.inspect",
        kind: Kind::Read,
        input: ROLE_INSPECT_INPUT,
        content_field: None,
        output: ROLE_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"role":"backend-engineer"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000040","key":"backend-engineer","name":"Backend Engineer","body":"Builds backend services.","revision":1}"#,
        },
    },
    Command {
        resource: "role",
        verb: "create",
        summary: "Create a catalog role",
        call: "role.create",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: ROLE_CREATE_INPUT,
        content_field: Some("body"),
        output: ENTITY_OP_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"name":"Backend Engineer","key":"backend-engineer","body":"Builds backend services."}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000040","revision":1,"generation":3}"#,
        },
    },
    Command {
        resource: "role",
        verb: "update",
        summary: "Update a catalog role",
        call: "role.update",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::Required,
        },
        input: ROLE_UPDATE_INPUT,
        content_field: Some("body"),
        output: ENTITY_OP_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"role":"0193a5c0-0000-7000-8000-000000000040","name":"Senior Backend Engineer"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000040","revision":2,"generation":4}"#,
        },
    },
    Command {
        resource: "hr",
        verb: "list",
        summary: "List HR hire requests",
        call: "hr.hire.list",
        kind: Kind::List,
        input: HR_LIST_INPUT,
        content_field: None,
        output: HIRE_REQUEST_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000050","revision":1,"state":"drafting","requested_by_employee_id":"0193a5c0-0000-7000-8000-000000000002","home_team_id":"0193a5c0-0000-7000-8000-000000000001","proposal":{"name":"Turing","handle":"turing"},"reason":"Need backend engineer","created_at":"2026-11-21T00:00:00Z","updated_at":"2026-11-21T00:00:00Z"}]"#,
        },
    },
    Command {
        resource: "hr",
        verb: "inspect",
        summary: "Inspect HR hire request",
        call: "hr.hire.inspect",
        kind: Kind::Read,
        input: HR_INSPECT_INPUT,
        content_field: None,
        output: HIRE_REQUEST_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"request_id":"0193a5c0-0000-7000-8000-000000000050"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000050","revision":1,"state":"drafting","requested_by_employee_id":"0193a5c0-0000-7000-8000-000000000002","home_team_id":"0193a5c0-0000-7000-8000-000000000001","proposal":{"name":"Turing","handle":"turing"},"reason":"Need backend engineer","created_at":"2026-11-21T00:00:00Z","updated_at":"2026-11-21T00:00:00Z"}"#,
        },
    },
    Command {
        resource: "hr",
        verb: "hire",
        summary: "Hire, draft, confirm, or withdraw agent",
        call: "hr.hire",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::Optional,
        },
        input: HR_HIRE_INPUT,
        content_field: None,
        output: HIRE_OP_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"team":"0193a5c0-0000-7000-8000-000000000001","name":"Turing","handle":"turing","reason":"Need backend engineer"}"#,
            output: r#"{"request_id":"0193a5c0-0000-7000-8000-000000000050","state":"hired","employee_id":"0193a5c0-0000-7000-8000-000000000051","revision":1,"generation":5}"#,
        },
    },
    Command {
        resource: "agent",
        verb: "update",
        summary: "Update agent profile or manager",
        call: "member.agent_update",
        kind: Kind::Write {
            ack: Ack::Committed,
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
            ack: Ack::Committed,
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
            ack: Ack::Committed,
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
            ack: Ack::Committed,
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
            ack: Ack::Committed,
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
            output: r#"{"employee_id":"0193a5c0-0000-7000-8000-000000000051","model_profile_id":"0193a5c0-0000-7000-8000-000000000060","model_id":"sonnet","tools_config":[],"permission_preset":"autonomous_sandboxed","enabled":true,"system_prompt_markdown":"You are an engineer.","revision":1}"#,
        },
    },
    Command {
        resource: "agent",
        verb: "execution-set",
        summary: "Configure agent model profile, preset, and prompt",
        call: "member.agent_execution_set",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::Optional,
        },
        input: AGENT_EXECUTION_SET_INPUT,
        content_field: Some("system_prompt_markdown"),
        output: ENTITY_OP_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"member":"0193a5c0-0000-7000-8000-000000000051","model_profile_id":"0193a5c0-0000-7000-8000-000000000060","model_id":"sonnet","permission_preset":"autonomous_sandboxed","system_prompt_markdown":"You are an engineer."}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000051","revision":1,"generation":10}"#,
        },
    },
    Command {
        resource: "member",
        verb: "set-hiring",
        summary: "Grant or revoke hiring authority",
        call: "member.set_may_hire",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::Required,
        },
        input: MEMBER_SET_MAY_HIRE_INPUT,
        content_field: None,
        output: ENTITY_OP_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"member":"0193a5c0-0000-7000-8000-000000000051","may_hire":true}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000051","revision":7,"generation":11}"#,
        },
    },
    Command {
        resource: "team",
        verb: "set-limit",
        summary: "Set team member cap (1..=100)",
        call: "team.set_max_members",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::Required,
        },
        input: TEAM_SET_MAX_MEMBERS_INPUT,
        content_field: None,
        output: ENTITY_OP_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"team":"0193a5c0-0000-7000-8000-000000000001","max_members":10}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000001","revision":2,"generation":12}"#,
        },
    },
];
