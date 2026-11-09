//! Live events: what happens in the office that is not in the organization's log, for
//! `subscribe`'s pushers. `message.received` and `message.state_changed` (from the mailbox) are
//! kept in a ring of [`CAPACITY`]; `member.presence_changed` (from a status line) is kept apart,
//! the latest per member, so a status flood cannot push message hints out. They are hints with
//! no replay: a subscriber gets those after it subscribed. One that falls more than a ring behind
//! is told by [`Batch::lost`]: the pusher ends its subscription (closes the connection) and the
//! client re-lists and subscribes again. They carry headers and cards, never bodies. A publish
//! wakes the followers, which are asleep on the organization's change signal.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use org_mailbox::{MessageEvent, MessageEvents};
use org_proto::directory::MemberPresenceChangedData;
use org_proto::msg::{MessageReceivedData, MessageStateChangedData};

/// How many message events the ring keeps.
pub(crate) const CAPACITY: usize = 1024;

/// A message event: what the ring holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Live {
    Received(Box<MessageReceivedData>),
    StateChanged(MessageStateChangedData),
}

/// What a reader got from [`LiveEvents::since`].
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Batch {
    pub events: Vec<Live>,
    /// The number to ask from next.
    pub next: u64,
    /// Events after the reader's cursor were dropped from the full ring before it read them.
    pub lost: bool,
}

#[derive(Default)]
struct Ring {
    /// The number the next event takes (the first is 1; 0 is "before everything").
    next: u64,
    events: VecDeque<(u64, Live)>,
}

/// The latest presence of each member, and when it was set. A status flood takes one slot per
/// member and cannot push message events out of the ring.
#[derive(Default)]
struct Presence {
    next: u64,
    latest: HashMap<String, (u64, MemberPresenceChangedData)>,
}

pub(crate) struct LiveEvents {
    ring: Mutex<Ring>,
    presence: Mutex<Presence>,
    wake: Box<dyn Fn() + Send + Sync>,
}

impl LiveEvents {
    /// `wake` is called after every publish, outside the locks.
    pub(crate) fn new(wake: Box<dyn Fn() + Send + Sync>) -> Self {
        Self {
            ring: Mutex::new(Ring {
                next: 1,
                events: VecDeque::new(),
            }),
            presence: Mutex::new(Presence {
                next: 1,
                latest: HashMap::new(),
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

    /// A member's presence changed. Only the latest per member is kept.
    pub(crate) fn publish_presence(&self, data: MemberPresenceChangedData) {
        {
            let mut presence = self.presence.lock().unwrap_or_else(|p| p.into_inner());
            let number = presence.next;
            presence.next += 1;
            presence
                .latest
                .insert(data.member_id.clone(), (number, data));
        }
        (self.wake)();
    }

    /// Where a subscriber starts so that it sees only what is published after: the last message
    /// event so far and the last presence so far (0 before any).
    pub(crate) fn head(&self) -> Head {
        Head {
            messages: self.ring.lock().unwrap_or_else(|p| p.into_inner()).next - 1,
            presence: self.presence.lock().unwrap_or_else(|p| p.into_inner()).next - 1,
        }
    }

    /// The message events after `after` and up to `upto` (both numbers), oldest first. `lost` if
    /// the ring had dropped some after `after` before this read.
    pub(crate) fn since(&self, after: u64, upto: u64) -> Batch {
        let ring = self.ring.lock().unwrap_or_else(|p| p.into_inner());
        let oldest = ring.events.front().map_or(ring.next, |(number, _)| *number);
        // Numbers `after + 1 .. oldest` existed once and are gone (an empty ring has dropped
        // nothing: `oldest` is then the next number to be made).
        let lost = oldest > after + 1;
        let events: Vec<Live> = ring
            .events
            .iter()
            .filter(|(number, _)| *number > after && *number <= upto)
            .map(|(_, event)| event.clone())
            .collect();
        Batch {
            events,
            next: upto.max(after),
            lost,
        }
    }

    /// The latest presence of members changed after `after` and up to `upto`, oldest first, and
    /// the number to ask from next.
    pub(crate) fn presence_since(
        &self,
        after: u64,
        upto: u64,
    ) -> (Vec<MemberPresenceChangedData>, u64) {
        let presence = self.presence.lock().unwrap_or_else(|p| p.into_inner());
        let mut changed: Vec<&(u64, MemberPresenceChangedData)> = presence
            .latest
            .values()
            .filter(|(number, _)| *number > after && *number <= upto)
            .collect();
        changed.sort_by_key(|(number, _)| *number);
        (
            changed.into_iter().map(|(_, data)| data.clone()).collect(),
            upto.max(after),
        )
    }
}

/// Where a subscriber starts reading the live events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Head {
    pub messages: u64,
    pub presence: u64,
}

/// The mailbox publishes its events here.
impl MessageEvents for LiveEvents {
    fn publish(&self, event: MessageEvent) {
        match event {
            MessageEvent::Received(data) => self.publish_live(Live::Received(data)),
            MessageEvent::StateChanged(data) => self.publish_live(Live::StateChanged(data)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use org_proto::directory::MemberPresence;

    use super::*;

    fn presence(member: &str, status: &str) -> MemberPresenceChangedData {
        MemberPresenceChangedData {
            member_id: member.to_owned(),
            presence: MemberPresence::Offline,
            last_seen_at: "2026-11-09T12:00:00.000Z".to_owned(),
            status: Some(status.to_owned()),
            status_expires_at: None,
            current_task_id: None,
        }
    }

    fn state(id: &str) -> Live {
        Live::StateChanged(MessageStateChangedData {
            id: id.to_owned(),
            thread_id: "T".to_owned(),
            to_member_id: "M".to_owned(),
            state: org_proto::msg::MessageLifecycleState::Read,
            triage_gen: 0,
            snoozed_until: None,
            snooze_after_task: false,
            reason: None,
            expired_waiting_person: false,
            updated_at: "2026-11-09T12:00:00.000Z".to_owned(),
        })
    }

    #[test]
    fn a_subscriber_sees_only_what_is_published_after_its_head_and_each_publish_wakes() {
        let woken = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&woken);
        let live = LiveEvents::new(Box::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        live.publish_live(state("a"));
        live.publish_presence(presence("A", "x"));
        let start = live.head();
        assert_eq!(
            start,
            Head {
                messages: 1,
                presence: 1
            }
        );
        let none = live.since(start.messages, start.messages);
        assert_eq!((none.events.len(), none.next, none.lost), (0, 1, false));
        live.publish_live(state("b"));
        live.publish_live(state("c"));
        live.publish_presence(presence("A", "y"));
        assert_eq!(woken.load(Ordering::SeqCst), 5);
        let head = live.head();
        let got = live.since(start.messages, head.messages);
        assert_eq!(got.events, [state("b"), state("c")]);
        assert_eq!((got.next, got.lost), (3, false));
        assert_eq!(
            live.presence_since(start.presence, head.presence),
            (vec![presence("A", "y")], 2)
        );
        // `upto` holds back what was published after it.
        let held = live.since(start.messages, 2);
        assert_eq!((held.events, held.next), (vec![state("b")], 2));
    }

    #[test]
    fn a_flood_of_presence_cannot_push_message_events_out() {
        let live = LiveEvents::new(Box::new(|| {}));
        live.publish_live(state("urgent"));
        for n in 0..CAPACITY * 3 {
            live.publish_presence(presence("A", &n.to_string()));
        }
        let head = live.head();
        let got = live.since(0, head.messages);
        assert_eq!(got.events, [state("urgent")]);
        assert!(!got.lost);
        let (presences, _) = live.presence_since(0, head.presence);
        assert_eq!(presences.len(), 1, "one slot per member");
        assert_eq!(
            presences[0].status.as_deref(),
            Some(&*(CAPACITY * 3 - 1).to_string())
        );
    }

    #[test]
    fn a_reader_that_fell_behind_the_ring_is_told_it_lost_events() {
        let live = LiveEvents::new(Box::new(|| {}));
        for n in 0..CAPACITY + 10 {
            live.publish_live(state(&n.to_string()));
        }
        let head = live.head();
        let late = live.since(0, head.messages);
        assert!(late.lost, "the first ten are gone");
        assert_eq!(late.events.len(), CAPACITY);
        assert_eq!(late.events[0], state("10"));
        // Exactly one evicted: the reader's next event (11) is still there, but 10 is gone only
        // for a reader at 9.
        assert!(live.since(9, head.messages).lost);
        assert!(!live.since(10, head.messages).lost);
        // One that is just inside the ring lost nothing.
        let ok = live.since(10, head.messages);
        assert!(!ok.lost);
        assert_eq!(ok.events.len(), CAPACITY);
        // And one that is caught up reads nothing.
        let caught_up = live.since(head.messages, head.messages);
        assert!(!caught_up.lost && caught_up.events.is_empty());
    }
}
