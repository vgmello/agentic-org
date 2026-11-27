//! Ordered, per-request authority checks. The worker supplies clock, MAC and live state.
use super::super::{live_owner, live_person, Binding, Shared};
use crate::revocations::Revocations;
use org_proto::{ApiError, Call, ErrorCode, Grant, OfficeInfo, OfficeRole, Request};
use std::sync::Mutex;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum AuthRefusal {
    Opaque(ErrorCode),
    Detailed(ApiError),
}

#[cfg(test)]
impl AuthRefusal {
    pub(super) fn code(&self) -> ErrorCode {
        match self {
            Self::Opaque(code) => *code,
            Self::Detailed(err) => err.code,
        }
    }
}

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

fn is_agent_session_read_method(method: &str) -> bool {
    matches!(
        method,
        org_proto::external_auth::MemberExternalSessions::METHOD
            | org_proto::external_auth::MemberExternalPending::METHOD
            | org_proto::sessions::MemberSessionList::METHOD
    )
}

#[cfg(test)]
pub(super) fn authorize(
    request: &Request,
    channel: &str,
    office: &OfficeInfo,
    checks: &impl Checks,
) -> Result<String, ErrorCode> {
    authorize_request(request, channel, office, checks).map_err(|refusal| refusal.code())
}

fn verify_grant_envelope(
    request: &Request,
    channel: &str,
    office: &OfficeInfo,
    checks: &impl Checks,
) -> Result<String, AuthRefusal> {
    let principal = request
        .principal()
        .ok()
        .flatten()
        .ok_or(AuthRefusal::Opaque(ErrorCode::NotAuthorized))?;
    let claims = &principal.grant.claims;
    if claims.channel_id != channel
        || !checks.verify(&principal.grant)
        || checks.now_ms().is_none_or(|now| now >= claims.exp)
    {
        return Err(AuthRefusal::Opaque(ErrorCode::NotAuthorized));
    }
    if claims.office_id != office.id || claims.org_id != office.organization_id {
        return Err(AuthRefusal::Opaque(ErrorCode::NotFound));
    }
    if checks.revoked(&claims.session_id) {
        return Err(AuthRefusal::Opaque(ErrorCode::NotAuthorized));
    }
    Ok(claims.member.clone())
}

fn authorize_agent_session_read(
    office: &OfficeInfo,
    member: &str,
    checks: &impl Checks,
) -> Result<String, AuthRefusal> {
    if let Some(person) = checks.person(member) {
        return Ok(person);
    }
    // Valid, unexpired, unrevoked grant for this office, but the caller is not a live
    // organization person: return an accurate authorization error rather than opaque refusal.
    let err = if office.role == OfficeRole::Satellite && member == office.id {
        ApiError::new(
            ErrorCode::NotAuthorized,
            "signed-in account is not linked to an active organization person; only the owner, a person who may hire, or the agent's manager or team lead may view agent sessions",
        )
        .with_hint(
            "Sign in with an account linked to an active organization person, or inspect sessions from HQ.",
        )
    } else {
        ApiError::new(
            ErrorCode::NotAuthorized,
            "only an active organization person (owner, hiring manager, or the agent's manager or team lead) may view agent sessions",
        )
        .with_hint(
            "Sign in as an active organization person with permission to view this agent's sessions.",
        )
    };
    Err(AuthRefusal::Detailed(err))
}

pub(super) fn authorize_request(
    request: &Request,
    channel: &str,
    office: &OfficeInfo,
    checks: &impl Checks,
) -> Result<String, AuthRefusal> {
    let member = verify_grant_envelope(request, channel, office, checks)?;
    if requires_owner(&request.method) {
        return checks
            .owner(&member)
            .ok_or(AuthRefusal::Opaque(ErrorCode::NotAuthorized));
    }
    if is_member_scoped_method(&request.method) {
        // Any live person: the office's attachment policy then decides file by file.
        return checks
            .person(&member)
            .ok_or(AuthRefusal::Opaque(ErrorCode::NotAuthorized));
    }
    if office.role == OfficeRole::Satellite
        && is_satellite_operator_method(&request.method)
        && member == office.id
    {
        // A satellite host's local instance administrator may not hold an org member link yet;
        // the host signs a local-only operator grant with `member == office.id`, admitted here
        // exclusively for read-only replication/org-office/provider discovery and office-local
        // runtime binding/readiness operations (never org-wide mutations).
        return Ok(member);
    }
    if is_agent_session_read_method(&request.method) {
        return authorize_agent_session_read(office, &member, checks);
    }
    checks
        .person(&member)
        .ok_or(AuthRefusal::Opaque(ErrorCode::NotAuthorized))
}

#[cfg(test)]
mod tests;
