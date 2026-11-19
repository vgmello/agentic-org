//! `subscribe`: a connection that follows the organization's log. After the `subscribe` answer a
//! thread of its own pushes `org.changed` events, in log order, each read from the log (so a
//! commit is never missed and never sent twice), and sleeps until the next commit. It also pushes
//! the live events that happen after it subscribed (`message.received`,
//! `message.state_changed`, `member.presence_changed`, see `live`): hints with no replay, and
//! `member.presence_changed` at most once a second for a member (the latest one waits, and a
//! waiting one is dropped if the subscription ends first: a client that subscribes again asks
//! `who`). A subscriber that falls more than the live ring behind is disconnected: it re-lists
//! and subscribes again.
//!
//! A subscription belongs to the binding that made it: any later `msg.bind` on the connection
//! (failed or not) ends it, and the client subscribes again once it is bound. Events go out
//! through the connection's one writer lock, so they never interleave with an answer.

use std::collections::HashMap;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use org_proto::directory::{MemberPresenceChanged, MemberPresenceChangedData};
use org_proto::msg::{MessageReceived, MessageStateChanged};
use org_proto::org::{OrgChangedData, Position};
use org_proto::{
    ApiError, ErrorCode, Event, EventKind, OrgChanged, Request, Subscribe, SubscribeParams,
};

use crate::live::{Head, Live, LiveEvents};

/// How many log entries are read at a time.
const PAGE: usize = 100;

/// How long things may take on a connection.
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    /// The most a follower sleeps before it looks at its connection again (a commit wakes it at
    /// once; this only bounds how long a stopped subscription lingers).
    pub idle: Duration,
    /// The most one frame may take to reach a client; then the connection is dropped.
    pub write: Duration,
    /// The least time between two `member.presence_changed` events for one member.
    pub presence_gap: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            idle: Duration::from_millis(200),
            write: Duration::from_secs(10),
            presence_gap: Duration::from_secs(1),
        }
    }
}

/// What a follower needs of the office. `Err` is the cause, for the server's log only.
pub trait OrgFeed: Send + Sync {
    /// The position of the last log entry.
    fn head(&self) -> Result<Position, String>;
    /// Whether a subscriber that holds the organization as of `after` can be sent what follows.
    fn can_resume_from(&self, after: Position) -> Result<bool, String>;
    /// Up to `limit` commands after `after`, oldest first.
    fn changes_after(&self, after: Position, limit: usize) -> Result<Vec<OrgChangedData>, String>;
    /// A number that changes when a command commits or a live event is published; read it
    /// before reading the log.
    fn token(&self) -> u64;
    /// Waits until the token is no longer `seen`, or `timeout`; returns the token.
    fn wait(&self, seen: u64, timeout: Duration) -> u64;
}

/// Checks a `subscribe` and says where the events start: after `from_position`, or after the
/// head when it is absent (live events only). `bound` is whether the connection is bound.
pub fn begin(
    request: &Request,
    feed: &dyn OrgFeed,
    app_bound: bool,
    already: bool,
) -> Result<Position, ApiError> {
    // Who may subscribe is decided before anything about the params is said. Only the app, for
    // now: a replay from a position shows more than `org.snapshot` does (earlier versions,
    // removed members, other callers' command ids), so a member binding needs its own filter.
    if !app_bound {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "bind before subscribing",
        ));
    }
    let params: SubscribeParams = request.params::<Subscribe>().map_err(|_| {
        ApiError::new(
            ErrorCode::InvalidRequest,
            "the params of `subscribe` are not valid",
        )
    })?;
    if already {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "this connection is already subscribed",
        ));
    }
    let unavailable = || ApiError::new(ErrorCode::StorageError, "the office could not answer");
    // This office has no work feed to resume yet: a client that asks for one takes the snapshot.
    if params.from_work_position.is_some() {
        return Err(ApiError::new(
            ErrorCode::SnapshotRequired,
            "take a fresh snapshot",
        ));
    }
    match params.from_position {
        Some(after) => {
            if feed.can_resume_from(after).map_err(|_| unavailable())? {
                Ok(after)
            } else {
                Err(ApiError::new(
                    ErrorCode::SnapshotRequired,
                    "take a fresh org.snapshot and subscribe from its position",
                ))
            }
        }
        None => feed.head().map_err(|_| unavailable()),
    }
}

/// Where a pusher starts: after this log position, and after these live events.
#[derive(Clone, Copy)]
pub struct Start {
    pub position: Position,
    pub live: Head,
}

/// What a pusher needs of the connection it writes to.
pub struct Wire<'a> {
    pub out: &'a Mutex<UnixStream>,
    /// Moves on when the binding changes or the reader is gone; `mine` is the value this pusher
    /// was made under.
    pub generation: &'a AtomicU64,
    pub mine: u64,
    /// The events sent on the connection so far, whichever subscription sent them.
    pub seq: &'a AtomicU64,
    pub timing: Timing,
}

impl Wire<'_> {
    /// Writes one event. `false` when the pusher must stop: the generation moved on (no event
    /// goes out, and none takes a number it does not send) or the write failed (the connection
    /// is shut down).
    pub(crate) fn emit<K: EventKind>(&self, data: &K::Data) -> bool {
        let stream = self.out.lock().unwrap_or_else(|p| p.into_inner());
        // Checked under the writer lock: once a re-bind has moved the generation on, not one more
        // event goes out.
        if self.generation.load(Ordering::SeqCst) != self.mine {
            return false;
        }
        // Counts per connection, so a client that subscribes again sees no restart and no gap
        // (the number is taken here, under the lock, right before the write).
        let number = self.seq.load(Ordering::SeqCst) + 1;
        let Ok(event) = Event::new::<K>(number, data) else {
            let _ = stream.shutdown(std::net::Shutdown::Both);
            return false;
        };
        if crate::bounded::write_message(&stream, &event, self.timing.write).is_err() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
            return false;
        }
        self.seq.store(number, Ordering::SeqCst);
        true
    }
}

/// Presence events, at most one a `gap` per member: the first goes at once, one that comes sooner
/// waits (a newer one replaces it) and goes when the gap is over.
struct PresenceGate {
    gap: Duration,
    sent: HashMap<String, Instant>,
    waiting: HashMap<String, MemberPresenceChangedData>,
}

impl PresenceGate {
    fn new(gap: Duration) -> Self {
        Self {
            gap,
            sent: HashMap::new(),
            waiting: HashMap::new(),
        }
    }

    /// Takes a new event; `Some` if it may go now.
    fn offer(
        &mut self,
        data: MemberPresenceChangedData,
        now: Instant,
    ) -> Option<MemberPresenceChangedData> {
        let due = self
            .sent
            .get(&data.member_id)
            .is_none_or(|at| now.duration_since(*at) >= self.gap);
        if due {
            self.waiting.remove(&data.member_id);
            self.sent.insert(data.member_id.clone(), now);
            Some(data)
        } else {
            self.waiting.insert(data.member_id.clone(), data);
            None
        }
    }

    /// The events whose gap is over.
    fn due(&mut self, now: Instant) -> Vec<MemberPresenceChangedData> {
        let ready: Vec<String> = self
            .waiting
            .keys()
            .filter(|member| {
                self.sent
                    .get(*member)
                    .is_none_or(|at| now.duration_since(*at) >= self.gap)
            })
            .cloned()
            .collect();
        ready
            .into_iter()
            .filter_map(|member| {
                self.sent.insert(member.clone(), now);
                self.waiting.remove(&member)
            })
            .collect()
    }

    /// How long until the earliest waiting event is due.
    fn next_due(&self, now: Instant) -> Option<Duration> {
        self.waiting
            .keys()
            .filter_map(|member| self.sent.get(member))
            .map(|at| self.gap.saturating_sub(now.duration_since(*at)))
            .min()
    }
}

/// Pushes the events after `start` until the connection's generation moves on from `mine` (a
/// re-bind, or the reader is gone) or a write fails. A failure to read the log or to write an
/// event closes the connection: the client reconnects and resumes from the last position it got.
#[cfg(test)]
pub fn push(feed: &dyn OrgFeed, live: &LiveEvents, wire: &Wire<'_>, start: Start) {
    push_with_hint(feed, live, wire, start, |_| true);
}

pub(crate) fn push_with_hint(
    feed: &dyn OrgFeed,
    live: &LiveEvents,
    wire: &Wire<'_>,
    start: Start,
    deliver_hint: impl Fn(&org_proto::inbox::InboxChangedData) -> bool,
) {
    let mut last = start.position;
    let mut messages_cursor = start.live.messages;
    let mut presence_cursor = start.live.presence;
    let mut presence = PresenceGate::new(wire.timing.presence_gap);
    loop {
        if wire.generation.load(Ordering::SeqCst) != wire.mine {
            return;
        }
        // Before reading, so a commit while reading wakes the next wait at once.
        let token = feed.token();
        // The live events published so far were published after their commits (a message needs
        // its sender and recipient, a status line its member), so the log read below already has
        // what they refer to. Later ones wait for the next round.
        let upto = live.head();
        loop {
            let Ok(batch) = feed.changes_after(last, PAGE) else {
                close(wire.out);
                return;
            };
            if batch.is_empty() {
                break;
            }
            for data in batch {
                if !wire.emit::<OrgChanged>(&data) {
                    return;
                }
                last = data.position;
            }
        }
        let batch = live.since(messages_cursor, upto.messages);
        if batch.lost {
            // The subscriber fell more than a ring behind: some hints are gone. Say so the one
            // way the protocol has, by closing: the client subscribes again and lists again.
            close(wire.out);
            return;
        }
        messages_cursor = batch.next;
        for event in batch.events {
            let sent = match event {
                Live::Received(data) => wire.emit::<MessageReceived>(&data),
                Live::StateChanged(data) => wire.emit::<MessageStateChanged>(&data),
                Live::InboxChanged(data) => deliver_hint(&data),
            };
            if !sent {
                return;
            }
        }
        let (changes, next) = live.presence_since(presence_cursor, upto.presence);
        presence_cursor = next;
        for data in changes {
            if let Some(data) = presence.offer(data, Instant::now()) {
                if !wire.emit::<MemberPresenceChanged>(&data) {
                    return;
                }
            }
        }
        for data in presence.due(Instant::now()) {
            if !wire.emit::<MemberPresenceChanged>(&data) {
                return;
            }
        }
        let sleep = presence
            .next_due(Instant::now())
            .map_or(wire.timing.idle, |due| due.min(wire.timing.idle));
        feed.wait(token, sleep.max(Duration::from_millis(1)));
    }
}

fn close(out: &Mutex<UnixStream>) {
    let stream = out.lock().unwrap_or_else(|p| p.into_inner());
    let _ = stream.shutdown(std::net::Shutdown::Both);
}

#[cfg(test)]
mod tests {
    use org_proto::directory::MemberPresence;

    use super::*;

    fn data(member: &str, status: &str) -> MemberPresenceChangedData {
        MemberPresenceChangedData {
            member_id: member.to_owned(),
            presence: MemberPresence::Offline,
            last_seen_at: "2026-11-09T12:00:00.000Z".to_owned(),
            status: Some(status.to_owned()),
            status_expires_at: None,
            current_task_id: None,
        }
    }

    #[test]
    fn the_first_presence_goes_at_once_a_sooner_one_waits_and_the_latest_wins() {
        let _spawn = crate::transfer::tests::without_process_spawn();
        let gap = Duration::from_secs(1);
        let mut gate = PresenceGate::new(gap);
        let t0 = Instant::now();
        assert_eq!(gate.offer(data("A", "one"), t0), Some(data("A", "one")));
        // Within the gap: held, and a newer one replaces it.
        assert_eq!(
            gate.offer(data("A", "two"), t0 + Duration::from_millis(100)),
            None
        );
        assert_eq!(
            gate.offer(data("A", "three"), t0 + Duration::from_millis(200)),
            None
        );
        assert!(gate.due(t0 + Duration::from_millis(999)).is_empty());
        assert_eq!(
            gate.next_due(t0 + Duration::from_millis(400)),
            Some(Duration::from_millis(600))
        );
        assert_eq!(gate.due(t0 + gap), [data("A", "three")]);
        // Sent again at t0+gap: the next gap starts there.
        assert_eq!(
            gate.offer(data("A", "four"), t0 + gap + Duration::from_millis(10)),
            None
        );
        assert_eq!(
            gate.next_due(t0 + gap + Duration::from_millis(10)),
            Some(Duration::from_millis(990))
        );
    }

    #[test]
    fn members_are_gated_apart_and_one_after_the_gap_goes_at_once() {
        let _spawn = crate::transfer::tests::without_process_spawn();
        let gap = Duration::from_secs(1);
        let mut gate = PresenceGate::new(gap);
        let t0 = Instant::now();
        assert!(gate.offer(data("A", "a"), t0).is_some());
        assert!(
            gate.offer(data("B", "b"), t0).is_some(),
            "another member is not held"
        );
        assert!(
            gate.offer(data("A", "a2"), t0 + gap).is_some(),
            "after the gap it goes now"
        );
        assert_eq!(gate.next_due(t0 + gap), None, "nothing waits");
        // A held event that a later one overtakes after the gap is not sent twice.
        assert!(gate
            .offer(data("B", "b2"), t0 + Duration::from_millis(10))
            .is_none());
        assert!(gate.offer(data("B", "b3"), t0 + gap).is_some());
        assert!(gate.due(t0 + gap * 2).is_empty());
    }
}
