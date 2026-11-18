//! Locking that carries on past a panic in another thread, and joining a thread with a bound.

use std::sync::{Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// How often [`join_by`] looks whether its thread has ended.
const JOIN_POLL: Duration = Duration::from_millis(5);

/// Locks a mutex, carrying on past a panic in another thread: every lock here guards plain
/// data that is valid at every step.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Joins `thread` if it ends by `until`, and says whether it did. A thread still running then
/// is left to run on its own (its handle is dropped): a stopping host does not wait past its
/// bound for a thread that waits for something else, such as a write lock of the registry.
pub(crate) fn join_by<T>(thread: JoinHandle<T>, until: Instant) -> bool {
    while !thread.is_finished() {
        if Instant::now() >= until {
            return false;
        }
        thread::sleep(JOIN_POLL);
    }
    let _ = thread.join();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thread_is_joined_when_it_ends_in_time_and_left_when_it_does_not() {
        let quick = thread::spawn(|| thread::sleep(Duration::from_millis(20)));
        assert!(join_by(quick, Instant::now() + Duration::from_secs(10)));
        let (release, released) = std::sync::mpsc::channel::<()>();
        let stuck = thread::spawn(move || {
            let _ = released.recv();
        });
        let asked = Instant::now();
        assert!(!join_by(stuck, asked + Duration::from_millis(100)));
        let took = asked.elapsed();
        assert!(took >= Duration::from_millis(100), "{took:?}");
        assert!(took < Duration::from_secs(1), "{took:?}");
        drop(release);
    }
}
