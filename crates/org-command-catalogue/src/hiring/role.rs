//! Declarative rows for `role list|inspect|create|update`.

use org_command::{Ack, Command, Example, Field, IfMatch, Kind, Type};

use super::schema::{READ_ERRORS, WRITE_ERRORS};
use crate::shared::ENTITY_OP_OUTPUT;

const ROLE_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Role UUIDv7"),
    Field::optional("key", Type::String, "Optional lowercase role key"),
    Field::required("name", Type::String, "Role name"),
    Field::required("body", Type::String, "Role description markdown"),
    Field::required("revision", Type::Integer, "Record revision"),
    Field::optional("created_at", Type::String, "Creation timestamp"),
    Field::optional("updated_at", Type::String, "Last update timestamp"),
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

/// The `role` rows.
pub const ROLE_COMMANDS: &[Command] = &[
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
            ack: Ack::Durable,
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
            ack: Ack::Durable,
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
];
