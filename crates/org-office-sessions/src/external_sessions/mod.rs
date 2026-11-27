//! `SessionManager` external (BYOA) lease lifecycle and workspace sign-in orchestration
//! (`TASK-137`, `TASK-211`).
//!
//! Separated from [`crate::external`] (which holds the leaf lease record, RAM-only pending login
//! / nonce state, and pure signature/validation helpers) so [`crate::recovery`] and
//! [`crate::manager`] can depend on [`crate::external`] without introducing a module cycle.

use org_proto::external_auth::{
    ExternalSessionView, MemberExternalBindParams, DEFAULT_LEASE_TTL_SECS, MAX_LEASE_TTL_SECS,
    MIN_LEASE_TTL_SECS,
};
use org_proto::{ApiError, ErrorCode};

use crate::external::{
    decode_pubkey, refuse_builtin_agent, validate_optional_hostname,
    validate_optional_workspace_path, validate_workspace_label,
};
use crate::manager::SessionManager;

mod lease;
mod pending;
mod signin;

impl SessionManager {
    /// [`refuse_builtin_agent`] by employee ID; an unreadable record is refused too.
    pub(crate) fn refuse_builtin_agent_id(&self, employee_id: &str) -> Result<(), ApiError> {
        match self.office.load_employee(employee_id) {
            Ok(Some(employee)) => refuse_builtin_agent(&employee),
            Ok(None) => Ok(()),
            Err(_) => Err(ApiError::new(
                ErrorCode::StorageError,
                "could not read the agent's record",
            )),
        }
    }

    /// `member.external_bind`: atomically authorizes and installs an external workspace lease for
    /// `employee_id` in one operator call (`TASK-211`), bypassing `PendingLogins` entirely so
    /// unauthenticated pending-login traffic can never DoS binding or leak approval codes.
    pub fn external_bind(
        &self,
        actor: &str,
        employee_id: &str,
        params: &MemberExternalBindParams,
    ) -> Result<ExternalSessionView, ApiError> {
        let ttl = params.ttl_secs.unwrap_or(DEFAULT_LEASE_TTL_SECS);
        if !(MIN_LEASE_TTL_SECS..=MAX_LEASE_TTL_SECS).contains(&ttl) {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                format!("--ttl must be between {MIN_LEASE_TTL_SECS}s and {MAX_LEASE_TTL_SECS}s"),
            ));
        }
        if decode_pubkey(&params.pubkey_ed25519_b64).is_none() {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "pubkey_ed25519_b64 must be a base64 Ed25519 public key (32 bytes)",
            ));
        }
        let label = validate_workspace_label(&params.workspace_label)?;
        let hostname = validate_optional_hostname(params.hostname.as_deref())?;
        let workspace_path = validate_optional_workspace_path(params.workspace_path.as_deref())?;
        self.authorize_owner_or_person_inner(actor, false)?;
        let _serial = self
            .external
            .lease_mutation
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let employee = self.require_agent(employee_id)?;
        refuse_builtin_agent(&employee)?;
        self.authorize_external_operator(actor, &employee)?;
        let employee = self.require_active_agent(employee_id)?;
        self.commit_external_lease(lease::LeaseCommit {
            actor,
            employee: &employee,
            pubkey_ed25519_b64: params.pubkey_ed25519_b64.trim(),
            workspace_label: &label,
            hostname: hostname.as_deref(),
            workspace_path: workspace_path.as_deref(),
            ttl,
            takeover: params.takeover,
            verb: "bind",
        })
    }
}
