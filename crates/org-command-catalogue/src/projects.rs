//! Declarative command rows for `project list|inspect|create|add-team|remove-team`.

use org_command::{Ack, Command, Example, Field, IfMatch, Kind, Type};
use org_proto::ErrorCode;

use crate::shared::{MUTATION_OUTPUT, STRING_TYPE};

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

const PROJECT_ADD_TEAM_INPUT: &[Field] = &[
    Field::required("project_id", Type::String, "Project UUIDv7"),
    Field::required("team_id", Type::String, "Contributing team UUIDv7"),
    Field::optional(
        "project_team_id",
        Type::String,
        "Caller-supplied project-team link UUIDv7",
    ),
];

const PROJECT_REMOVE_TEAM_INPUT: &[Field] = &[
    Field::required("project_team_id", Type::String, "Project-team link UUIDv7"),
    Field::optional(
        "areas_to_team_id",
        Type::String,
        "Remaining team UUIDv7 to inherit areas owned by the leaving team",
    ),
];

/// The `project` rows.
pub const COMMANDS: &[Command] = &[
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
            ack: Ack::Durable,
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
    Command {
        resource: "project",
        verb: "add-team",
        summary: "Add a contributing team to a project",
        call: "project.add_team",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: PROJECT_ADD_TEAM_INPUT,
        content_field: None,
        output: MUTATION_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"project_id":"0193a5c0-0000-7000-8000-000000000030","team_id":"0193a5c0-0000-7000-8000-000000000011"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000031","status":"committed","position":{"epoch":1,"seq":12}}"#,
        },
    },
    Command {
        resource: "project",
        verb: "remove-team",
        summary: "Remove a contributing team from a project",
        call: "project.remove_team",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: PROJECT_REMOVE_TEAM_INPUT,
        content_field: None,
        output: MUTATION_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"project_team_id":"0193a5c0-0000-7000-8000-000000000031"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000031","status":"committed","position":{"epoch":1,"seq":13}}"#,
        },
    },
];

/// All command row slices in the projects domain.
pub const ALL: &[&[Command]] = &[COMMANDS];
