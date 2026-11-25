//! The group-write exception: two facts about a file that two capabilities judge by.
//!
//! A stock Ubuntu or Debian account has a group of its own (a "user private group")
//! and a file-creation mask of `0002`, so everything the account makes is writable by
//! that group. Two rules of the host therefore admit group write access for that one
//! group and for no other:
//!
//! - the directories above a release credential directory
//!   (`crate::credentials`, Linux host design section 3.2), and
//! - the image file the Linux admission adapter copies (`crate::admission`, design
//!   section 3.1).
//!
//! Both ask this module two questions: which group is the service user's private group
//! ([`private_group_of`]), and whether the admitted file carries an access control
//! list ([`has_access_acl`]).
//!
//! # The private group
//!
//! A group is the user's private group when all of these hold (`decision.rs`):
//!
//! - it is the user's primary group (the group of the user's account entry);
//! - its name is the user's name;
//! - its member list (the fourth field of its group entry) is empty, or names only the
//!   user.
//!
//! # What "private" does not establish
//!
//! The rule reads two entries of the account database and nothing else. It does
//! **not** establish that no other account can act as a member of the group:
//!
//! - **Another account whose primary group is this group is not detected.** Such an
//!   account is a member of the group, and no member list names it (for example
//!   `useradd -g <the group> other`). It can write wherever the group can. For a
//!   directory above the credential directory that means: it can read no item (the
//!   credential directory is `0700`), but it can rename the directories below the one
//!   it can write to, and so put an older copy of the store that the service user
//!   owns in the store's place, unnoticed. For an image file it means: it can rewrite
//!   the worker image a development build then runs.
//! - Finding such an account takes a walk over every account (`getpwent`). That walk
//!   is unbounded and incomplete with a directory service (LDAP, sssd), and it is not
//!   done, by decision.
//!
//! What bounds this: making an account with another account's group as its primary
//! group takes root (or the directory service's administrator), which the host already
//! trusts; and a release installation does not rely on the exception at all (a
//! root-owned release path and a credential directory whose ancestors are not
//! group-writable). An operator who shares a primary group between accounts on
//! purpose must not keep a credential directory or a worker image below a
//! group-writable directory of that group.
//!
//! # No access control list
//!
//! A POSIX access ACL can give a user or a group that the mode does not name the right
//! to write, and the mode then shows that right in its group bits (the ACL's mask).
//! A file that is admitted only because its group is the private group must therefore
//! carry no access ACL at all: with one, "the group can write" no longer means "the
//! private group can write". The same holds for a root-owned sticky directory admitted
//! by the other exception. A file without the group's write bit needs no look: the
//! mask then denies writing to every ACL entry.
//!
//! # Systems
//!
//! GNU/Linux reads the two account entries with `getpwuid_r` and `getgrgid_r`, the
//! reentrant calls, and the ACL as the `system.posix_acl_access` attribute
//! (`linux.rs`). Every other system has the refusing adapter (`refused.rs`): both
//! questions answer with the refusal, as an error. An error is what keeps every caller
//! closed: a caller grants nothing to a private group it could not learn, and admits
//! no file whose ACL it could not read; "no private group" would do for the first
//! question, but "no ACL" for the second would admit.
//!
//! The decision is pure and is compiled and tested on every target (`decision.rs`).
//! This module is private to the crate: it is no contract of its own.

// Only the GNU/Linux adapters of admission and credentials ask these questions, so on
// every other target the entries and what they reach have no caller.
#![cfg_attr(
    not(all(target_os = "linux", target_env = "gnu")),
    allow(
        dead_code,
        reason = "only GNU/Linux admission and credential adapters call account queries"
    )
)]

use std::io;
use std::os::fd::BorrowedFd;

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod linux;
#[cfg(all(target_os = "linux", target_env = "gnu"))]
use linux as native;

#[cfg_attr(
    all(target_os = "linux", target_env = "gnu"),
    allow(
        dead_code,
        reason = "refusing fallback module is used on non-GNU/Linux targets and in unit tests"
    )
)]
mod refused;
#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
use refused as native;

mod decision;

// Test support for the tests of the two GNU/Linux adapters that ask these questions:
// a real ACL, set as the attribute itself.
#[cfg(all(test, target_os = "linux", target_env = "gnu"))]
pub(crate) use linux::set_access_acl_for_test;

/// The private group of the user `user`, or `None` if the user has none: no account
/// entry, no group entry for its primary group, or a primary group that is not private
/// by the rule above. The module documentation says what a private group does not
/// establish.
///
/// An error means the account database could not be read, or this system has no
/// adapter. A caller that grants something to the private group grants nothing then.
pub(crate) fn private_group_of(user: u32) -> io::Result<Option<u32>> {
    native::private_group_of(user)
}

/// Whether the open file or directory `file` carries a POSIX access ACL. The
/// descriptor may be one opened with `O_PATH`.
///
/// An error means the answer could not be had (this system has no adapter, or the
/// attribute could not be read). A caller that admits a file only without an ACL
/// refuses then.
pub(crate) fn has_access_acl(file: BorrowedFd<'_>) -> io::Result<bool> {
    native::has_access_acl(file)
}

/// What every refusing adapter in this module answers.
fn refusal() -> org_platform::Refused {
    org_platform::Refused::Unsupported {
        capability: "the group-write exception",
        os: std::env::consts::OS,
    }
}
