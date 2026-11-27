//! `team list|inspect|create|update|move|archive|set-limit`.

use org_command::{Ack, Answer, Command, Example, Field, IfMatch, Kind, Request, Type};
use org_proto::org::{OrgSnapshot, Team};
use org_proto::ErrorCode;
use serde_json::{json, Map, Value};

use super::schema::{fetch_snapshot, team_map, team_value, READ_ERRORS, TEAM_FIELDS, WRITE_ERRORS};
use super::{send_write, ConflictTarget};
use crate::runtime::card::{card_value, who, DIRECTORY_CARD_TYPE};
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{
    entity_id, opt_bool, opt_str, require_if_match, require_str, send_entity_write,
    ENTITY_OP_OUTPUT, MUTATION_OUTPUT,
};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport, Refusal};

const TEAM_INSPECT_INPUT: &[Field] = &[Field::required(
    "team",
    Type::String,
    "Team ID or unique team name",
)];

const TEAM_INSPECT_OUTPUT: &[Field] = &[
    Field::required("id", Type::String, "Team ID"),
    Field::required("name", Type::String, "Team name"),
    Field::optional("owner_team_id", Type::String, "Parent team ID"),
    Field::optional("lead_member_id", Type::String, "Lead employee ID"),
    Field::required("version", Type::Integer, "Record revision"),
    Field::required(
        "members",
        Type::List(&DIRECTORY_CARD_TYPE),
        "Members of this team",
    ),
];

const TEAM_CREATE_INPUT: &[Field] = &[
    Field::required("name", Type::String, "New team name"),
    Field::optional("parent_team_id", Type::String, "Parent team UUIDv7"),
    Field::optional("team_id", Type::String, "Caller-supplied team UUIDv7"),
];

const TEAM_UPDATE_INPUT: &[Field] = &[
    Field::required("team", Type::String, "Team UUIDv7"),
    Field::optional("name", Type::String, "New team name"),
    Field::optional("lead_member_id", Type::String, "New lead employee UUIDv7"),
    Field::optional("clear_lead", Type::Boolean, "Clear the current team lead"),
];

const TEAM_MOVE_INPUT: &[Field] = &[
    Field::required("team", Type::String, "Team UUIDv7"),
    Field::required("parent_team_id", Type::String, "New parent team UUIDv7"),
];

const TEAM_ARCHIVE_INPUT: &[Field] = &[Field::required("team", Type::String, "Team UUIDv7")];

const TEAM_SET_MAX_MEMBERS_INPUT: &[Field] = &[
    Field::required("team", Type::String, "Team UUIDv7"),
    Field::required(
        "max_members",
        Type::Integer,
        "Maximum active team members (1..=100)",
    ),
];

/// The `team` rows.
pub(super) const COMMANDS: &[Command] = &[
    Command {
        resource: "team",
        verb: "list",
        summary: "List active teams",
        call: "org.snapshot",
        kind: Kind::List,
        input: &[],
        content_field: None,
        output: TEAM_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000001","name":"Core","version":1}]"#,
        },
    },
    Command {
        resource: "team",
        verb: "inspect",
        summary: "Inspect team and members",
        call: "org.snapshot",
        kind: Kind::Read,
        input: TEAM_INSPECT_INPUT,
        content_field: None,
        output: TEAM_INSPECT_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"team":"Core"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000001","name":"Core","version":1,"members":[{"id":"0193a5c0-0000-7000-8000-000000000002","name":"Owner","handle":"owner","kind":"person","owner":true,"presence":"offline","teams":[{"id":"0193a5c0-0000-7000-8000-000000000001","name":"Core"}]}]}"#,
        },
    },
    Command {
        resource: "team",
        verb: "create",
        summary: "Create a team",
        call: "team.create",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: TEAM_CREATE_INPUT,
        content_field: None,
        output: MUTATION_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"name":"Platform"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000010","status":"committed","position":{"epoch":1,"seq":3}}"#,
        },
    },
    Command {
        resource: "team",
        verb: "update",
        summary: "Rename team or change lead",
        call: "team.rename",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: TEAM_UPDATE_INPUT,
        content_field: None,
        output: MUTATION_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"team":"0193a5c0-0000-7000-8000-000000000010","name":"Core Platform"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000010","status":"committed","position":{"epoch":1,"seq":4}}"#,
        },
    },
    Command {
        resource: "team",
        verb: "move",
        summary: "Reparent a team",
        call: "team.move",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: TEAM_MOVE_INPUT,
        content_field: None,
        output: MUTATION_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"team":"0193a5c0-0000-7000-8000-000000000010","parent_team_id":"0193a5c0-0000-7000-8000-000000000001"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000010","status":"committed","position":{"epoch":1,"seq":5}}"#,
        },
    },
    Command {
        resource: "team",
        verb: "archive",
        summary: "Archive an empty team",
        call: "team.delete",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: TEAM_ARCHIVE_INPUT,
        content_field: None,
        output: MUTATION_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"team":"0193a5c0-0000-7000-8000-000000000010"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000010","status":"committed","position":{"epoch":1,"seq":6}}"#,
        },
    },
    Command {
        resource: "team",
        verb: "set-limit",
        summary: "Set team member cap (1..=100)",
        call: "team.set_max_members",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: TEAM_SET_MAX_MEMBERS_INPUT,
        content_field: None,
        output: ENTITY_OP_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"team":"0193a5c0-0000-7000-8000-000000000001","max_members":10}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000001","revision":2,"generation":12}"#,
        },
    },
];

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
