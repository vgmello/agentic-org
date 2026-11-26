//! `directory list` (shorthand `who`): active people and agents with handles, roles and live
//! presence.

use org_command::{Alias, Answer, Command, Example, Field, Kind, Request, Type};
use org_proto::ErrorCode;
use serde_json::{Map, Value};

use crate::runtime::card::{card_value, who};
use crate::runtime::domain::CommandCue;
use crate::runtime::input::opt_str;
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport, Refusal};

/// `who` is `directory list`.
pub(super) const WHO_ALIAS: Alias = Alias {
    words: &["who"],
    command: &["directory", "list"],
};

const READ_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotAuthorized,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::Unavailable,
];

const DIRECTORY_LIST_INPUT: &[Field] = &[
    Field::optional("team", Type::String, "Optional team ID or name filter"),
    Field::optional(
        "member",
        Type::String,
        "Optional member ID or handle filter",
    ),
];

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
    Field::optional("role_title", Type::String, "Role title"),
    Field::required("presence", Type::String, "Current presence"),
];

/// The `directory` rows.
pub(super) const COMMANDS: &[Command] = &[Command {
    resource: "directory",
    verb: "list",
    summary: "List org directory members (`who`)",
    call: "who",
    kind: Kind::List,
    input: DIRECTORY_LIST_INPUT,
    content_field: None,
    output: DIRECTORY_CARD_FIELDS,
    errors: READ_ERRORS,
    example: Example {
        input: "{}",
        output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000003","name":"HR","handle":"hr","kind":"agent","owner":false,"agent_status":"active","role_title":"HR Lead","presence":"offline"}]"#,
    },
}];

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
