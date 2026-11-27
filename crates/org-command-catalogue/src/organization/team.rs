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
];
