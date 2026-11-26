//! The shared B2.4 / B3 organization and project command table (`org-command` rows, `doc-28` §3.2).

use std::sync::OnceLock;

use org_command::{Ack, Command, CommandTable, Example, Field, IfMatch, Kind, Type};
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
const STRING_TYPE: Type = Type::String;

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
        Type::Enum(&[
            "online",
            "available",
            "busy",
            "idle",
            "needs_you",
            "waiting_for_answer",
            "paused",
            "unable_to_run",
            "with_you",
            "asleep",
            "offline",
            "unknown",
        ]),
        "Current presence",
    ),
    Field::required(
        "teams",
        Type::List(&CARD_TEAM_TYPE),
        "Team membership cards",
    ),
];

const DIRECTORY_CARD_TYPE: Type = Type::Object(DIRECTORY_CARD_FIELDS);

const PROJECT_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Project ID"),
    Field::required("number", Type::Integer, "Project sequence number"),
    Field::required("key", Type::String, "Project reference key (P-<number>)"),
    Field::required("slug", Type::String, "Canonical project slug"),
    Field::required("name", Type::String, "Project name"),
    Field::required("owner_team_id", Type::String, "Accountable owning team ID"),
    Field::required(
        "contributing_team_ids",
        Type::List(&STRING_TYPE),
        "Contributing team IDs",
    ),
    Field::required("archived", Type::Boolean, "Whether the project is archived"),
    Field::required("revision", Type::Integer, "Project record revision"),
];

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

const MUTATION_OUTPUT: &[Field] = &[
    Field::required("id", Type::String, "Target entity UUIDv7"),
    Field::required(
        "status",
        Type::Enum(&["committed", "checked", "queued"]),
        "Office command acknowledgement status",
    ),
    Field::optional(
        "position",
        Type::Object(POSITION_FIELDS),
        "Committed organization position",
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

const MEMBER_INSPECT_INPUT: &[Field] = &[Field::required(
    "member",
    Type::String,
    "Employee ID, handle, or owner",
)];

const PERSON_ADD_INPUT: &[Field] = &[
    Field::required("name", Type::String, "Person display name"),
    Field::optional("handle", Type::String, "Addressing handle"),
    Field::optional("home_team_id", Type::String, "Home team UUIDv7"),
    Field::optional("contact", Type::String, "Contact information"),
    Field::optional("member_id", Type::String, "Caller-supplied person UUIDv7"),
];

const PERSON_UPDATE_INPUT: &[Field] = &[
    Field::required("member", Type::String, "Person UUIDv7"),
    Field::optional("name", Type::String, "Updated display name"),
    Field::optional("contact", Type::String, "Updated contact information"),
    Field::optional("clear_contact", Type::Boolean, "Clear contact information"),
    Field::optional(
        "reports_to_member_id",
        Type::String,
        "Manager employee UUIDv7",
    ),
    Field::optional(
        "clear_reports_to",
        Type::Boolean,
        "Clear the person's reporting manager",
    ),
    Field::optional("title_in_team", Type::String, "Updated title in home team"),
];

const PERSON_MOVE_INPUT: &[Field] = &[
    Field::required("member", Type::String, "Person UUIDv7"),
    Field::required("to_team_id", Type::String, "Destination home team UUIDv7"),
];

const PERSON_DEACTIVATE_INPUT: &[Field] =
    &[Field::required("member", Type::String, "Person UUIDv7")];

const PROJECT_LIST_INPUT: &[Field] = &[Field::optional(
    "include_archived",
    Type::Boolean,
    "Include archived projects",
)];

const PROJECT_INSPECT_INPUT: &[Field] = &[Field::required(
    "project",
    Type::String,
    "Project UUIDv7, P-<number> key, or slug",
)];

const PROJECT_CREATE_INPUT: &[Field] = &[
    Field::required("name", Type::String, "Project name"),
    Field::optional("slug", Type::String, "Canonical lowercase project slug"),
    Field::optional(
        "owner_team_id",
        Type::String,
        "Existing accountable owning team UUIDv7",
    ),
    Field::optional(
        "create_team_id",
        Type::String,
        "Caller-supplied UUIDv7 for the default new owning team",
    ),
    Field::optional(
        "parent_team_id",
        Type::String,
        "Parent team UUIDv7 for a newly created owning team",
    ),
    Field::optional(
        "contributing_team_ids",
        Type::List(&STRING_TYPE),
        "Contributing team UUIDv7s",
    ),
    Field::optional("project_id", Type::String, "Caller-supplied project UUIDv7"),
];

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

/// All organization and project commands exposed on the shared `org-command` grammar.
pub const COMMANDS: &[Command] = &[
    Command {
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
    },
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
            ack: Ack::Committed,
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
            ack: Ack::Committed,
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
            ack: Ack::Committed,
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
            ack: Ack::Committed,
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
        resource: "person",
        verb: "inspect",
        summary: "Inspect person directory card",
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
        resource: "person",
        verb: "add",
        summary: "Add a person",
        call: "member.add_person",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: PERSON_ADD_INPUT,
        content_field: None,
        output: MUTATION_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"name":"Ada Lovelace","handle":"ada","home_team_id":"0193a5c0-0000-7000-8000-000000000001"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000020","status":"committed","position":{"epoch":1,"seq":7}}"#,
        },
    },
    Command {
        resource: "person",
        verb: "update",
        summary: "Update person profile or manager",
        call: "member.update_person",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::Required,
        },
        input: PERSON_UPDATE_INPUT,
        content_field: None,
        output: MUTATION_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"member":"0193a5c0-0000-7000-8000-000000000020","name":"Ada Byron"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000020","status":"committed","position":{"epoch":1,"seq":8}}"#,
        },
    },
    Command {
        resource: "person",
        verb: "move",
        summary: "Move person to another team",
        call: "member.move",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::Required,
        },
        input: PERSON_MOVE_INPUT,
        content_field: None,
        output: MUTATION_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"member":"0193a5c0-0000-7000-8000-000000000020","to_team_id":"0193a5c0-0000-7000-8000-000000000010"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000020","status":"committed","position":{"epoch":1,"seq":9}}"#,
        },
    },
    Command {
        resource: "person",
        verb: "deactivate",
        summary: "Deactivate a person",
        call: "member.remove",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::Required,
        },
        input: PERSON_DEACTIVATE_INPUT,
        content_field: None,
        output: MUTATION_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"member":"0193a5c0-0000-7000-8000-000000000020"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000020","status":"committed","position":{"epoch":1,"seq":10}}"#,
        },
    },
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
        resource: "project",
        verb: "list",
        summary: "List org projects",
        call: "project.list",
        kind: Kind::List,
        input: PROJECT_LIST_INPUT,
        content_field: None,
        output: PROJECT_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000030","number":1,"key":"P-1","slug":"billing","name":"Billing","owner_team_id":"0193a5c0-0000-7000-8000-000000000010","contributing_team_ids":[],"archived":false,"revision":1}]"#,
        },
    },
    Command {
        resource: "project",
        verb: "inspect",
        summary: "Inspect project by ID, key, or slug",
        call: "project.inspect",
        kind: Kind::Read,
        input: PROJECT_INSPECT_INPUT,
        content_field: None,
        output: PROJECT_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"project":"P-1"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000030","number":1,"key":"P-1","slug":"billing","name":"Billing","owner_team_id":"0193a5c0-0000-7000-8000-000000000010","contributing_team_ids":[],"archived":false,"revision":1}"#,
        },
    },
    Command {
        resource: "project",
        verb: "create",
        summary: "Create a project and owning team",
        call: "project.create",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: PROJECT_CREATE_INPUT,
        content_field: None,
        output: MUTATION_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"name":"Billing"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000030","status":"committed","position":{"epoch":1,"seq":11}}"#,
        },
    },
];

/// Every domain's rows: the one list both tables below are built from.
const DOMAINS: &[&[Command]] = &[
    COMMANDS,
    crate::hiring_table::HIRING_COMMANDS,
    crate::session_table::SESSION_COMMANDS,
    crate::msg_table::MSG_COMMANDS,
    crate::task_table::TASK_COMMANDS,
    crate::git_table::GIT_COMMANDS,
    crate::process_table::PROCESS_COMMANDS,
    crate::budget_table::BUDGET_COMMANDS,
    crate::external_auth::operator::EXTERNAL_COMMANDS,
];

/// Builds and validates the shared organization/project/HR/session/messaging command table:
/// every row's names, fields and example are checked. For the contract tests; a running
/// command uses [`runtime_table`].
pub fn table() -> CommandTable {
    CommandTable::new(DOMAINS).expect("agent-os command table is valid")
}

/// The same rows for a running command, built once per process and without the checks:
/// `agent-os` does nothing before its call that the call does not need (`doc-28` §2.11,
/// `doc-7` #18), and [`table`]'s contract tests are what make skipping the checks safe.
pub fn runtime_table() -> &'static CommandTable {
    static TABLE: OnceLock<CommandTable> = OnceLock::new();
    TABLE.get_or_init(|| CommandTable::unchecked(DOMAINS))
}

#[cfg(test)]
mod tests;
