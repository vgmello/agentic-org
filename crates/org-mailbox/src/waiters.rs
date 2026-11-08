use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::Instant;

use crate::MailboxError;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // Protected operations only change bookkeeping and never invoke user code.
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

#[derive(Default)]
struct Signal {
    generation: Mutex<u64>,
    changed: Condvar,
}

impl Signal {
    fn wake(&self) {
        let mut generation = lock(&self.generation);
        *generation = generation.wrapping_add(1);
        self.changed.notify_all();
    }
}

#[derive(Default)]
struct RegistryState {
    next_id: u64,
    rows: HashMap<u64, (String, Arc<Signal>)>,
}

#[derive(Clone, Default)]
pub(crate) struct WaiterRegistry(Arc<Mutex<RegistryState>>);

impl WaiterRegistry {
    pub(crate) fn register(&self, member: &str) -> Result<Registration, MailboxError> {
        let signal = Arc::new(Signal::default());
        let mut state = lock(&self.0);
        let id = state.next_id;
        state.next_id = id.checked_add(1).ok_or(MailboxError::Unavailable)?;
        state.rows.insert(id, (member.to_owned(), signal.clone()));
        Ok(Registration {
            registry: Arc::downgrade(&self.0),
            id,
            signal,
        })
    }

    pub(crate) fn notify(&self, member: &str) {
        let signals: Vec<_> = lock(&self.0)
            .rows
            .values()
            .filter(|(target, _)| target == member)
            .map(|(_, signal)| signal.clone())
            .collect();
        // Release the registry before acquiring per-waiter locks.
        for signal in signals {
            signal.wake();
        }
    }
}

pub(crate) struct Registration {
    registry: Weak<Mutex<RegistryState>>,
    id: u64,
    signal: Arc<Signal>,
}

impl Registration {
    pub(crate) fn generation(&self) -> u64 {
        *lock(&self.signal.generation)
    }

    pub(crate) fn wait(
        &self,
        observed: u64,
        deadline: Instant,
        cancellation: &Cancellation,
    ) -> WaitOutcome {
        let mut generation = lock(&self.signal.generation);
        loop {
            if cancellation.is_cancelled() {
                return WaitOutcome::Cancelled;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return WaitOutcome::Deadline;
            }
            if *generation != observed {
                return WaitOutcome::Notified;
            }
            let (next, _) = self
                .signal
                .changed
                .wait_timeout(generation, remaining)
                .unwrap_or_else(|error| error.into_inner());
            generation = next;
        }
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        if let Some(registry) = self.registry.upgrade() {
            lock(&registry).rows.remove(&self.id);
        }
    }
}

pub(crate) enum WaitOutcome {
    Notified,
    Deadline,
    Cancelled,
}

struct AttachedWaiter {
    registry: Weak<Mutex<RegistryState>>,
    id: u64,
    signal: Weak<Signal>,
}

#[derive(Default)]
struct CancelState {
    cancelled: AtomicBool,
    waiters: Mutex<Vec<AttachedWaiter>>,
}

impl CancelState {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        let waiters = std::mem::take(&mut *lock(&self.waiters));
        for waiter in waiters {
            if let Some(registry) = waiter.registry.upgrade() {
                lock(&registry).rows.remove(&waiter.id);
            }
            if let Some(signal) = waiter.signal.upgrade() {
                signal.wake();
            }
        }
    }
}

/// Kept by the connection owner. Explicit cancellation or dropping it cancels
/// every worker using the corresponding Cancellation, including blocked waits.
pub struct CancelHandle(Arc<CancelState>);

impl CancelHandle {
    pub fn cancel(&self) {
        self.0.cancel();
    }
}

impl Drop for CancelHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Cloneable worker-side token. Dropping a worker's token does not cancel peers.
#[derive(Clone)]
pub struct Cancellation(Arc<CancelState>);

impl Cancellation {
    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }

    pub(crate) fn attach(&self, registration: &Registration) {
        let mut attached = lock(&self.0.waiters);
        if self.is_cancelled() {
            if let Some(registry) = registration.registry.upgrade() {
                lock(&registry).rows.remove(&registration.id);
            }
            registration.signal.wake();
            return;
        }
        attached.retain(|waiter| waiter.signal.strong_count() != 0);
        attached.push(AttachedWaiter {
            registry: registration.registry.clone(),
            id: registration.id,
            signal: Arc::downgrade(&registration.signal),
        });
    }
}

pub fn cancellation_pair() -> (CancelHandle, Cancellation) {
    let state = Arc::new(CancelState::default());
    (CancelHandle(state.clone()), Cancellation(state))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn drop_handle_unregisters_immediately_even_before_worker_waits() {
        let registry = WaiterRegistry::default();
        let waiter = registry.register("recipient").unwrap();
        let (handle, cancellation) = cancellation_pair();
        cancellation.attach(&waiter);
        let generation = waiter.generation();
        drop(handle);
        assert!(lock(&registry.0).rows.is_empty());
        assert!(matches!(
            waiter.wait(
                generation,
                Instant::now() + Duration::from_secs(10),
                &cancellation
            ),
            WaitOutcome::Cancelled
        ));
    }

    #[test]
    fn notification_before_block_is_not_lost() {
        let registry = WaiterRegistry::default();
        let waiter = registry.register("recipient").unwrap();
        let (_handle, cancellation) = cancellation_pair();
        cancellation.attach(&waiter);
        let generation = waiter.generation();
        registry.notify("recipient");
        assert!(matches!(
            waiter.wait(
                generation,
                Instant::now() + Duration::from_secs(10),
                &cancellation
            ),
            WaitOutcome::Notified
        ));
    }
}
