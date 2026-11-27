//! External (BYOA) agent leases and the single-active-lease invariant (`TASK-137`, `doc-102`).
//!
//! An agent is driven by at most one thing at a time: an office-launched managed run *or* one
//! external workspace that a person approved. This module owns the external side:
//!
//! - **Pending logins** ([`codes`]): RAM-only, bounded, 10-minute codes with a 3-attempt lockout.
//!   Nothing is written under `sessions/` until a person approves, so the store's employee scan
//!   and startup recovery never see a synthetic folder.
//! - **Challenges** ([`nonces`]): single-use, 60-second nonces bound to the lease session and
//!   its public key; answers are Ed25519 signatures over
//!   [`org_proto::external_auth::challenge_message`].
//! - **Leases** ([`lease`]): `external-lease.v1.json` (0600, atomic, fully synced) and
//!   `external-history.jsonl` (0600, append-only, redacted) inside the agent's existing
//!   `sessions/<employee-id>/` folder, cached in RAM. Approving supersedes the previous lease;
//!   a live managed turn refuses approval unless `--takeover` parks it.
//! - **Fast tokens** live in [`crate::RunBindingRegistry`] (RAM-only) and exist only while the
//!   lease is active.
//!
//! Lock order: `lease_mutation` (outermost: approvals, revocations, logouts and the final
//! check-and-issue of an exchange), then an employee lock, then `registry` and `bindings`, then
//! `leases` (a leaf, never held across I/O or another lock).

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

mod codes;
mod nonces;

pub(crate) use codes::PendingLogins;
pub(crate) use nonces::{verify_signature, NonceBook, NonceEntry};

/// Current on-disk lease format.
pub const EXTERNAL_LEASE_VERSION: u32 = 1;

/// One agent's external lease (`sessions/<employee-id>/external-lease.v1.json`). Holds the
/// workspace's public key, never a private key or a token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalLeaseRecord {
    pub version: u32,
    pub session_id: String,
    pub employee_id: String,
    pub pubkey_ed25519_b64: String,
    pub workspace_label: String,
    pub approved_by: String,
    pub approved_at: String,
    pub expires_at: String,
    #[serde(default)]
    pub revoked_at: Option<String>,
}

impl ExternalLeaseRecord {
    /// The lease's expiry in epoch milliseconds; an unreadable expiry is already expired.
    pub fn expires_at_ms(&self) -> u64 {
        org_proto::parse_rfc3339_ms(&self.expires_at).unwrap_or(0)
    }

    /// Unrevoked and unexpired at `now_ms`.
    pub fn is_active(&self, now_ms: u64) -> bool {
        self.revoked_at.is_none() && now_ms < self.expires_at_ms()
    }
}

/// The external-auth state a [`crate::SessionManager`] keeps in RAM.
#[derive(Default)]
pub(crate) struct ExternalAuthState {
    pub(crate) pending: Mutex<PendingLogins>,
    pub(crate) nonces: Mutex<NonceBook>,
    /// Lease cache: `Some(None)` means "read from disk: no lease". Filled lazily, updated by every
    /// approval and revocation before it returns.
    leases: Mutex<HashMap<String, Option<ExternalLeaseRecord>>>,
    /// Serializes approvals, revocations, logouts and the final check-and-issue of a token
    /// exchange (rare actions). Taken before `registry` and `leases`.
    pub(crate) lease_mutation: Mutex<()>,
    /// Agents whose lease RAM holds revoked but whose file still says active (a failed durable
    /// write): the next revocation call or approval writes it.
    unpersisted: Mutex<HashSet<String>>,
}

impl ExternalAuthState {
    /// The cached lease of `employee_id`, if the cache has an answer.
    pub(crate) fn cached(&self, employee_id: &str) -> Option<Option<ExternalLeaseRecord>> {
        self.leases
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(employee_id)
            .cloned()
    }

    /// Whether the cache says `employee_id` holds an active lease at `now_ms`. A cache miss is
    /// "no": callers that must not miss a lease load it first
    /// ([`crate::SessionManager::external_lease`]) and only then take the registry lock.
    pub(crate) fn cached_active(&self, employee_id: &str, now_ms: u64) -> bool {
        self.leases
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(employee_id)
            .is_some_and(|lease| lease.as_ref().is_some_and(|l| l.is_active(now_ms)))
    }

    /// Fills the cache from a disk read unless an approval or revocation got there first.
    pub(crate) fn fill(&self, employee_id: &str, loaded: Option<ExternalLeaseRecord>) {
        self.leases
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .entry(employee_id.to_owned())
            .or_insert(loaded);
    }

    /// Replaces the cached lease, answering the previous cache entry.
    pub(crate) fn replace(
        &self,
        employee_id: &str,
        lease: Option<ExternalLeaseRecord>,
    ) -> Option<Option<ExternalLeaseRecord>> {
        self.leases
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(employee_id.to_owned(), lease)
    }

    /// Records that `employee_id`'s revocation is in RAM but not yet on disk.
    pub(crate) fn mark_unpersisted(&self, employee_id: &str) {
        self.unpersisted
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(employee_id.to_owned());
    }

    /// Whether `employee_id`'s lease file still owes a revocation RAM already made.
    pub(crate) fn owes_disk(&self, employee_id: &str) -> bool {
        self.unpersisted
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains(employee_id)
    }

    /// `employee_id`'s lease file now says what RAM says.
    pub(crate) fn settle_persisted(&self, employee_id: &str) {
        self.unpersisted
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(employee_id);
    }

    /// Restores a cache entry after a failed durable write.
    pub(crate) fn restore(&self, employee_id: &str, previous: Option<Option<ExternalLeaseRecord>>) {
        let mut leases = self.leases.lock().unwrap_or_else(|p| p.into_inner());
        match previous {
            Some(lease) => {
                leases.insert(employee_id.to_owned(), lease);
            }
            None => {
                leases.remove(employee_id);
            }
        }
    }
}

#[cfg(test)]
impl ExternalAuthState {
    /// Holds `lease_mutation` as an approval does, for tests of what must wait for it.
    pub(crate) fn lease_mutation_for_test(&self) -> std::sync::MutexGuard<'_, ()> {
        self.lease_mutation
            .lock()
            .unwrap_or_else(|p| p.into_inner())
    }

    /// Publishes `lease` in the cache as an approval or revocation does; the caller holds
    /// [`Self::lease_mutation_for_test`].
    pub(crate) fn publish_for_test(&self, lease: ExternalLeaseRecord) {
        let employee_id = lease.employee_id.clone();
        self.replace(&employee_id, Some(lease));
    }
}

/// First 16 hex digits of SHA-256 over the raw public key, for operators to compare.
pub(crate) fn pubkey_fingerprint(pubkey: &[u8]) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, pubkey);
    digest.as_ref()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Decodes a standard-base64 Ed25519 public key, refusing anything but 32 bytes.
pub(crate) fn decode_pubkey(b64: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    if b64.len() > 64 {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .ok()?;
    (bytes.len() == org_proto::external_auth::ED25519_PUBLIC_KEY_LEN).then_some(bytes)
}

#[cfg(test)]
mod tests;

/// Validates and trims an external workspace label against the wire byte budget and control-char
/// rules (single choke-point shared by `auth.login_request` and `member.external_bind`).
pub(crate) fn validate_workspace_label(label: &str) -> Result<String, org_proto::ApiError> {
    use org_proto::external_auth::MAX_WORKSPACE_LABEL_BYTES;
    let trimmed = label.trim();
    if trimmed.is_empty()
        || trimmed.len() > MAX_WORKSPACE_LABEL_BYTES
        || trimmed.chars().any(char::is_control)
    {
        return Err(org_proto::ApiError::new(
            org_proto::ErrorCode::InvalidRequest,
            format!(
                "the workspace label must be 1 to {MAX_WORKSPACE_LABEL_BYTES} bytes without control characters"
            ),
        ));
    }
    Ok(trimmed.to_owned())
}

/// Built-in office agents (Chief of Staff, HR) run only inside the office: an external workspace
/// would run them outside their office sandbox, so no step of an external sign-in or bind
/// (request, approval, bind, challenge, exchange) is ever theirs (`TASK-80.1`, `ADR-0036`).
pub(crate) fn refuse_builtin_agent(
    employee: &org_office::organization_family::records::Employee,
) -> Result<(), org_proto::ApiError> {
    use org_office::organization_family::records::BuiltinKey;
    let Some(key) = employee.builtin_key else {
        return Ok(());
    };
    let message = match key {
        BuiltinKey::ChiefOfStaff => "the Chief of Staff runs only inside the office",
        BuiltinKey::Hr => "the built-in HR agent runs only inside the office",
    };
    Err(org_proto::ApiError::new(
        org_proto::ErrorCode::NotAuthorized,
        message,
    ))
}
