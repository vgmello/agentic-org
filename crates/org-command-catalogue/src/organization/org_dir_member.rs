//! Declarative rows for `org inspect`, `directory list` (`who`), and `member set-hiring`.

use org_command::{Ack, Alias, Command, Example, Field, IfMatch, Kind, Type};
use org_proto::ErrorCode;

use super::schema::{READ_ERRORS, TEAM_TYPE, WRITE_ERRORS};
use crate::shared::{ENTITY_OP_OUTPUT, POSITION_FIELDS};

const MEMBER_SUMMARY_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Employee ID"),
    Field::required(
        "kind",
        Type::Enum(&["person", "agent", "unknown"]),
        "Employee kind",
    ),
    Field::required("name", Type::String, "Display name"),
    Field::optional("handle", Type::String, "Addressing handle"),
    Field::optional("home_team_id", Type::String, "Home team ID"),
    Field::optional("title_in_team", Type::String, "Title in home team"),
    Field::required("owner", Type::Boolean, "Whether this member is the owner"),
    Field::required("version", Type::Integer, "Record revision"),
];

const MEMBER_SUMMARY_TYPE: Type = Type::Object(MEMBER_SUMMARY_FIELDS);

const ORG_INSPECT_OUTPUT: &[Field] = &[
    Field::required(
        "position",
        Type::Object(POSITION_FIELDS),
        "Committed organization position",
    ),
    Field::required("teams", Type::List(&TEAM_TYPE), "Active teams"),
    Field::required(
        "members",
        Type::List(&MEMBER_SUMMARY_TYPE),
        "Active members",
    ),
];

/// The `org` rows.
pub const ORG_COMMANDS: &[Command] = &[Command {
    resource: "org",
    verb: "inspect",
    summary: "Inspect org structure, teams, and members",
    call: "org.snapshot",
    kind: Kind::Read,
    input: &[],
    content_field: None,
    output: ORG_INSPECT_OUTPUT,
    errors: READ_ERRORS,
    example: Example {
        input: "{}",
        output: r#"{"position":{"epoch":1,"seq":2},"teams":[{"id":"0193a5c0-0000-7000-8000-000000000001","name":"Core","version":1}],"members":[{"id":"0193a5c0-0000-7000-8000-000000000002","kind":"person","name":"Owner","handle":"owner","home_team_id":"0193a5c0-0000-7000-8000-000000000001","owner":true,"version":1}]}"#,
    },
}];

/// `who` is `directory list`.
pub const WHO_ALIAS: Alias = Alias {
    words: &["who"],
    command: &["directory", "list"],
};

const DIRECTORY_READ_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotAuthorized,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::Unavailable,
];

const DIRECTORY_LIST_INPUT: &[Field] = &[
    Field::optional("team", Type::String, "Optional team ID or name filter"),
    Field::optional(
        "member",
        Type::String,
        "Optional member ID or handle filter",
    ),
];

const DIRECTORY_LIST_CARD_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Employee ID"),
    Field::required("name", Type::String, "Display name"),
    Field::optional("handle", Type::String, "Addressing handle"),
    Field::required(
        "kind",
        Type::Enum(&["person", "agent", "unknown"]),
        "Employee kind",
    ),
    Field::required("owner", Type::Boolean, "Whether this member is the owner"),
    Field::optional(
        "agent_status",
        Type::Enum(&["active", "paused", "offboarded", "unknown"]),
        "Agent lifecycle status",
    ),
    Field::optional("role_title", Type::String, "Role title"),
    Field::required("presence", Type::String, "Current presence"),
];

/// The `directory` rows.
pub const DIRECTORY_COMMANDS: &[Command] = &[Command {
    resource: "directory",
    verb: "list",
    summary: "List org directory members (`who`)",
    call: "who",
    kind: Kind::List,
    input: DIRECTORY_LIST_INPUT,
    content_field: None,
    output: DIRECTORY_LIST_CARD_FIELDS,
    errors: DIRECTORY_READ_ERRORS,
    example: Example {
        input: "{}",
        output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000003","name":"HR","handle":"hr","kind":"agent","owner":false,"agent_status":"active","role_title":"HR Lead","presence":"offline"}]"#,
    },
}];

const MEMBER_SET_MAY_HIRE_INPUT: &[Field] = &[
    Field::required("member", Type::String, "Employee UUIDv7"),
    Field::optional(
        "allow",
        Type::Boolean,
        "Grant the right to hire agents (give this or --revoke)",
    ),
    Field::optional(
        "revoke",
        Type::Boolean,
        "Take the right to hire agents away (give this or --allow)",
    ),
];

/// The `member` rows.
pub const MEMBER_COMMANDS: &[Command] = &[Command {
    resource: "member",
    verb: "set-hiring",
    summary: "Grant or revoke hiring authority",
    call: "member.set_may_hire",
    kind: Kind::Write {
        ack: Ack::Durable,
        if_match: IfMatch::Required,
    },
    input: MEMBER_SET_MAY_HIRE_INPUT,
    content_field: None,
    output: ENTITY_OP_OUTPUT,
    errors: WRITE_ERRORS,
    example: Example {
        input: r#"{"member":"0193a5c0-0000-7000-8000-000000000051","allow":true}"#,
        output: r#"{"id":"0193a5c0-0000-7000-8000-000000000051","revision":7,"generation":11}"#,
    },
}];
