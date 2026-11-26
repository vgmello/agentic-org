//! Shared `org-command` dispatch over any [`OfficeTransport`] (bound agent socket or
//! grant-bound person host channel), implementing B2.4/B3 rows and CLI-4/5/6.
//!
//! The transport contract the `*_dispatch` slices share lives in the leaf
//! [`crate::transport`]; it is re-exported here so `dispatch::OfficeTransport` and friends
//! keep their paths. This module routes a request to a slice, and no slice imports it.

use org_command::{Ack, Answer, KeySource, Office, Request};
use org_proto::org::{CommandAccepted, CommandStatus};
use org_proto::ErrorCode;
use serde_json::{json, Map, Value};

use crate::keys::UuidV7KeySource;
pub use crate::transport::{derive_uuid_v7, hint_for, OfficeTransport, Refusal, TransportOffice};

mod read;
mod write;

use read::{
    fetch_snapshot, read_member_inspect, read_org_inspect, read_project_inspect, read_project_list,
    read_team_inspect, read_team_list,
};
use write::{
    write_person_add, write_person_deactivate, write_person_move, write_person_update,
    write_project_create, write_team_archive, write_team_create, write_team_move,
    write_team_update,
};

/// The command table's [`Office`] over a transport: each request is dispatched by [`execute`].
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
    if let Some(outcome) = crate::hiring_dispatch::try_execute(transport, request) {
        return outcome;
    }
    if let Some(outcome) = crate::session_dispatch::try_execute(transport, request) {
        return outcome;
    }
    if let Some(outcome) = crate::msg_dispatch::try_execute(transport, request) {
        return outcome;
    }
    if let Some(outcome) = crate::task_dispatch::try_execute(transport, request) {
        return outcome;
    }
    if let Some(outcome) = crate::git_dispatch::try_execute(transport, request) {
        return outcome;
    }
    if let Some(outcome) = crate::process_dispatch::try_execute(transport, request) {
        return outcome;
    }
    if let Some(outcome) = crate::budget_dispatch::try_execute(transport, request) {
        return outcome;
    }
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
