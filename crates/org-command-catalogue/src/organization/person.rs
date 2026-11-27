//! Declarative rows for `person inspect|add|update|move|deactivate`.

use org_command::{Ack, Command, Example, Field, IfMatch, Kind, Type};

use super::schema::{READ_ERRORS, WRITE_ERRORS};
use crate::shared::{DIRECTORY_CARD_FIELDS, MEMBER_INSPECT_INPUT, MUTATION_OUTPUT};

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

/// The `person` rows.
pub const PERSON_COMMANDS: &[Command] = &[
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
            ack: Ack::Durable,
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
            ack: Ack::Durable,
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
            ack: Ack::Durable,
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
            ack: Ack::Durable,
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
];
