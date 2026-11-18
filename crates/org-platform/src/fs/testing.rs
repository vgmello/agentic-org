//! Test support for consumers of [`full_sync`](super::full_sync) (`test-support`).
//!
//! A consumer's test that proves "this publication was flushed to the device" cannot
//! see the system calls from outside. The adapters count each step that succeeded, per
//! thread, and the test reads the counts before and after the code it exercises.

use std::cell::Cell;

/// The steps of [`full_sync`](super::full_sync) that have succeeded on this thread.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FullSyncCounts {
    /// Successful `fsync` calls.
    pub fsync: u64,
    /// Successful device flushes (`F_FULLFSYNC`). Stays zero on a system whose
    /// `full_sync` has no separate device flush.
    pub device_flush: u64,
}

thread_local! {
    /// Written by the adapters that flush; the refusing adapter flushes nothing.
    pub(super) static COUNTS: Cell<FullSyncCounts> = const {
        Cell::new(FullSyncCounts {
            fsync: 0,
            device_flush: 0,
        })
    };
}

/// This thread's counts since it started.
pub fn full_sync_counts() -> FullSyncCounts {
    COUNTS.get()
}
