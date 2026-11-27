//! The transport contract every feature slice shares: [`OfficeTransport`] (a bound agent socket
//! or a grant-bound person host channel), the [`TransportOffice`] adapter, the deterministic
//! entity IDs of a retried create ([`derive_uuid_v7`], [`command_id`], [`entity_id`]), the
//! acknowledgement a write reports ([`write_ack`], [`refuse_durable`]), list pages
//! ([`parse_cursor`], [`paginate`]) and the hint of a refusal ([`hint_for`]).
//!
//! Part of the leaf `runtime`: it names nothing outside `runtime`, so every feature slice and the
//! registry (which implements [`org_command::Office`] for [`TransportOffice`]) depend on it and
//! never on each other.

pub use org_command::Refusal;
use org_command::{Ack, Answer, Request};
use org_proto::ErrorCode;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::runtime::keys::try_new_uuid_v7;

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

    /// Whether this transport speaks for a caller the office bound, so a command that can run
    /// without the office (`session boot`) must ask it and fail closed when it cannot. A
    /// grant-bound person channel always is; `agent-os` is when its environment names a session
    /// token or a binding file, whether or not that binding then works.
    fn is_bound(&self) -> bool {
        true
    }

    /// The bound caller's employee ID, when the transport knows it without asking the office.
    fn caller_member_id(&self) -> Option<String> {
        None
    }

    /// The `waiting_messages` hint the office gave with the most recent call, taken so it is
    /// reported once: by the command that made the call, never by the next one. `None` for a
    /// transport whose office gives no such hint.
    fn take_waiting_messages(&mut self) -> Option<u32> {
        None
    }
}

/// A borrowed transport is the transport it borrows: `TransportOffice(&mut office)` runs the
/// command table over an office the caller keeps.
impl<T: OfficeTransport + ?Sized> OfficeTransport for &mut T {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        (**self).call_method(method, params)
    }

    fn call_method_with_timeout(
        &mut self,
        method: &str,
        params: Value,
        timeout: std::time::Duration,
    ) -> Result<Value, Refusal> {
        (**self).call_method_with_timeout(method, params, timeout)
    }

    fn is_bound(&self) -> bool {
        (**self).is_bound()
    }

    fn caller_member_id(&self) -> Option<String> {
        (**self).caller_member_id()
    }

    fn take_waiting_messages(&mut self) -> Option<u32> {
        (**self).take_waiting_messages()
    }
}

/// Adapts any [`OfficeTransport`] into an [`org_command::Office`] (implemented by the crate's
/// registry, which routes each request to its feature slice).
pub struct TransportOffice<T>(pub T);

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

/// The office's ID of the change `request` makes: the caller's `--idempotency-key` when it is a
/// UUIDv7, the UUIDv7 [`derive_uuid_v7`] makes of `(domain, key)` for any other key, and a fresh
/// UUIDv7 only without a key (a dry run). Repeating a write with the same key always repeats
/// the same ID, so the office answers the retry from its receipt instead of applying it twice.
pub fn command_id(request: &Request, domain: &str) -> Result<String, Refusal> {
    match request.idempotency_key.as_deref() {
        Some(key) if org_proto::is_uuid_v7(key) => Ok(key.to_owned()),
        Some(key) => Ok(derive_uuid_v7(domain, key)),
        None => fresh_id(),
    }
}

/// The ID of the record a create makes: `explicit` when the caller gave one, else the UUIDv7
/// derived from `(domain, key)`, so a retried create makes the same record; a fresh one only
/// without a key.
pub fn entity_id(
    request: &Request,
    explicit: Option<&str>,
    domain: &str,
) -> Result<String, Refusal> {
    if let Some(id) = explicit {
        return Ok(id.to_owned());
    }
    match request.idempotency_key.as_deref() {
        Some(key) => Ok(derive_uuid_v7(domain, key)),
        None => fresh_id(),
    }
}

fn fresh_id() -> Result<String, Refusal> {
    try_new_uuid_v7().ok_or_else(|| {
        Refusal::new(
            ErrorCode::Unavailable,
            "Could not make a UUIDv7 for this change.",
            "Retry the command, or give a UUIDv7 with --idempotency-key.",
        )
    })
}

/// The acknowledgement a completed write reports: the one its row declares (`--schema` shows
/// it), never more. An always-durable command reports `durable` whether or not `--durable` was
/// given; a command acknowledged at commit reports `committed`, and [`refuse_durable`] keeps
/// `--durable` from reaching it.
pub fn write_ack(request: &Request) -> Ack {
    request.command.ack().unwrap_or(Ack::Committed)
}

/// Refuses `--durable` on a write its row acknowledges at commit (messages, channels, status
/// lines, private notes, and the Git operations the office does not journal): the office
/// cannot wait for the drive for them, and a write must never run as if it had.
pub fn refuse_durable(request: &Request) -> Result<(), Refusal> {
    if request.durable && request.command.ack() == Some(Ack::Committed) {
        let name = request.command.name();
        return Err(Refusal::new(
            ErrorCode::Unsupported,
            format!(
                "`{name}` is acknowledged when the office commits it and cannot wait until the drive has it, so --durable is refused. Nothing was sent."
            ),
            format!("Run `{name}` again without --durable; `--schema` shows each command's `ack`."),
        ));
    }
    Ok(())
}

/// One page of `items`, which are every record in order: `--limit` records (20 by default, at
/// most 200) from the `--cursor` offset, `meta.next_cursor` when more follow, and `meta.total`.
pub fn paginate(request: &Request, items: Vec<Value>) -> Result<Answer, Refusal> {
    let offset = parse_cursor(request.cursor.as_deref())?;
    let total = items.len();
    let limit = request.limit.unwrap_or(org_command::DEFAULT_LIMIT) as usize;
    let page: Vec<Value> = items.into_iter().skip(offset).take(limit).collect();
    let next = offset.saturating_add(page.len());
    let mut answer = Answer::completed(Value::Array(page));
    if next < total {
        answer.next_cursor = Some(next.to_string());
    }
    answer.total = Some(total as u64);
    Ok(answer)
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
        ErrorCode::RateLimited => {
            "Wait for the cooldown in error.retry_after_ms (or the message) to pass, then retry the command."
        }
        ErrorCode::Timeout => {
            "The change may have been made: retry with the same --idempotency-key (error.retry_with), or read the record first."
        }
        ErrorCode::StorageError => {
            "The office could not read or write its storage; check the office status, then retry."
        }
        _ => "Check the flags with --help and retry.",
    }
}

#[cfg(test)]
mod tests;
