//! Clocks a test controls.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::change::{Clock, Timestamp};

/// Always the same instant.
#[derive(Clone, Copy, Debug)]
pub struct FixedClock(pub Timestamp);

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        self.0
    }
}

/// Advances by `step` milliseconds every time it is read.
#[derive(Debug)]
pub struct SteppingClock {
    next: AtomicU64,
    step: u64,
}

impl SteppingClock {
    pub fn new(start: Timestamp, step: u64) -> Self {
        Self {
            next: AtomicU64::new(start.0),
            step,
        }
    }
}

impl Clock for SteppingClock {
    fn now(&self) -> Timestamp {
        Timestamp(self.next.fetch_add(self.step, Ordering::Relaxed))
    }
}
