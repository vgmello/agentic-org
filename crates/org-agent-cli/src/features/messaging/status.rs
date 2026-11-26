//! `status set|clear`: the short live status line teammates see in the directory.

use org_command::{Ack, Answer, Command, Example, Field, IfMatch, Kind, Refusal, Request, Type};
use org_proto::directory::StatusSetResult;
use serde_json::{Map, Value};

use super::schema::WRITE_ERRORS;
use super::with_ack;
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{decode_err, insert_opt, opt_u64, reject_dry_run, require_str};
use crate::runtime::transport::OfficeTransport;

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
pub(super) const COMMANDS: &[Command] = &[
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

/// `Use when:` guidance of each `status` command.
pub(super) const CUES: &[CommandCue] = &[
    ("status", "clear", "clearing your live status line when you finish an activity or go idle."),
    ("status", "set", "publishing a short live status line (with optional `--ttl-ms`) so teammates see what you are doing."),
];

pub(super) fn write_status<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    clear: bool,
) -> Result<Answer, Refusal> {
    reject_dry_run(request)?;
    let mut params = Map::new();
    if clear {
        params.insert("text".into(), Value::Null);
    } else {
        let text = require_str(request, "text")?;
        params.insert("text".into(), Value::String(text.to_owned()));
        if let Some(ttl_ms) = opt_u64(request, "ttl_ms")? {
            params.insert("ttl_ms".into(), Value::from(ttl_ms));
        }
    }
    let raw = transport.call_method("status.set", Value::Object(params))?;
    let res: StatusSetResult = serde_json::from_value(raw).map_err(|_| decode_err("status.set"))?;
    let mut obj = Map::new();
    insert_opt(&mut obj, "status", res.status.as_deref());
    insert_opt(&mut obj, "status_expires_at", res.expires_at.as_deref());
    Ok(with_ack(Answer::completed(Value::Object(obj)), request))
}
