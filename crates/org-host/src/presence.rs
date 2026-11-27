//! The host's use of the platform's presence adapter (`org_platform_host::presence`).
//! Only `confirm` invokes OS authentication; context queries and tests never treat the
//! adapter's availability as successful presence. A system without an adapter is never
//! a local context and confirms nothing.
use std::os::unix::net::UnixStream;

use org_platform_host::presence::{self, ConfirmReason, Confirmation};

/// Whether the process at the other end of `peer` (a connection's socket) runs in the local
/// context a release requires. A failed query is not local.
pub(crate) fn local(peer: &UnixStream) -> bool {
    presence::local(peer).unwrap_or(false)
}
pub(crate) fn current() -> bool {
    presence::current().unwrap_or(false)
}
pub(crate) fn confirm(cancelled: impl Fn() -> bool) -> Result<(), ()> {
    confirm_for(ConfirmReason::RecoveryCode, cancelled)
}
pub(crate) fn confirm_for(reason: ConfirmReason, cancelled: impl Fn() -> bool) -> Result<(), ()> {
    match presence::confirm_for(reason, cancelled) {
        Ok(Confirmation::OsDialog) => Ok(()),
        // The browser approval that completes this kind (ADR-0017 decision 11) is not
        // built: without it nothing is confirmed.
        Ok(Confirmation::BrowserApproval) | Err(_) => Err(()),
    }
}
