//! `caller ∩ tess` (`TASK-80.1`, `doc-97`): what the built-in Chief of Staff may do on the
//! office socket is what her own authority allows **and** what the member her run acts for may
//! do, checked live on every call, here, before any method runs, and again on what it answers.
//!
//! Her run's binding names the member it acts for, in the office's RAM only
//! (`RunBindingRegistry::acting_for_caller_id`): no flag or parameter of a call can name or
//! change it. Each method is in one class:
//!
//! - **never**: host secrets, provider credentials and enrollment, invitations, office removal,
//!   external leases, answering approvals, changing her own execution or hiring power, reading
//!   chats as the owner, her private task notes (one bucket every caller's turns would share),
//!   the mailbox reads that consume or summarise every message at once (`msg.read`,
//!   `msg.wait`, `msg.notify`, `chat.events_poll`), and long-polling an ask (`chat.wait_ask`).
//!   Refused even when she acts for the owner.
//! - **the caller is the owner**: organization-wide spend and budgets, structure changes,
//!   starting, stopping and reading other sessions (hers included), and any method not named
//!   below (fail closed).
//! - **the caller may hire**: the hire flow (`ADR-0029`), checked per team against the caller
//!   ([`scope`]): her organization-wide reach never widens the caller's.
//! - **open**: reads and messages any active member may make; still checked against her own
//!   authority by the method itself, and what a read answers is narrowed to what the caller
//!   may also see ([`scope`]): a DM is shown only to a participant of it. A call that acts on
//!   one conversation or message (`dm.open`, marking read, muting, reacting, replying,
//!   answering, triage) is refused when the caller is not in it.
//!
//! A run acting for a member who is no longer an active person may do nothing at all. A run
//! acting for nobody (a wake) gets only the open class, and no DM at all. When the office
//! cannot read what the gate needs, every bound member is refused (fail closed).

use org_proto::chat::ConversationSummary;
use org_proto::{ApiError, ErrorCode, Request, Response};

use super::auth::Binding;
use super::shared::Shared;

mod scope;

/// The refusal's text: it names the rule, never the caller.
pub(super) const DELEGATION_DENIED: &str =
    "the Chief of Staff can't do this for the member she is acting for";

/// What the gate reads from the office for the Chief of Staff's calls (`TASK-80.1`). Every read
/// fails closed: an error refuses the call.
pub trait ChiefOfStaffPort: Send + Sync {
    /// Who `member`'s live run acts for when `member` is the Chief of Staff; `Ok(None)` for
    /// every other member. An error (her identity unreadable) refuses every bound member.
    fn delegation(&self, member: &str) -> Result<Option<Delegation>, ApiError>;
    /// Whether `caller` may hire into `team_id` right now (`ADR-0029` decision 2).
    fn caller_may_hire_into(&self, caller: &str, team_id: &str) -> Result<bool, ApiError>;
    /// The team hire request `request_id` hires into; `None` for an unknown request.
    fn hire_team(&self, request_id: &str) -> Result<Option<String>, ApiError>;
    /// Every conversation `member` may see: channels they reach and DMs they are in (for the
    /// owner, every DM in the organization).
    fn visible_conversations(&self, member: &str) -> Result<Vec<ConversationSummary>, ApiError>;
    /// The conversation message `message_id` (a chat or mailbox message, or an ask) is in;
    /// `None` for an unknown message.
    fn message_conversation(&self, message_id: &str) -> Result<Option<String>, ApiError>;
    /// The member a `dm.open` participant reference (an ID, handle, name or `owner`) names,
    /// read without writing anything; `None` when it names nobody.
    fn resolve_member(&self, reference: &str) -> Result<Option<String>, ApiError>;
    /// Whether `member` is the Chief of Staff; `true` when her identity cannot be read (fail
    /// closed), and only ever used to withhold something from her.
    fn is_chief_of_staff(&self, member: &str) -> bool;
    /// Records that the Chief of Staff drafted hire `request_id` acting for `caller` (RAM only).
    fn record_draft(&self, request_id: &str, caller: &str);
    /// The member the Chief of Staff drafted hire `request_id` for, while this office runs.
    fn draft_caller(&self, request_id: &str) -> Option<String>;
}

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
    "chat.events_poll",
    "chat.read_as_owner",
    "chat.wait_ask",
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
    "msg.notify",
    "msg.read",
    "msg.wait",
    "office.remove",
    "office.restart",
];
const NEVER_PREFIXES: &[&str] = &[
    "auth.",
    "cost.subscription_usage.credential_",
    "office.invitation.",
    "provider.",
    "tasks.notes.",
];
const CALLER_HIRES: &[&str] = &[
    "hr.hire",
    "hr.hire.confirm",
    "hr.hire.draft",
    "hr.hire.inspect",
    "hr.hire.list",
    "hr.hire.withdraw",
];
/// Open methods named exactly; checked before the never-prefixes (`auth.whoami` is her own
/// identity, not a sign-in).
const OPEN: &[&str] = &[
    "auth.whoami",
    "budget.agent_own",
    "hello",
    "member.agent_readiness",
    "member.session_grants",
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
    if NEVER.contains(&method) {
        Access::Never
    } else if OPEN.contains(&method) {
        Access::Open
    } else if starts(NEVER_PREFIXES) {
        Access::Never
    } else if CALLER_HIRES.contains(&method) {
        Access::CallerHires
    } else if starts(OPEN_PREFIXES) {
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

/// The Chief of Staff's gate for one call, before any method runs: `Ok(None)` for every member
/// but her (and for a person on a grant); `Ok(Some(delegation))` when what she acts for allows
/// the call, which then runs and is narrowed by [`narrow`]; `Err(refusal)` otherwise, and for
/// every bound member when the office cannot read her delegation (fail closed).
pub(super) fn gate(
    request: &Request,
    shared: &Shared,
    binding: &Binding,
) -> Result<Option<Delegation>, Response> {
    if binding.via_grant() {
        return Ok(None);
    }
    let (Some(member), Some(port)) = (binding.member(), shared.ports.delegation.as_deref()) else {
        return Ok(None);
    };
    let delegation = match port.delegation(member) {
        Ok(Some(delegation)) => delegation,
        Ok(None) => return Ok(None),
        Err(err) => return Err(Response::error(request.id, err)),
    };
    if !allowed(access(&request.method), &delegation) {
        return Err(denied(request.id));
    }
    scope::admit(request, port, member, &delegation)?;
    Ok(Some(delegation))
}

/// Narrows what a call the gate admitted answers to what the member her run acts for may also
/// see ([`scope`]).
pub(super) fn narrow(
    request: &Request,
    shared: &Shared,
    binding: &Binding,
    delegation: &Delegation,
    response: Response,
) -> Response {
    match (binding.member(), shared.ports.delegation.as_deref()) {
        (Some(member), Some(port)) => scope::narrow(request, port, member, delegation, response),
        _ => response,
    }
}

/// The gate's refusal of `id`.
pub(super) fn denied(id: u64) -> Response {
    Response::error(
        id,
        ApiError::new(ErrorCode::NotAuthorized, DELEGATION_DENIED),
    )
}

#[cfg(test)]
mod office_tests;
#[cfg(test)]
pub(super) mod tests;
