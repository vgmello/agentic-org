//! Live events: what happens in the office that is not in the organization's log, kept in a small
//! ring for `subscribe`'s pushers: `message.received` and `message.state_changed` (from the
//! mailbox) and `member.presence_changed` (from a status line). They are hints with no replay:
//! a subscriber gets those after it subscribed, and one that fell more than [`CAPACITY`] behind
//! loses the oldest (it takes `msg.list` / `who` again). They carry headers and cards, never
//! bodies. A publish wakes the followers, which are asleep on the organization's change signal.

use std::collections::VecDeque;
use std::sync::Mutex;

use org_mailbox::{MessageEvent, MessageEvents};
use org_proto::directory::MemberPresenceChangedData;
use org_proto::msg::{MessageReceivedData, MessageStateChangedData};

/// How many events the ring keeps.
pub(crate) const CAPACITY: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Live {
    Received(Box<MessageReceivedData>),
    StateChanged(MessageStateChangedData),
    Presence(MemberPresenceChangedData),
}

#[derive(Default)]
struct Ring {
    /// The number the next event takes (the first is 1; 0 is "before everything").
    next: u64,
    events: VecDeque<(u64, Live)>,
}

pub(crate) struct LiveEvents {
    ring: Mutex<Ring>,
    wake: Box<dyn Fn() + Send + Sync>,
}

impl LiveEvents {
    /// `wake` is called after every publish, outside the ring's lock.
    pub(crate) fn new(wake: Box<dyn Fn() + Send + Sync>) -> Self {
        Self {
            ring: Mutex::new(Ring {
                next: 1,
                events: VecDeque::new(),
            }),
            wake,
        }
    }

    pub(crate) fn publish_live(&self, event: Live) {
        {
            let mut ring = self.ring.lock().unwrap_or_else(|p| p.into_inner());
            let number = ring.next;
            ring.next += 1;
            ring.events.push_back((number, event));
            if ring.events.len() > CAPACITY {
                ring.events.pop_front();
            }
        }
        (self.wake)();
    }

    /// The number of the last event so far (0 before any): a subscriber that starts here sees
    /// only what is published after.
    pub(crate) fn head(&self) -> u64 {
        self.ring.lock().unwrap_or_else(|p| p.into_inner()).next - 1
    }

    /// The events after `after`, oldest first, and the number to ask from next.
    pub(crate) fn since(&self, after: u64) -> (Vec<Live>, u64) {
        let ring = self.ring.lock().unwrap_or_else(|p| p.into_inner());
        let events: Vec<Live> = ring
            .events
            .iter()
            .filter(|(number, _)| *number > after)
            .map(|(_, event)| event.clone())
            .collect();
        (events, (ring.next - 1).max(after))
    }
}

/// The mailbox publishes its events here.
impl MessageEvents for LiveEvents {
    fn publish(&self, event: MessageEvent) {
        match event {
            MessageEvent::Received(data) => self.publish_live(Live::Received(data)),
            MessageEvent::StateChanged(data) => self.publish_live(Live::StateChanged(data)),
            // Any other kind of message event is not pushed yet.
            #[allow(unreachable_patterns)]
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use org_proto::directory::MemberPresence;

    use super::*;

    fn presence(member: &str) -> Live {
        Live::Presence(MemberPresenceChangedData {
            member_id: member.to_owned(),
            presence: MemberPresence::Offline,
            last_seen_at: "2026-11-09T12:00:00.000Z".to_owned(),
            status: None,
            status_expires_at: None,
            current_task_id: None,
        })
    }

    #[test]
    fn a_subscriber_sees_only_what_is_published_after_its_head_and_each_publish_wakes() {
        let woken = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&woken);
        let live = LiveEvents::new(Box::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        live.publish_live(presence("A"));
        let start = live.head();
        assert_eq!(start, 1);
        assert_eq!(live.since(start), (vec![], 1));
        live.publish_live(presence("B"));
        live.publish_live(presence("C"));
        assert_eq!(woken.load(Ordering::SeqCst), 3);
        let (events, cursor) = live.since(start);
        assert_eq!(events, [presence("B"), presence("C")]);
        assert_eq!(cursor, 3);
        assert_eq!(live.since(cursor), (vec![], 3));
    }

    #[test]
    fn the_ring_drops_the_oldest_and_a_late_reader_gets_what_is_left() {
        let live = LiveEvents::new(Box::new(|| {}));
        for n in 0..CAPACITY + 10 {
            live.publish_live(presence(&n.to_string()));
        }
        let (events, cursor) = live.since(0);
        assert_eq!(events.len(), CAPACITY);
        assert_eq!(events[0], presence("10"));
        assert_eq!(cursor as usize, CAPACITY + 10);
    }
}
