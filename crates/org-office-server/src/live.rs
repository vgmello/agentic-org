//! Live events: what happens in the office that is not in the organization's log, for
//! `subscribe`'s pushers. `message.received` and `message.state_changed` (from the mailbox) are
//! kept in a ring of [`CAPACITY`]; `member.presence_changed` (from a status line) is kept apart,
//! the latest per member, so a status flood cannot push message hints out. They are hints with
//! no replay: a subscriber gets those after it subscribed. One that falls more than a ring behind
//! is told by [`Batch::lost`]: the pusher ends its subscription (closes the connection) and the
//! client re-lists and subscribes again. They carry headers and cards, never bodies. A publish
//! wakes the followers, which are asleep on the organization's change signal.
//!
//! Agents' coarse live turn activity (`member.activity_changed`, `TASK-103`) is kept like
//! presence, the latest per member and numbered on the presence cursor, so a busy agent's phase
//! changes never push message events out of the ring either. It comes from the session
//! manager's post-redaction stream ([`follow_session_activity`]) and carries a phase and a
//! tool's name only. The Chief of Staff's turns serve one caller's private partition: only the
//! owner gets her tool detail ([`LiveEvents::activity_since`]).

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use org_mailbox::{MessageEvent, MessageEvents};
use org_office_sessions::{SessionStreamEvent, SessionStreamHub, StreamFilter};
use org_proto::directory::MemberPresenceChangedData;
use org_proto::msg::{MessageReceivedData, MessageStateChangedData};
use org_proto::sessions::MemberActivityChangedData;

/// How many message events the ring keeps.
pub(crate) const CAPACITY: usize = 1024;

/// A message event: what the ring holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Live {
    Received(Box<MessageReceivedData>),
    StateChanged(MessageStateChangedData),
    InboxChanged(org_proto::inbox::InboxChangedData),
    /// A committed domain mutation (such as a review change) that refreshes browser
    /// organization/review views via `org.changed` (`TASK-92.2`).
    OrgChanged(u64),
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
/// member and cannot push message events out of the ring. Agents' live activity shares the
/// numbering, one slot per member too.
#[derive(Default)]
struct Presence {
    next: u64,
    latest: HashMap<String, (u64, MemberPresenceChangedData)>,
    activity: HashMap<String, (u64, LatestActivity)>,
}

/// An agent's latest coarse activity; `private` when only the owner may see its tool detail.
struct LatestActivity {
    data: MemberActivityChangedData,
    private: bool,
}

pub(crate) struct LiveEvents {
    ring: Mutex<Ring>,
    presence: Mutex<Presence>,
    wake: Box<dyn Fn() + Send + Sync>,
    mailbox_changes: AtomicU64,
    org_changes: AtomicU64,
    owner_member_id: Option<String>,
}

impl LiveEvents {
    /// `wake` is called after every publish, outside the locks.
    #[cfg(test)]
    pub(crate) fn new(wake: Box<dyn Fn() + Send + Sync>) -> Self {
        Self::with_owner(wake, None)
    }

    /// Creates a live event bus scoped so `inbox.changed` mailbox recount triggers only
    /// advance when `owner_member_id`'s own mailbox changes (`doc-5` B6).
    pub(crate) fn for_owner(wake: Box<dyn Fn() + Send + Sync>, owner_member_id: String) -> Self {
        Self::with_owner(wake, Some(owner_member_id))
    }

    fn with_owner(wake: Box<dyn Fn() + Send + Sync>, owner_member_id: Option<String>) -> Self {
        Self {
            ring: Mutex::new(Ring {
                next: 1,
                events: VecDeque::new(),
            }),
            presence: Mutex::new(Presence {
                next: 1,
                latest: HashMap::new(),
                activity: HashMap::new(),
            }),
            wake,
            mailbox_changes: AtomicU64::new(0),
            org_changes: AtomicU64::new(0),
            owner_member_id,
        }
    }

    pub(crate) fn mailbox_head(&self) -> u64 {
        self.mailbox_changes.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(crate) fn org_changed_head(&self) -> u64 {
        self.org_changes.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(crate) fn org_change_head(&self) -> u64 {
        self.org_changed_head()
    }
    /// Marks one committed change to an Inbox source (owner mailbox, pending tool approval,
    /// budget hold, or review) so `inbox::Hint` recounts and emits `inbox.changed` (`TASK-102`).
    pub(crate) fn record_inbox_changed(&self) {
        self.mailbox_changes.fetch_add(1, Ordering::Release);
        (self.wake)();
    }

    /// Marks one committed domain mutation affecting organization/review views so subscribers
    /// and browser SSE streams receive `org.changed` immediately (`TASK-92.2`).
    pub(crate) fn record_org_changed(&self) {
        let seq = self.org_changes.fetch_add(1, Ordering::AcqRel) + 1;
        self.publish_live(Live::OrgChanged(seq));
    }

    /// Marks one committed change relevant to the owner's mailbox so `inbox::Hint` recounts.
    pub(crate) fn record_mailbox_change(&self) {
        self.record_inbox_changed();
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

    /// An agent's coarse live activity changed (`TASK-103`). Only the latest per member is kept,
    /// and a repeat of it publishes nothing.
    pub(crate) fn publish_activity(&self, data: MemberActivityChangedData, private: bool) {
        {
            let mut presence = self.presence.lock().unwrap_or_else(|p| p.into_inner());
            let unchanged = presence
                .activity
                .get(&data.member_id)
                .is_some_and(|(_, l)| {
                    l.private == private
                        && l.data.phase == data.phase
                        && l.data.active_tool_name == data.active_tool_name
                        && l.data.active_tool_started_at == data.active_tool_started_at
                        && l.data.supports_steer == data.supports_steer
                });
            if unchanged {
                return;
            }
            let number = presence.next;
            presence.next += 1;
            presence.activity.insert(
                data.member_id.clone(),
                (number, LatestActivity { data, private }),
            );
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

impl LiveEvents {
    /// The latest activity of agents changed after `after` and up to `upto` (presence numbers),
    /// oldest first. Without `owner`, a private (Chief of Staff) activity comes without its tool
    /// detail.
    pub(crate) fn activity_since(
        &self,
        after: u64,
        upto: u64,
        owner: bool,
    ) -> Vec<MemberActivityChangedData> {
        let presence = self.presence.lock().unwrap_or_else(|p| p.into_inner());
        let mut changed: Vec<&(u64, LatestActivity)> = presence
            .activity
            .values()
            .filter(|(number, _)| *number > after && *number <= upto)
            .collect();
        changed.sort_by_key(|(number, _)| *number);
        changed
            .into_iter()
            .map(|(_, latest)| {
                if latest.private && !owner {
                    latest.data.clone().without_tool_detail()
                } else {
                    latest.data.clone()
                }
            })
            .collect()
    }
}

/// Feeds every agent's coalesced coarse activity from the session manager's post-redaction
/// stream into `live`'s presence feed (`TASK-103`), until `live` is gone. Holds `live` weakly.
pub(crate) fn follow_session_activity(live: &Arc<LiveEvents>, hub: &SessionStreamHub) -> bool {
    let live = Arc::downgrade(live);
    hub.add_sink(StreamFilter::All, move |frame| {
        let Some(live) = live.upgrade() else {
            return false;
        };
        if let SessionStreamEvent::Activity(activity) = &frame.event {
            live.publish_activity(
                MemberActivityChangedData {
                    member_id: frame.key.employee_id.clone(),
                    phase: activity.phase,
                    active_tool_name: activity.active_tool_name.clone(),
                    active_tool_started_at: activity.active_tool_started_at.clone(),
                    supports_steer: activity.supports_steer,
                    changed_at: frame.at.clone(),
                },
                frame.key.is_private(),
            );
        }
        true
    })
}

/// Connects `sessions`' pending tool approval lifecycle (arrival, decision, cancel, stop) to
/// `live`'s inbox revision counter so `inbox::Hint` recounts and emits `inbox.changed`
/// (`TASK-102`). Holds `live` weakly to avoid reference cycles at office shutdown.
pub(crate) fn follow_session_approvals(
    live: &Arc<LiveEvents>,
    sessions: &org_office_sessions::SessionManager,
) {
    let live = Arc::downgrade(live);
    sessions.set_approval_change_hook(Arc::new(move || {
        if let Some(live) = live.upgrade() {
            live.record_inbox_changed();
        }
    }));
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
        // DeliveryFailed is an additional notification for the same Received commit.
        // Only owner-addressed mailbox changes trigger `inbox.changed` recount (`doc-5` B6).
        let owner_relevant = match (&event, self.owner_member_id.as_deref()) {
            (MessageEvent::DeliveryFailed(_), _) => false,
            (_, None) => true,
            (MessageEvent::Received(data), Some(owner)) => data.header.to_member_id == owner,
            (MessageEvent::StateChanged(data), Some(owner)) => data.to_member_id == owner,
        };
        if owner_relevant {
            self.mailbox_changes.fetch_add(1, Ordering::Release);
        }
        match event {
            MessageEvent::Received(data) => self.publish_live(Live::Received(data)),
            MessageEvent::StateChanged(data) => self.publish_live(Live::StateChanged(data)),
            // The durable notice already reaches the feed through Received. This
            // adapter does not forward the additional delivery-failed notification.
            MessageEvent::DeliveryFailed(_) => {}
        }
    }
}

#[cfg(test)]
mod tests;
