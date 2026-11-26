//! The errors a start of the web process reports, and how a reaped child ended.

use std::{io, time::Duration};

use org_platform_host::admission::Child;

pub(super) const POLL: Duration = Duration::from_millis(50);

fn unavailable() -> io::Error {
    io::Error::other("authenticated browser service unavailable")
}

pub(super) fn invalid<T>(_: T) -> io::Error {
    unavailable()
}

/// The launch check refused the web process at `step`. Its cause names only the check
/// and the executable's identity, so it is kept for the supervisor's log line.
pub(super) fn refused(step: &'static str) -> impl FnOnce(String) -> io::Error {
    move |why| io::Error::other(format!("web process admission refused at {step}: {why}"))
}

/// A failed start step with a fixed, secret-free cause for the supervisor's log line.
pub(super) fn failed(cause: &str) -> io::Error {
    io::Error::other(format!("web process start failed: {cause}"))
}

/// A start that ended because the host is stopping.
pub(super) fn host_stopping() -> io::Error {
    failed("the host is stopping")
}

/// A failed start step caused by an OS error. Only the error's kind is kept, never
/// its text, so the cause stays fixed and secret-free.
pub(super) fn os_failed(cause: &'static str) -> impl FnOnce(io::Error) -> io::Error {
    move |error| failed(&format!("{cause} ({:?})", error.kind()))
}

/// The control stream ended where a line was expected. Its text is that of any other
/// failed step; the type lets the start tell a child that went away by itself.
#[derive(Debug)]
pub(super) struct ControlEof(pub(super) String);

impl std::fmt::Display for ControlEof {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ControlEof {}

/// How the reaped child ended, for a log line.
pub(super) fn exited(process: &Child) -> String {
    process
        .ended()
        .unwrap_or_else(|| "status unknown".to_owned())
}
