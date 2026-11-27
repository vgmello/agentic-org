//! `team list|inspect|create|update|move|archive|set-limit`.

use org_command::{Answer, Request};
use org_proto::org::{OrgSnapshot, Team};
use org_proto::ErrorCode;
use serde_json::{json, Map, Value};

use super::schema::{fetch_snapshot, team_map, team_value};
use super::{send_write, ConflictTarget};
use crate::runtime::card::{card_value, who};
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{
    entity_id, opt_bool, opt_str, require_if_match, require_str, send_entity_write,
};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport, Refusal};

/// `Use when:` guidance of each `team` command.
pub(super) const CUES: &[CommandCue] = &[
    (
        "team",
        "archive",
        "archiving an empty team that is no longer active (requires `--if-match`).",
    ),
    (
        "team",
        "create",
        "creating a new team (optionally nested under `--parent-team-id`).",
    ),
    (
        "team",
        "inspect",
        "viewing one team's metadata, lead, and active member directory cards.",
    ),
    (
        "team",
        "list",
        "listing all active teams, their parent hierarchy, and leads.",
    ),
    (
        "team",
        "move",
        "re-parenting a team under a different parent team (requires `--if-match`).",
    ),
    (
        "team",
        "set-limit",
        "updating a team's maximum active member cap (`1..=100`; requires `--if-match`).",
    ),
    (
        "team",
        "update",
        "renaming a team or changing/clearing its lead (requires `--if-match`).",
    ),
];

/// Runs a `team` command.
pub(super) fn dispatch<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    Some(match request.command.verb {
        "list" => read_team_list(transport, request),
        "inspect" => read_team_inspect(transport, request),
        "create" => write_team_create(transport, request),
        "update" => write_team_update(transport, request),
        "move" => write_team_move(transport, request),
        "archive" => write_team_archive(transport, request),
        "set-limit" => write_team_set_max_members(transport, request),
        _ => return None,
    })
}

fn read_team_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    parse_cursor(request.cursor.as_deref())?;
    let snapshot = fetch_snapshot(transport)?;
    let mut teams: Vec<&Team> = snapshot.teams.iter().collect();
    teams.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    paginate(request, teams.into_iter().map(team_value).collect())
}

fn read_team_inspect<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let target = require_str(request, "team")?;
    let snapshot = fetch_snapshot(transport)?;
    let team = find_team(&snapshot, target)?;
    let members: Vec<Value> = who(transport, json!({ "team": team.id }))?
        .iter()
        .map(card_value)
        .collect();
    let mut obj = team_map(team);
    obj.insert("members".into(), Value::Array(members));
    Ok(Answer::completed(Value::Object(obj)))
}

fn find_team<'a>(snapshot: &'a OrgSnapshot, target: &str) -> Result<&'a Team, Refusal> {
    if let Some(team) = snapshot.teams.iter().find(|t| t.id == target) {
        return Ok(team);
    }
    let needle = target.trim().to_lowercase();
    let matches: Vec<&Team> = snapshot
        .teams
        .iter()
        .filter(|t| t.name.trim().to_lowercase() == needle)
        .collect();
    match matches.as_slice() {
        [only] => Ok(only),
        [] => Err(Refusal::new(
            ErrorCode::NotFound,
            "No active team matches that reference.",
            "Run `team list` to see active teams.",
        )),
        _ => Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "More than one team has that name.",
            "Pass the team's UUIDv7 ID with --team.",
        )),
    }
}

fn write_team_create<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let name = require_str(request, "name")?;
    let team_id = entity_id(request, Some("team_id"), "team.create")?;
    let mut params = Map::new();
    params.insert("team_id".into(), json!(team_id));
    params.insert("name".into(), json!(name));
    if let Some(parent) = opt_str(request, "parent_team_id") {
        params.insert("owner_team_id".into(), json!(parent));
    }
    send_write(transport, request, "team.create", params, &team_id, None)
}

fn write_team_update<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let team_id = require_str(request, "team")?;
    let expected = require_if_match(request)?;
    let name = opt_str(request, "name");
    let lead = opt_str(request, "lead_member_id");
    let clear_lead = opt_bool(request, "clear_lead").unwrap_or(false);
    let wants_lead = lead.is_some() || clear_lead;
    if name.is_some() && wants_lead {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "Rename and lead change cannot be combined in one team update.",
            "Run `team update --name ...` and `team update --lead-member-id ...` separately.",
        ));
    }
    if lead.is_some() && clear_lead {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "--lead-member-id and --clear-lead cannot both be set.",
            "Pass either --lead-member-id <id> or --clear-lead.",
        ));
    }
    let mut params = Map::new();
    params.insert("team_id".into(), json!(team_id));
    params.insert("expected_version".into(), json!(expected));
    let method = if let Some(new_name) = name {
        params.insert("name".into(), json!(new_name));
        "team.rename"
    } else if wants_lead {
        if let Some(lead_id) = lead {
            params.insert("lead_member_id".into(), json!(lead_id));
        }
        "team.set_lead"
    } else {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "Nothing to update on this team.",
            "Pass --name, --lead-member-id, or --clear-lead.",
        ));
    };
    send_write(
        transport,
        request,
        method,
        params,
        team_id,
        Some(ConflictTarget::Team(team_id)),
    )
}

fn write_team_move<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let team_id = require_str(request, "team")?;
    let expected = require_if_match(request)?;
    let mut params = Map::new();
    params.insert("team_id".into(), json!(team_id));
    params.insert("expected_version".into(), json!(expected));
    let parent = require_str(request, "parent_team_id")?;
    params.insert("owner_team_id".into(), json!(parent));
    send_write(
        transport,
        request,
        "team.move",
        params,
        team_id,
        Some(ConflictTarget::Team(team_id)),
    )
}

fn write_team_archive<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let team_id = require_str(request, "team")?;
    let expected = require_if_match(request)?;
    let mut params = Map::new();
    params.insert("team_id".into(), json!(team_id));
    params.insert("expected_version".into(), json!(expected));
    send_write(
        transport,
        request,
        "team.delete",
        params,
        team_id,
        Some(ConflictTarget::Team(team_id)),
    )
}

fn write_team_set_max_members<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let team_id = require_str(request, "team")?;
    let max_members = request
        .input
        .get("max_members")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            Refusal::new(
                ErrorCode::InvalidRequest,
                "Missing required --max-members integer flag.",
                "Pass --max-members <1..=100>.",
            )
        })?;
    let expected = require_if_match(request)?;
    let mut params = Map::new();
    params.insert("team_id".into(), json!(team_id));
    params.insert("expected_version".into(), json!(expected));
    params.insert("max_members".into(), json!(max_members));
    send_entity_write(transport, request, "team.set_max_members", params)
}
