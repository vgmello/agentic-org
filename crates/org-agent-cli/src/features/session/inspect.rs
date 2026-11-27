//! `session inspect` (`CLI-9`) and `grant list` (`CLI-10`): an agent session's state and usage,
//! and its effective permission presets, tool rules and sandbox boundaries.

use org_command::{Answer, Refusal, Request};
use org_proto::sessions::SessionListResult;
use serde_json::{Map, Value};

use crate::runtime::domain::CommandCue;
use crate::runtime::input::{decode_err, encode, opt_str};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport};

/// `Use when:` guidance of each `session` command here.
pub(super) const CUES: &[CommandCue] = &[
    ("session", "inspect", "checking your (or a named agent's) live session state, workspace paths, stall status, and token usage."),
    ("session", "list", "listing recorded and live conversation sessions for an agent."),
];

/// `Use when:` guidance of each `grant` command.
pub(super) const GRANT_CUES: &[CommandCue] = &[
    ("grant", "list", "verifying your (or a named agent's) effective permission preset, writable roots, and deny/allow rules."),
];

pub(super) fn read_session_inspect<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let mut params = Map::new();
    if let Some(emp) = opt_str(request, "employee_id") {
        params.insert("employee_id".into(), Value::String(emp.to_owned()));
    }
    let raw = transport.call_method("member.session_inspect", Value::Object(params))?;
    Ok(Answer::completed(raw))
}

pub(super) fn read_session_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    parse_cursor(request.cursor.as_deref())?;
    let mut params = Map::new();
    if let Some(emp) = opt_str(request, "employee_id") {
        params.insert("employee_id".into(), Value::String(emp.to_owned()));
    }
    let raw = transport.call_method("member.session_list", Value::Object(params))?;
    let res: SessionListResult =
        serde_json::from_value(raw).map_err(|_| decode_err("member.session_list"))?;
    let items = match encode(&res.sessions, "member.session_list")? {
        Value::Array(items) => items,
        _ => Vec::new(),
    };
    paginate(request, items)
}

pub(super) fn read_grant_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let mut params = Map::new();
    if let Some(emp) = opt_str(request, "employee_id") {
        params.insert("employee_id".into(), Value::String(emp.to_owned()));
    }
    let raw = transport.call_method("member.session_grants", Value::Object(params))?;
    Ok(Answer::completed(raw))
}
