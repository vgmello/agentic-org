//! Ordered, per-request authority checks. The worker supplies clock, MAC and live state.
use super::super::{live_owner, live_person, Binding, Shared};
use crate::revocations::Revocations;
use org_proto::{Call, ErrorCode, Grant, OfficeInfo, OfficeRole, Request};
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
            | "organization.default_dod.set"
            | "member.session_approval"
            | "member.session_clear_stall"
            | "provider.enrollment_start"
            | "provider.enrollment_status"
            | "provider.enrollment_submit"
            | "provider.enrollment_cancel"
            | "provider.credential_status"
            | "provider.credential_revoke"
            | "cost.subscription_usage.list"
            | "cost.subscription_usage.collect"
            | "cost.subscription_usage.credential_set"
            | "cost.subscription_usage.credential_revoke"
            | "repo.credential_set"
            | "repo.credential_status"
            | "repo.credential_revoke"
    )
}

/// The attachment calls (`TASK-98.1`): never owner-only and never a satellite operator call;
/// a host channel serves them to any live person, and the office's `AuthorizationPolicy`
/// decides per file (a person's private space stays theirs alone).
fn is_member_scoped_method(method: &str) -> bool {
    org_proto::attachments::ATTACHMENT_METHODS.contains(&method)
}

fn is_satellite_operator_method(method: &str) -> bool {
    matches!(
        method,
        org_proto::federation::OfficeSyncStatus::METHOD
            | org_proto::federation::OfficeDirectoryList::METHOD
            | org_proto::providers::ProviderConnectionList::METHOD
            | org_proto::providers::ProviderBindingList::METHOD
            | org_proto::providers::ProviderBindingSet::METHOD
            | org_proto::providers::ProviderConnectionRefresh::METHOD
            | org_proto::hiring::ModelProfileList::METHOD
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
    } else if is_member_scoped_method(&request.method) {
        // Any live person: the office's attachment policy then decides file by file.
        checks
            .person(&claims.member)
            .ok_or(ErrorCode::NotAuthorized)
    } else if office.role == OfficeRole::Satellite
        && is_satellite_operator_method(&request.method)
        && claims.member == office.id
    {
        // A satellite host's local instance administrator may not hold an org member link yet;
        // the host signs a local-only operator grant with `member == office.id`, admitted here
        // exclusively for read-only replication/org-office/provider discovery and office-local
        // runtime binding/readiness operations (never org-wide mutations).
        Ok(claims.member.clone())
    } else {
        checks
            .person(&claims.member)
            .ok_or(ErrorCode::NotAuthorized)
    }
}

#[cfg(test)]
mod tests;
