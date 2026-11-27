//! The directory card: the one view of an employee (`who`) that several slices print —
//! `person inspect` and `team inspect` (organization), `agent inspect` (hiring) and
//! `directory list` / `who` (organization). Its rendering lives here once, so the slices share
//! the view without naming each other; its declarative schema lives in
//! `org_command_catalogue::shared`.

use org_command::{Answer, Refusal, Request};
use org_proto::directory::{DirectoryCard, WhoResult};
use org_proto::org::MemberKind;
use org_proto::ErrorCode;
use serde_json::{json, Map, Value};

use crate::runtime::input::{decode_err, require_str};
use crate::runtime::transport::OfficeTransport;

/// The card as a command prints it; each row's schema keeps the fields it declares.
pub(crate) fn card_value(card: &DirectoryCard) -> Value {
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

/// Every card `who` answers for `params`.
pub(crate) fn who<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    params: Value,
) -> Result<Vec<DirectoryCard>, Refusal> {
    let raw = transport.call_method("who", params)?;
    let who: WhoResult = serde_json::from_value(raw).map_err(|_| decode_err("who"))?;
    Ok(who.members)
}

/// Answers `person inspect` or `agent inspect` (`resource`): the card of `--member`, refused when
/// it names no active employee of that kind.
pub(crate) fn inspect_member<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    resource: &str,
) -> Result<Answer, Refusal> {
    let member = require_str(request, "member")?;
    let cards = who(transport, json!({ "member": member }))?;
    let expected = if resource == "person" {
        MemberKind::Person
    } else {
        MemberKind::Agent
    };
    let Some(card) = cards.iter().find(|c| c.kind == expected) else {
        return Err(Refusal::new(
            ErrorCode::NotFound,
            format!("No active {resource} matches that reference."),
            "Run `org inspect` to see active members.",
        ));
    };
    Ok(Answer::completed(card_value(card)))
}
