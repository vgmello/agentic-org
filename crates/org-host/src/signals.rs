//! Waiting for a shutdown signal: SIGTERM (launchd, at logout) or SIGINT (Ctrl-C in a
//! terminal), with no handler (`org_platform::posix`'s masks and `sigwait`).
//!
//! [`ShutdownSignals::block`] blocks both signals and replaces inherited ignored dispositions
//! before any thread starts. Every thread inherits the mask, so subsequent signals stay pending
//! until [`ShutdownSignals::wait`] takes one with `sigwait`. No asynchronous handler is needed.
//! Office servers are unaffected:
//! std clears the signal mask of every process it spawns.

use std::io;

use org_platform::posix::{self, SignalSet};

pub struct ShutdownSignals {
    set: SignalSet,
}

impl ShutdownSignals {
    /// Claims SIGTERM and SIGINT for shutdown, blocking them in this thread and every thread
    /// it starts from now on, even if a background launcher ignored them. Call first in `main`.
    pub fn block() -> io::Result<Self> {
        let set = SignalSet::of(&[libc::SIGTERM, libc::SIGINT])?;
        posix::block_signals(&set)?;
        // Block BEFORE changing dispositions, before any threads start: restoring defaults
        // while unblocked would let a shutdown signal terminate the host without cleanup.
        // Ownership starts once BOTH resets complete; signals received before then are not
        // promised. Subsequent signals stay pending for wait(). Inherited SIG_IGN would
        // otherwise let the OS discard them, even with the signal blocked.
        // SIGHUP, SIGPIPE, SIGQUIT and all other dispositions remain as inherited.
        for signal in [libc::SIGTERM, libc::SIGINT] {
            default_disposition(signal)?;
        }
        Ok(Self { set })
    }

    /// Waits until SIGTERM or SIGINT arrives, and returns it.
    pub fn wait(&self) -> io::Result<i32> {
        posix::wait_for_signal(&self.set)
    }
}

fn default_disposition(signal: libc::c_int) -> io::Result<()> {
    posix::default_disposition(signal)
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_invalid_disposition_signal_returns_the_os_error() {
        let error = super::default_disposition(-1).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(libc::EINVAL));
    }
}
