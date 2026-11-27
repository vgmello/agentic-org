//! RAM-only pending external logins (`TASK-137`): bounded, 10-minute approval codes with a
//! 3-attempt lockout. Nothing here touches disk.
//!
//! A code is `LLLL-SSSS` in Crockford base32: `LLLL` finds the request, `SSSS` is the secret.
//! Approval reads the request `LLLL` names first ([`PendingLogins::find_live`], no attempt
//! spent), so the caller's authority over that agent is checked before the secret is: only then
//! does a wrong `SSSS` count against the request ([`PendingLogins::check_code`]), and the third
//! miss locks it. A person without authority over the agent can never lock its sign-in.

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
/// Most records kept at all, live or settled (a settled one stays pollable for a grace): the
/// oldest settled record makes room for a new login.
pub(crate) const MAX_ENTRIES_TOTAL: usize = 2 * MAX_PENDING_TOTAL;
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
    pub(crate) hostname: Option<String>,
    pub(crate) workspace_path: Option<String>,
    pub(crate) requested_ms: u64,
    pub(crate) expires_ms: u64,
    attempts: u32,
    state: Settle,
}

/// Inputs to [`PendingLogins::insert`].
pub(crate) struct PendingInsert<'a> {
    pub(crate) request_id: String,
    pub(crate) employee_id: &'a str,
    pub(crate) pubkey_ed25519_b64: &'a str,
    pub(crate) workspace_label: &'a str,
    pub(crate) hostname: Option<&'a str>,
    pub(crate) workspace_path: Option<&'a str>,
    pub(crate) now_ms: u64,
}

/// Public snapshot of one live pending login for human operator inspection.
/// Carries only the masked `code_prefix` (`XXXX-••••`), NEVER the secret half.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PendingSnapshot {
    pub(crate) request_id: String,
    pub(crate) employee_id: String,
    pub(crate) pubkey_ed25519_b64: String,
    pub(crate) workspace_label: String,
    pub(crate) hostname: Option<String>,
    pub(crate) workspace_path: Option<String>,
    pub(crate) requested_ms: u64,
    pub(crate) expires_ms: u64,
    pub(crate) code_prefix: String,
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

    /// Masked approval code prefix (`XXXX-••••`); never includes the secret half.
    fn code_prefix(&self) -> String {
        format!("{}-••••", self.lookup)
    }

    fn snapshot(&self) -> PendingSnapshot {
        PendingSnapshot {
            request_id: self.request_id.clone(),
            employee_id: self.employee_id.clone(),
            pubkey_ed25519_b64: self.pubkey_ed25519_b64.clone(),
            workspace_label: self.workspace_label.clone(),
            hostname: self.hostname.clone(),
            workspace_path: self.workspace_path.clone(),
            requested_ms: self.requested_ms,
            expires_ms: self.expires_ms,
            code_prefix: self.code_prefix(),
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

/// The live request a code's lookup half names, read without spending an attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodeTarget {
    pub(crate) request_id: String,
    pub(crate) employee_id: String,
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

fn too_many() -> ApiError {
    ApiError::new(
        ErrorCode::Unavailable,
        "too many sign-ins are waiting for approval on this office; retry later",
    )
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

    /// Drops the oldest settled records until a new one fits under [`MAX_ENTRIES_TOTAL`].
    fn make_room(&mut self, now_ms: u64) -> Result<(), ApiError> {
        while self.entries.len() >= MAX_ENTRIES_TOTAL {
            let oldest = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| !e.is_live(now_ms))
                .min_by_key(|(_, e)| e.settled_ms())
                .map(|(at, _)| at)
                .ok_or_else(too_many)?;
            self.entries.remove(oldest);
        }
        Ok(())
    }

    /// Records a new pending login and answers it with its full code (`LLLL-SSSS`).
    pub(crate) fn insert(
        &mut self,
        input: PendingInsert<'_>,
    ) -> Result<(PendingLogin, String), ApiError> {
        let now_ms = input.now_ms;
        self.prune(now_ms);
        let live: Vec<&PendingLogin> = self.entries.iter().filter(|e| e.is_live(now_ms)).collect();
        if live.len() >= MAX_PENDING_TOTAL {
            return Err(too_many());
        }
        if live
            .iter()
            .filter(|e| e.employee_id == input.employee_id)
            .count()
            >= MAX_PENDING_PER_EMPLOYEE
        {
            return Err(ApiError::new(
                ErrorCode::Unavailable,
                "this agent already has sign-ins waiting for approval; approve one or wait",
            ));
        }
        self.make_room(now_ms)?;
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
            request_id: input.request_id,
            lookup,
            secret,
            employee_id: input.employee_id.to_owned(),
            pubkey_ed25519_b64: input.pubkey_ed25519_b64.to_owned(),
            workspace_label: input.workspace_label.to_owned(),
            hostname: input.hostname.map(str::to_owned),
            workspace_path: input.workspace_path.map(str::to_owned),
            requested_ms: now_ms,
            expires_ms: now_ms.saturating_add(APPROVAL_CODE_TTL_SECS * 1000),
            attempts: 0,
            state: Settle::Pending,
        };
        self.entries.push(entry.clone());
        Ok((entry, code))
    }

    /// Snapshots all live pending logins (optionally filtered to `employee_id`), oldest first.
    /// Never exposes the secret half of any approval code.
    pub(crate) fn list_live(
        &mut self,
        employee_id: Option<&str>,
        now_ms: u64,
    ) -> Vec<PendingSnapshot> {
        self.prune(now_ms);
        let mut out: Vec<PendingSnapshot> = self
            .entries
            .iter()
            .filter(|e| e.is_live(now_ms))
            .filter(|e| employee_id.is_none_or(|id| e.employee_id == id))
            .map(PendingLogin::snapshot)
            .collect();
        out.sort_by_key(|s| s.requested_ms);
        out
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

    /// The live request `code`'s lookup half names. Reads only: no attempt is spent and the
    /// secret half is not looked at, so the caller's authority over the agent can be checked
    /// before [`Self::check_code`].
    pub(crate) fn find_live(&mut self, code: &str, now_ms: u64) -> Result<CodeTarget, ApiError> {
        self.prune(now_ms);
        let (lookup, _) = normalize_code(code).ok_or_else(bad_code)?;
        let entry = self.live_by_lookup(&lookup, now_ms)?;
        Ok(CodeTarget {
            request_id: entry.request_id.clone(),
            employee_id: entry.employee_id.clone(),
        })
    }

    fn live_by_lookup(&mut self, lookup: &str, now_ms: u64) -> Result<&mut PendingLogin, ApiError> {
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
        Ok(entry)
    }

    /// Checks `code`'s secret half against the live request `target` names, without consuming
    /// it. Call only after the caller's authority over `target`'s agent is established: a wrong
    /// secret counts as an attempt and the [`MAX_APPROVAL_ATTEMPTS`]th locks the request.
    pub(crate) fn check_code(
        &mut self,
        code: &str,
        target: &CodeTarget,
        now_ms: u64,
    ) -> Result<PendingLogin, ApiError> {
        self.prune(now_ms);
        let (lookup, secret) = normalize_code(code).ok_or_else(bad_code)?;
        let entry = self.live_by_lookup(&lookup, now_ms)?;
        if entry.request_id != target.request_id || entry.employee_id != target.employee_id {
            // The lookup now names another request (the first expired and its half was reused).
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "that approval code changed while it was checked; run `agent-os login` again",
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
