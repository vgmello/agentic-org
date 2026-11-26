//! `role list|inspect|create|update`: the reusable role definitions of the role catalog.

use org_command::{Ack, Answer, Command, Example, Field, IfMatch, Kind, Request, Type};
use org_proto::hiring::{RoleListResult, RoleRecord};
use serde_json::{json, Map};

use super::schema::{READ_ERRORS, WRITE_ERRORS};
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{
    copy_opt_bool, copy_opt_str, decode_err, encode, entity_id, opt_str, require_if_match,
    require_str, send_entity_write, ENTITY_OP_OUTPUT,
};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport, Refusal};

const ROLE_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Role UUIDv7"),
    Field::optional("key", Type::String, "Optional lowercase role key"),
    Field::required("name", Type::String, "Role name"),
    Field::required("body", Type::String, "Role description markdown"),
    Field::required("revision", Type::Integer, "Record revision"),
    Field::optional("created_at", Type::String, "Creation timestamp"),
    Field::optional("updated_at", Type::String, "Last update timestamp"),
];

const ROLE_INSPECT_INPUT: &[Field] = &[Field::required(
    "role",
    Type::String,
    "Role UUIDv7, key, or unique name",
)];

const ROLE_CREATE_INPUT: &[Field] = &[
    Field::required("name", Type::String, "Role display name"),
    Field::optional("key", Type::String, "Optional lowercase slug key"),
    Field::optional("body", Type::String, "Role description markdown"),
    Field::optional("role_id", Type::String, "Caller-supplied role UUIDv7"),
];

const ROLE_UPDATE_INPUT: &[Field] = &[
    Field::required("role", Type::String, "Role UUIDv7"),
    Field::optional("name", Type::String, "Updated role name"),
    Field::optional("key", Type::String, "Updated lowercase role key"),
    Field::optional("clear_key", Type::Boolean, "Clear the role key"),
    Field::optional("body", Type::String, "Updated role description markdown"),
];

/// The `role` rows.
pub(super) const COMMANDS: &[Command] = &[
    Command {
        resource: "role",
        verb: "list",
        summary: "List catalog roles",
        call: "role.list",
        kind: Kind::List,
        input: &[],
        content_field: None,
        output: ROLE_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000040","key":"backend-engineer","name":"Backend Engineer","body":"Builds backend services.","revision":1}]"#,
        },
    },
    Command {
        resource: "role",
        verb: "inspect",
        summary: "Inspect a catalog role",
        call: "role.inspect",
        kind: Kind::Read,
        input: ROLE_INSPECT_INPUT,
        content_field: None,
        output: ROLE_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"role":"backend-engineer"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000040","key":"backend-engineer","name":"Backend Engineer","body":"Builds backend services.","revision":1}"#,
        },
    },
    Command {
        resource: "role",
        verb: "create",
        summary: "Create a catalog role",
        call: "role.create",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: ROLE_CREATE_INPUT,
        content_field: Some("body"),
        output: ENTITY_OP_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"name":"Backend Engineer","key":"backend-engineer","body":"Builds backend services."}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000040","revision":1,"generation":3}"#,
        },
    },
    Command {
        resource: "role",
        verb: "update",
        summary: "Update a catalog role",
        call: "role.update",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: ROLE_UPDATE_INPUT,
        content_field: Some("body"),
        output: ENTITY_OP_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"role":"0193a5c0-0000-7000-8000-000000000040","name":"Senior Backend Engineer"}"#,
            output: r#"{"id":"0193a5c0-0000-7000-8000-000000000040","revision":2,"generation":4}"#,
        },
    },
];

/// `Use when:` guidance of each `role` command.
pub(super) const CUES: &[CommandCue] = &[
    (
        "role",
        "create",
        "defining a new reusable role template in the organization role catalog.",
    ),
    (
        "role",
        "inspect",
        "reading one catalog role's full Markdown instructions and metadata by ID, key, or name.",
    ),
    (
        "role",
        "list",
        "browsing available catalog roles before drafting an agent hire request.",
    ),
    (
        "role",
        "update",
        "updating a catalog role's name, key, or Markdown description (requires `--if-match`).",
    ),
];

/// Runs a `role` command.
pub(super) fn dispatch<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    Some(match request.command.verb {
        "list" => read_role_list(transport, request),
        "inspect" => read_role_inspect(transport, request),
        "create" => write_role_create(transport, request),
        "update" => write_role_update(transport, request),
        _ => return None,
    })
}

fn read_role_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    parse_cursor(request.cursor.as_deref())?;
    let raw = transport.call_method("role.list", json!({}))?;
    let listed: RoleListResult =
        serde_json::from_value(raw).map_err(|_| decode_err("role.list"))?;
    let items = serde_json::to_value(&listed.roles)
        .ok()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();
    paginate(request, items)
}

fn read_role_inspect<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let role = require_str(request, "role")?;
    let raw = transport.call_method("role.inspect", json!({ "role": role }))?;
    let record: RoleRecord = serde_json::from_value(raw).map_err(|_| decode_err("role.inspect"))?;
    Ok(Answer::completed(encode(&record, "role.inspect")?))
}

fn write_role_create<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let name = require_str(request, "name")?;
    let role_id = entity_id(request, Some("role_id"), "role.create")?;
    let mut params = Map::new();
    params.insert("role_id".into(), json!(role_id));
    params.insert("name".into(), json!(name));
    if let Some(key) = opt_str(request, "key") {
        params.insert("key".into(), json!(key));
    }
    if let Some(body) = opt_str(request, "body") {
        params.insert("body".into(), json!(body));
    }
    send_entity_write(transport, request, "role.create", params)
}

fn write_role_update<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let role_id = require_str(request, "role")?;
    let expected = require_if_match(request)?;
    let mut params = Map::new();
    params.insert("role_id".into(), json!(role_id));
    params.insert("expected_version".into(), json!(expected));
    copy_opt_str(request, &mut params, "name", "name");
    copy_opt_str(request, &mut params, "key", "key");
    copy_opt_bool(request, &mut params, "clear_key", "clear_key");
    copy_opt_str(request, &mut params, "body", "body");
    send_entity_write(transport, request, "role.update", params)
}
