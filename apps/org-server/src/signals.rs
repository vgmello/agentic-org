//! SIGTERM and SIGINT ask the server to stop: the handler only sets a flag (all it may safely
//! do), and the accept loop notices it, closes the databases and exits cleanly.
#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, Ordering};

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_signal: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

/// Installs the handlers.
pub fn install() {
    // SAFETY: `on_signal` only stores to an atomic, which is async-signal-safe, and the
    // sigaction struct is fully initialised before use.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = on_signal as extern "C" fn(libc::c_int) as usize;
        libc::sigemptyset(&mut action.sa_mask);
        action.sa_flags = 0;
        for signal in [libc::SIGTERM, libc::SIGINT] {
            libc::sigaction(signal, &action, std::ptr::null_mut());
        }
    }
}

/// Whether a stop signal arrived.
pub fn stop_requested() -> bool {
    STOP.load(Ordering::SeqCst)
}
