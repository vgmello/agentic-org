//! Waiting for a shutdown signal: SIGTERM (launchd, at logout) or SIGINT (Ctrl-C in a
//! terminal), with plain libc and no handler.
//!
//! [`ShutdownSignals::block`] blocks both signals and replaces inherited ignored dispositions
//! before any thread starts. Every thread inherits the mask, so subsequent signals stay pending
//! until [`ShutdownSignals::wait`] takes one with `sigwait`. No asynchronous handler is needed.
//! Office servers are unaffected:
//! std clears the signal mask of every process it spawns.

use std::io;
use std::mem::MaybeUninit;

pub struct ShutdownSignals {
    set: libc::sigset_t,
}

impl ShutdownSignals {
    /// Claims SIGTERM and SIGINT for shutdown, blocking them in this thread and every thread
    /// it starts from now on, even if a background launcher ignored them. Call first in `main`.
    pub fn block() -> io::Result<Self> {
        let mut set = MaybeUninit::<libc::sigset_t>::uninit();
        // SAFETY: sigemptyset initializes `set`; sigaddset and pthread_sigmask get valid
        // pointers to it.
        let set = unsafe {
            if libc::sigemptyset(set.as_mut_ptr()) != 0 {
                return Err(io::Error::last_os_error());
            }
            for signal in [libc::SIGTERM, libc::SIGINT] {
                if libc::sigaddset(set.as_mut_ptr(), signal) != 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            set.assume_init()
        };
        // SAFETY: a valid, initialized set; the old mask isn't wanted.
        let rc = unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut()) };
        if rc != 0 {
            return Err(io::Error::from_raw_os_error(rc));
        }
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
        let mut signal = 0;
        // SAFETY: a valid, initialized set and a valid out-pointer.
        let rc = unsafe { libc::sigwait(&self.set, &mut signal) };
        if rc == 0 {
            Ok(signal)
        } else {
            Err(io::Error::from_raw_os_error(rc))
        }
    }
}

fn default_disposition(signal: libc::c_int) -> io::Result<()> {
    // SAFETY: all-zero sigaction fields are valid; sigemptyset initializes its mask.
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = libc::SIG_DFL;
    // SAFETY: valid initialized action, a writable mask and no old-action output.
    unsafe {
        if libc::sigemptyset(&mut action.sa_mask) != 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::sigaction(signal, &action, std::ptr::null_mut()) != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_invalid_disposition_signal_returns_the_os_error() {
        let error = super::default_disposition(-1).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(libc::EINVAL));
    }
}
