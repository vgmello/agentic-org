//! The limits on a run: a timeout, size caps, and one run per person per feature at a time.

use std::collections::HashSet;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

/// Caps for one run. [`Limits::default`] is the design's (`docs/design/smart-features.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// A run that takes longer is `timeout`, and its answer is thrown away.
    pub timeout: Duration,
    /// Bytes of input and context together (each field also has its own cap in the feature's schema).
    pub max_input_bytes: usize,
    /// Bytes of runtime answer.
    pub max_output_bytes: usize,
    /// Context items one call may carry.
    pub max_context_items: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            timeout: Duration::from_secs(60),
            max_input_bytes: 16 * 1024,
            max_output_bytes: 64 * 1024,
            max_context_items: 8,
        }
    }
}

/// Lets one run per (person, feature) go at a time. A second one is refused at once, not queued.
#[derive(Debug, Default)]
pub struct RunGate {
    active: Mutex<HashSet<(String, String)>>,
}

/// Holds a slot of the [`RunGate`]; dropping it (also on a panic) frees the slot.
#[derive(Debug)]
pub struct RunPermit<'a> {
    gate: &'a RunGate,
    key: (String, String),
}

impl RunGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// The slot for this person and feature, or `None` when a run of theirs is still going.
    pub fn try_acquire(&self, user: &str, feature: &str) -> Option<RunPermit<'_>> {
        let key = (user.to_owned(), feature.to_owned());
        let mut active = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        if !active.insert(key.clone()) {
            return None;
        }
        Some(RunPermit { gate: self, key })
    }
}

impl Drop for RunPermit<'_> {
    fn drop(&mut self) {
        let mut active = self
            .gate
            .active
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        active.remove(&self.key);
    }
}
