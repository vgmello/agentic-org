//! Engine-neutral multi-person account types and one-time link subject helpers (`TASK-38` / A6).

use crate::accounts::identity::AccountId;
use crate::links::{Refused, Subject};
use crate::sessions::Ended;

pub const INVITE_PREFIX: &str = "invite:";
pub const ACCOUNT_PREFIX: &str = "account:";

/// Builds the canonical registration-link subject `"invite:<org_id>:<member_id>"`.
pub fn invite_subject(org_id: &str, member_id: &str) -> Result<Subject, crate::links::Error> {
    Subject::new(&format!("{INVITE_PREFIX}{org_id}:{member_id}"))
}

/// Builds the canonical set-password link subject `"account:<account_id>"`.
pub fn account_subject(account_id: &AccountId) -> Result<Subject, crate::links::Error> {
    Subject::new(&format!("{ACCOUNT_PREFIX}{}", account_id.as_str()))
}

/// Parses `"invite:<org_id>:<member_id>"` into `(org_id, member_id)`.
pub fn parse_invite_subject(subject: &str) -> Option<(&str, &str)> {
    let rest = subject.strip_prefix(INVITE_PREFIX)?;
    let (org_id, member_id) = rest.split_once(':')?;
    if org_office_launch::is_storage_id(org_id) && org_proto::is_valid_member_id(member_id) {
        Some((org_id, member_id))
    } else {
        None
    }
}

/// Parses `"account:<account_id>"` into [`AccountId`].
pub fn parse_account_subject(subject: &str) -> Option<AccountId> {
    let raw = subject.strip_prefix(ACCOUNT_PREFIX)?;
    AccountId::parse(raw).ok()
}

/// Refusal or storage failure when issuing an organization invitation link.
#[derive(Debug, PartialEq, Eq)]
pub enum InviteIssueError {
    AlreadyLinked,
    AuthDisabled,
    Invalid,
    Store,
}

/// Outcome of redeeming an organization invitation link to register a new account.
#[derive(Debug, PartialEq, Eq)]
pub enum InviteRegistration {
    Registered {
        account_id: AccountId,
        org_id: String,
        member_id: String,
    },
    Link(Refused),
    NotAnInvite,
    AlreadyLinked,
    LoginTaken,
}

/// Outcome of redeeming an organization invitation link for an existing account (SIGN-35).
#[derive(Debug, PartialEq, Eq)]
pub enum InviteAttach {
    Attached { org_id: String, member_id: String },
    Link(Refused),
    NotAnInvite,
    AlreadyLinked,
    AlreadyMember,
    AccountUnavailable,
}

/// Refusal or storage failure when issuing a member password-reset link.
#[derive(Debug, PartialEq, Eq)]
pub enum MemberResetIssueError {
    NotLinked,
    Store,
}

/// Outcome of redeeming a one-time `set_password` link.
#[derive(Debug, PartialEq, Eq)]
pub enum SetPasswordOutcome {
    Updated {
        account_id: AccountId,
        login: String,
        ended: Vec<Ended>,
    },
    Link(Refused),
}

/// Outcome of removing a member's sign-in access from an organization.
#[derive(Debug, PartialEq, Eq)]
pub enum RemoveAccessOutcome {
    Removed {
        account_id: Option<AccountId>,
        ended: Vec<Ended>,
    },
    IsOwner,
    NotFound,
}

/// Outcome of transferring organization ownership to another active member account.
#[derive(Debug, PartialEq, Eq)]
pub enum OrgOwnerHandoverOutcome {
    Transferred {
        previous_member_id: String,
        to_account_id: AccountId,
        to_member_id: String,
    },
    NotOwner,
    InvalidTarget,
}

/// Outcome of transferring the single instance administrator role to another active account.
#[derive(Debug, PartialEq, Eq)]
pub enum AdminHandoverOutcome {
    Transferred,
    NotAdmin,
    InvalidTarget,
}
