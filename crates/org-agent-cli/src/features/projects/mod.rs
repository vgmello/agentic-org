//! The projects slice: `project list|inspect|create` (B3), each with its row, guidance and
//! handler.

use org_command::{
    Ack, Answer, Command, CommandTable, Example, Field, IfMatch, Kind, Request, Type,
};
use org_proto::projects::{ProjectListResult, ProjectRecord};
use org_proto::ErrorCode;
use serde_json::{json, Map, Value};

use crate::runtime::domain::{CommandCue, Domain};
use crate::runtime::input::{
    decode_err, entity_id, opt_bool, opt_str, require_str, send_command, MUTATION_OUTPUT,
};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport, Refusal};

/// The projects slice as the registry composes it.
pub(crate) const DOMAIN: Domain = Domain {
    commands: &[COMMANDS],
    resources: &[(
        "project",
        "listing, inspecting, or creating organization projects and their owning teams.",
    )],
    cues: &[CUES],
    dispatch,
};

const STRING_TYPE: Type = Type::String;

const READ_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotFound,
    ErrorCode::NotAuthorized,
    ErrorCode::Unavailable,
];

const WRITE_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::InvalidName,
    ErrorCode::AlreadyExists,
    ErrorCode::NotFound,
    ErrorCode::NotEmpty,
    ErrorCode::Conflict,
    ErrorCode::OperationReused,
    ErrorCode::NotAuthorized,
    ErrorCode::Unavailable,
];

const PROJECT_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Project ID"),
    Field::required("number", Type::Integer, "Project sequence number"),
    Field::required("key", Type::String, "Project reference key (P-<number>)"),
    Field::required("slug", Type::String, "Canonical project slug"),
    Field::required("name", Type::String, "Project name"),
    Field::required("owner_team_id", Type::String, "Accountable owning team ID"),
    Field::required(
        "contributing_team_ids",
        Type::List(&STRING_TYPE),
        "Contributing team IDs",
    ),
    Field::required("archived", Type::Boolean, "Whether the project is archived"),
    Field::required("revision", Type::Integer, "Project record revision"),
];

const PROJECT_LIST_INPUT: &[Field] = &[Field::optional(
    "include_archived",
    Type::Boolean,
    "Include archived projects",
)];

const PROJECT_INSPECT_INPUT: &[Field] = &[Field::required(
    "project",
    Type::String,
    "Project UUIDv7, P-<number> key, or slug",
)];

const PROJECT_CREATE_INPUT: &[Field] = &[
    Field::required("name", Type::String, "Project name"),
    Field::optional("slug", Type::String, "Canonical lowercase project slug"),
    Field::optional(
        "owner_team_id",
        Type::String,
        "Existing accountable owning team UUIDv7",
    ),
    Field::optional(
        "create_team_id",
        Type::String,
        "Caller-supplied UUIDv7 for the default new owning team",
    ),
    Field::optional(
        "parent_team_id",
        Type::String,
        "Parent team UUIDv7 for a newly created owning team",
    ),
    Field::optional(
        "contributing_team_ids",
        Type::List(&STRING_TYPE),
        "Contributing team UUIDv7s",
    ),
    Field::optional("project_id", Type::String, "Caller-supplied project UUIDv7"),
];

/// The `project` rows.
const COMMANDS: &[Command] = &[
    Command {
        resource: "project",
        verb: "list",
        summary: "List org projects",
        call: "project.list",
        kind: Kind::List,
        input: PROJECT_LIST_INPUT,
        content_field: None,
        output: PROJECT_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000030","number":1,"key":"P-1","slug":"billing","name":"Billing","owner_team_id":"0193a5c0-0000-7000-8000-000000000010","contributing_team_ids":[],"archived":false,"revision":1}]"#,
        },
    },
    Command {
        resource: "project",
        verb: "inspect",
        summary: "Inspect project by ID, key, or slug",
        call: "project.inspect",
        kind: Kind::Read,
        input: PROJECT_INSPECT_INPUT,
        content_field: None,
        output: PROJECT_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"project":"P-1"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000030","number":1,"key":"P-1","slug":"billing","name":"Billing","owner_team_id":"0193a5c0-0000-7000-8000-000000000010","contributing_team_ids":[],"archived":false,"revision":1}"#,
        },
    },
    Command {
        resource: "project",
        verb: "create",
        summary: "Create a project and owning team",
        call: "project.create",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: PROJECT_CREATE_INPUT,
        content_field: None,
        output: MUTATION_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"name":"Billing"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000030","status":"committed","position":{"epoch":1,"seq":11}}"#,
        },
    },
];

/// `Use when:` guidance of each `project` command.
const CUES: &[CommandCue] = &[
    ("project", "create", "creating a new project with an existing or newly created accountable owning team."),
    ("project", "inspect", "fetching one project's owning team, contributing teams, key, slug, and archive status."),
    ("project", "list", "discovering active (or archived) projects and their `P-<n>` keys, slugs, and owning teams."),
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
