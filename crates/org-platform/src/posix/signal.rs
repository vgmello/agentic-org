//! Signal sets and masks, waiting for a blocked signal, default dispositions, and the
//! handlers a consumer may install: a stop request and a stop-on-signal for a test child.
//!
//! A mask is the calling thread's (`pthread_sigmask`); a thread started later inherits the
//! mask of the thread that starts it. A disposition is the whole process's.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

use super::check;

/// A set of signals (`sigset_t`).
#[derive(Clone, Copy)]
pub struct SignalSet(libc::sigset_t);

impl std::fmt::Debug for SignalSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SignalSet")
    }
}

impl SignalSet {
    /// The empty set.
    pub fn empty() -> io::Result<Self> {
        // All-zero bytes first: on GNU/Linux the C library's set is wider than the kernel's,
        // and a call that fills one in may write only the kernel's prefix.
        // SAFETY: sigset_t is plain data for which all-zero bytes are a valid value.
        let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
        // SAFETY: `set` is a live, exclusively borrowed sigset_t that sigemptyset writes.
        check(unsafe { libc::sigemptyset(&mut set) })?;
        Ok(Self(set))
    }

    /// The set of exactly `signals`. A number that is not a signal is `EINVAL`.
    pub fn of(signals: &[libc::c_int]) -> io::Result<Self> {
        let mut set = Self::empty()?;
        for &signal in signals {
            // SAFETY: `set.0` is a live, initialized, exclusively borrowed sigset_t.
            check(unsafe { libc::sigaddset(&mut set.0, signal) })?;
        }
        Ok(set)
    }

    /// Whether `signal` is in the set.
    pub fn contains(&self, signal: libc::c_int) -> bool {
        // SAFETY: `self.0` is an initialized sigset_t that sigismember only reads.
        unsafe { libc::sigismember(&self.0, signal) == 1 }
    }
}

fn thread_mask(how: libc::c_int, set: &SignalSet) -> io::Result<SignalSet> {
    let mut previous = SignalSet::empty()?;
    // SAFETY: `set.0` is an initialized sigset_t read by the call, and `previous.0` a live,
    // exclusively borrowed, already initialized one that it writes.
    let rc = unsafe { libc::pthread_sigmask(how, &set.0, &mut previous.0) };
    if rc != 0 {
        return Err(io::Error::from_raw_os_error(rc));
    }
    Ok(previous)
}

/// Block `set` in the calling thread (adding it to the mask) and return the mask it had.
pub fn block_signals(set: &SignalSet) -> io::Result<SignalSet> {
    thread_mask(libc::SIG_BLOCK, set)
}

/// Make `set` the calling thread's whole mask, and return the mask it had.
pub fn set_signal_mask(set: &SignalSet) -> io::Result<SignalSet> {
    thread_mask(libc::SIG_SETMASK, set)
}

/// The signals pending for the calling thread or the process: raised while blocked.
pub fn pending_signals() -> io::Result<SignalSet> {
    let mut set = SignalSet::empty()?;
    // SAFETY: `set.0` is a live, initialized, exclusively borrowed sigset_t it writes.
    check(unsafe { libc::sigpending(&mut set.0) })?;
    Ok(set)
}

/// Wait until a signal of `set` is pending, take it and return its number (`sigwait`).
/// Block `set` first, in every thread, or another thread may take it instead.
pub fn wait_for_signal(set: &SignalSet) -> io::Result<libc::c_int> {
    let mut signal = 0;
    // SAFETY: `set.0` is an initialized sigset_t it reads and `signal` a live int it writes.
    let rc = unsafe { libc::sigwait(&set.0, &mut signal) };
    if rc != 0 {
        return Err(io::Error::from_raw_os_error(rc));
    }
    Ok(signal)
}

fn install(signal: libc::c_int, handler: libc::sighandler_t, flags: libc::c_int) -> io::Result<()> {
    // SAFETY: sigaction is plain data for which all-zero bytes are a valid value.
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = handler;
    action.sa_flags = flags;
    // SAFETY: `action.sa_mask` is a live, exclusively borrowed sigset_t it initializes.
    check(unsafe { libc::sigemptyset(&mut action.sa_mask) })?;
    // SAFETY: `action` is fully initialized and only read; no previous action is asked
    // for. `handler` is SIG_DFL, SIG_IGN or one of this module's handlers, which are
    // async-signal-safe.
    check(unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) }).map(drop)
}

/// Give `signal` its default disposition (`SIG_DFL`) in this process, replacing an
/// inherited `SIG_IGN` (with which the system would discard it even while blocked).
pub fn default_disposition(signal: libc::c_int) -> io::Result<()> {
    install(signal, libc::SIG_DFL, 0)
}

/// Ignore `signal` in this process (`SIG_IGN`). Unlike a handled signal, an ignored one
/// stays ignored in a program this process starts. The disposition is the whole process's.
pub fn ignore_signal(signal: libc::c_int) -> io::Result<()> {
    install(signal, libc::SIG_IGN, 0)
}

/// Whether `signal` is ignored (`SIG_IGN`) in this process now (`sigaction` query).
pub fn is_ignored(signal: libc::c_int) -> io::Result<bool> {
    // SAFETY: sigaction is plain data for which all-zero bytes are a valid value.
    let mut current: libc::sigaction = unsafe { std::mem::zeroed() };
    // SAFETY: no new action is given, and `current` is a live, exclusively borrowed record
    // that sigaction writes.
    check(unsafe { libc::sigaction(signal, std::ptr::null(), &mut current) })?;
    Ok(current.sa_sigaction == libc::SIG_IGN)
}

/// Whether one of the signals given to [`request_stop_on`] arrived.
static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);

/// The stop handler: only a lock-free atomic store, which is async-signal-safe.
extern "C" fn record_stop(_signal: libc::c_int) {
    STOP_REQUESTED.store(true, Ordering::SeqCst);
}

/// Make each of `signals` (`SIGTERM`, `SIGINT`) only record a request to stop, which
/// [`stop_requested`] reports, instead of ending the process. The handler does nothing
/// else, so a blocking call it interrupts fails with `EINTR` (no `SA_RESTART`) and the
/// caller's loop notices. The disposition is the whole process's.
pub fn request_stop_on(signals: &[libc::c_int]) -> io::Result<()> {
    for &signal in signals {
        install(
            signal,
            record_stop as extern "C" fn(libc::c_int) as libc::sighandler_t,
            0,
        )?;
    }
    Ok(())
}

/// Whether a signal given to [`request_stop_on`] has arrived.
pub fn stop_requested() -> bool {
    STOP_REQUESTED.load(Ordering::SeqCst)
}

/// The stop-self handler: `kill` and `getpid` are async-signal-safe.
extern "C" fn stop_self(_signal: libc::c_int) {
    // SAFETY: getpid and kill take integers, touch no memory and are async-signal-safe.
    unsafe {
        libc::kill(libc::getpid(), libc::SIGSTOP);
    }
}

/// Make `signal` stop this whole process (`SIGSTOP` to itself) where it would otherwise
/// end it, so that a parent sees the stop (`waitpid` with `WUNTRACED`) and decides; for
/// example `SIGXFSZ`, at the file-size limit a test child set. The disposition is the
/// whole process's.
pub fn stop_on_signal(signal: libc::c_int) -> io::Result<()> {
    install(
        signal,
        stop_self as extern "C" fn(libc::c_int) as libc::sighandler_t,
        0,
    )
}
