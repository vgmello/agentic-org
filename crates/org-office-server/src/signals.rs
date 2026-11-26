//! SIGTERM and SIGINT ask the server to stop: the handler only sets a flag (all it may safely
//! do; `org_platform::posix::request_stop_on`), and the accept loop notices it, closes the
//! databases and exits cleanly.

use org_platform::posix;

/// Installs the handlers. The signals are fixed and valid, so installing them cannot fail on
/// a supported system; a failure is ignored as before (the default disposition stays).
pub fn install() {
    let _ = posix::request_stop_on(&[libc::SIGTERM, libc::SIGINT]);
}

/// Whether a stop signal arrived.
pub fn stop_requested() -> bool {
    posix::stop_requested()
}
