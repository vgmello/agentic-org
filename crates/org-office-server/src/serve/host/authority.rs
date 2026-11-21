//! Ordered, per-request authority checks. The worker supplies clock, MAC and live state.
use super::super::{live_owner, live_person, Binding, Shared};
use crate::revocations::Revocations;
use org_proto::{ErrorCode, Grant, OfficeInfo, Request};
use std::sync::Mutex;

pub(super) struct Live<'a> {
    pub shared: &'a Shared,
    pub revocations: &'a Mutex<Revocations>,
}
impl Checks for Live<'_> {
    fn verify(&self, grant: &Grant) -> bool {
        self.shared.auth.verifies(grant)
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
            via_grant: true,
        };
        matches!(live_owner(self.shared, &binding), Ok(true)).then(|| id.to_owned())
    }
    fn person(&self, id: &str) -> Option<String> {
        let binding = Binding::Member {
            id: id.to_owned(),
            via_grant: true,
        };
        matches!(live_person(self.shared, &binding), Ok(true)).then(|| id.to_owned())
    }
}

pub(super) trait Checks {
    fn verify(&self, grant: &Grant) -> bool;
    fn now_ms(&self) -> Option<u64>;
    fn revoked(&self, session: &str) -> bool;
    fn owner(&self, member: &str) -> Option<String>;
    fn person(&self, member: &str) -> Option<String> {
        self.owner(member)
    }
}

/// Organization structure mutations and the owner's inbox require the live organization owner
/// (`doc-78` / `TASK-38`). Reads and member-scoped calls allow any active `Person` member.
fn requires_owner(method: &str) -> bool {
    matches!(
        method,
        "inbox.list"
            | "team.create"
            | "team.rename"
            | "team.move"
            | "team.set_lead"
            | "team.delete"
            | "team.set_title"
            | "member.add_person"
            | "member.update_person"
            | "member.add_agent"
            | "member.update_agent"
            | "member.set_agent_status"
            | "member.offboard"
            | "member.remove"
            | "member.set_reports_to"
            | "member.move"
            | "member.set_run_mode"
            | "project.create"
            | "project.add_team"
            | "project.remove_team"
    )
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
    if requires_owner(&request.method) {
        checks.owner(&claims.member).ok_or(ErrorCode::NotAuthorized)
    } else {
        checks
            .person(&claims.member)
            .ok_or(ErrorCode::NotAuthorized)
    }
}

#[cfg(test)]
mod tests;
