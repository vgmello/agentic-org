//! [`SessionStreamHub`]: the office's one live stream of what agent turns are doing, after
//! redaction (`TASK-103`; the same primitive serves `TASK-80.4`'s Ask Tess streaming).
//!
//! The pump publishes each frame for its run's [`SessionStreamKey`] (organization, agent and,
//! for the Chief of Staff, the partition the turn serves). Frames are post-redaction and
//! coarse: an [`SessionStreamEvent::Activity`] carries the phase and a tool's *name* only,
//! never a tool's input, output, diff, a file's contents or thinking text (`doc-40` CHAT-4),
//! and is published only when the activity changes (coalesced). Streaming reply text
//! ([`SessionStreamEvent::Delta`]) is display-only and never stored.
//!
//! Subscribers choose what they see with a [`StreamFilter`]:
//! - [`SessionStreamHub::subscribe`] gives a bounded queue ([`SUBSCRIBER_QUEUE`] frames); a
//!   subscriber that falls behind loses frames and is told by
//!   [`SessionStreamSubscription::take_lagged`], and a dropped subscription is forgotten on the
//!   next publish. The pump never blocks on a subscriber.
//! - [`SessionStreamHub::add_sink`] calls a function for every matching frame, on the
//!   publishing thread, outside the hub's lock: it must be quick and never block (the office
//!   bridges coalesced activity into its live presence feed this way). It returns `false` to
//!   unsubscribe.
//!
//! Authorization is the subscriber's business: whoever hands a subscription to a caller checks
//! first that the caller may see that agent (and, for the Chief of Staff, that partition).
//!
//! A leaf: it depends on nothing in this crate.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use org_proto::sessions::ActivityPhase;

/// How many frames one [`SessionStreamSubscription`] queues before it starts losing them.
pub const SUBSCRIBER_QUEUE: usize = 256;
/// How many subscribers (queues and sinks) the hub holds at once; beyond it, subscribing fails.
pub const MAX_SUBSCRIBERS: usize = 256;
/// The longest tool name an activity frame carries, in UTF-8 bytes.
pub const MAX_TOOL_NAME_BYTES: usize = 64;

/// Whose turn a frame is about.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionStreamKey {
    pub org_id: String,
    pub employee_id: String,
    /// The Chief of Staff partition the turn serves (`tess:ask:<caller_id>`, ...); `None` for
    /// every other agent.
    pub partition: Option<String>,
}

impl SessionStreamKey {
    /// A partitioned (Chief of Staff) turn serves one caller's private conversation, so what
    /// it is doing in detail is not for the organization at large.
    pub fn is_private(&self) -> bool {
        self.partition.is_some()
    }
}

/// Coarse live activity of a session's running turn. `phase: None` is idle.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionActivity {
    pub phase: Option<ActivityPhase>,
    /// The running tool's coarse name ([`coarse_tool_name`]), only while
    /// [`ActivityPhase::RunningTool`].
    pub active_tool_name: Option<String>,
    /// When the running tool started (RFC 3339 UTC).
    pub active_tool_started_at: Option<String>,
    /// The runtime takes a person's steer natively, without interrupting the turn.
    pub supports_steer: bool,
}

impl SessionActivity {
    /// The same activity, idle (keeping what the runtime supports).
    pub fn idle(&self) -> Self {
        Self {
            supports_steer: self.supports_steer,
            ..Self::default()
        }
    }

    /// The same activity in `phase` with no tool running.
    pub fn in_phase(&self, phase: ActivityPhase) -> Self {
        Self {
            phase: Some(phase),
            supports_steer: self.supports_steer,
            ..Self::default()
        }
    }
}

/// How a turn ended, coarsely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnEnd {
    Completed,
    /// Interrupted (by a person, or by the runtime).
    Interrupted,
    /// Anything else (failed, refused, a limit, the quota, signed out).
    Failed,
}

/// One thing that happened in a turn.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SessionStreamEvent {
    /// The coarse activity changed (published only on a change).
    Activity(SessionActivity),
    /// Streaming reply text, redacted. Display only: never stored.
    Delta { text: String },
    /// The turn ended.
    TurnEnded { outcome: TurnEnd },
}

/// A published frame: whose turn, when (RFC 3339 UTC), and what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionStreamFrame {
    pub key: SessionStreamKey,
    pub at: String,
    pub event: SessionStreamEvent,
}

/// Which frames a subscriber gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamFilter {
    /// Every frame of the office (a composition-root bridge, e.g. the live presence feed).
    All,
    /// Every frame of one agent, whatever partition its turn serves.
    Employee(String),
    /// One agent's frames for one partition only (e.g. a caller's own `tess:ask:<id>`).
    Partition {
        employee_id: String,
        partition: String,
    },
}

impl StreamFilter {
    fn matches(&self, key: &SessionStreamKey) -> bool {
        match self {
            Self::All => true,
            Self::Employee(id) => *id == key.employee_id,
            Self::Partition {
                employee_id,
                partition,
            } => *employee_id == key.employee_id && key.partition.as_ref() == Some(partition),
        }
    }
}

type Sink = Arc<dyn Fn(&SessionStreamFrame) -> bool + Send + Sync>;

enum Subscriber {
    Queue {
        filter: StreamFilter,
        tx: SyncSender<SessionStreamFrame>,
        lagged: Arc<AtomicBool>,
    },
    Sink {
        filter: StreamFilter,
        sink: Sink,
    },
}

/// The office's live session stream (cheap to clone: clones share the subscribers).
#[derive(Clone, Default)]
pub struct SessionStreamHub {
    subscribers: Arc<Mutex<Vec<Subscriber>>>,
}

impl SessionStreamHub {
    pub fn new() -> Self {
        Self::default()
    }

    /// A bounded queue of the frames `filter` matches, from now on; `None` when the hub already
    /// holds [`MAX_SUBSCRIBERS`].
    pub fn subscribe(&self, filter: StreamFilter) -> Option<SessionStreamSubscription> {
        let mut subscribers = self.lock();
        if subscribers.len() >= MAX_SUBSCRIBERS {
            return None;
        }
        let (tx, rx) = mpsc::sync_channel(SUBSCRIBER_QUEUE);
        let lagged = Arc::new(AtomicBool::new(false));
        subscribers.push(Subscriber::Queue {
            filter,
            tx,
            lagged: Arc::clone(&lagged),
        });
        Some(SessionStreamSubscription { rx, lagged })
    }

    /// Calls `sink` for every frame `filter` matches, from now on, until it returns `false`.
    /// It runs on the publishing (pump) thread outside the hub's lock, so it must be quick,
    /// never block and never subscribe. Returns `false` when the hub already holds
    /// [`MAX_SUBSCRIBERS`].
    pub fn add_sink(
        &self,
        filter: StreamFilter,
        sink: impl Fn(&SessionStreamFrame) -> bool + Send + Sync + 'static,
    ) -> bool {
        let mut subscribers = self.lock();
        if subscribers.len() >= MAX_SUBSCRIBERS {
            return false;
        }
        subscribers.push(Subscriber::Sink {
            filter,
            sink: Arc::new(sink),
        });
        true
    }

    /// How many subscribers the hub holds (a dropped queue counts until the next publish).
    pub fn subscriber_count(&self) -> usize {
        self.lock().len()
    }

    /// Hands `frame` to every matching subscriber without ever blocking: a full queue loses the
    /// frame (and is marked lagged), a dropped one is forgotten, and sinks run after the lock is
    /// released; a sink that returns `false` is forgotten.
    pub fn publish(&self, frame: &SessionStreamFrame) {
        let mut sinks: Vec<Sink> = Vec::new();
        self.lock().retain(|subscriber| match subscriber {
            Subscriber::Queue { filter, tx, lagged } => {
                if !filter.matches(&frame.key) {
                    return true;
                }
                match tx.try_send(frame.clone()) {
                    Ok(()) => true,
                    Err(TrySendError::Full(_)) => {
                        lagged.store(true, Ordering::Release);
                        true
                    }
                    Err(TrySendError::Disconnected(_)) => false,
                }
            }
            Subscriber::Sink { filter, sink } => {
                if filter.matches(&frame.key) {
                    sinks.push(Arc::clone(sink));
                }
                true
            }
        });
        let finished: Vec<Sink> = sinks.into_iter().filter(|sink| !sink(frame)).collect();
        if finished.is_empty() {
            return;
        }
        self.lock().retain(|subscriber| match subscriber {
            Subscriber::Sink { sink, .. } => !finished.iter().any(|f| Arc::ptr_eq(f, sink)),
            Subscriber::Queue { .. } => true,
        });
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Subscriber>> {
        self.subscribers.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// One subscriber's bounded queue (see [`SessionStreamHub::subscribe`]). Dropping it
/// unsubscribes.
pub struct SessionStreamSubscription {
    rx: Receiver<SessionStreamFrame>,
    lagged: Arc<AtomicBool>,
}

impl SessionStreamSubscription {
    /// The next frame, waiting up to `timeout`.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<SessionStreamFrame> {
        self.rx.recv_timeout(timeout).ok()
    }

    /// The next frame, if one is queued.
    pub fn try_recv(&self) -> Option<SessionStreamFrame> {
        self.rx.try_recv().ok()
    }

    /// Whether frames were lost because this subscriber fell behind, since the last call.
    pub fn take_lagged(&self) -> bool {
        self.lagged.swap(false, Ordering::AcqRel)
    }
}

/// What a run publishes under: the hub and its run's key.
#[derive(Clone)]
pub(crate) struct StreamPublisher {
    hub: SessionStreamHub,
    key: SessionStreamKey,
}

impl StreamPublisher {
    pub(crate) fn new(hub: SessionStreamHub, key: SessionStreamKey) -> Self {
        Self { hub, key }
    }

    pub(crate) fn publish(&self, at: String, event: SessionStreamEvent) {
        self.hub.publish(&SessionStreamFrame {
            key: self.key.clone(),
            at,
            event,
        });
    }
}

/// A tool name as an activity frame may carry it: control characters dropped, whitespace
/// collapsed, at most [`MAX_TOOL_NAME_BYTES`] (cut on a character boundary); `None` if nothing
/// is left. The caller passes the already-redacted name.
pub fn coarse_tool_name(name: &str) -> Option<String> {
    let mut out = String::new();
    for word in name.split(|c: char| c.is_whitespace() || c.is_control()) {
        if word.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
        if out.len() > MAX_TOOL_NAME_BYTES {
            break;
        }
    }
    out.truncate(out.floor_char_boundary(MAX_TOOL_NAME_BYTES));
    let trimmed = out.trim_end();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

#[cfg(test)]
#[path = "stream_hub_tests.rs"]
mod tests;
