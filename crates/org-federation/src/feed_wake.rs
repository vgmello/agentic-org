//! Monotonic condition-variable wake token shared between office feeds and federation loops.

use std::sync::{Condvar, Mutex};
use std::time::Duration;

/// Shared monotonic wake token notified on every committed organization change or live event.
#[derive(Default)]
pub struct FeedWake {
    seq: Mutex<u64>,
    cv: Condvar,
}

impl FeedWake {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn notify(&self) {
        let mut seq = self.seq.lock().unwrap_or_else(|p| p.into_inner());
        *seq = seq.saturating_add(1);
        self.cv.notify_all();
    }

    pub fn token(&self) -> u64 {
        *self.seq.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn wait(&self, seen: u64, timeout: Duration) -> u64 {
        let guard = self.seq.lock().unwrap_or_else(|p| p.into_inner());
        if *guard != seen {
            return *guard;
        }
        let (guard, _) = self
            .cv
            .wait_timeout_while(guard, timeout, |cur| *cur == seen)
            .unwrap_or_else(|p| p.into_inner());
        *guard
    }
}
