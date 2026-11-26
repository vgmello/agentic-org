//! The order of an individual signal's checks (identity, credentials, mark, identity
//! again, then the signal) and the bounded wait for an exit.

use std::io;
use std::time::Instant;

use crate::process::{check_time, MarkEvidence, ProcessIdentity, SignalOutcome};

use super::libproc::{live_matches, safe_credentials};
use super::EXIT_RECHECK_INTERVAL;

pub(super) fn wait_after_registration(
    deadline: Instant,
    mut now: impl FnMut() -> Instant,
    mut gone: impl FnMut() -> io::Result<bool>,
    mut wait: impl FnMut(Instant) -> io::Result<()>,
) -> io::Result<bool> {
    // XNU can refuse kqueue registration while libproc still reports the exact
    // process as live. Treat the queue as a hint and keep checking that identity.
    while now() < deadline {
        let exited = gone()?;
        let observed = now();
        if observed >= deadline {
            break;
        }
        if exited {
            return Ok(true);
        }
        wait(deadline.min(observed + EXIT_RECHECK_INTERVAL))?;
    }
    Ok(false)
}

// Keep the read-mark-read-check-signal ordering testable without a racing process.
pub(super) fn signal_with(
    target: &ProcessIdentity,
    mark_accepted: &dyn Fn(&MarkEvidence) -> bool,
    deadline: Instant,
    uid: u32,
    mut info: impl FnMut() -> io::Result<Option<libc::proc_bsdinfo>>,
    mark: impl FnOnce(&libc::proc_bsdinfo) -> io::Result<MarkEvidence>,
    send: impl FnOnce() -> io::Result<SignalOutcome>,
) -> io::Result<SignalOutcome> {
    let before = info();
    check_time(deadline)?;
    let Some(before) = before? else {
        return Ok(SignalOutcome::Gone);
    };
    if !live_matches(&before, target) {
        return Ok(SignalOutcome::Gone);
    }
    if !safe_credentials(&before, uid) {
        return Err(unsafe_target());
    }
    let mark = mark(&before)?;
    let after = info();
    check_time(deadline)?;
    if !recheck_after_mark(target, mark_accepted, after?.as_ref(), &mark, uid)? {
        return Ok(SignalOutcome::Gone);
    }
    check_time(deadline)?;
    let result = send();
    check_time(deadline)?;
    result
}

pub(super) fn recheck_after_mark(
    target: &ProcessIdentity,
    mark_accepted: &dyn Fn(&MarkEvidence) -> bool,
    after: Option<&libc::proc_bsdinfo>,
    mark: &MarkEvidence,
    uid: u32,
) -> io::Result<bool> {
    let Some(after) = after else {
        return Ok(false);
    };
    if !live_matches(after, target) {
        return Ok(false);
    }
    if !safe_credentials(after, uid) {
        return Err(unsafe_target());
    }
    if !mark_accepted(mark) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "recovery mark changed or is unreadable",
        ));
    }
    Ok(true)
}

fn unsafe_target() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "recovery target credentials are unsafe",
    )
}
