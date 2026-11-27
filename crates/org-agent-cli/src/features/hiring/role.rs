//! `role list|inspect|create|update`: the reusable role definitions of the role catalog.

use org_command::{Answer, Request};
use org_proto::hiring::{RoleListResult, RoleRecord};
use serde_json::{json, Map};

use crate::runtime::domain::CommandCue;
use crate::runtime::input::{
    copy_opt_bool, copy_opt_str, decode_err, encode, entity_id, opt_str, require_if_match,
    require_str, send_entity_write,
};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport, Refusal};

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
