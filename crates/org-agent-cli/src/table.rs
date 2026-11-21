//! The `agent-os` command table (`org-command` rows, `doc-28` §3).

use org_command::{Command, CommandTable, Example, Field, Kind, Type};
use org_proto::ErrorCode;

const POSITION_FIELDS: &[Field] = &[
    Field::required("epoch", Type::Integer, "Organization log epoch"),
    Field::required("seq", Type::Integer, "Organization log sequence"),
];

const TEAM_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Team ID"),
    Field::required("name", Type::String, "Team name"),
    Field::optional("owner_team_id", Type::String, "Parent team ID"),
    Field::optional("lead_member_id", Type::String, "Lead employee ID"),
    Field::required("version", Type::Integer, "Record revision"),
];

const CARD_TEAM_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Team ID"),
    Field::required("name", Type::String, "Team name"),
    Field::optional("title_in_team", Type::String, "Employee title in the team"),
];

const CARD_TEAM_TYPE: Type = Type::Object(CARD_TEAM_FIELDS);
const TEAM_TYPE: Type = Type::Object(TEAM_FIELDS);

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

const DIRECTORY_CARD_FIELDS: &[Field] = &[
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
    Field::optional("role_title", Type::String, "Agent role title"),
    Field::required(
        "presence",
        Type::Enum(&["online", "working", "idle", "asleep", "offline", "unknown"]),
        "Current presence",
    ),
    Field::required(
        "teams",
        Type::List(&CARD_TEAM_TYPE),
        "Team membership cards",
    ),
];

const DIRECTORY_CARD_TYPE: Type = Type::Object(DIRECTORY_CARD_FIELDS);

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

const MEMBER_INSPECT_INPUT: &[Field] = &[Field::required(
    "member",
    Type::String,
    "Employee ID, handle, or owner",
)];

const READ_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotFound,
    ErrorCode::NotAuthorized,
    ErrorCode::Unavailable,
];

/// All commands exposed by `agent-os` on the shared `org-command` grammar.
pub const COMMANDS: &[Command] = &[
    Command {
        resource: "org",
        verb: "inspect",
        summary: "Inspect the organization structure, teams, and members",
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
    },
    Command {
        resource: "team",
        verb: "list",
        summary: "List active teams in the organization",
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
        summary: "Inspect a team and its active members",
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
        resource: "person",
        verb: "inspect",
        summary: "Inspect a person's directory card",
        call: "who",
        kind: Kind::Read,
        input: MEMBER_INSPECT_INPUT,
        content_field: None,
        output: DIRECTORY_CARD_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"member":"owner"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000002","name":"Owner","handle":"owner","kind":"person","owner":true,"presence":"offline","teams":[{"id":"0193a5c0-0000-7000-8000-000000000001","name":"Core"}]}"#,
        },
    },
    Command {
        resource: "agent",
        verb: "inspect",
        summary: "Inspect an agent's directory card",
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
];

/// Builds and validates the `agent-os` command table.
pub fn table() -> CommandTable {
    CommandTable::new(&[COMMANDS]).expect("agent-os command table is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_table_passes_all_org_command_contract_checks() {
        let table = table();
        org_command::testkit::check_rows(&table);
    }
}
