//! The host's use of the platform's presence adapter (`org_platform_host::presence`).
//! Only `confirm` invokes OS authentication; context queries and tests never treat the
//! adapter's availability as successful presence. A system without an adapter is never
//! a local context and confirms nothing.
use org_platform_host::presence::{self, Confirmation};

pub(super) fn local(connection: &crate::service::listener::Connection) -> bool {
    presence::local(connection.socket()).unwrap_or(false)
}
pub(crate) fn current() -> bool {
    presence::current().unwrap_or(false)
}
pub(super) fn confirm(cancelled: impl Fn() -> bool) -> Result<(), ()> {
    match presence::confirm(cancelled) {
        Ok(Confirmation::OsDialog) => Ok(()),
        // The browser approval that completes this kind (ADR-0017 decision 11) is not
        // built: without it nothing is confirmed.
        Ok(Confirmation::BrowserApproval) | Err(_) => Err(()),
    }
}
