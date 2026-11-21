//! Shared `org-command` dispatch over any [`OfficeTransport`] (bound agent socket or
//! grant-bound person host channel), implementing B2.4/B3 rows and CLI-4/5/6.

use org_command::{Ack, Answer, KeySource, Office, Refusal, Request};
use org_proto::directory::{DirectoryCard, WhoResult};
use org_proto::org::{CommandAccepted, CommandStatus, MemberKind, OrgSnapshot, Team};
use org_proto::projects::{ProjectListResult, ProjectRecord};
use org_proto::ErrorCode;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::keys::UuidV7KeySource;

/// Transport abstraction shared by `agent-os` (`org-client`) and `agentic-org` (`host.sock`).
pub trait OfficeTransport {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal>;
}

/// Adapts any [`OfficeTransport`] into an [`org_command::Office`].
pub struct TransportOffice<T>(pub T);

impl<T: OfficeTransport> Office for TransportOffice<T> {
    fn call(&mut self, request: &Request) -> Result<Answer, Refusal> {
        execute(&mut self.0, request)
    }
}

/// Dispatches one parsed `org-command` [`Request`] over `transport`.
pub fn execute<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    match (request.command.resource, request.command.verb) {
        ("org", "inspect") => read_org_inspect(transport),
        ("team", "list") => read_team_list(transport, request),
        ("team", "inspect") => read_team_inspect(transport, request),
        ("team", "create") => write_team_create(transport, request),
        ("team", "update") => write_team_update(transport, request),
        ("team", "move") => write_team_move(transport, request),
        ("team", "archive") => write_team_archive(transport, request),
        (r @ ("person" | "agent"), "inspect") => read_member_inspect(transport, request, r),
        ("person", "add") => write_person_add(transport, request),
        ("person", "update") => write_person_update(transport, request),
        ("person", "move") => write_person_move(transport, request),
        ("person", "deactivate") => write_person_deactivate(transport, request),
        ("project", "list") => read_project_list(transport, request),
        ("project", "inspect") => read_project_inspect(transport, request),
        ("project", "create") => write_project_create(transport, request),
        (resource, verb) => Err(Refusal::new(
            ErrorCode::Unsupported,
            format!("`{resource} {verb}` is not wired to the office yet."),
            "Run `--help` to see available commands.",
        )),
    }
}

fn fetch_snapshot<T: OfficeTransport + ?Sized>(transport: &mut T) -> Result<OrgSnapshot, Refusal> {
    let raw = transport.call_method("org.snapshot", json!({}))?;
    serde_json::from_value(raw).map_err(|_| {
        Refusal::new(
            ErrorCode::Unavailable,
            "The office returned an unreadable organization snapshot.",
            "Retry the command or check the office version.",
        )
    })
}

fn read_org_inspect<T: OfficeTransport + ?Sized>(transport: &mut T) -> Result<Answer, Refusal> {
    let snapshot = fetch_snapshot(transport)?;
    Ok(Answer::completed(snapshot_value(&snapshot)))
}

fn read_team_list<T: OfficeTransport + ?Sized>(
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

fn read_team_inspect<T: OfficeTransport + ?Sized>(
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

fn read_member_inspect<T: OfficeTransport + ?Sized>(
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

fn read_project_list<T: OfficeTransport + ?Sized>(
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

fn read_project_inspect<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let project = require_str(request, "project")?;
    let raw = transport.call_method("project.inspect", json!({ "project": project }))?;
    let record: ProjectRecord =
        serde_json::from_value(raw).map_err(|_| decode_err("project.inspect"))?;
    Ok(Answer::completed(project_value(&record)))
}

fn write_team_create<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let name = require_str(request, "name")?;
    let team_id = entity_id(request, "team_id", "team.create")?;
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

fn write_person_add<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let name = require_str(request, "name")?;
    let member_id = entity_id(request, "member_id", "member.add_person")?;
    let mut params = Map::new();
    params.insert("member_id".into(), json!(member_id));
    params.insert("name".into(), json!(name));
    if let Some(handle) = opt_str(request, "handle") {
        params.insert("handle".into(), json!(handle));
    }
    if let Some(home) = opt_str(request, "home_team_id") {
        params.insert("home_team_id".into(), json!(home));
    }
    if let Some(contact) = opt_str(request, "contact") {
        params.insert("contact".into(), json!(contact));
    }
    send_write(
        transport,
        request,
        "member.add_person",
        params,
        &member_id,
        None,
    )
}

fn person_update_params(
    request: &Request,
    member_id: &str,
    expected: u64,
) -> Result<(&'static str, Map<String, Value>), Refusal> {
    let name = opt_str(request, "name");
    let contact = opt_str(request, "contact");
    let clear_contact = opt_bool(request, "clear_contact").unwrap_or(false);
    let reports_to = opt_str(request, "reports_to_member_id");
    let clear_reports_to = opt_bool(request, "clear_reports_to").unwrap_or(false);
    let title_in_team = opt_str(request, "title_in_team");

    let wants_profile = name.is_some() || contact.is_some() || clear_contact;
    let wants_manager = reports_to.is_some() || clear_reports_to;
    let wants_title = title_in_team.is_some();
    match usize::from(wants_profile) + usize::from(wants_manager) + usize::from(wants_title) {
        0 => {
            return Err(Refusal::new(
                ErrorCode::InvalidRequest,
                "Nothing to update on this person.",
                "Pass --name/--contact/--clear-contact, --reports-to-member-id/--clear-reports-to, or --title-in-team.",
            ))
        }
        1 => {}
        _ => {
            return Err(Refusal::new(
                ErrorCode::InvalidRequest,
                "Profile, reporting manager, and team title updates must be run separately.",
                "Update one group of fields per `person update` call.",
            ))
        }
    }

    let mut params = Map::new();
    params.insert("expected_version".into(), json!(expected));
    if wants_profile {
        params.insert("member_id".into(), json!(member_id));
        if let Some(n) = name {
            params.insert("name".into(), json!(n));
        }
        if let Some(c) = contact {
            params.insert("contact".into(), json!(c));
        }
        if clear_contact {
            params.insert("clear_contact".into(), json!(true));
        }
        return Ok(("member.update_person", params));
    }
    if wants_manager {
        if reports_to.is_some() && clear_reports_to {
            return Err(Refusal::new(
                ErrorCode::InvalidRequest,
                "--reports-to-member-id and --clear-reports-to cannot both be set.",
                "Pass either --reports-to-member-id <id> or --clear-reports-to.",
            ));
        }
        params.insert("member_id".into(), json!(member_id));
        if let Some(mgr) = reports_to {
            params.insert("reports_to_member_id".into(), json!(mgr));
        }
        return Ok(("member.set_reports_to", params));
    }
    params.insert("membership_id".into(), json!(member_id));
    if let Some(title) = title_in_team {
        params.insert("title_in_team".into(), json!(title));
    }
    Ok(("team.set_title", params))
}

fn write_person_update<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let member_id = require_str(request, "member")?;
    let expected = require_if_match(request)?;
    let (method, params) = person_update_params(request, member_id, expected)?;
    send_write(
        transport,
        request,
        method,
        params,
        member_id,
        Some(ConflictTarget::Member(member_id)),
    )
}

fn write_person_move<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let member_id = require_str(request, "member")?;
    let to_team_id = require_str(request, "to_team_id")?;
    let expected = require_if_match(request)?;
    let new_membership_id = derived_or_fresh_id(request, "member.move.new_membership");
    let mut params = Map::new();
    params.insert("membership_id".into(), json!(member_id));
    params.insert("expected_version".into(), json!(expected));
    params.insert("new_membership_id".into(), json!(new_membership_id));
    params.insert("to_team_id".into(), json!(to_team_id));
    send_write(
        transport,
        request,
        "member.move",
        params,
        member_id,
        Some(ConflictTarget::Member(member_id)),
    )
}

fn write_person_deactivate<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let member_id = require_str(request, "member")?;
    let expected = require_if_match(request)?;
    let mut params = Map::new();
    params.insert("member_id".into(), json!(member_id));
    params.insert("expected_version".into(), json!(expected));
    send_write(
        transport,
        request,
        "member.remove",
        params,
        member_id,
        Some(ConflictTarget::Member(member_id)),
    )
}

fn write_project_create<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let name = require_str(request, "name")?;
    let project_id = entity_id(request, "project_id", "project.create.id")?;
    let mut params = Map::new();
    params.insert("project_id".into(), json!(project_id));
    params.insert("name".into(), json!(name));
    if let Some(slug) = opt_str(request, "slug") {
        params.insert("slug".into(), json!(slug));
    }
    if let Some(owner_team) = opt_str(request, "owner_team_id") {
        params.insert("owner_team_id".into(), json!(owner_team));
    } else {
        let create_team = entity_id(request, "create_team_id", "project.create.owner_team")?;
        params.insert("create_team_id".into(), json!(create_team));
    }
    if let Some(parent_team) = opt_str(request, "parent_team_id") {
        params.insert("parent_team_id".into(), json!(parent_team));
    }
    if let Some(contrib) = request.input.get("contributing_team_ids") {
        params.insert("contributing_team_ids".into(), contrib.clone());
    }
    send_write(
        transport,
        request,
        "project.create",
        params,
        &project_id,
        None,
    )
}

enum ConflictTarget<'a> {
    Team(&'a str),
    Member(&'a str),
}

fn send_write<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    method: &str,
    mut params: Map<String, Value>,
    target_id: &str,
    conflict_target: Option<ConflictTarget<'_>>,
) -> Result<Answer, Refusal> {
    if request.dry_run {
        params.insert("dry_run".into(), json!(true));
    } else {
        let key = request.idempotency_key.as_deref().ok_or_else(|| {
            Refusal::new(
                ErrorCode::InvalidRequest,
                "Write is missing an idempotency key.",
                "Pass --idempotency-key <uuidv7> or let the CLI mint one.",
            )
        })?;
        params.insert("command_id".into(), json!(key));
    }
    let raw = match transport.call_method(method, Value::Object(params)) {
        Ok(value) => value,
        Err(mut refusal) => {
            if refusal.code == ErrorCode::Conflict {
                if let Some(target) = conflict_target {
                    attach_conflict_retry_with(transport, target, &mut refusal);
                }
            }
            return Err(refusal);
        }
    };
    let accepted: CommandAccepted = serde_json::from_value(raw).map_err(|_| decode_err(method))?;
    let status_str = match accepted.status {
        CommandStatus::Committed => "committed",
        CommandStatus::Checked => "checked",
        CommandStatus::Queued => "queued",
        _ => return Err(decode_err(method)),
    };
    let mut data = Map::new();
    data.insert("id".into(), json!(target_id));
    data.insert("status".into(), json!(status_str));
    if let Some(pos) = &accepted.position {
        data.insert(
            "position".into(),
            json!({ "epoch": pos.epoch, "seq": pos.seq }),
        );
    }
    let mut answer = Answer::completed(Value::Object(data));
    if !request.dry_run {
        answer.operation_id = request.idempotency_key.clone();
        answer.ack = Some(if request.durable {
            Ack::Durable
        } else {
            Ack::Committed
        });
    }
    Ok(answer)
}

fn attach_conflict_retry_with<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    target: ConflictTarget<'_>,
    refusal: &mut Refusal,
) {
    let Ok(snapshot) = fetch_snapshot(transport) else {
        return;
    };
    let current = match target {
        ConflictTarget::Team(id) => snapshot
            .teams
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.version),
        ConflictTarget::Member(id) => snapshot
            .members
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.version),
    };
    if let Some(version) = current {
        let mut map = Map::new();
        map.insert("if-match".into(), json!(version));
        refusal.retry_with = Some(map);
    }
}

/// Derives a canonical lowercase UUIDv7 deterministically from `(domain, key)` so retrying a
/// create command with the same `--idempotency-key` reproduces the exact same entity UUIDv7.
pub fn derive_uuid_v7(domain: &str, key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"agentic-org/cli-entity-id/v1\0");
    hasher.update(domain.as_bytes());
    hasher.update(b"\0");
    hasher.update(key.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
    )
}

fn entity_id(request: &Request, field: &str, domain: &str) -> Result<String, Refusal> {
    if let Some(explicit) = opt_str(request, field) {
        return Ok(explicit.to_owned());
    }
    Ok(derived_or_fresh_id(request, domain))
}

fn derived_or_fresh_id(request: &Request, domain: &str) -> String {
    if let Some(key) = &request.idempotency_key {
        derive_uuid_v7(domain, key)
    } else {
        let mut keys = UuidV7KeySource;
        keys.new_key()
    }
}

fn require_str<'a>(request: &'a Request, field: &str) -> Result<&'a str, Refusal> {
    request
        .input
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Refusal::new(
                ErrorCode::InvalidRequest,
                format!("Missing required --{} flag.", field.replace('_', "-")),
                format!("Pass --{} <value>.", field.replace('_', "-")),
            )
        })
}

fn opt_str<'a>(request: &'a Request, field: &str) -> Option<&'a str> {
    request.input.get(field).and_then(Value::as_str)
}

fn opt_bool(request: &Request, field: &str) -> Option<bool> {
    request.input.get(field).and_then(Value::as_bool)
}

fn require_if_match(request: &Request) -> Result<u64, Refusal> {
    request.if_match.ok_or_else(|| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            "This update requires --if-match <revision>.",
            "Inspect the current record and pass its revision with --if-match.",
        )
    })
}

fn decode_err(method: &str) -> Refusal {
    Refusal::new(
        ErrorCode::Unavailable,
        format!("The office returned an unreadable `{method}` reply."),
        "Retry the command or check the office version.",
    )
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

fn team_value(team: &Team) -> Value {
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

fn card_value(card: &DirectoryCard) -> Value {
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

fn project_value(project: &ProjectRecord) -> Value {
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

pub(crate) fn parse_cursor(cursor: Option<&str>) -> Result<usize, Refusal> {
    let Some(raw) = cursor else {
        return Ok(0);
    };
    raw.parse::<usize>().map_err(|_| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            "Invalid pagination cursor.",
            "Pass the next_cursor returned by the previous list call.",
        )
    })
}

pub fn hint_for(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::NotAuthorized | ErrorCode::NotBound => {
            "Check your session binding and permissions for this action."
        }
        ErrorCode::NotFound => "Check the ID or name and retry.",
        ErrorCode::Conflict => "Read the current record and retry with the latest --if-match.",
        ErrorCode::OperationReused => {
            "This idempotency key was already used with different input; use a new key."
        }
        ErrorCode::Unsupported => "This office build does not serve that command yet.",
        ErrorCode::Unavailable | ErrorCode::NotReady => "Wait briefly and retry the command.",
        _ => "Check the flags with --help and retry.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_entity_ids_are_valid_uuid_v7_deterministic_and_domain_separated() {
        let key = "0193a5c0-0000-7000-8000-000000000001";
        let a = derive_uuid_v7("team.create", key);
        let b = derive_uuid_v7("team.create", key);
        let c = derive_uuid_v7("project.create.id", key);
        assert!(org_proto::is_uuid_v7(&a), "{a}");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, key);
    }

    #[test]
    fn parse_cursor_accepts_non_negative_offsets_and_rejects_garbage() {
        assert_eq!(parse_cursor(None).unwrap(), 0);
        assert_eq!(parse_cursor(Some("0")).unwrap(), 0);
        assert_eq!(parse_cursor(Some("25")).unwrap(), 25);
        assert_eq!(
            parse_cursor(Some("not-a-number")).unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }
}
