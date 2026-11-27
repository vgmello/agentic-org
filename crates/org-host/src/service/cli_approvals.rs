//! Browser-approved terminal sign-in approvals (FC-173 / SIGN-40 / SES-8).
//!
//! Each approval is created by a `host.sock` connection and bound to that exact connection.
//! Only the SHA-256 digest of the one-time 32-byte token (domain-separated with the instance
//! ID) is stored in memory; the link secret travels once in `cli.approval.create`'s URL
//! fragment (`#...`). Approvals expire after 10 minutes (`APPROVAL_TTL_MS`), work once,
//! and are purged automatically when the requesting `host.sock` connection closes.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::sync::atomic::Ordering;
use std::sync::{Mutex, Weak};

use org_host_control::browser::CliApprovalDecision;
use org_host_control::ops::CliApprovalCreateResult;
use org_host_control::{LinkToken, OneTimeLinkUrl};
use org_proto::{ApiError, ErrorCode};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

use crate::accounts::identity::AccountId;
use crate::control_listener::Connection;

/// 10-minute approval link lifetime (`doc-48` §4a.2 / `SIGN-40`).
pub(crate) const APPROVAL_TTL_MS: i64 = 10 * 60 * 1000;
const MAX_PENDING_APPROVALS: usize = 64;
const MAX_PER_CONNECTION: usize = 4;
const DOMAIN: &[u8] = b"agentic-org/cli-approval/v1";
const TOKEN_BYTES: usize = 32;
const TOKEN_TEXT_LEN: usize = 43;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ApprovalDigest([u8; 32]);

impl ApprovalDigest {
    fn of(instance_id: &str, token: &[u8; TOKEN_BYTES]) -> Self {
        let mut hash = Sha256::new();
        for field in [DOMAIN, instance_id.as_bytes()] {
            hash.update((field.len() as u32).to_be_bytes());
            hash.update(field);
        }
        hash.update(token);
        Self(hash.finalize().into())
    }
}

enum State {
    Pending,
    Approved(AccountId),
    Declined,
}

struct Entry {
    digest: ApprovalDigest,
    connection: Weak<std::sync::atomic::AtomicBool>,
    expires_at_ms: i64,
    state: State,
}

pub(crate) enum PollOutcome {
    Pending,
    Approved(AccountId),
    Declined,
    Expired,
}

#[derive(Default)]
pub(crate) struct CliApprovals {
    entries: Mutex<HashMap<String, Entry>>,
}

impl CliApprovals {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn create(
        &self,
        instance_id: &str,
        origin: &str,
        connection: &Connection,
        now_ms: i64,
    ) -> Result<CliApprovalCreateResult, ApiError> {
        let alive = connection.liveness();
        if alive
            .upgrade()
            .is_none_or(|closed| closed.load(Ordering::SeqCst))
        {
            return Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "connection is closed",
            ));
        }
        let mut raw = [0u8; TOKEN_BYTES];
        File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut raw))
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "randomness unavailable"))?;
        let digest = ApprovalDigest::of(instance_id, &raw);
        let encoded = base64url_encode(&raw);
        raw.zeroize();

        let approval_id = crate::ids::uuid_v7()
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "id generator unavailable"))?;
        let expires_at_ms = now_ms.saturating_add(APPROVAL_TTL_MS);
        let url_text = Zeroizing::new(format!("{origin}/cli-approve#{}", encoded.as_str()));
        let url = OneTimeLinkUrl::new(url_text.to_string())
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "invalid approval url"))?;

        let mut map = self
            .entries
            .lock()
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "approvals unavailable"))?;
        prune(&mut map, now_ms);
        let conn_count = map.values().filter(|e| e.connection.ptr_eq(&alive)).count();
        if map.len() >= MAX_PENDING_APPROVALS || conn_count >= MAX_PER_CONNECTION {
            return Err(ApiError::new(
                ErrorCode::Unavailable,
                "too many pending terminal sign-in approvals",
            ));
        }
        map.insert(
            approval_id.clone(),
            Entry {
                digest,
                connection: alive,
                expires_at_ms,
                state: State::Pending,
            },
        );
        Ok(CliApprovalCreateResult {
            approval_id,
            url,
            expires_at_ms,
        })
    }

    pub(crate) fn decide(
        &self,
        instance_id: &str,
        link: &LinkToken,
        account_id: AccountId,
        approve: bool,
        now_ms: i64,
    ) -> Result<CliApprovalDecision, ApiError> {
        let Some(raw) = parse_token(link.expose()) else {
            return Err(ApiError::new(
                ErrorCode::NotFound,
                "approval link not found",
            ));
        };
        let digest = ApprovalDigest::of(instance_id, &raw);
        drop(raw);

        let mut map = self
            .entries
            .lock()
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "approvals unavailable"))?;
        prune(&mut map, now_ms);
        let entry = map
            .values_mut()
            .find(|e| e.digest == digest)
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "approval link not found"))?;
        if now_ms >= entry.expires_at_ms || !matches!(entry.state, State::Pending) {
            return Err(ApiError::new(
                ErrorCode::NotFound,
                "approval link not found",
            ));
        }
        if approve {
            entry.state = State::Approved(account_id);
            Ok(CliApprovalDecision::Approved)
        } else {
            entry.state = State::Declined;
            Ok(CliApprovalDecision::Declined)
        }
    }

    pub(crate) fn poll(
        &self,
        connection: &Connection,
        approval_id: &str,
        now_ms: i64,
    ) -> Result<PollOutcome, ApiError> {
        let alive = connection.liveness();
        let mut map = self
            .entries
            .lock()
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "approvals unavailable"))?;
        let Some(entry) = map.get(approval_id) else {
            return Ok(PollOutcome::Expired);
        };
        if !entry.connection.ptr_eq(&alive) {
            return Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "approval belongs to another connection",
            ));
        }
        if now_ms >= entry.expires_at_ms {
            map.remove(approval_id);
            return Ok(PollOutcome::Expired);
        }
        match entry.state {
            State::Pending => Ok(PollOutcome::Pending),
            State::Declined => {
                map.remove(approval_id);
                Ok(PollOutcome::Declined)
            }
            State::Approved(_) => {
                let Some(removed) = map.remove(approval_id) else {
                    return Ok(PollOutcome::Expired);
                };
                match removed.state {
                    State::Approved(account_id) => Ok(PollOutcome::Approved(account_id)),
                    _ => Ok(PollOutcome::Expired),
                }
            }
        }
    }

    pub(crate) fn disconnect(&self, connection: &Connection) {
        let alive = connection.liveness();
        if let Ok(mut map) = self.entries.lock() {
            map.retain(|_, entry| !entry.connection.ptr_eq(&alive));
        }
    }
}

fn prune(map: &mut HashMap<String, Entry>, now_ms: i64) {
    map.retain(|_, entry| {
        let live_conn = entry
            .connection
            .upgrade()
            .is_some_and(|closed| !closed.load(Ordering::SeqCst));
        live_conn && now_ms < entry.expires_at_ms
    });
}

fn base64url_encode(bytes: &[u8; TOKEN_BYTES]) -> Zeroizing<String> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = Zeroizing::new(String::with_capacity(TOKEN_TEXT_LEN));
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((triple >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((triple >> 6) & 0x3f) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(TABLE[(triple & 0x3f) as usize] as char);
        }
    }
    out
}

fn parse_token(text: &str) -> Option<Zeroizing<[u8; TOKEN_BYTES]>> {
    if text.len() != TOKEN_TEXT_LEN {
        return None;
    }
    let decode = |b: u8| -> Option<u32> {
        match b {
            b'A'..=b'Z' => Some((b - b'A') as u32),
            b'a'..=b'z' => Some((b - b'a' + 26) as u32),
            b'0'..=b'9' => Some((b - b'0' + 52) as u32),
            b'-' => Some(62),
            b'_' => Some(63),
            _ => None,
        }
    };
    let mut out = Zeroizing::new([0u8; TOKEN_BYTES]);
    let mut idx = 0;
    for chunk in text.as_bytes().chunks(4) {
        let c0 = decode(chunk[0])?;
        let c1 = decode(chunk[1])?;
        if chunk.len() == 4 {
            let c2 = decode(chunk[2])?;
            let c3 = decode(chunk[3])?;
            let triple = (c0 << 18) | (c1 << 12) | (c2 << 6) | c3;
            out[idx] = ((triple >> 16) & 0xff) as u8;
            out[idx + 1] = ((triple >> 8) & 0xff) as u8;
            out[idx + 2] = (triple & 0xff) as u8;
            idx += 3;
        } else if chunk.len() == 3 {
            let c2 = decode(chunk[2])?;
            if (c2 & 0x03) != 0 {
                return None;
            }
            let triple = (c0 << 18) | (c1 << 12) | (c2 << 6);
            out[idx] = ((triple >> 16) & 0xff) as u8;
            out[idx + 1] = ((triple >> 8) & 0xff) as u8;
            idx += 2;
        } else {
            return None;
        }
    }
    (idx == TOKEN_BYTES).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fragment(url: &OneTimeLinkUrl) -> LinkToken {
        let (_, secret) = url.expose().split_once('#').expect("fragment");
        LinkToken::new(secret.to_owned()).unwrap()
    }

    #[test]
    fn approval_lifecycle_enforces_single_use_ttl_and_connection_binding() {
        let approvals = CliApprovals::new();
        let conn1 = Connection::for_test();
        let conn2 = Connection::for_test();
        let instance = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607182";
        let account = AccountId::parse("01M72N8MG00000000000000A01").unwrap();

        let created = approvals
            .create(instance, "http://127.0.0.1:7420", &conn1, 1_000)
            .unwrap();
        assert!(created
            .url
            .expose()
            .starts_with("http://127.0.0.1:7420/cli-approve#"));
        assert_eq!(created.expires_at_ms, 1_000 + APPROVAL_TTL_MS);

        // Wrong connection cannot poll.
        assert_eq!(
            approvals
                .poll(&conn2, &created.approval_id, 2_000)
                .err()
                .unwrap()
                .code,
            ErrorCode::NotAuthorized
        );
        // Same connection sees Pending before decision.
        assert!(matches!(
            approvals.poll(&conn1, &created.approval_id, 2_000).unwrap(),
            PollOutcome::Pending
        ));

        // Browser approves once; second decide is NotFound.
        let link = fragment(&created.url);
        assert_eq!(
            approvals
                .decide(instance, &link, account.clone(), true, 3_000)
                .unwrap(),
            CliApprovalDecision::Approved
        );
        assert_eq!(
            approvals
                .decide(instance, &link, account.clone(), true, 3_001)
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );

        // Poll consumes the approval once; second poll sees Expired.
        assert!(matches!(
            approvals.poll(&conn1, &created.approval_id, 3_002).unwrap(),
            PollOutcome::Approved(ref id) if id == &account
        ));
        assert!(matches!(
            approvals.poll(&conn1, &created.approval_id, 3_003).unwrap(),
            PollOutcome::Expired
        ));

        // Declined approval reports Declined once and never signs in.
        let declined = approvals
            .create(instance, "http://127.0.0.1:7420", &conn1, 10_000)
            .unwrap();
        assert_eq!(
            approvals
                .decide(
                    instance,
                    &fragment(&declined.url),
                    account.clone(),
                    false,
                    11_000
                )
                .unwrap(),
            CliApprovalDecision::Declined
        );
        assert!(matches!(
            approvals
                .poll(&conn1, &declined.approval_id, 11_001)
                .unwrap(),
            PollOutcome::Declined
        ));

        // 10-minute expiry refuses both decide and poll.
        let expiring = approvals
            .create(instance, "http://127.0.0.1:7420", &conn1, 20_000)
            .unwrap();
        assert_eq!(
            approvals
                .decide(
                    instance,
                    &fragment(&expiring.url),
                    account,
                    true,
                    20_000 + APPROVAL_TTL_MS
                )
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert!(matches!(
            approvals
                .poll(&conn1, &expiring.approval_id, 20_000 + APPROVAL_TTL_MS)
                .unwrap(),
            PollOutcome::Expired
        ));
    }

    #[test]
    fn post_approval_expiry_disconnect_purge_and_per_connection_cap_are_enforced() {
        let approvals = CliApprovals::new();
        let conn = Connection::for_test();
        let instance = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607182";
        let account = AccountId::parse("01M72N8MG00000000000000A01").unwrap();

        // Approved before expiry, polled at/after expiry -> Expired (never mints session).
        let late = approvals
            .create(instance, "http://127.0.0.1:7420", &conn, 1_000)
            .unwrap();
        assert_eq!(
            approvals
                .decide(instance, &fragment(&late.url), account, true, 2_000)
                .unwrap(),
            CliApprovalDecision::Approved
        );
        assert!(matches!(
            approvals
                .poll(&conn, &late.approval_id, 1_000 + APPROVAL_TTL_MS)
                .unwrap(),
            PollOutcome::Expired
        ));

        // Per-connection cap (`MAX_PER_CONNECTION = 4`) refuses a fifth pending approval.
        let mut ids = Vec::new();
        for _ in 0..MAX_PER_CONNECTION {
            ids.push(
                approvals
                    .create(instance, "http://127.0.0.1:7420", &conn, 5_000)
                    .unwrap()
                    .approval_id,
            );
        }
        assert_eq!(
            approvals
                .create(instance, "http://127.0.0.1:7420", &conn, 5_001)
                .unwrap_err()
                .code,
            ErrorCode::Unavailable
        );

        // Disconnect purges all pending approvals owned by that connection.
        approvals.disconnect(&conn);
        for id in ids {
            assert!(matches!(
                approvals.poll(&conn, &id, 5_002).unwrap(),
                PollOutcome::Expired
            ));
        }
    }
}
