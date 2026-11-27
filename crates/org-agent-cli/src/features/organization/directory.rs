//! `directory list` (shorthand `who`): active people and agents with handles, roles and live
//! presence.

use org_command::{Answer, Request};
use serde_json::{Map, Value};

use crate::runtime::card::{card_value, who};
use crate::runtime::domain::CommandCue;
use crate::runtime::input::opt_str;
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport, Refusal};

/// `Use when:` guidance of each `directory` command.
pub(super) const CUES: &[CommandCue] = &[
    ("directory", "list", "finding active people and agents, `@handles`, roles, and live presence (shorthand: `agent-os who`)."),
];

/// Runs a `directory` command.
pub(super) fn dispatch<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    match request.command.verb {
        "list" => Some(read_directory_list(transport, request)),
        _ => None,
    }
}

fn read_directory_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    parse_cursor(request.cursor.as_deref())?;
    let mut params = Map::new();
    if let Some(team) = opt_str(request, "team") {
        params.insert("team".into(), Value::String(team.to_owned()));
    }
    if let Some(member) = opt_str(request, "member") {
        params.insert("member".into(), Value::String(member.to_owned()));
    }
    let cards = who(transport, Value::Object(params))?;
    paginate(request, cards.iter().map(card_value).collect())
}
