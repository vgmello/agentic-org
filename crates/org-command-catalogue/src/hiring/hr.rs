//! Declarative rows for `hr list|inspect|hire`.

use org_command::{Ack, Command, Example, Field, IfMatch, Kind, Type};

use super::schema::{EFFORT_TYPE, READ_ERRORS, WRITE_ERRORS};
use crate::shared::STRING_TYPE;

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
    Field::optional("effort", EFFORT_TYPE, "Reasoning effort"),
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
        "effort",
        EFFORT_TYPE,
        "Reasoning effort (low, medium, high, xhigh, max); omitted keeps the runtime default",
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

/// The `hr` rows.
pub const HR_COMMANDS: &[Command] = &[
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
            ack: Ack::Durable,
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
];
