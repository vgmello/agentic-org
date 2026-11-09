//! Events to `host.sock` subscribers, in order with the snapshots the host answers.
//!
//! Everything here runs under one lock: building and queueing an event to every subscriber,
//! taking a snapshot (`organization.list`, `office.list`) and queueing its answer, and adding a
//! subscriber after queueing its `subscribe` answer. Callers change their state before they
//! publish, so on any connection an event queued after a snapshot's answer is newer than it, and
//! an event queued before it is already reflected in it: an event is never older than a
//! response written before it (the org-proto host requirement for `office.status_changed`).

use std::sync::{Mutex, MutexGuard, Weak};

use org_proto::EventKind;
use serde_json::Value;

use crate::sync::lock;

/// Something that receives events: a subscribed connection. Queueing never blocks.
pub(crate) trait Subscriber: Send + Sync {
    fn send_event(&self, name: &str, data: Value);
}

#[derive(Default)]
pub(crate) struct Broadcast {
    subscribers: Mutex<Vec<Weak<dyn Subscriber>>>,
}

/// The broadcast lock, held: events published through it go out in one critical section with
/// whatever else runs under it (see [`Broadcast::critical`]).
pub(crate) struct Held<'a> {
    subscribers: MutexGuard<'a, Vec<Weak<dyn Subscriber>>>,
}

impl Held<'_> {
    /// Queues event `E` with `data` to every subscriber.
    pub(crate) fn publish<E: EventKind>(&mut self, data: &E::Data) {
        let Ok(data) = serde_json::to_value(data) else {
            return;
        };
        self.subscribers.retain(|s| s.strong_count() > 0);
        for subscriber in self.subscribers.iter().filter_map(Weak::upgrade) {
            subscriber.send_event(E::NAME, data.clone());
        }
    }
}

impl Broadcast {
    /// Builds event `E`'s data with `data` (which may read state; `None` publishes nothing) and
    /// queues it to every subscriber, all under the lock.
    pub(crate) fn publish<E: EventKind>(&self, data: impl FnOnce() -> Option<E::Data>) {
        self.critical(|held| {
            if let Some(data) = data() {
                held.publish::<E>(&data);
            }
        });
    }

    /// Runs `f` under the lock: a change, the responses that report it and the events that
    /// announce it, all in one critical section, so no other event or snapshot comes between.
    pub(crate) fn critical<R>(&self, f: impl FnOnce(&mut Held<'_>) -> R) -> R {
        let mut held = Held {
            subscribers: lock(&self.subscribers),
        };
        f(&mut held)
    }

    /// Runs `answer`, which takes a snapshot and queues the response carrying it, under the
    /// lock.
    pub(crate) fn snapshot<R>(&self, answer: impl FnOnce() -> R) -> R {
        let _ordered = lock(&self.subscribers);
        answer()
    }

    /// Runs `respond`, which queues the `subscribe` answer and says whether it succeeded, then
    /// adds `subscriber` (once: subscribing again changes nothing), all under the lock. So no
    /// event overtakes the answer, and an event published meanwhile either reaches the new
    /// subscriber or is reflected in every snapshot it takes after the answer.
    pub(crate) fn subscribe(
        &self,
        subscriber: Weak<dyn Subscriber>,
        respond: impl FnOnce() -> bool,
    ) {
        let mut subscribers = lock(&self.subscribers);
        if respond() && !subscribers.iter().any(|s| Weak::ptr_eq(s, &subscriber)) {
            subscribers.push(subscriber);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{mpsc, Arc};
    use std::thread;
    use std::time::Duration;

    /// An event whose data is a state's version.
    struct Version;

    impl EventKind for Version {
        const NAME: &'static str = "version";
        type Data = u64;
    }

    /// Records the versions it's sent, in order.
    #[derive(Default)]
    struct Recorder(Mutex<Vec<u64>>);

    impl Subscriber for Recorder {
        fn send_event(&self, _name: &str, data: Value) {
            lock(&self.0).push(data.as_u64().unwrap());
        }
    }

    /// L2: an event's data is read under the broadcast lock, so an event is never older than
    /// one queued before it. Deterministic: right after the data closure reads the state (v1),
    /// a hook changes it to v2 and publishes that from another thread, then waits for that
    /// publish to finish. With the data read under the lock, that publish can't finish until
    /// this one is queued, so the order is [1, 2]. Were the data read before the lock (the bug
    /// this catches), v2 would be queued first and the stale v1 after it: [2, 1].
    #[test]
    fn event_data_is_read_under_the_lock_so_events_never_go_back_in_time() {
        let broadcast = Arc::new(Broadcast::default());
        let recorder = Arc::new(Recorder::default());
        let subscriber: Weak<dyn Subscriber> = Arc::downgrade(&recorder) as Weak<Recorder>;
        broadcast.subscribe(subscriber, || true);
        let state = Arc::new(AtomicU64::new(1));
        let (published, newer_published) = mpsc::channel();
        let mut newer = None;
        broadcast.publish::<Version>(|| {
            // Under the lock, without depending on timing (`try_lock` doesn't block).
            assert!(
                broadcast.subscribers.try_lock().is_err(),
                "the event's data is read outside the broadcast lock"
            );
            let seen = state.load(Ordering::SeqCst);
            // The hook: the state changes right after this read, and the change is announced.
            let (broadcast, state) = (Arc::clone(&broadcast), Arc::clone(&state));
            newer = Some(thread::spawn(move || {
                state.store(2, Ordering::SeqCst);
                broadcast.publish::<Version>(|| Some(state.load(Ordering::SeqCst)));
                let _ = published.send(());
            }));
            // As long as that publish could take (it can't finish while the lock is held).
            let _ = newer_published.recv_timeout(Duration::from_millis(100));
            Some(seen)
        });
        newer.unwrap().join().unwrap();
        assert_eq!(*lock(&recorder.0), [1, 2]);
    }
}
