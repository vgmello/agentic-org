//! `org inspect`: the organization snapshot of teams, members and log position.

use org_command::{Answer, Request};
use org_proto::org::OrgSnapshot;
use serde_json::{json, Map, Value};

use super::schema::{fetch_snapshot, team_value};
use crate::runtime::domain::CommandCue;
use crate::runtime::transport::{OfficeTransport, Refusal};

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
