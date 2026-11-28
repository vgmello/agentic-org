//! Declarative rows for `team list|inspect|create|update|move|archive|set-limit`.

use org_command::{Ack, Command, Example, Field, IfMatch, Kind, Type};

use super::schema::{READ_ERRORS, TEAM_FIELDS, WRITE_ERRORS};
use crate::shared::{DIRECTORY_CARD_TYPE, ENTITY_OP_OUTPUT, MUTATION_OUTPUT};

const TEAM_INSPECT_INPUT: &[Field] = &[Field::required(
    "team",
    Type::String,
    "Team ID or unique team name",
)];

const TEAM_INSPECT_OUTPUT: &[Field] = &[
    Field::required("id", Type::String, "Team ID"),
    Field::required("name", Type::String, "Team name"),
    Field::optional("owner_team_id", Type::String, "Parent team ID"),
    Field::optional("lead_member_id", Type::String, "Lead employee ID"),
    Field::required("version", Type::Integer, "Record revision"),
    Field::required(
        "members",
        Type::List(&DIRECTORY_CARD_TYPE),
        "Members of this team",
    ),
];

const TEAM_CREATE_INPUT: &[Field] = &[
    Field::required("name", Type::String, "New team name"),
    Field::optional("parent_team_id", Type::String, "Parent team UUIDv7"),
    Field::optional("team_id", Type::String, "Caller-supplied team UUIDv7"),
];

const TEAM_UPDATE_INPUT: &[Field] = &[
    Field::required("team", Type::String, "Team UUIDv7"),
    Field::optional("name", Type::String, "New team name"),
    Field::optional("lead_member_id", Type::String, "New lead employee UUIDv7"),
    Field::optional("clear_lead", Type::Boolean, "Clear the current team lead"),
];

const TEAM_MOVE_INPUT: &[Field] = &[
    Field::required("team", Type::String, "Team UUIDv7"),
    Field::required("parent_team_id", Type::String, "New parent team UUIDv7"),
];

const TEAM_ARCHIVE_INPUT: &[Field] = &[Field::required("team", Type::String, "Team UUIDv7")];

const TEAM_SET_MAX_MEMBERS_INPUT: &[Field] = &[
    Field::required("team", Type::String, "Team UUIDv7"),
    Field::required(
        "max_members",
        Type::Integer,
        "Maximum active team members (1..=100)",
    ),
];

const TEAM_REQUEST_LIMIT_INPUT: &[Field] = &[
    Field::required("team", Type::String, "Team UUIDv7"),
    Field::required(
        "max_members",
        Type::Integer,
        "Requested active member cap (1..=100, above current)",
    ),
    Field::required("reason", Type::String, "Single-line justification"),
    Field::optional("request_id", Type::String, "Caller-supplied request UUIDv7"),
];

const TEAM_LIMIT_REQUESTS_INPUT: &[Field] = &[
    Field::optional("team", Type::String, "Filter by team UUIDv7"),
    Field::optional(
        "state",
        Type::Enum(&["pending", "approved", "denied"]),
        "Filter by request state",
    ),
    Field::optional(
        "approver",
        Type::String,
        "Filter by approver employee UUIDv7",
    ),
];

const TEAM_DECIDE_LIMIT_INPUT: &[Field] = &[
    Field::required("request", Type::String, "Team limit request UUIDv7"),
    Field::optional(
        "approve",
        Type::Boolean,
        "Approve and raise team max_members",
    ),
    Field::optional(
        "deny",
        Type::Boolean,
        "Deny without changing team max_members",
    ),
    Field::optional("reason", Type::String, "Optional single-line decision note"),
];

const TEAM_LIMIT_REQUEST_OUTPUT: &[Field] = &[
    Field::required("id", Type::String, "Limit request UUIDv7"),
    Field::required("team_id", Type::String, "Team UUIDv7"),
    Field::required("state", Type::String, "pending, approved, or denied"),
    Field::required("requested_by_employee_id", Type::String, "Requester UUIDv7"),
    Field::required(
        "approver_employee_id",
        Type::String,
        "Assigned manager UUIDv7",
    ),
    Field::required("current_max_members", Type::Integer, "Cap when requested"),
    Field::required("requested_max_members", Type::Integer, "Requested cap"),
    Field::required("reason", Type::String, "Requester reason"),
    Field::optional("decision_reason", Type::String, "Approver note"),
    Field::optional("decided_by_employee_id", Type::String, "Decider UUIDv7"),
    Field::optional("decided_at", Type::String, "Decision timestamp"),
    Field::required("revision", Type::Integer, "Request revision"),
    Field::required("created_at", Type::String, "Creation timestamp"),
    Field::required("updated_at", Type::String, "Last update timestamp"),
];

const TEAM_LIMIT_OP_OUTPUT: &[Field] = &[
    Field::required("request_id", Type::String, "Limit request UUIDv7"),
    Field::required("team_id", Type::String, "Team UUIDv7"),
    Field::required("state", Type::String, "pending, approved, or denied"),
    Field::required(
        "approver_employee_id",
        Type::String,
        "Assigned manager UUIDv7",
    ),
    Field::required("requested_max_members", Type::Integer, "Requested cap"),
    Field::required("revision", Type::Integer, "Request revision"),
    Field::optional(
        "team_revision",
        Type::Integer,
        "New team revision if approved",
    ),
    Field::required("generation", Type::Integer, "Store generation"),
];

/// The `team` rows.
pub const TEAM_COMMANDS: &[Command] = &[
    Command {
        resource: "team",
        verb: "list",
        summary: "List active teams",
        call: "org.snapshot",
        kind: Kind::List,
        input: &[],
        content_field: None,
        output: TEAM_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000001","name":"Core","version":1}]"#,
        },
    },
    Command {
        resource: "team",
        verb: "inspect",
        summary: "Inspect team and members",
        call: "org.snapshot",
        kind: Kind::Read,
        input: TEAM_INSPECT_INPUT,
        content_field: None,
        output: TEAM_INSPECT_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"team":"Core"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000001","name":"Core","version":1,"members":[{"id":"0193a5c0-0000-7000-8000-000000000002","name":"Owner","handle":"owner","kind":"person","owner":true,"presence":"offline","teams":[{"id":"0193a5c0-0000-7000-8000-000000000001","name":"Core"}]}]}"#,
        },
    },
    Command {
        resource: "team",
        verb: "create",
        summary: "Create a team",
        call: "team.create",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: TEAM_CREATE_INPUT,
        content_field: None,
        output: MUTATION_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"name":"Platform"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000010","status":"committed","position":{"epoch":1,"seq":3}}"#,
        },
    },
    Command {
        resource: "team",
        verb: "update",
        summary: "Rename team or change lead",
        call: "team.rename",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: TEAM_UPDATE_INPUT,
        content_field: None,
        output: MUTATION_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"team":"0193a5c0-0000-7000-8000-000000000010","name":"Core Platform"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000010","status":"committed","position":{"epoch":1,"seq":4}}"#,
        },
    },
    Command {
        resource: "team",
        verb: "move",
        summary: "Reparent a team",
        call: "team.move",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: TEAM_MOVE_INPUT,
        content_field: None,
        output: MUTATION_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"team":"0193a5c0-0000-7000-8000-000000000010","parent_team_id":"0193a5c0-0000-7000-8000-000000000001"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000010","status":"committed","position":{"epoch":1,"seq":5}}"#,
        },
    },
    Command {
        resource: "team",
        verb: "archive",
        summary: "Archive an empty team",
        call: "team.delete",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: TEAM_ARCHIVE_INPUT,
        content_field: None,
        output: MUTATION_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"team":"0193a5c0-0000-7000-8000-000000000010"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000010","status":"committed","position":{"epoch":1,"seq":6}}"#,
        },
    },
    Command {
        resource: "team",
        verb: "set-limit",
        summary: "Set team member cap (1..=100)",
        call: "team.set_max_members",
        kind: Kind::Write {
            ack: Ack::Durable,
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
    Command {
        resource: "team",
        verb: "request-limit",
        summary: "Request team cap increase",
        call: "team.limit_request.create",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: TEAM_REQUEST_LIMIT_INPUT,
        content_field: None,
        output: TEAM_LIMIT_OP_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"team":"0193a5c0-0000-7000-8000-000000000001","max_members":12,"reason":"Adding QA specialists"}"#,
            output: r#"{"request_id":"0193a5c0-0000-7000-8000-000000000099","team_id":"0193a5c0-0000-7000-8000-000000000001","state":"pending","approver_employee_id":"0193a5c0-0000-7000-8000-000000000002","requested_max_members":12,"revision":1,"generation":13}"#,
        },
    },
    Command {
        resource: "team",
        verb: "limit-requests",
        summary: "List team cap requests",
        call: "team.limit_request.list",
        kind: Kind::List,
        input: TEAM_LIMIT_REQUESTS_INPUT,
        content_field: None,
        output: TEAM_LIMIT_REQUEST_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"state":"pending"}"#,
            output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000099","team_id":"0193a5c0-0000-7000-8000-000000000001","state":"pending","requested_by_employee_id":"0193a5c0-0000-7000-8000-000000000020","approver_employee_id":"0193a5c0-0000-7000-8000-000000000002","current_max_members":7,"requested_max_members":12,"reason":"Adding QA specialists","revision":1,"created_at":"2026-11-27T00:00:00Z","updated_at":"2026-11-27T00:00:00Z"}]"#,
        },
    },
    Command {
        resource: "team",
        verb: "decide-limit",
        summary: "Approve or deny cap request",
        call: "team.limit_request.decide",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: TEAM_DECIDE_LIMIT_INPUT,
        content_field: None,
        output: TEAM_LIMIT_OP_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"request":"0193a5c0-0000-7000-8000-000000000099","approve":true}"#,
            output: r#"{"request_id":"0193a5c0-0000-7000-8000-000000000099","team_id":"0193a5c0-0000-7000-8000-000000000001","state":"approved","approver_employee_id":"0193a5c0-0000-7000-8000-000000000002","requested_max_members":12,"revision":2,"team_revision":3,"generation":14}"#,
        },
    },
];
