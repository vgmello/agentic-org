//! Waiting for a shutdown signal: SIGTERM (launchd, at logout) or SIGINT (Ctrl-C in a
//! terminal), with plain libc and no handler.
//!
//! [`ShutdownSignals::block`] blocks both signals before any thread starts, so every thread
//! inherits the mask and they stay pending until [`ShutdownSignals::wait`] takes one with
//! `sigwait`. That leaves nothing to do in a signal handler. Office servers are unaffected:
//! std clears the signal mask of every process it spawns.

use std::io;
use std::mem::MaybeUninit;

pub struct ShutdownSignals {
    set: libc::sigset_t,
}

impl ShutdownSignals {
    /// Blocks SIGTERM and SIGINT in this thread and every thread it starts from now on. Call
    /// it first in `main`.
    pub fn block() -> io::Result<Self> {
        let mut set = MaybeUninit::<libc::sigset_t>::uninit();
        // SAFETY: sigemptyset initializes `set`; sigaddset and pthread_sigmask get valid
        // pointers to it.
        let set = unsafe {
            libc::sigemptyset(set.as_mut_ptr());
            libc::sigaddset(set.as_mut_ptr(), libc::SIGTERM);
            libc::sigaddset(set.as_mut_ptr(), libc::SIGINT);
            set.assume_init()
        };
        // SAFETY: a valid, initialized set; the old mask isn't wanted.
        let rc = unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut()) };
        if rc != 0 {
            return Err(io::Error::from_raw_os_error(rc));
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
