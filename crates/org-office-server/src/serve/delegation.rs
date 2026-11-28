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
//!   `msg.wait`, `msg.notify`, `chat.events_poll`), long-polling an ask (`chat.wait_ask`), and
//!   cross-entity search (`search.query`, which would otherwise span every DM she is in and her
//!   private notes across callers).
//!   Refused even when she acts for the owner.
//! - **the caller is the owner**: organization-wide spend and budgets, structure changes,
//!   starting, stopping and reading other sessions (hers included), and any method not named
//!   below (fail closed).
//! - **the caller may hire**: the hire flow (`ADR-0029`), checked per team against the caller
//!   ([`scope`]): her organization-wide reach never widens the caller's.
//! - **open**: reads and messages any active member may make; still checked against her own
//!   authority by the method itself, and what a read answers is narrowed to what the caller
//!   may also see ([`scope`]): a DM is shown only to a participant of it. A call that acts on
//!   one conversation or message (`dm.open`, sending, asking, marking read, muting, reacting,
//!   replying, answering, triage) is refused when the caller is not in it; only for the owner
//!   does a message, ask or answer carry staged files (`TASK-240`).
//!
//! A file bound to a message, an ask or an answer is inspected, read, listed, edited or
//! re-attached only for a caller in its conversation (`TASK-240`), even for the owner: she is
//! in every member's DM with her, the owner is not.
//!
//! A run acting for a member who is no longer an active person may do nothing at all. A run
//! acting for nobody (a wake) gets only the open class, and no DM at all. When the office
//! cannot read what the gate needs, every bound member is refused (fail closed).

use org_command_catalogue::MutationPolicy;
use org_proto::ask::ChatAnswer;
use org_proto::chat::{ChatReact, ConversationKind, ConversationSummary};
use org_proto::msg::MsgReply;
use org_proto::{ApiError, ErrorCode, Request, Response};

use super::auth::Binding;
use super::shared::Shared;

mod scope;

pub(crate) use scope::check_delegated_attachment;

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
    /// The conversation a file bound to a message, an ask or an answer is in; `None` for any
    /// other file and for an unknown one (`TASK-240`).
    fn attachment_conversation(&self, attachment_id: &str) -> Result<Option<String>, ApiError>;
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
pub(crate) enum Access {
    Open,
    CallerHires,
    CallerOwner,
    Never,
}

const NEVER: &[&str] = &[
    "agent.resume",
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
    "member.external_bind",
    "member.external_pending",
    "member.external_renew",
    "member.external_revoke",
    "member.external_sessions",
    "member.session_approval",
    "member.set_may_hire",
    "msg.notify",
    "msg.read",
    "msg.wait",
    "office.metrics",
    "office.remove",
    "office.restart",
    "search.query",
    "suggestion.apply",
    "suggestion.dismiss",
];
const NEVER_PREFIXES: &[&str] = &[
    "auth.",
    "copilot.",
    "cost.subscription_usage.credential_",
    "office.invitation.",
    "provider.",
    "tasks.notes.",
    "tess.",
];
/// Read-only provider pool/quota queries permitted when acting for the live owner, checked before
/// `NEVER_PREFIXES` (`provider.`) so credentials, enrollment, and mutations stay `Access::Never`.
const CALLER_OWNER_OVERRIDES: &[&str] = &["provider.connection.list", "provider.quota.status"];
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
    "suggestion.create",
    "suggestion.inspect",
    "suggestion.list",
    "suggestion.withdraw",
    "ui.present",
    "who",
    "worktree.list",
];
const OPEN_PREFIXES: &[&str] = &["chat.", "dm.", "msg.", "skill.", "tasks."];

/// Read-only operator RPCs that execute with the verified live owner's actor identity after
/// [`gate`] proves `caller.active && caller.is_person && caller.is_owner` (`TASK-80.4`).
const OWNER_DELEGATED_READS: &[&str] = &[
    "budget.get",
    "budget.list",
    "cost.price.list",
    "cost.query",
    "cost.subscription_usage.list",
    "inbox.list",
    "member.agent_execution_get",
    "member.session_inspect",
    "member.session_list",
    "model_profile.list",
    "provider.connection.list",
    "provider.quota.status",
];

/// Non-CLI protocol and read-only wire calls outside `CommandTable::rows()`.
const NON_CLI_READ_ONLY: &[&str] = &[
    "attachment.inspect",
    "attachment.list",
    "attachment.read_chunk",
    "auth.whoami",
    "chat.conversations",
    "chat.inbox",
    "chat.messages",
    "chat.watching_model_get",
    "cost.export_csv",
    "docs.index",
    "docs.inspect",
    "docs.list",
    "hello",
    "member.session_transcript",
    "model_profile.models",
    "msg.bind",
    "msg.get",
    "msg.human_needs_you",
    "msg.threads",
    "operation.get",
    "process.view",
    "subscribe",
    "tasks.comments",
];

/// Non-CLI low-risk caller-reply / reaction wire calls outside `CommandTable::rows()`.
/// Restricted by [`enforce_mutation_policy`] to Tess's 1:1 DM with the active caller.
const NON_CLI_AUTO_SAFE: &[&str] = &["chat.answer", "chat.react"];

/// The class of `method`. A method named nowhere needs the owner.
pub(crate) fn access(method: &str) -> Access {
    let starts = |prefixes: &[&str]| prefixes.iter().any(|p| method.starts_with(p));
    if NEVER.contains(&method) {
        Access::Never
    } else if OPEN.contains(&method) {
        Access::Open
    } else if CALLER_OWNER_OVERRIDES.contains(&method) {
        Access::CallerOwner
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

/// Declarative mutation policy for an office RPC method (`TASK-80.4`), consulting the shared
/// `agent-os` [`org_command::CommandTable`] first and failing closed to `ConfirmRequired` for any
/// unknown method.
pub(super) fn mutation_policy_for_rpc(method: &str) -> MutationPolicy {
    if let Some(policy) = org_command_catalogue::runtime_table().mutation_policy_for_call(method) {
        return policy;
    }
    if NON_CLI_READ_ONLY.contains(&method) {
        MutationPolicy::ReadOnly
    } else if NON_CLI_AUTO_SAFE.contains(&method) {
        MutationPolicy::AutoSafe
    } else {
        MutationPolicy::ConfirmRequired
    }
}

/// Returns the verified live owner's member ID for read-only operator RPCs delegated through
/// Tess while acting for the owner, so owner-only read endpoints authorize her read without
/// widening any write or non-owner path.
pub(super) fn delegated_owner_read_actor<'a>(
    request: &Request,
    delegation: Option<&'a Delegation>,
) -> Option<&'a str> {
    let delegation = delegation?;
    if !(delegation.caller.active && delegation.caller.is_person && delegation.caller.is_owner) {
        return None;
    }
    let method = request.method.as_str();
    if !OWNER_DELEGATED_READS.contains(&method) {
        return None;
    }
    if (method == "member.session_inspect" || method == "member.session_list")
        && request
            .params
            .get("employee_id")
            .and_then(serde_json::Value::as_str)
            .is_none()
    {
        return None;
    }
    delegation.caller_id.as_deref()
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
    enforce_method_mutation_policy(request)?;
    if let Some(caller_id) = delegation.caller_id.as_deref() {
        enforce_autosafe_target_policy(request, port, member, caller_id)?;
    } else if is_caller_dm_only_reply(&request.method) {
        return Err(policy_violation(request.id, &request.method));
    }
    scope::admit(request, port, member, &delegation)?;
    Ok(Some(delegation))
}

fn enforce_method_mutation_policy(request: &Request) -> Result<(), Response> {
    match mutation_policy_for_rpc(&request.method) {
        MutationPolicy::ReadOnly | MutationPolicy::AutoSafe => Ok(()),
        MutationPolicy::ConfirmRequired => Err(policy_violation(request.id, &request.method)),
        MutationPolicy::ForbiddenForAgent => Err(denied(request.id)),
    }
}

fn enforce_autosafe_target_policy(
    request: &Request,
    port: &dyn ChiefOfStaffPort,
    tess_id: &str,
    caller_id: &str,
) -> Result<(), Response> {
    if mutation_policy_for_rpc(&request.method) == MutationPolicy::AutoSafe
        && is_caller_dm_only_reply(&request.method)
        && !targets_caller_direct_dm(request, port, tess_id, caller_id)?
    {
        return Err(policy_violation(request.id, &request.method));
    }
    Ok(())
}

#[cfg(test)]
fn enforce_mutation_policy(
    request: &Request,
    port: &dyn ChiefOfStaffPort,
    tess_id: &str,
    caller_id: &str,
) -> Result<(), Response> {
    enforce_method_mutation_policy(request)?;
    enforce_autosafe_target_policy(request, port, tess_id, caller_id)
}

fn is_caller_dm_only_reply(method: &str) -> bool {
    matches!(method, "msg.reply" | "chat.answer" | "chat.react")
}

/// Verifies that a `msg.reply`, `chat.answer`, or `chat.react` targets a message inside Tess's
/// 1:1 DM with `caller_id` (exact active participants `{tess_id, caller_id}`, never a channel or
/// multi-person group DM). Fails closed on malformed parameters, unknown messages, or port errors.
fn targets_caller_direct_dm(
    request: &Request,
    port: &dyn ChiefOfStaffPort,
    tess_id: &str,
    caller_id: &str,
) -> Result<bool, Response> {
    let Some(message_id) = target_message_id(request) else {
        return Ok(false);
    };
    let Some(conversation_id) = port
        .message_conversation(&message_id)
        .map_err(|err| Response::error(request.id, err))?
    else {
        return Ok(false);
    };
    let conversations = port
        .visible_conversations(caller_id)
        .map_err(|err| Response::error(request.id, err))?;
    Ok(conversations
        .iter()
        .find(|c| c.id == conversation_id)
        .is_some_and(|c| is_exact_one_to_one_dm(c, tess_id, caller_id)))
}

fn target_message_id(request: &Request) -> Option<String> {
    match request.method.as_str() {
        "msg.reply" => request.params::<MsgReply>().ok().map(|p| p.to_id),
        "chat.answer" => request.params::<ChatAnswer>().ok().map(|p| p.ask_id),
        "chat.react" => request.params::<ChatReact>().ok().map(|p| p.message_id),
        _ => None,
    }
}

fn is_exact_one_to_one_dm(
    conversation: &ConversationSummary,
    tess_id: &str,
    caller_id: &str,
) -> bool {
    if conversation.kind != ConversationKind::Dm || tess_id == caller_id {
        return false;
    }
    let mut has_tess = false;
    let mut has_caller = false;
    for member in conversation.members.iter().filter(|m| m.left_at.is_none()) {
        if member.employee_id == tess_id {
            has_tess = true;
        } else if member.employee_id == caller_id {
            has_caller = true;
        } else {
            return false;
        }
    }
    has_tess && has_caller
}

fn policy_violation(id: u64, method: &str) -> Response {
    Response::error(
        id,
        ApiError::policy_violation(
            format!(
                "direct mutation `{method}` requires caller confirmation when acting for a member"
            ),
            format!(
                "Inspect the target record and revision first, then propose this change with `agent-os suggestion create` instead of calling `{method}` directly."
            ),
        ),
    )
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
