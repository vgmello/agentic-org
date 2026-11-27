//! `member set-hiring`: granting or revoking an employee's right to hire agents.

use org_command::{Ack, Answer, Command, Example, Field, IfMatch, Kind, Request, Type};
use org_proto::ErrorCode;
use serde_json::{json, Map};

use super::schema::WRITE_ERRORS;
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{
    opt_bool, require_if_match, require_str, send_entity_write, ENTITY_OP_OUTPUT,
};
use crate::runtime::transport::{OfficeTransport, Refusal};

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
pub(super) const COMMANDS: &[Command] = &[Command {
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

/// `Use when:` guidance of each `member` command.
pub(super) const CUES: &[CommandCue] = &[
    ("member", "set-hiring", "granting (`--allow`) or revoking (`--revoke`) an employee's authorization to hire agents (requires `--if-match`)."),
];

/// Runs a `member` command.
pub(super) fn dispatch<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    match request.command.verb {
        "set-hiring" => Some(write_member_set_may_hire(transport, request)),
        _ => None,
    }
}

fn write_member_set_may_hire<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let member_id = require_str(request, "member")?;
    let allow = opt_bool(request, "allow").unwrap_or(false);
    let revoke = opt_bool(request, "revoke").unwrap_or(false);
    let may_hire = match (allow, revoke) {
        (true, false) => true,
        (false, true) => false,
        (true, true) => {
            return Err(Refusal::new(
                ErrorCode::InvalidRequest,
                "--allow and --revoke cannot be given together.",
                "Pass --allow to grant the right to hire, or --revoke to take it away.",
            ))
        }
        (false, false) => {
            return Err(Refusal::new(
                ErrorCode::InvalidRequest,
                "`member set-hiring` needs --allow or --revoke.",
                "Pass --allow to grant the right to hire, or --revoke to take it away.",
            ))
        }
    };
    let expected = require_if_match(request)?;
    let mut params = Map::new();
    params.insert("member_id".into(), json!(member_id));
    params.insert("expected_version".into(), json!(expected));
    params.insert("may_hire".into(), json!(may_hire));
    send_entity_write(transport, request, "member.set_may_hire", params)
}
