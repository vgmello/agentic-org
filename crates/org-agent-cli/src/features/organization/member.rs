//! `member set-hiring`: granting or revoking an employee's right to hire agents.

use org_command::{Answer, Request};
use org_proto::ErrorCode;
use serde_json::{json, Map};

use crate::runtime::domain::CommandCue;
use crate::runtime::input::{opt_bool, require_if_match, require_str, send_entity_write};
use crate::runtime::transport::{OfficeTransport, Refusal};

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
