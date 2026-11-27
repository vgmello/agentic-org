//! `org inspect`: the organization snapshot of teams, members and log position.

use org_command::{Answer, Command, Example, Field, Kind, Request, Type};
use org_proto::org::OrgSnapshot;
use serde_json::{json, Map, Value};

use super::schema::{fetch_snapshot, team_value, READ_ERRORS, TEAM_TYPE};
use crate::runtime::domain::CommandCue;
use crate::runtime::input::POSITION_FIELDS;
use crate::runtime::transport::{OfficeTransport, Refusal};

const MEMBER_SUMMARY_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Employee ID"),
    Field::required(
        "kind",
        Type::Enum(&["person", "agent", "unknown"]),
        "Employee kind",
    ),
    Field::required("name", Type::String, "Display name"),
    Field::optional("handle", Type::String, "Addressing handle"),
    Field::optional("home_team_id", Type::String, "Home team ID"),
    Field::optional("title_in_team", Type::String, "Title in home team"),
    Field::required("owner", Type::Boolean, "Whether this member is the owner"),
    Field::required("version", Type::Integer, "Record revision"),
];

const MEMBER_SUMMARY_TYPE: Type = Type::Object(MEMBER_SUMMARY_FIELDS);

const ORG_INSPECT_OUTPUT: &[Field] = &[
    Field::required(
        "position",
        Type::Object(POSITION_FIELDS),
        "Committed organization position",
    ),
    Field::required("teams", Type::List(&TEAM_TYPE), "Active teams"),
    Field::required(
        "members",
        Type::List(&MEMBER_SUMMARY_TYPE),
        "Active members",
    ),
];

/// The `org` rows.
pub(super) const COMMANDS: &[Command] = &[Command {
    resource: "org",
    verb: "inspect",
    summary: "Inspect org structure, teams, and members",
    call: "org.snapshot",
    kind: Kind::Read,
    input: &[],
    content_field: None,
    output: ORG_INSPECT_OUTPUT,
    errors: READ_ERRORS,
    example: Example {
        input: "{}",
        output: r#"{"position":{"epoch":1,"seq":2},"teams":[{"id":"0193a5c0-0000-7000-8000-000000000001","name":"Core","version":1}],"members":[{"id":"0193a5c0-0000-7000-8000-000000000002","kind":"person","name":"Owner","handle":"owner","home_team_id":"0193a5c0-0000-7000-8000-000000000001","owner":true,"version":1}]}"#,
    },
}];

/// `Use when:` guidance of each `org` command.
pub(super) const CUES: &[CommandCue] = &[(
    "org",
    "inspect",
    "reading the full organization snapshot of active teams, members, and log position.",
)];

/// Runs an `org` command.
pub(super) fn dispatch<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    match request.command.verb {
        "inspect" => Some(read_org_inspect(transport)),
        _ => None,
    }
}

fn read_org_inspect<T: OfficeTransport + ?Sized>(transport: &mut T) -> Result<Answer, Refusal> {
    let snapshot = fetch_snapshot(transport)?;
    Ok(Answer::completed(snapshot_value(&snapshot)))
}

fn snapshot_value(snapshot: &OrgSnapshot) -> Value {
    let teams: Vec<Value> = snapshot.teams.iter().map(team_value).collect();
    let members: Vec<Value> = snapshot
        .members
        .iter()
        .map(|m| {
            let mut map = Map::new();
            map.insert("id".into(), json!(m.id));
            map.insert("kind".into(), json!(m.kind));
            map.insert("name".into(), json!(m.name));
            if let Some(handle) = &m.handle {
                map.insert("handle".into(), json!(handle));
            }
            if let Some(home) = &m.home_team_id {
                map.insert("home_team_id".into(), json!(home));
            }
            if let Some(title) = &m.title_in_team {
                map.insert("title_in_team".into(), json!(title));
            }
            map.insert("owner".into(), json!(m.owner));
            map.insert("version".into(), json!(m.version));
            Value::Object(map)
        })
        .collect();
    json!({
        "position": {
            "epoch": snapshot.position.epoch,
            "seq": snapshot.position.seq,
        },
        "teams": teams,
        "members": members,
    })
}
