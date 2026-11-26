//! The transport contract every `*_dispatch` slice shares: [`OfficeTransport`] (a bound agent
//! socket or a grant-bound person host channel), the [`TransportOffice`] adapter, the
//! deterministic entity IDs of a retried create ([`derive_uuid_v7`]), list cursors
//! ([`parse_cursor`]) and the hint of a refusal ([`hint_for`]).
//!
//! A leaf: it imports no other module of the crate, so the slices and the router
//! (`dispatch`, which implements [`org_command::Office`] for [`TransportOffice`]) all depend
//! on it and never on each other.

pub use org_command::Refusal;
use org_proto::ErrorCode;
use serde_json::Value;
use sha2::{Digest, Sha256};

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

/// Adapts any [`OfficeTransport`] into an [`org_command::Office`] (implemented by
/// `dispatch`, which routes each request to its slice).
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
            "Wait for the cooldown given in the message to pass, then retry the command."
        }
        ErrorCode::StorageError => {
            "The office could not read or write its storage; check the office status, then retry."
        }
        _ => "Check the flags with --help and retry.",
    }
}

#[cfg(test)]
mod tests;
