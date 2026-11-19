//! Locking that carries on past a panic in another thread.

use std::sync::{Mutex, MutexGuard};

/// Locks a mutex, carrying on past a panic in another thread: every lock here guards plain
/// data that is valid at every step.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
