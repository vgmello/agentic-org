//! The organization's rules: what a command may do, and what it becomes.
//!
//! This is the domain half of the organization data (teams, members, memberships and the teams
//! working on a project; see `docs/design/teams-and-org-chart.md`). It has no storage in it.
//!
//! - [`decide`] checks one command against the current state and returns the **resolved
//!   changes** it makes (T1's [`OrgChange`]s: whole entities as they are afterwards, with their
//!   new versions), or the error HQ answers with. It never changes anything.
//! - [`apply`] turns one change into writes, mechanically. It validates nothing: HQ calls it
//!   right after `decide`, in the same transaction that logs the entry, and a satellite calls
//!   it on a received entry, so the two cannot drift.
//!
//! State is read through [`OrgView`] and written through [`OrgWrite`], which `store.rs`
//! implements over a transaction (so a later command in the same batch sees an earlier
//! one's effects). Tests use an in-memory implementation.
//!
//! Errors follow the one precedence rule in `org_proto::org`: `not_found`, then
//! `invalid_request`, `invalid_name`, the domain codes (`owner_protected`, `cycle`,
//! `not_a_member`, `not_empty`), `conflict`, `already_exists`.

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
pub use decide::{decide, owner_bootstrap};
pub use view::{OrgView, OrgWrite};

use org_proto::{ApiError, ErrorCode};

/// The longest client-made id, in characters.
pub const ID_MAX_CHARS: usize = 64;

pub(crate) fn fail(code: ErrorCode, message: &str) -> ApiError {
    ApiError::new(code, message)
}
