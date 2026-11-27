//! The projects slice: `project list|inspect|create` (B3), each with its row, guidance and
//! handler.

use org_command::{Answer, CommandTable, Request};
use org_proto::projects::{ProjectListResult, ProjectRecord};
use serde_json::{json, Map, Value};

use crate::runtime::domain::{CommandCue, Domain};
use crate::runtime::input::{
    decode_err, entity_id, opt_bool, opt_str, require_if_match, require_str, send_command,
};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport, Refusal};

/// The projects slice as the registry composes it.
pub(crate) const DOMAIN: Domain = Domain {
    commands: org_command_catalogue::projects::ALL,
    resources: &[(
        "project",
        "listing, inspecting, or creating organization projects and their owning teams.",
    )],
    cues: &[CUES],
    dispatch,
};

/// `Use when:` guidance of each `project` command.
const CUES: &[CommandCue] = &[
    ("project", "add-team", "adding a contributing team to an existing project."),
    ("project", "create", "creating a new project with an existing or newly created accountable owning team."),
    ("project", "inspect", "fetching one project's owning team, contributing teams, key, slug, and archive status."),
    ("project", "list", "discovering active (or archived) projects and their `P-<n>` keys, slugs, and owning teams."),
    ("project", "remove-team", "removing a contributing team link from a project (requires `--if-match`)."),
];

fn dispatch(
    transport: &mut dyn OfficeTransport,
    request: &Request,
    _table: &CommandTable,
) -> Option<Result<Answer, Refusal>> {
    if request.command.resource != "project" {
        return None;
    }
    Some(match request.command.verb {
        "list" => read_project_list(transport, request),
        "inspect" => read_project_inspect(transport, request),
        "create" => write_project_create(transport, request),
        "add-team" => write_project_add_team(transport, request),
        "remove-team" => write_project_remove_team(transport, request),
        _ => return None,
    })
}

fn read_project_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    // A bad cursor is refused before the office is asked.
    parse_cursor(request.cursor.as_deref())?;
    let include_archived = opt_bool(request, "include_archived").unwrap_or(false);
    let raw = transport.call_method(
        "project.list",
        json!({ "include_archived": include_archived }),
    )?;
    let listed: ProjectListResult =
        serde_json::from_value(raw).map_err(|_| decode_err("project.list"))?;
    paginate(request, listed.projects.iter().map(project_value).collect())
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

fn write_project_create<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let name = require_str(request, "name")?;
    let project_id = entity_id(request, Some("project_id"), "project.create.id")?;
    let mut params = Map::new();
    params.insert("project_id".into(), json!(project_id));
    params.insert("name".into(), json!(name));
    if let Some(slug) = opt_str(request, "slug") {
        params.insert("slug".into(), json!(slug));
    }
    if let Some(owner_team) = opt_str(request, "owner_team_id") {
        params.insert("owner_team_id".into(), json!(owner_team));
    } else {
        let create_team = entity_id(request, Some("create_team_id"), "project.create.owner_team")?;
        params.insert("create_team_id".into(), json!(create_team));
    }
    if let Some(parent_team) = opt_str(request, "parent_team_id") {
        params.insert("parent_team_id".into(), json!(parent_team));
    }
    if let Some(contrib) = request.input.get("contributing_team_ids") {
        params.insert("contributing_team_ids".into(), contrib.clone());
    }
    send_command(transport, request, "project.create", params, &project_id)
}

fn write_project_add_team<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let project_id = require_str(request, "project_id")?;
    let team_id = require_str(request, "team_id")?;
    let project_team_id = entity_id(request, Some("project_team_id"), "project.add_team.id")?;
    let mut params = Map::new();
    params.insert("project_team_id".into(), json!(project_team_id));
    params.insert("project_id".into(), json!(project_id));
    params.insert("team_id".into(), json!(team_id));
    send_command(
        transport,
        request,
        "project.add_team",
        params,
        &project_team_id,
    )
}

fn write_project_remove_team<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let project_team_id = require_str(request, "project_team_id")?;
    let expected = require_if_match(request)?;
    let mut params = Map::new();
    params.insert("project_team_id".into(), json!(project_team_id));
    params.insert("expected_version".into(), json!(expected));
    if let Some(areas_to) = opt_str(request, "areas_to_team_id") {
        params.insert("areas_to_team_id".into(), json!(areas_to));
    }
    send_command(
        transport,
        request,
        "project.remove_team",
        params,
        project_team_id,
    )
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

#[cfg(test)]
mod tests;
