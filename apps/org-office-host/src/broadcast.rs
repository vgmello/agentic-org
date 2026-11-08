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
