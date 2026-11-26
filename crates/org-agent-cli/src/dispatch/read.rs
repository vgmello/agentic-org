//! Organization, team, member and project reads, and the JSON views they answer with.

use org_command::{Answer, Request};
use org_proto::directory::{DirectoryCard, WhoResult};
use org_proto::org::{MemberKind, OrgSnapshot, Team};
use org_proto::projects::{ProjectListResult, ProjectRecord};
use org_proto::ErrorCode;
use serde_json::{json, Map, Value};

use super::{decode_err, opt_bool, require_str};
use crate::transport::{parse_cursor, OfficeTransport, Refusal};

pub(super) fn fetch_snapshot<T: OfficeTransport + ?Sized>(
    transport: &mut T,
) -> Result<OrgSnapshot, Refusal> {
    let raw = transport.call_method("org.snapshot", json!({}))?;
    serde_json::from_value(raw).map_err(|_| {
        Refusal::new(
            ErrorCode::Unavailable,
            "The office returned an unreadable organization snapshot.",
            "Retry the command or check the office version.",
        )
    })
}

pub(super) fn read_org_inspect<T: OfficeTransport + ?Sized>(
    transport: &mut T,
) -> Result<Answer, Refusal> {
    let snapshot = fetch_snapshot(transport)?;
    Ok(Answer::completed(snapshot_value(&snapshot)))
}

pub(super) fn read_team_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let offset = parse_cursor(request.cursor.as_deref())?;
    let snapshot = fetch_snapshot(transport)?;
    let mut teams: Vec<&Team> = snapshot.teams.iter().collect();
    teams.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    let total = teams.len();
    let limit = request.limit.unwrap_or(org_command::DEFAULT_LIMIT) as usize;
    let items: Vec<Value> = teams
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(team_value)
        .collect();
    let next = offset.saturating_add(items.len());
    let mut answer = Answer::completed(Value::Array(items));
    if next < total {
        answer.next_cursor = Some(next.to_string());
    }
    Ok(answer)
}

pub(super) fn read_team_inspect<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let target = require_str(request, "team")?;
    let snapshot = fetch_snapshot(transport)?;
    let team = find_team(&snapshot, target)?;
    let raw = transport.call_method("who", json!({ "team": team.id }))?;
    let who: WhoResult = serde_json::from_value(raw).map_err(|_| decode_err("who"))?;
    let members: Vec<Value> = who.members.iter().map(card_value).collect();
    let mut obj = match team_value(team) {
        Value::Object(map) => map,
        _ => unreachable!(),
    };
    obj.insert("members".into(), Value::Array(members));
    Ok(Answer::completed(Value::Object(obj)))
}

pub(super) fn read_member_inspect<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    resource: &str,
) -> Result<Answer, Refusal> {
    let member = require_str(request, "member")?;
    let raw = transport.call_method("who", json!({ "member": member }))?;
    let who: WhoResult = serde_json::from_value(raw).map_err(|_| decode_err("who"))?;
    let expected = if resource == "person" {
        MemberKind::Person
    } else {
        MemberKind::Agent
    };
    let Some(card) = who.members.iter().find(|c| c.kind == expected) else {
        return Err(Refusal::new(
            ErrorCode::NotFound,
            format!("No active {resource} matches that reference."),
            "Run `org inspect` to see active members.",
        ));
    };
    Ok(Answer::completed(card_value(card)))
}

pub(super) fn read_project_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let offset = parse_cursor(request.cursor.as_deref())?;
    let include_archived = opt_bool(request, "include_archived").unwrap_or(false);
    let raw = transport.call_method(
        "project.list",
        json!({ "include_archived": include_archived }),
    )?;
    let listed: ProjectListResult =
        serde_json::from_value(raw).map_err(|_| decode_err("project.list"))?;
    let total = listed.projects.len();
    let limit = request.limit.unwrap_or(org_command::DEFAULT_LIMIT) as usize;
    let items: Vec<Value> = listed
        .projects
        .iter()
        .skip(offset)
        .take(limit)
        .map(project_value)
        .collect();
    let next = offset.saturating_add(items.len());
    let mut answer = Answer::completed(Value::Array(items));
    if next < total {
        answer.next_cursor = Some(next.to_string());
    }
    Ok(answer)
}

pub(super) fn read_project_inspect<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let project = require_str(request, "project")?;
    let raw = transport.call_method("project.inspect", json!({ "project": project }))?;
    let record: ProjectRecord =
        serde_json::from_value(raw).map_err(|_| decode_err("project.inspect"))?;
    Ok(Answer::completed(project_value(&record)))
}

pub(super) fn find_team<'a>(snapshot: &'a OrgSnapshot, target: &str) -> Result<&'a Team, Refusal> {
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

pub(super) fn team_value(team: &Team) -> Value {
    let mut map = Map::new();
    map.insert("id".into(), json!(team.id));
    map.insert("name".into(), json!(team.name));
    if let Some(parent) = &team.owner_team_id {
        map.insert("owner_team_id".into(), json!(parent));
    }
    if let Some(lead) = &team.lead_member_id {
        map.insert("lead_member_id".into(), json!(lead));
    }
    map.insert("version".into(), json!(team.version));
    Value::Object(map)
}

pub(super) fn snapshot_value(snapshot: &OrgSnapshot) -> Value {
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

pub(super) fn card_value(card: &DirectoryCard) -> Value {
    let teams: Vec<Value> = card
        .teams
        .iter()
        .map(|t| {
            let mut map = Map::new();
            map.insert("id".into(), json!(t.id));
            map.insert("name".into(), json!(t.name));
            if let Some(title) = &t.title_in_team {
                map.insert("title_in_team".into(), json!(title));
            }
            Value::Object(map)
        })
        .collect();
    let mut map = Map::new();
    map.insert("id".into(), json!(card.id));
    map.insert("name".into(), json!(card.name));
    if let Some(handle) = &card.handle {
        map.insert("handle".into(), json!(handle));
    }
    map.insert("kind".into(), json!(card.kind));
    map.insert("owner".into(), json!(card.owner));
    if let Some(status) = card.agent_status {
        map.insert("agent_status".into(), json!(status));
    }
    if let Some(title) = &card.role_title {
        map.insert("role_title".into(), json!(title));
    }
    map.insert("presence".into(), json!(card.presence));
    map.insert("teams".into(), Value::Array(teams));
    Value::Object(map)
}

pub(super) fn project_value(project: &ProjectRecord) -> Value {
    json!({
        "id": project.id,
        "number": project.number,
        "key": project.key,
        "slug": project.slug,
        "name": project.name,
        "owner_team_id": project.owner_team_id,
        "contributing_team_ids": project.contributing_team_ids,
        "archived": project.archived,
        "revision": project.revision,
    })
}
