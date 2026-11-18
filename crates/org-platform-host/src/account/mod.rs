//! The service user's private group: one rule for two capabilities.
//!
//! A stock Ubuntu or Debian account has a group of its own (a "user private group")
//! and a file-creation mask of `0002`, so everything the account makes is writable by
//! that group. Such a file or directory is as closed as one without the group bit,
//! because nobody else is in the group. Two rules of the host therefore admit group
//! write access for that one group and for no other:
//!
//! - the directories above a release credential directory
//!   (`crate::credentials`, Linux host design section 3.2), and
//! - the image file the Linux admission adapter copies (`crate::admission`, design
//!   section 3.1).
//!
//! # The rule
//!
//! A group is the user's private group when all of these hold (`decision.rs`):
//!
//! - it is the user's primary group (the group of the user's account entry);
//! - its name is the user's name;
//! - it has no other members: the member list of its group entry is empty, or names
//!   only the user.
//!
//! The last point is what the account database can say without being enumerated. An
//! account whose *primary* group is this group is a member of it that no member list
//! names; finding one would take a walk over every account (`getpwent`), which is not
//! reentrant, is unbounded with a directory service, and is not complete there either.
//! Creating a second account with the same primary group takes root, which the host
//! already trusts.
//!
//! # Systems
//!
//! GNU/Linux only: `linux.rs` reads the two entries with `getpwuid_r` and
//! `getgrgid_r`, the reentrant calls. No other system has a caller, so none has a
//! probe; the decision is compiled and tested on every target.
//!
//! This module is private to the crate: it is no contract of its own.

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod linux;
#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub(crate) use linux::private_group_of;

// The rule itself. Only the GNU/Linux probe calls it; every target tests it.
#[cfg(any(all(target_os = "linux", target_env = "gnu"), test))]
mod decision;
