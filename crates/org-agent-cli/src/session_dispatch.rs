//! Dispatch for C3 `session inspect` (`CLI-9`), `grant list` (`CLI-10`), and `directory list` / `who` (`B2 Q9`).

use org_command::{Answer, Refusal, Request};
use org_proto::directory::{DirectoryCard, WhoResult};
use org_proto::org::{AgentStatus, MemberKind};
use org_proto::ErrorCode;
use serde_json::Value;

use crate::dispatch::OfficeTransport;

/// Attempts to handle a C3 session, grant, or directory command.
pub fn try_execute<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    match (request.command.resource, request.command.verb) {
        ("session", "inspect") => Some(read_session_inspect(transport, request)),
        ("grant", "list") => Some(read_grant_list(transport, request)),
        ("directory", "list") => Some(read_directory_list(transport, request)),
        _ => None,
    }
}

fn read_session_inspect<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let mut params = serde_json::Map::new();
    if let Some(emp) = opt_str(request, "employee_id") {
        params.insert("employee_id".into(), Value::String(emp.to_owned()));
    }
    let raw = transport.call_method("member.session_inspect", Value::Object(params))?;
    Ok(Answer::completed(raw))
}

fn read_grant_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let mut params = serde_json::Map::new();
    if let Some(emp) = opt_str(request, "employee_id") {
        params.insert("employee_id".into(), Value::String(emp.to_owned()));
    }
    let raw = transport.call_method("member.session_grants", Value::Object(params))?;
    Ok(Answer::completed(raw))
}

fn read_directory_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let mut params = serde_json::Map::new();
    if let Some(team) = opt_str(request, "team") {
        params.insert("team".into(), Value::String(team.to_owned()));
    }
    if let Some(member) = opt_str(request, "member") {
        params.insert("member".into(), Value::String(member.to_owned()));
    }
    let raw = transport.call_method("who", Value::Object(params))?;
    let who: WhoResult = serde_json::from_value(raw).map_err(|_| {
        Refusal::new(
            ErrorCode::Unavailable,
            "The office returned an unreadable directory response.",
            "Retry the command or check the office version.",
        )
    })?;
    let cards: Vec<Value> = who.members.iter().map(card_summary).collect();
    Ok(Answer::completed(Value::Array(cards)))
}

fn card_summary(card: &DirectoryCard) -> Value {
    let presence = serde_json::to_value(card.presence)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned());
    let mut map = serde_json::Map::new();
    map.insert("id".into(), Value::String(card.id.clone()));
    map.insert("name".into(), Value::String(card.name.clone()));
    map.insert(
        "kind".into(),
        Value::String(member_kind_str(card.kind).into()),
    );
    map.insert("owner".into(), Value::Bool(card.owner));
    map.insert("presence".into(), Value::String(presence));
    insert_opt(&mut map, "handle", card.handle.as_deref());
    insert_opt(
        &mut map,
        "agent_status",
        card.agent_status.map(agent_status_str),
    );
    insert_opt(&mut map, "role_title", card.role_title.as_deref());
    Value::Object(map)
}

fn member_kind_str(kind: MemberKind) -> &'static str {
    match kind {
        MemberKind::Person => "person",
        MemberKind::Agent => "agent",
        _ => "unknown",
    }
}

fn agent_status_str(status: AgentStatus) -> &'static str {
    match status {
        AgentStatus::Active => "active",
        AgentStatus::Paused => "paused",
        AgentStatus::Offboarded => "offboarded",
        _ => "unknown",
    }
}

fn insert_opt(map: &mut serde_json::Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(v) = value {
        map.insert(key.into(), Value::String(v.to_owned()));
    }
}

fn opt_str<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request.input.get(name).and_then(Value::as_str)
}
