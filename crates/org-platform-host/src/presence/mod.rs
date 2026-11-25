//! Local presence for the release of the recovery code.
//!
//! Linux host design section 4, `presence`; the requirements are ADR-0011
//! (REC-DEL-1). The host is the consumer. It keeps the release policy: who may ask,
//! one operation in flight, what is rechecked after the confirmation, and what each
//! [`Confirmation`] kind still requires. This module only asks the operating system.
//!
//! Only [`confirm`] invokes OS authentication. The context queries, [`local`] and
//! [`current`], prompt nobody, and no adapter treats its own availability as presence.
//!
//! # Systems
//!
//! - macOS: the Objective-C bridge `bridge.m`, which `build.rs` compiles from pinned
//!   bytes. The peer is identified by its kernel audit token; the confirmation is
//!   the OS device-owner authentication dialog.
//! - GNU/Linux: no adapter yet; every function refuses, as on every other target.
//!   The design's terminal check and browser approval are written later.
//! - Every other Unix target: every function returns the refusal
//!   ([`org_platform::Refused`]).

#![deny(unsafe_op_in_unsafe_fn)]

use std::os::unix::net::UnixStream;

use org_platform::Refused;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as native;

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod linux;
#[cfg(all(target_os = "linux", target_env = "gnu"))]
use linux as native;

#[cfg_attr(
    target_os = "macos",
    allow(
        dead_code,
        reason = "refusing fallback module is used on unsupported targets and in unit tests"
    )
)]
mod refused;
#[cfg(not(any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))))]
use refused as native;

/// How a release was confirmed. The host branches on this kind, never on the system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confirmation {
    /// The person authenticated in the operating system's own dialog. This is the
    /// whole confirmation.
    OsDialog,
    /// The system has no such dialog. Nothing is confirmed yet: the host must require
    /// the signed-in administrator's approval of this run in the browser as the one
    /// confirmation (ADR-0017 decision 11).
    BrowserApproval,
}

/// Why there is no confirmation. Fixed categories only: no OS error, exception or
/// user text crosses this boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unconfirmed {
    /// Cancelled, denied, unable, timed out, or any failure of the OS query.
    Denied,
    /// This build target has no presence adapter.
    Refused(Refused),
}

/// Whether the process at the other end of `peer`, a connected Unix socket, runs in
/// the local context a release requires. No prompt. Any failed query answers `false`.
///
/// macOS: the peer's audit token must name a live non-root process of this user in
/// this graphic, non-remote console session, with a controlling terminal, in its
/// foreground process group.
pub fn local(peer: &UnixStream) -> Result<bool, Refused> {
    native::local_peer(peer)
}

/// Whether this process itself runs in that local context. No prompt.
pub fn current() -> Result<bool, Refused> {
    native::current_context()
}

/// Why OS presence confirmation is being requested; selects the fixed OS dialog text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmReason {
    /// Showing and recording an organization recovery code.
    RecoveryCode,
    /// Issuing a one-time password reset link for the existing administrator.
    ResetAdminPassword,
    /// Issuing a new administrator registration link while none exists.
    CreateAdminRegistrationLink,
}

/// Ask the person at the machine to confirm one release. Blocks until the answer,
/// until `cancelled` returns true, or until the adapter's deadline (macOS: 90
/// seconds), whichever is first. `cancelled` is polled throughout; a confirmation
/// that arrives after it turned true is not returned.
pub fn confirm(cancelled: impl Fn() -> bool) -> Result<Confirmation, Unconfirmed> {
    native::confirm_presence(ConfirmReason::RecoveryCode, cancelled)
}

/// Ask the person at the machine to confirm a specific local administrative action.
pub fn confirm_for(
    reason: ConfirmReason,
    cancelled: impl Fn() -> bool,
) -> Result<Confirmation, Unconfirmed> {
    native::confirm_presence(reason, cancelled)
}

/// What every refusing adapter in this capability answers.
fn refusal() -> Refused {
    Refused::Unsupported {
        capability: "recovery presence",
        os: std::env::consts::OS,
    }
}
