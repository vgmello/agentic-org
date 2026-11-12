//! Ordered, per-request authority checks. The worker supplies clock, MAC and live state.
use super::super::{live_owner, Binding, LaunchAuth, Shared};
use crate::revocations::Revocations;
use org_proto::{ErrorCode, Grant, OfficeInfo, Request};
use std::sync::Mutex;

pub(super) struct Live<'a> {
    pub shared: &'a Shared,
    pub revocations: &'a Mutex<Revocations>,
}
impl Checks for Live<'_> {
    fn verify(&self, grant: &Grant) -> bool {
        match &self.shared.auth {
            LaunchAuth::Grants { _key } => crate::grant::verify(_key, grant),
            LaunchAuth::Legacy(_) => false,
        }
    }
    fn now_ms(&self) -> Option<u64> {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|time| u64::try_from(time.as_millis()).ok())
    }
    fn revoked(&self, session: &str) -> bool {
        self.revocations
            .lock()
            .map(|state| state.contains(session))
            .unwrap_or(true)
    }
    fn owner(&self, id: &str) -> Option<String> {
        let binding = Binding::Member {
            id: id.to_owned(),
            with_app_token: true,
        };
        matches!(live_owner(self.shared, &binding), Ok(true)).then(|| id.to_owned())
    }
}

pub(super) trait Checks {
    fn verify(&self, grant: &Grant) -> bool;
    fn now_ms(&self) -> Option<u64>;
    fn revoked(&self, session: &str) -> bool;
    fn owner(&self, member: &str) -> Option<String>;
}

pub(super) fn authorize(
    request: &Request,
    channel: &str,
    office: &OfficeInfo,
    checks: &impl Checks,
) -> Result<String, ErrorCode> {
    let principal = request
        .principal()
        .ok()
        .flatten()
        .ok_or(ErrorCode::NotAuthorized)?;
    let claims = &principal.grant.claims;
    if claims.channel_id != channel {
        return Err(ErrorCode::NotAuthorized);
    }
    if !checks.verify(&principal.grant) {
        return Err(ErrorCode::NotAuthorized);
    }
    if checks.now_ms().is_none_or(|now| now >= claims.exp) {
        return Err(ErrorCode::NotAuthorized);
    }
    if claims.office_id != office.id || claims.org_id != office.organization_id {
        return Err(ErrorCode::NotFound);
    }
    if checks.revoked(&claims.session_id) {
        return Err(ErrorCode::NotAuthorized);
    }
    checks.owner(&claims.member).ok_or(ErrorCode::NotAuthorized)
}

#[cfg(test)]
mod tests;
