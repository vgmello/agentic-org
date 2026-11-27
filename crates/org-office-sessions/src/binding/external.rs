//! External (BYOA) fast tokens in the [`RunBindingRegistry`] (`TASK-137`).
//!
//! An entry exists only while its lease is active: approving a new lease, revoking, logging out,
//! pausing or offboarding removes it. Every check is RAM-only and constant-time on the token, so
//! a bound hot-path command costs no key-store call, no disk read and no signature.

use super::{
    constant_time_eq, AcceptedCommandRecord, ActiveRunBinding, BindingMode, BindingRejection,
    RunBindingRegistry,
};

/// Most accepted commands kept in RAM per external binding. Never written to disk per command:
/// the bound hot path does no I/O (`doc-7` #18).
const MAX_EXTERNAL_ACCEPTED_COMMANDS: usize = 512;

impl RunBindingRegistry {
    /// Mints a fresh fast token for `employee_id`'s external lease `session_id`, valid until
    /// `expires_at_ms`, replacing any earlier external token for that employee.
    pub fn issue_external(
        &self,
        employee_id: impl Into<String>,
        session_id: impl Into<String>,
        expires_at_ms: u64,
    ) -> Result<ActiveRunBinding, String> {
        let token = Self::mint_token()?;
        let session_id = session_id.into();
        let binding = ActiveRunBinding {
            token,
            employee_id: employee_id.into(),
            run_id: session_id.clone(),
            session_id,
            mode: BindingMode::External { expires_at_ms },
            accepted_commands: Vec::new(),
            // An external lease never acts for anyone (the Chief of Staff gets none).
            acting_for_caller_id: None,
        };
        let mut map = self.external.lock().unwrap_or_else(|p| p.into_inner());
        map.insert(binding.employee_id.clone(), binding.clone());
        Ok(binding)
    }

    /// Verifies `presented_token` against `employee_id`'s external fast token at `now_ms`:
    /// constant-time match and unexpired. A same-UID peer is the socket's accept gate.
    pub fn verify_external(
        &self,
        employee_id: &str,
        presented_token: Option<&str>,
        now_ms: u64,
    ) -> Result<ActiveRunBinding, BindingRejection> {
        let Some(token) = presented_token.filter(|t| !t.is_empty()) else {
            return Err(BindingRejection::MissingToken);
        };
        let binding = {
            let map = self.external.lock().unwrap_or_else(|p| p.into_inner());
            map.get(employee_id).cloned()
        };
        let Some(binding) = binding else {
            return Err(BindingRejection::UnknownToken);
        };
        if binding.employee_id != employee_id {
            return Err(BindingRejection::MemberMismatch);
        }
        if !constant_time_eq(binding.token.as_bytes(), token.as_bytes()) {
            return Err(BindingRejection::UnknownToken);
        }
        match binding.mode {
            BindingMode::External { expires_at_ms } if now_ms < expires_at_ms => Ok(binding),
            BindingMode::External { .. } => Err(BindingRejection::Expired),
            BindingMode::Managed { .. } => Err(BindingRejection::UnknownToken),
        }
    }

    /// Whether `employee_id` still holds the unexpired external token of lease `session_id`: the
    /// per-call liveness check of an externally bound connection.
    pub fn external_active(&self, employee_id: &str, session_id: &str, now_ms: u64) -> bool {
        self.external_expiry(employee_id, session_id)
            .is_some_and(|expires_at_ms| now_ms < expires_at_ms)
    }

    /// The expiry of `employee_id`'s external token for lease `session_id`, if it holds one.
    pub fn external_expiry(&self, employee_id: &str, session_id: &str) -> Option<u64> {
        let map = self.external.lock().unwrap_or_else(|p| p.into_inner());
        map.get(employee_id)
            .filter(|b| b.session_id == session_id)
            .and_then(|b| match b.mode {
                BindingMode::External { expires_at_ms } => Some(expires_at_ms),
                BindingMode::Managed { .. } => None,
            })
    }

    /// Whether `employee_id` holds any unexpired external token.
    pub fn has_active_external(&self, employee_id: &str, now_ms: u64) -> bool {
        let map = self.external.lock().unwrap_or_else(|p| p.into_inner());
        map.get(employee_id).is_some_and(
            |b| matches!(b.mode, BindingMode::External { expires_at_ms } if now_ms < expires_at_ms),
        )
    }

    /// Revokes `employee_id`'s external token (only when its lease is `session_id`, if given).
    /// Answers whether a token was removed.
    pub fn revoke_external(&self, employee_id: &str, session_id: Option<&str>) -> bool {
        let mut map = self.external.lock().unwrap_or_else(|p| p.into_inner());
        let matches = map
            .get(employee_id)
            .is_some_and(|b| session_id.is_none_or(|id| b.session_id == id));
        if matches {
            map.remove(employee_id);
        }
        matches
    }

    /// Accepted commands recorded on `employee_id`'s external binding.
    pub fn external_accepted_commands(&self, employee_id: &str) -> Vec<AcceptedCommandRecord> {
        let map = self.external.lock().unwrap_or_else(|p| p.into_inner());
        map.get(employee_id)
            .map(|b| b.accepted_commands.clone())
            .unwrap_or_default()
    }

    pub(super) fn record_external_command(
        &self,
        employee_id: &str,
        record: AcceptedCommandRecord,
    ) -> Option<BindingMode> {
        let mut map = self.external.lock().unwrap_or_else(|p| p.into_inner());
        let binding = map.get_mut(employee_id)?;
        if binding.accepted_commands.len() < MAX_EXTERNAL_ACCEPTED_COMMANDS {
            binding.accepted_commands.push(record);
        }
        Some(binding.mode)
    }
}

#[cfg(test)]
mod tests;
