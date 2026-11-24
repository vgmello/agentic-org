//! Private SDK ownership. Only `confirm` invokes OS authentication; context
//! queries and tests never treat SDK availability as successful presence.
use std::{ffi::c_void, ptr::NonNull};
unsafe extern "C" {
    fn ag_recovery_local(token: *const u8, length: usize) -> i32;
    fn ag_recovery_current() -> i32;
    fn ag_recovery_begin_reason(reason_code: i32) -> *mut c_void;
    fn ag_recovery_poll(handle: *mut c_void) -> i32;
    fn ag_recovery_free(handle: *mut c_void);
}
pub(super) fn local(token: &AuditToken) -> bool {
    // SAFETY: exactly the supported audit-token bytes live for this call.
    unsafe { ag_recovery_local(token.as_ptr(), token.len()) == 1 }
}
pub(super) fn current() -> bool {
    // SAFETY: synchronous SDK query, no authentication prompt or retained pointer.
    unsafe { ag_recovery_current() == 1 }
}
struct Context(NonNull<c_void>);
impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: sole retained handle; SDK callbacks retain their own ARC heap state.
        unsafe { ag_recovery_free(self.0.as_ptr()) };
    }
}
pub(super) fn confirm(
    reason: super::ConfirmReason,
    cancelled: impl Fn() -> bool,
) -> Result<(), ()> {
    use std::time::{Duration, Instant};
    if cancelled() {
        return Err(());
    }
    let code = match reason {
        super::ConfirmReason::RecoveryCode => 0,
        super::ConfirmReason::ResetAdminPassword => 1,
        super::ConfirmReason::CreateAdminRegistrationLink => 2,
    };
    // SAFETY: fixed SDK ABI returns one retained heap handle or null; no Rust
    // state/address crosses the asynchronous callback boundary.
    let context = Context(NonNull::new(unsafe { ag_recovery_begin_reason(code) }).ok_or(())?);
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if cancelled() || Instant::now() >= deadline {
            return Err(());
        }
        // SAFETY: context retains this handle until Drop, even on late callbacks.
        match unsafe { ag_recovery_poll(context.0.as_ptr()) } {
            0 => std::thread::sleep(Duration::from_millis(50)),
            1 => return if cancelled() { Err(()) } else { Ok(()) },
            _ => return Err(()),
        }
    }
}

use std::os::unix::net::UnixStream;

/// A process's audit token (`audit_token_t`): eight 32-bit words.
type AuditToken = [u8; 32];

use super::{ConfirmReason, Confirmation, Unconfirmed};
use org_platform::Refused;

pub(super) fn local_peer(peer: &UnixStream) -> Result<bool, Refused> {
    // The one read of the peer's audit token is the admission contract's.
    Ok(crate::admission::peer(peer).is_ok_and(|peer| local(&peer.macos_audit_token)))
}

pub(super) fn current_context() -> Result<bool, Refused> {
    Ok(current())
}

pub(super) fn confirm_presence(
    reason: ConfirmReason,
    cancelled: impl Fn() -> bool,
) -> Result<Confirmation, Unconfirmed> {
    confirm(reason, cancelled)
        .map(|()| Confirmation::OsDialog)
        .map_err(|()| Unconfirmed::Denied)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sdk_null_handle_and_invalid_token_fail_without_authentication_prompt() {
        // SAFETY: specified null/invalid inputs, not evaluatePolicy.
        unsafe {
            assert_eq!(ag_recovery_poll(std::ptr::null_mut()), 2);
            ag_recovery_free(std::ptr::null_mut());
        }
        assert!(!local(&[0; 32]));
    }
}
