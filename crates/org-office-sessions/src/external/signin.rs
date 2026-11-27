//! Workspace sign-in answered on the office socket before a connection binds (`auth.*`,
//! `TASK-137`): login requests and polls, challenges, the token exchange, and logout.
//!
//! Every refusal that could tell a caller whether a lease, session or nonce exists is the same
//! opaque `not_authorized`.

use org_proto::external_auth::{
    challenge_message, AuthChallengeResult, AuthExchangeResult, AuthLoginPollResult,
    AuthLoginRequestParams, AuthLoginRequestResult, AuthLogoutResult, ChallengePurpose,
    SignedChallengeParams, FAST_TOKEN_TTL_SECS,
};
use org_proto::{ApiError, ErrorCode};
use zeroize::Zeroizing;

use crate::credentials::{format_rfc3339_ms, is_safe_id, now_ms};
use crate::manager::SessionManager;

use super::nonces::{verify_signature, NonceEntry};
use super::{decode_pubkey, refuse_builtin_agent, validate_workspace_label, ExternalLeaseRecord};

fn refused() -> ApiError {
    ApiError::new(
        ErrorCode::NotAuthorized,
        "this office cannot sign in this workspace",
    )
}

impl SessionManager {
    /// `auth.login_request`: records a RAM-only pending login for an active local agent and
    /// answers the approval code a person must enter.
    pub fn external_login_request(
        &self,
        employee_id: &str,
        params: &AuthLoginRequestParams,
    ) -> Result<AuthLoginRequestResult, ApiError> {
        if decode_pubkey(&params.pubkey_ed25519_b64).is_none() {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "pubkey_ed25519_b64 must be a base64 Ed25519 public key (32 bytes)",
            ));
        }
        let label = validate_workspace_label(&params.workspace_label)?;
        let employee = self.require_active_agent(employee_id)?;
        refuse_builtin_agent(&employee)?;
        let emp_id = employee.id.as_str();
        let (entry, code) = self
            .external
            .pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(
                self.office.next_uuid_v7(),
                emp_id,
                params.pubkey_ed25519_b64.trim(),
                &label,
                now_ms(),
            )?;
        Ok(AuthLoginRequestResult {
            request_id: entry.request_id,
            member_id: emp_id.to_owned(),
            handle: (!employee.handle.is_empty()).then(|| employee.handle.clone()),
            office_id: self.office.info().office_id.clone(),
            approve_command: format!("agentic-org agent approve {code}"),
            approval_code: code,
            expires_at: format_rfc3339_ms(entry.expires_ms),
            web_path: format!("/members/{emp_id}"),
        })
    }

    /// `auth.login_poll`: where a pending login stands.
    pub fn external_login_poll(&self, request_id: &str) -> Result<AuthLoginPollResult, ApiError> {
        let answer = self
            .external
            .pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .poll(request_id, now_ms())
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "no such sign-in request"))?;
        Ok(AuthLoginPollResult {
            state: answer.state,
            member_id: answer.employee_id,
            session_id: answer.session_id,
            lease_expires_at: answer.lease_expires_at,
        })
    }

    /// `auth.challenge`: a single-use nonce for `employee_id`'s active lease `session_id`.
    pub fn external_challenge(
        &self,
        employee_id: &str,
        session_id: &str,
        purpose: ChallengePurpose,
    ) -> Result<AuthChallengeResult, ApiError> {
        self.refuse_builtin_agent_id(employee_id)?;
        let now = now_ms();
        let lease = self
            .external_lease(employee_id)
            .filter(|l| l.is_active(now) && l.session_id == session_id)
            .ok_or_else(refused)?;
        let (nonce, entry) = self
            .external
            .nonces
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .issue(
                |issued_ms, expires_ms| NonceEntry {
                    employee_id: employee_id.to_owned(),
                    session_id: lease.session_id.clone(),
                    pubkey_ed25519_b64: lease.pubkey_ed25519_b64.clone(),
                    purpose,
                    issued_ms,
                    expires_ms,
                },
                now,
            )?;
        Ok(AuthChallengeResult {
            member_id: employee_id.to_owned(),
            office_id: self.office.info().office_id.clone(),
            nonce,
            expires_at: format_rfc3339_ms(entry.expires_ms),
        })
    }

    /// Spends the answer's nonce (single use, whatever the outcome), then checks it was issued
    /// to this agent, lease, key and purpose and that the lease key signed it.
    fn verify_signed_answer(
        &self,
        employee_id: &str,
        params: &SignedChallengeParams,
        purpose: ChallengePurpose,
    ) -> Result<ExternalLeaseRecord, ApiError> {
        if !is_safe_id(&params.nonce) || !is_safe_id(&params.session_id) {
            return Err(refused());
        }
        let now = now_ms();
        let entry = self
            .external
            .nonces
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take(&params.nonce, now)
            .ok_or_else(refused)?;
        if entry.employee_id != employee_id
            || entry.session_id != params.session_id
            || entry.purpose != purpose
        {
            return Err(refused());
        }
        let lease = self
            .external_lease(employee_id)
            .filter(|l| {
                l.is_active(now)
                    && l.session_id == entry.session_id
                    && l.pubkey_ed25519_b64 == entry.pubkey_ed25519_b64
            })
            .ok_or_else(refused)?;
        let pubkey = decode_pubkey(&lease.pubkey_ed25519_b64).ok_or_else(refused)?;
        let message = challenge_message(
            purpose,
            &self.office.info().office_id,
            employee_id,
            &lease.session_id,
            &params.nonce,
        );
        if !verify_signature(&pubkey, &message, &params.signature_ed25519_b64) {
            return Err(refused());
        }
        Ok(lease)
    }

    /// `auth.exchange`: a fresh RAM-only fast token for a correctly signed exchange nonce.
    ///
    /// The final "is this lease still the active one" check and the token's issue happen under
    /// `lease_mutation`, the lock every approval, revocation and logout holds: a revocation
    /// either lands first (no token is issued) or after (and removes the token), and a newer
    /// lease's token can never be replaced by a token of the lease it superseded.
    pub fn external_exchange(
        &self,
        employee_id: &str,
        params: &SignedChallengeParams,
    ) -> Result<AuthExchangeResult, ApiError> {
        self.refuse_builtin_agent_id(employee_id)?;
        let signed = self.verify_signed_answer(employee_id, params, ChallengePurpose::Exchange)?;
        let (lease, binding, expires_ms) = {
            let _serial = self
                .external
                .lease_mutation
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            let now = now_ms();
            let lease = self
                .external_lease(employee_id)
                .filter(|l| {
                    l.is_active(now)
                        && l.session_id == signed.session_id
                        && l.pubkey_ed25519_b64 == signed.pubkey_ed25519_b64
                })
                .ok_or_else(refused)?;
            let expires_ms = now
                .saturating_add(FAST_TOKEN_TTL_SECS * 1000)
                .min(lease.expires_at_ms());
            let binding = self
                .bindings
                .issue_external(employee_id, &lease.session_id, expires_ms)
                .map_err(|_| ApiError::new(ErrorCode::Unavailable, "system entropy unavailable"))?;
            (lease, binding, expires_ms)
        };
        // A lifecycle event (not a per-command line), written outside the lock.
        self.record_external_event(
            employee_id,
            "token_issued",
            Some(&lease.session_id),
            None,
            None,
        );
        Ok(AuthExchangeResult {
            member_id: employee_id.to_owned(),
            office_id: self.office.info().office_id.clone(),
            session_id: lease.session_id.clone(),
            token: Zeroizing::new(binding.expose_token().to_owned()),
            expires_at: format_rfc3339_ms(expires_ms),
            lease_expires_at: lease.expires_at,
        })
    }

    /// `auth.logout`: ends the lease for a correctly signed logout nonce.
    pub fn external_logout(
        &self,
        employee_id: &str,
        params: &SignedChallengeParams,
    ) -> Result<AuthLogoutResult, ApiError> {
        let lease = self.verify_signed_answer(employee_id, params, ChallengePurpose::Logout)?;
        let ended = self.end_external_lease(
            employee_id,
            Some(&lease.session_id),
            Some(employee_id),
            "logout",
        )?;
        if ended.is_some() {
            let _ = self.process_due_wakes_for(employee_id);
        }
        Ok(AuthLogoutResult {
            member_id: employee_id.to_owned(),
            session_id: lease.session_id,
            revoked: ended.is_some(),
        })
    }
}
