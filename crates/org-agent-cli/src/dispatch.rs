//! Shared `org-command` dispatch over any [`OfficeTransport`] (bound agent socket or
//! grant-bound person host channel), implementing B2.4/B3 rows and CLI-4/5/6.

pub use org_command::Refusal;
use org_command::{Ack, Answer, KeySource, Office, Request};
use org_proto::directory::{DirectoryCard, WhoResult};
use org_proto::org::{CommandAccepted, CommandStatus, MemberKind, OrgSnapshot, Team};
use org_proto::projects::{ProjectListResult, ProjectRecord};
use org_proto::ErrorCode;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::keys::UuidV7KeySource;

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

/// Transport abstraction shared by `agent-os` (`org-client`) and `agentic-org` (`host.sock`).
pub trait OfficeTransport {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal>;

    /// Like [`Self::call_method`], with an explicit timeout budget for short-hold waits.
    fn call_method_with_timeout(
        &mut self,
        method: &str,
        params: Value,
        _timeout: std::time::Duration,
    ) -> Result<Value, Refusal> {
        self.call_method(method, params)
    }
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
mod tests;
