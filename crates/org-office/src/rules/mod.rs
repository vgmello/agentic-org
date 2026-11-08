//! The organization's rules: what a command may do, and what it becomes.
//!
//! This is the domain half of the organization data (teams, members, memberships and the teams
//! working on a project; see `docs/design/teams-and-org-chart.md`). It has no storage in it.
//!
//! - [`fn@decide`] checks one command against the current state and returns the **resolved
//!   changes** it makes (T1's [`org_proto::org::OrgChange`]s: whole entities as they are
//!   afterwards, with their new versions), or the error HQ answers with. It never changes
//!   anything. The changes come in an order that is safe to apply one by one: what hangs off an
//!   entity (memberships, links) first, the entity itself last.
//! - [`fn@apply`] turns one change into writes, mechanically. It validates nothing: HQ calls it
//!   right after `decide`, in the same transaction that logs the entry, and a satellite calls
//!   it on a received entry, so the two cannot drift.
//!
//! State is read through [`OrgView`] and written through [`OrgWrite`], which `store.rs`
//! implements over a transaction (so a later command in the same batch sees an earlier
//! one's effects). Tests use an in-memory implementation.
//!
//! Errors follow the one precedence rule in `org_proto::org` (`not_found`, `invalid_request`,
//! `invalid_name`, `conflict`, `already_exists`). That rule does not place the domain codes
//! (`owner_protected`, `cycle`, `not_a_member`, `not_empty`); here they come after
//! `invalid_name` and before `conflict`, so a stale version never hides a rule that would
//! refuse the command whatever its version.

//! Limits the contract leaves to HQ: a client-made id is 1 to [`ID_MAX_CHARS`] ASCII letters,
//! digits, `-` or `_`; a contact and a title in a team are 1 to
//! [`org_proto::org::AGENT_TEXT_MAX_CHARS`] characters after trimming, without control
//! characters (all `invalid_request`). Setting a field to the value it has still raises the
//! version.

mod apply;
mod command;
mod decide;
mod view;

#[cfg(test)]
mod memory;
#[cfg(test)]
mod tests;

pub use apply::{apply, UnknownChange};
pub use command::OrgCommand;
pub use decide::{decide, name_key, owner_bootstrap};
pub use view::{OrgView, OrgWrite};

use org_proto::{ApiError, ErrorCode};

/// The longest client-made id, in characters.
pub const ID_MAX_CHARS: usize = 64;

pub(crate) fn fail(code: ErrorCode, message: &str) -> ApiError {
    ApiError::new(code, message)
}
