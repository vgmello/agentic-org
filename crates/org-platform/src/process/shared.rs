//! Code the macOS and GNU/Linux adapters share. The refusing adapter calls none of it,
//! so it is not compiled for a refusing target.

use std::io;
use std::time::Instant;

use super::{DeadlineElapsed, MarkEvidence};

pub(super) fn check_time(deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            DeadlineElapsed("native recovery deadline elapsed"),
        ))
    } else {
        Ok(())
    }
}

pub(super) fn valid_pid(pid: u32) -> bool {
    pid > 1 && pid <= i32::MAX as u32
}

/// The mark in a NUL-separated environment block. `prefix` is the variable's name
/// followed by `=`.
pub(super) fn native_mark(bytes: &[u8], prefix: &[u8]) -> MarkEvidence {
    if !bytes.is_empty() && bytes.last() != Some(&0) {
        return MarkEvidence::Unreadable;
    }
    let mut marks = bytes
        .split(|byte| *byte == 0)
        .filter_map(|entry| entry.strip_prefix(prefix));
    let Some(value) = marks.next() else {
        return MarkEvidence::Absent;
    };
    // Identical entries count as one mark on both platforms; conflicting values
    // are ambiguous evidence.
    if marks.any(|duplicate| duplicate != value) {
        return MarkEvidence::Unreadable;
    }
    let Ok(value) = std::str::from_utf8(value) else {
        return MarkEvidence::Unreadable;
    };
    MarkEvidence::Present(value.into())
}

/// Sets the soft and the hard core-dump limit of this process to 0. The same call on
/// macOS and GNU/Linux.
pub(super) fn no_core_dumps() -> io::Result<()> {
    let limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: a valid, initialized limit; the kernel keeps no pointer to it.
    if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) } != 0 {
        let error = io::Error::last_os_error();
        return Err(io::Error::new(
            error.kind(),
            format!("setrlimit(RLIMIT_CORE): {error}"),
        ));
    }
    Ok(())
}

/// Whether both core-dump limits of this process are 0.
#[cfg(any(test, feature = "test-support"))]
pub(super) fn core_dumps_disabled() -> io::Result<bool> {
    let mut limit = std::mem::MaybeUninit::<libc::rlimit>::uninit();
    // SAFETY: the kernel writes one `rlimit` into the storage it is given.
    if unsafe { libc::getrlimit(libc::RLIMIT_CORE, limit.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a successful `getrlimit` initialized it.
    let limit = unsafe { limit.assume_init() };
    Ok(limit.rlim_cur == 0 && limit.rlim_max == 0)
}
