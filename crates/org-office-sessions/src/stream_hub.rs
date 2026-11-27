//! [`SessionStreamHub`]: live post-redaction stream of agent turn activity and deltas (`TASK-103`).
//! Leaf module: authorization is enforced by callers before subscribing.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use org_proto::sessions::ActivityPhase;

pub const SUBSCRIBER_QUEUE: usize = 256;
pub const MAX_SUBSCRIBERS: usize = 256;
pub const MAX_TOOL_NAME_BYTES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionStreamKey {
    pub org_id: String,
    pub employee_id: String,
    pub partition: Option<String>,
}

impl SessionStreamKey {
    pub fn is_private(&self) -> bool {
        self.partition.is_some()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionActivity {
    pub phase: Option<ActivityPhase>,
    pub active_tool_name: Option<String>,
    pub active_tool_started_at: Option<String>,
    pub supports_steer: bool,
}

impl SessionActivity {
    pub fn idle(&self) -> Self {
        Self {
            supports_steer: self.supports_steer,
            ..Self::default()
        }
    }

    pub fn in_phase(&self, phase: ActivityPhase) -> Self {
        Self {
            phase: Some(phase),
            supports_steer: self.supports_steer,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnEnd {
    Completed,
    Interrupted,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SessionStreamEvent {
    Activity(SessionActivity),
    Delta { text: String },
    TurnEnded { outcome: TurnEnd },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionStreamFrame {
    pub key: SessionStreamKey,
    pub at: String,
    pub event: SessionStreamEvent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamFilter {
    All,
    Employee(String),
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
        /// Names the queue for its [`SessionStreamSubscription`]'s drop.
        id: u64,
        filter: StreamFilter,
        tx: SyncSender<SessionStreamFrame>,
        lagged: Arc<AtomicBool>,
    },
    Sink {
        filter: StreamFilter,
        sink: Sink,
    },
}

#[derive(Default)]
struct Subscribers {
    next_queue_id: u64,
    list: Vec<Subscriber>,
}

impl Subscribers {
    fn remove_queue(&mut self, queue_id: u64) {
        self.list
            .retain(|s| !matches!(s, Subscriber::Queue { id, .. } if *id == queue_id));
    }
}

#[derive(Clone, Default)]
pub struct SessionStreamHub {
    subscribers: Arc<Mutex<Subscribers>>,
}

impl SessionStreamHub {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn subscribe(&self, filter: StreamFilter) -> Option<SessionStreamSubscription> {
        let mut subscribers = self.lock();
        if subscribers.list.len() >= MAX_SUBSCRIBERS {
            return None;
        }
        let id = subscribers.next_queue_id;
        subscribers.next_queue_id = id.wrapping_add(1);
        let (tx, rx) = mpsc::sync_channel(SUBSCRIBER_QUEUE);
        let lagged = Arc::new(AtomicBool::new(false));
        subscribers.list.push(Subscriber::Queue {
            id,
            filter,
            tx,
            lagged: Arc::clone(&lagged),
        });
        Some(SessionStreamSubscription {
            rx,
            lagged,
            hub: Arc::downgrade(&self.subscribers),
            id,
        })
    }

    pub fn add_sink(
        &self,
        filter: StreamFilter,
        sink: impl Fn(&SessionStreamFrame) -> bool + Send + Sync + 'static,
    ) -> bool {
        let mut subscribers = self.lock();
        if subscribers.list.len() >= MAX_SUBSCRIBERS {
            return false;
        }
        subscribers.list.push(Subscriber::Sink {
            filter,
            sink: Arc::new(sink),
        });
        true
    }

    pub fn subscriber_count(&self) -> usize {
        self.lock().list.len()
    }

    pub fn publish(&self, frame: &SessionStreamFrame) {
        let mut sinks: Vec<Sink> = Vec::new();
        self.lock().list.retain(|subscriber| match subscriber {
            Subscriber::Queue {
                filter, tx, lagged, ..
            } => {
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
        self.lock().list.retain(|subscriber| match subscriber {
            Subscriber::Sink { sink, .. } => !finished.iter().any(|f| Arc::ptr_eq(f, sink)),
            Subscriber::Queue { .. } => true,
        });
    }

    fn lock(&self) -> MutexGuard<'_, Subscribers> {
        lock_subscribers(&self.subscribers)
    }
}

fn lock_subscribers(subscribers: &Mutex<Subscribers>) -> MutexGuard<'_, Subscribers> {
    subscribers.lock().unwrap_or_else(|p| p.into_inner())
}

/// One subscriber's bounded queue (see [`SessionStreamHub::subscribe`]). Dropping it
/// unsubscribes at once.
pub struct SessionStreamSubscription {
    rx: Receiver<SessionStreamFrame>,
    lagged: Arc<AtomicBool>,
    /// The hub's subscribers, without keeping the hub alive.
    hub: Weak<Mutex<Subscribers>>,
    id: u64,
}

impl Drop for SessionStreamSubscription {
    fn drop(&mut self) {
        if let Some(subscribers) = self.hub.upgrade() {
            lock_subscribers(&subscribers).remove_queue(self.id);
        }
    }
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
