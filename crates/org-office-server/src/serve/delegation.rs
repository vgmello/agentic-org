//! `caller ∩ tess` (`TASK-80.1`, `doc-97`): what the built-in Chief of Staff may do on the
//! office socket is what her own authority allows **and** what the member her run acts for may
//! do, checked live on every call, here, before any method runs.
//!
//! Her run's binding names the member it acts for, in the office's RAM only
//! (`RunBindingRegistry::acting_for_caller_id`): no flag or parameter of a call can name or
//! change it. Each method is in one class:
//!
//! - **never**: host secrets, provider credentials and enrollment, invitations, office removal,
//!   external leases, answering approvals, changing her own execution or hiring power, and
//!   reading chats as the owner. Refused even when she acts for the owner.
//! - **the caller is the owner**: organization-wide spend and budgets, structure changes,
//!   starting and stopping other agents' sessions, and any method not named below (fail
//!   closed).
//! - **the caller may hire**: the hire flow (`ADR-0029`); her own `may_hire` is checked again by
//!   the hire itself.
//! - **open**: reads and messages any active member may make; still checked against her own
//!   authority by the method itself (another agent's private notes, a DM she is not in and the
//!   owner's inbox stay refused there).
//!
//! A run acting for a member who is no longer an active person may do nothing at all. A run
//! acting for nobody (a wake) gets only the open class.

use org_proto::{ApiError, ErrorCode, Request, Response};

use super::auth::Binding;
use super::shared::Shared;

/// The refusal's text: it names the rule, never the caller.
pub(super) const DELEGATION_DENIED: &str =
    "the Chief of Staff can't do this for the member she is acting for";

/// Who the bound Chief of Staff run acts for, read live for one call.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Delegation {
    /// The member the run acts for; `None` for a run that acts for nobody.
    pub caller_id: Option<String>,
    /// That member's live authority (none at all for an unknown or absent member).
    pub caller: org_office::CallerAuthority,
}

/// What a method needs of the member the Chief of Staff acts for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Access {
    Open,
    CallerHires,
    CallerOwner,
    Never,
}

const NEVER: &[&str] = &[
    "chat.read_as_owner",
    "chat.watching_model_set",
    "cost.price.set",
    "cost.usage.correct",
    "cost.usage.record",
    "git.push",
    "member.agent_execution_set",
    "member.external_approve",
    "member.external_revoke",
    "member.external_sessions",
    "member.session_approval",
    "member.set_may_hire",
    "office.remove",
    "office.restart",
];
const NEVER_PREFIXES: &[&str] = &[
    "auth.",
    "cost.subscription_usage.credential_",
    "office.invitation.",
    "provider.",
];
const CALLER_HIRES: &[&str] = &[
    "hr.hire",
    "hr.hire.confirm",
    "hr.hire.draft",
    "hr.hire.withdraw",
];
const OPEN: &[&str] = &[
    "budget.agent_own",
    "hello",
    "hr.hire.inspect",
    "hr.hire.list",
    "member.agent_readiness",
    "member.session_grants",
    "member.session_inspect",
    "member.session_list",
    "msg.bind",
    "office.directory",
    "office.sync_status",
    "operation.get",
    "org.snapshot",
    "process.effective",
    "process.show",
    "process.view",
    "process.why",
    "project.inspect",
    "project.list",
    "repo.list",
    "repo.status",
    "role.inspect",
    "role.list",
    "status.set",
    "subscribe",
    "who",
    "worktree.list",
];
const OPEN_PREFIXES: &[&str] = &["chat.", "dm.", "msg.", "skill.", "tasks.", "work."];

/// The class of `method`. A method named nowhere needs the owner.
pub(super) fn access(method: &str) -> Access {
    let starts = |prefixes: &[&str]| prefixes.iter().any(|p| method.starts_with(p));
    if NEVER.contains(&method) || starts(NEVER_PREFIXES) {
        Access::Never
    } else if CALLER_HIRES.contains(&method) {
        Access::CallerHires
    } else if OPEN.contains(&method) || starts(OPEN_PREFIXES) {
        Access::Open
    } else {
        Access::CallerOwner
    }
}

/// Whether a run with `delegation` may call a method of class `access`.
pub(super) fn allowed(access: Access, delegation: &Delegation) -> bool {
    let caller = &delegation.caller;
    let live_person = caller.active && caller.is_person;
    match access {
        Access::Never => false,
        Access::Open => delegation.caller_id.is_none() || live_person,
        Access::CallerHires => live_person && caller.may_hire,
        Access::CallerOwner => live_person && caller.is_owner,
    }
}

/// The refusal of `request` when the bound member is the Chief of Staff and the member her run
/// acts for (or her acting for nobody) does not allow it; `None` for everyone else.
pub(super) fn refusal(request: &Request, shared: &Shared, binding: &Binding) -> Option<Response> {
    if binding.via_grant() {
        return None;
    }
    let member = binding.member()?;
    let port = shared.ports.delegation.as_ref()?;
    let delegation = port(member)?;
    if allowed(access(&request.method), &delegation) {
        return None;
    }
    Some(Response::error(
        request.id,
        ApiError::new(ErrorCode::NotAuthorized, DELEGATION_DENIED),
    ))
}

#[cfg(test)]
mod tests;
