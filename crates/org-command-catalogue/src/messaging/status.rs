//! Declarative rows for `status set|clear`.

use org_command::{Ack, Command, Example, Field, IfMatch, Kind, Type};

use super::schema::WRITE_ERRORS;

const STATUS_SET_INPUT: &[Field] = &[
    Field::required("text", Type::String, "Short status line text"),
    Field::optional(
        "ttl_ms",
        Type::Integer,
        "Optional expiry duration in milliseconds",
    ),
];

const STATUS_CLEAR_INPUT: &[Field] = &[];

const STATUS_OUTPUT: &[Field] = &[
    Field::optional(
        "status",
        Type::String,
        "Current status text (null when cleared)",
    ),
    Field::optional(
        "status_expires_at",
        Type::String,
        "RFC 3339 UTC expiry timestamp if set",
    ),
];

/// The `status` rows.
pub const STATUS_COMMANDS: &[Command] = &[
    Command {
        resource: "status",
        verb: "set",
        summary: "Set live status line",
        call: "status.set",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: STATUS_SET_INPUT,
        content_field: None,
        output: STATUS_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"text":"Reviewing API schema","ttl_ms":600000}"#,
            output: r#"{"status":"Reviewing API schema","status_expires_at":"2026-11-21T12:10:00.000Z"}"#,
        },
    },
    Command {
        resource: "status",
        verb: "clear",
        summary: "Clear live status line",
        call: "status.set",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: STATUS_CLEAR_INPUT,
        content_field: None,
        output: STATUS_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: "{}",
            output: r#"{}"#,
        },
    },
];
