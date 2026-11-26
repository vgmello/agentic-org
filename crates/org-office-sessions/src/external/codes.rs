//! RAM-only pending external logins (`TASK-137`): bounded, 10-minute approval codes with a
//! 3-attempt lockout. Nothing here touches disk.
//!
//! A code is `LLLL-SSSS` in Crockford base32: `LLLL` finds the request, `SSSS` is the secret.
//! A known `LLLL` with a wrong `SSSS` counts against that request; the third miss locks it.

use org_proto::external_auth::{ExternalLoginState, APPROVAL_CODE_TTL_SECS, MAX_APPROVAL_ATTEMPTS};
use org_proto::{ApiError, ErrorCode};
use ring::rand::{SecureRandom, SystemRandom};

use crate::binding::constant_time_eq;

/// Crockford base32: no `I`, `L`, `O` or `U`.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const PART_LEN: usize = 4;
/// Most pending logins on one office, and per agent.
pub(crate) const MAX_PENDING_TOTAL: usize = 32;
pub(crate) const MAX_PENDING_PER_EMPLOYEE: usize = 3;
/// How long a settled (approved, locked or expired) request stays pollable.
const SETTLED_GRACE_MS: u64 = 2 * 60 * 1000;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Settle {
    Pending,
    Approved {
        session_id: String,
        lease_expires_at: String,
        at_ms: u64,
    },
    Locked {
        at_ms: u64,
    },
}

/// One pending login. Its secret half never leaves this module.
#[derive(Clone)]
pub(crate) struct PendingLogin {
    pub(crate) request_id: String,
    lookup: String,
    secret: String,
    pub(crate) employee_id: String,
    pub(crate) pubkey_ed25519_b64: String,
    pub(crate) workspace_label: String,
    pub(crate) expires_ms: u64,
    attempts: u32,
    state: Settle,
}

impl std::fmt::Debug for PendingLogin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingLogin")
            .field("request_id", &self.request_id)
            .field("employee_id", &self.employee_id)
            .field("secret", &"<redacted>")
            .field("attempts", &self.attempts)
            .field("state", &self.state)
            .finish()
    }
}

impl PendingLogin {
    fn is_live(&self, now_ms: u64) -> bool {
        self.state == Settle::Pending && now_ms < self.expires_ms
    }

    fn settled_ms(&self) -> u64 {
        match self.state {
            Settle::Pending => self.expires_ms,
            Settle::Approved { at_ms, .. } | Settle::Locked { at_ms } => at_ms,
        }
    }
}

/// What a poll of a pending login answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PollAnswer {
    pub(crate) state: ExternalLoginState,
    pub(crate) employee_id: String,
    pub(crate) session_id: Option<String>,
    pub(crate) lease_expires_at: Option<String>,
}

/// The bounded set of pending logins.
#[derive(Default)]
pub(crate) struct PendingLogins {
    entries: Vec<PendingLogin>,
}

fn random_part(rng: &SystemRandom) -> Result<String, ApiError> {
    let mut bytes = [0u8; PART_LEN];
    rng.fill(&mut bytes)
        .map_err(|_| ApiError::new(ErrorCode::Unavailable, "system entropy unavailable"))?;
    Ok(bytes
        .iter()
        .map(|b| char::from(ALPHABET[usize::from(b & 31)]))
        .collect())
}

/// `ABCD-EFGH`, any case, spaces or hyphens anywhere; `O`→`0`, `I`/`L`→`1` (Crockford).
pub(crate) fn normalize_code(code: &str) -> Option<(String, String)> {
    if code.len() > 32 {
        return None;
    }
    let mut out = String::with_capacity(PART_LEN * 2);
    for ch in code.chars() {
        let mapped = match ch.to_ascii_uppercase() {
            '-' | ' ' | '\t' => continue,
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        };
        if !mapped.is_ascii() || !ALPHABET.contains(&(mapped as u8)) {
            return None;
        }
        out.push(mapped);
    }
    (out.len() == PART_LEN * 2).then(|| {
        let (lookup, secret) = out.split_at(PART_LEN);
        (lookup.to_owned(), secret.to_owned())
    })
}

fn bad_code() -> ApiError {
    ApiError::new(
        ErrorCode::InvalidRequest,
        "an approval code looks like ABCD-EFGH (as `agent-os login` printed it)",
    )
}

impl PendingLogins {
    fn prune(&mut self, now_ms: u64) {
        self.entries
            .retain(|e| now_ms < e.settled_ms().saturating_add(SETTLED_GRACE_MS));
    }

    /// Records a new pending login and answers it with its full code (`LLLL-SSSS`).
    pub(crate) fn insert(
        &mut self,
        request_id: String,
        employee_id: &str,
        pubkey_ed25519_b64: &str,
        workspace_label: &str,
        now_ms: u64,
    ) -> Result<(PendingLogin, String), ApiError> {
        self.prune(now_ms);
        let live: Vec<&PendingLogin> = self.entries.iter().filter(|e| e.is_live(now_ms)).collect();
        if live.len() >= MAX_PENDING_TOTAL {
            return Err(ApiError::new(
                ErrorCode::Unavailable,
                "too many sign-ins are waiting for approval on this office; retry later",
            ));
        }
        if live.iter().filter(|e| e.employee_id == employee_id).count() >= MAX_PENDING_PER_EMPLOYEE
        {
            return Err(ApiError::new(
                ErrorCode::Unavailable,
                "this agent already has sign-ins waiting for approval; approve one or wait",
            ));
        }
        let rng = SystemRandom::new();
        let mut lookup = random_part(&rng)?;
        let mut tries = 0;
        while self.entries.iter().any(|e| e.lookup == lookup) {
            tries += 1;
            if tries > 16 {
                return Err(ApiError::new(ErrorCode::Unavailable, "retry the sign-in"));
            }
            lookup = random_part(&rng)?;
        }
        let secret = random_part(&rng)?;
        let code = format!("{lookup}-{secret}");
        let entry = PendingLogin {
            request_id,
            lookup,
            secret,
            employee_id: employee_id.to_owned(),
            pubkey_ed25519_b64: pubkey_ed25519_b64.to_owned(),
            workspace_label: workspace_label.to_owned(),
            expires_ms: now_ms.saturating_add(APPROVAL_CODE_TTL_SECS * 1000),
            attempts: 0,
            state: Settle::Pending,
        };
        self.entries.push(entry.clone());
        Ok((entry, code))
    }

    /// Where `request_id` stands; `None` when it is unknown (or long settled).
    pub(crate) fn poll(&mut self, request_id: &str, now_ms: u64) -> Option<PollAnswer> {
        self.prune(now_ms);
        let entry = self.entries.iter().find(|e| e.request_id == request_id)?;
        let (state, session_id, lease_expires_at) = match &entry.state {
            Settle::Pending if now_ms < entry.expires_ms => {
                (ExternalLoginState::Pending, None, None)
            }
            Settle::Pending => (ExternalLoginState::Expired, None, None),
            Settle::Locked { .. } => (ExternalLoginState::Locked, None, None),
            Settle::Approved {
                session_id,
                lease_expires_at,
                ..
            } => (
                ExternalLoginState::Approved,
                Some(session_id.clone()),
                Some(lease_expires_at.clone()),
            ),
        };
        Some(PollAnswer {
            state,
            employee_id: entry.employee_id.clone(),
            session_id,
            lease_expires_at,
        })
    }

    /// Finds the live pending login `code` names without consuming it. A wrong secret for a
    /// known request counts as an attempt; the [`MAX_APPROVAL_ATTEMPTS`]th locks the request.
    pub(crate) fn check_code(&mut self, code: &str, now_ms: u64) -> Result<PendingLogin, ApiError> {
        self.prune(now_ms);
        let (lookup, secret) = normalize_code(code).ok_or_else(bad_code)?;
        let Some(entry) = self.entries.iter_mut().find(|e| e.lookup == lookup) else {
            return Err(ApiError::new(
                ErrorCode::NotFound,
                "no sign-in is waiting for that approval code (it may have expired)",
            ));
        };
        if !entry.is_live(now_ms) {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                match entry.state {
                    Settle::Locked { .. } => {
                        "that sign-in was locked after too many wrong codes; run `agent-os login` again"
                    }
                    Settle::Approved { .. } => "that sign-in is already approved",
                    Settle::Pending => "that approval code expired; run `agent-os login` again",
                },
            ));
        }
        if !constant_time_eq(entry.secret.as_bytes(), secret.as_bytes()) {
            entry.attempts += 1;
            if entry.attempts >= MAX_APPROVAL_ATTEMPTS {
                entry.state = Settle::Locked { at_ms: now_ms };
                return Err(ApiError::new(
                    ErrorCode::NotAuthorized,
                    "wrong approval code; the sign-in is now locked, run `agent-os login` again",
                ));
            }
            let left = MAX_APPROVAL_ATTEMPTS - entry.attempts;
            return Err(ApiError::new(
                ErrorCode::NotAuthorized,
                format!("wrong approval code ({left} attempt(s) left)"),
            ));
        }
        Ok(entry.clone())
    }

    /// Settles `request_id` as approved with lease `session_id`.
    pub(crate) fn mark_approved(
        &mut self,
        request_id: &str,
        session_id: &str,
        lease_expires_at: &str,
        now_ms: u64,
    ) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.request_id == request_id) {
            entry.state = Settle::Approved {
                session_id: session_id.to_owned(),
                lease_expires_at: lease_expires_at.to_owned(),
                at_ms: now_ms,
            };
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}
