//! Per-employee lifecycle serialization (`TASK-170`).
//!
//! One agent's durable wake queue, suspended-ask record, wake quota, and in-memory open required
//! asks are read-modify-written under that employee's own lock, so a slow operation for one agent
//! never stalls another agent's wakes, sends, or mid-turn deliveries.
//!
//! # Rules
//! - A thread holds at most one employee lock at a time (there is no order between employees, so
//!   cross-employee sweeps take each lock in turn).
//! - An employee lock is never held across an HQ permit RPC, a warm-permit acquisition (with its
//!   worktree and bare-repository disk walks), a bwrap sibling topology scan, or a session spawn.
//! - Lock order with the manager's other locks: employee lock, then `registry`, then a handle's
//!   `state`. Code holding `registry` never acquires an employee lock.
//! - Locks are not re-entrant: a function that takes the lock never calls another that takes it.

use std::collections::HashSet;
use std::sync::{Condvar, Mutex};

/// Keyed mutual exclusion over employee IDs. An entry exists only while its lock is held, so the
/// map never grows with the number of employees ever seen.
#[derive(Default)]
pub(crate) struct EmployeeLockMap {
    held: Mutex<HashSet<String>>,
    released: Condvar,
}

/// Holds one employee's lifecycle lock until dropped (including during unwinding, so a panic in
/// a critical section never leaves the employee locked).
#[must_use = "the employee lock is released as soon as the guard is dropped"]
pub(crate) struct EmployeeLockGuard<'a> {
    map: &'a EmployeeLockMap,
    employee_id: String,
}

impl EmployeeLockMap {
    /// Blocks until no other thread holds `employee_id`'s lock, then takes it.
    pub(crate) fn lock(&self, employee_id: &str) -> EmployeeLockGuard<'_> {
        let mut held = self.held.lock().unwrap_or_else(|p| p.into_inner());
        while held.contains(employee_id) {
            held = self.released.wait(held).unwrap_or_else(|p| p.into_inner());
        }
        held.insert(employee_id.to_owned());
        EmployeeLockGuard {
            map: self,
            employee_id: employee_id.to_owned(),
        }
    }

    /// Number of employee locks currently held (test observability only).
    #[cfg(test)]
    pub(crate) fn held_count(&self) -> usize {
        self.held.lock().unwrap_or_else(|p| p.into_inner()).len()
    }
}

impl Drop for EmployeeLockGuard<'_> {
    fn drop(&mut self) {
        let mut held = self.map.held.lock().unwrap_or_else(|p| p.into_inner());
        held.remove(&self.employee_id);
        drop(held);
        self.map.released.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::time::Duration;

    use super::EmployeeLockMap;

    #[test]
    fn different_employees_never_contend_and_entries_are_freed_on_release() {
        let map = Arc::new(EmployeeLockMap::default());
        let held_a = map.lock("emp-a");
        let (tx, rx) = mpsc::channel();
        let worker_map = Arc::clone(&map);
        let worker = std::thread::spawn(move || {
            let _b = worker_map.lock("emp-b");
            tx.send(()).unwrap();
        });
        rx.recv_timeout(Duration::from_secs(5))
            .expect("employee B must lock while employee A is held");
        worker.join().unwrap();
        assert_eq!(map.held_count(), 1);
        drop(held_a);
        assert_eq!(map.held_count(), 0);
    }

    #[test]
    fn same_employee_is_mutually_exclusive_and_survives_a_panicking_holder() {
        let map = Arc::new(EmployeeLockMap::default());
        let guard = map.lock("emp-a");
        let (tx, rx) = mpsc::channel();
        let waiter_map = Arc::clone(&map);
        let waiter = std::thread::spawn(move || {
            let _a = waiter_map.lock("emp-a");
            tx.send(()).unwrap();
        });
        assert!(
            rx.recv_timeout(Duration::from_millis(100)).is_err(),
            "a second holder of the same employee must wait"
        );
        drop(guard);
        rx.recv_timeout(Duration::from_secs(5))
            .expect("the waiter acquires after release");
        waiter.join().unwrap();

        let panic_map = Arc::clone(&map);
        let panicked = std::thread::spawn(move || {
            let _a = panic_map.lock("emp-a");
            panic!("holder panics inside its critical section");
        })
        .join();
        assert!(panicked.is_err());
        let _again = map.lock("emp-a");
        assert_eq!(map.held_count(), 1);
    }
}
