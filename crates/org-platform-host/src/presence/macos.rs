//! Private SDK ownership. Only `confirm` invokes OS authentication; context
//! queries and tests never treat SDK availability as successful presence.
use std::{ffi::c_void, ptr::NonNull};
unsafe extern "C" {
    fn ag_recovery_local(token: *const u8, length: usize) -> i32;
    fn ag_recovery_current() -> i32;
    fn ag_recovery_begin() -> *mut c_void;
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
pub(super) fn confirm(cancelled: impl Fn() -> bool) -> Result<(), ()> {
    use std::time::{Duration, Instant};
    if cancelled() {
        return Err(());
    }
    // SAFETY: fixed SDK ABI returns one retained heap handle or null; no Rust
    // state/address crosses the asynchronous callback boundary.
    let context = Context(NonNull::new(unsafe { ag_recovery_begin() }).ok_or(())?);
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

// The peer's audit token, read as `org-host`'s `peer.rs` reads it for the code-signing
// check: the same constant, type and function, copied until the admission contract
// holds the one copy.
use std::io;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;

/// An `audit_token_t`: eight 32-bit words.
const AUDIT_TOKEN_BYTES: usize = 32;

/// A process's audit token (`audit_token_t`).
type AuditToken = [u8; AUDIT_TOKEN_BYTES];

/// The peer's audit token, which the code-signing check identifies the caller by. Unlike a pid,
/// it names one process for its whole life and is never reused.
fn audit_token(stream: &UnixStream) -> io::Result<[u8; AUDIT_TOKEN_BYTES]> {
    let mut token = [0u8; AUDIT_TOKEN_BYTES];
    let mut len = AUDIT_TOKEN_BYTES as libc::socklen_t;
    // SAFETY: a valid socket fd, and an out-buffer of `len` bytes that outlives the call.
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERTOKEN,
            token.as_mut_ptr().cast(),
            &mut len,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    if len as usize != AUDIT_TOKEN_BYTES {
        return Err(io::Error::other(
            "the peer's audit token has an unexpected size",
        ));
    }
    Ok(token)
}

use super::{Confirmation, Unconfirmed};
use org_platform::Refused;

pub(super) fn local_peer(peer: &UnixStream) -> Result<bool, Refused> {
    Ok(audit_token(peer).is_ok_and(|token| local(&token)))
}

pub(super) fn current_context() -> Result<bool, Refused> {
    Ok(current())
}

pub(super) fn confirm_presence(cancelled: impl Fn() -> bool) -> Result<Confirmation, Unconfirmed> {
    confirm(cancelled)
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
