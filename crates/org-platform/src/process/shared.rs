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
