//! The signals a wrapper takes over while it supervises one child it must clean up after
//! (`agent-os exec`: end the lease, remove the private token folder). The same POSIX calls on
//! every system.
//!
//! - `SIGINT` and `SIGQUIT` are recorded and otherwise ignored. A terminal sends Ctrl-C (and
//!   Ctrl-\) to its whole foreground process group, so the child gets it and decides; the
//!   wrapper cleans up when the child exits.
//! - `SIGTERM` and `SIGHUP` are recorded as a termination request, which the wrapper forwards
//!   to the child ([`forward_signal`]) before it cleans up.
//!
//! The dispositions are handlers, never `SIG_IGN`: `execve` resets a handled signal to its
//! default in the new image, so a child started while the guard is held is not born ignoring
//! Ctrl-C. Handlers use `SA_RESTART`, so the wrapper's other threads see no `EINTR`. One guard
//! at a time per process; dropping it restores the dispositions it replaced.

use std::io;
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};

/// Whether a [`WrapperSignals`] is held.
static HELD: AtomicBool = AtomicBool::new(false);
/// The first termination signal received while held, or 0.
static TERMINATION: AtomicI32 = AtomicI32::new(0);
/// How many interrupts arrived while held.
static INTERRUPTS: AtomicU32 = AtomicU32::new(0);

const INTERRUPT_SIGNALS: [libc::c_int; 2] = [libc::SIGINT, libc::SIGQUIT];
const TERMINATION_SIGNALS: [libc::c_int; 2] = [libc::SIGTERM, libc::SIGHUP];

/// The handler: only lock-free atomic stores, which are async-signal-safe.
extern "C" fn record(signal: libc::c_int) {
    if INTERRUPT_SIGNALS.contains(&signal) {
        INTERRUPTS.fetch_add(1, Ordering::SeqCst);
    } else {
        let _ = TERMINATION.compare_exchange(0, signal, Ordering::SeqCst, Ordering::SeqCst);
    }
}

/// Holds the wrapper's signal dispositions (module documentation) until dropped.
#[derive(Debug)]
pub struct WrapperSignals {
    replaced: Vec<(libc::c_int, libc::sigaction)>,
}

impl WrapperSignals {
    /// Installs the handlers. Refuses (`AlreadyExists`) while another guard is held, and
    /// restores whatever it changed when one of the calls fails.
    pub fn install() -> io::Result<Self> {
        if HELD.swap(true, Ordering::SeqCst) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "the wrapper's signal guard is already held",
            ));
        }
        TERMINATION.store(0, Ordering::SeqCst);
        INTERRUPTS.store(0, Ordering::SeqCst);
        // Dropped on an early return: restores the signals already set and releases `HELD`.
        let mut guard = Self {
            replaced: Vec::with_capacity(4),
        };
        for signal in INTERRUPT_SIGNALS.into_iter().chain(TERMINATION_SIGNALS) {
            let previous = set_handler(signal)?;
            guard.replaced.push((signal, previous));
        }
        Ok(guard)
    }

    /// The first termination signal (`SIGTERM` or `SIGHUP`) received since [`Self::install`].
    pub fn termination(&self) -> Option<i32> {
        Some(TERMINATION.load(Ordering::SeqCst)).filter(|signal| *signal != 0)
    }

    /// Whether an interrupt (`SIGINT` or `SIGQUIT`) arrived since [`Self::install`].
    pub fn interrupted(&self) -> bool {
        INTERRUPTS.load(Ordering::SeqCst) > 0
    }
}

impl Drop for WrapperSignals {
    fn drop(&mut self) {
        for (signal, previous) in self.replaced.drain(..).rev() {
            // SAFETY: restores a disposition `sigaction` answered for this very signal; the
            // record is fully initialized and outlives the call.
            unsafe {
                libc::sigaction(signal, &previous, std::ptr::null_mut());
            }
        }
        HELD.store(false, Ordering::SeqCst);
    }
}

/// Installs [`record`] for `signal`, answering the disposition it replaced.
fn set_handler(signal: libc::c_int) -> io::Result<libc::sigaction> {
    // SAFETY: an all-zero `sigaction` is a valid record (integers, an empty set, no handler);
    // every field the call reads is set below.
    let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
    action.sa_sigaction = record as extern "C" fn(libc::c_int) as libc::sighandler_t;
    action.sa_flags = libc::SA_RESTART;
    // SAFETY: `sa_mask` is a valid, writable `sigset_t` owned by `action`.
    unsafe {
        libc::sigemptyset(&mut action.sa_mask);
    }
    // SAFETY: as above; `previous` is a writable output slot of the right size.
    let mut previous = unsafe { std::mem::zeroed::<libc::sigaction>() };
    // SAFETY: both records are valid for the call; the handler only stores atomics, which is
    // async-signal-safe, and lives for the whole program (a plain `extern "C" fn`).
    if unsafe { libc::sigaction(signal, &action, &mut previous) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(previous)
}

/// Sends `signal` (one of `SIGTERM`, `SIGHUP`, `SIGINT`, `SIGQUIT` or `SIGKILL`) to `pid`, a
/// child the caller started and has not reaped yet (so the PID cannot have been reused).
/// Refuses PIDs 0 and 1 and anything out of range, which would reach a group or `init`.
pub fn forward_signal(pid: u32, signal: i32) -> io::Result<()> {
    let pid = libc::pid_t::try_from(pid)
        .ok()
        .filter(|pid| *pid > 1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a child process id"))?;
    let allowed = [
        libc::SIGTERM,
        libc::SIGHUP,
        libc::SIGINT,
        libc::SIGQUIT,
        libc::SIGKILL,
    ];
    if !allowed.contains(&signal) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "only a termination or interrupt signal is forwarded",
        ));
    }
    // SAFETY: `kill` takes plain integers and touches no memory of this process.
    if unsafe { libc::kill(pid, signal) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// The exit code a shell reports for `status`: the child's own code, or 128 plus the signal
/// that ended it (`130` for Ctrl-C, `143` for `SIGTERM`); 1 when neither is known.
pub fn shell_exit_code(status: &ExitStatus) -> u8 {
    use std::os::unix::process::ExitStatusExt as _;
    match (status.code(), status.signal()) {
        (Some(code), _) => u8::try_from(code & 0xff).unwrap_or(1),
        (None, Some(signal)) => u8::try_from(128 + (signal & 0x7f)).unwrap_or(1),
        (None, None) => 1,
    }
}

/// The exit code for a wrapper that ends because of `signal` before its child ran.
pub fn signal_exit_code(signal: i32) -> u8 {
    u8::try_from(128 + (signal & 0x7f)).unwrap_or(1)
}

/// `SIGINT`, for a caller that reports an interrupt the way a shell does.
pub const SIGINT: i32 = libc::SIGINT;
/// `SIGTERM`, for a caller that forwards a termination request.
pub const SIGTERM: i32 = libc::SIGTERM;

#[cfg(test)]
#[path = "wrapper_signals_tests.rs"]
mod tests;
