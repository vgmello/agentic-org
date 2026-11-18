//! The change stream: what committed, in feed order, from a bounded ring. Nothing polls
//! the projection; a client that falls out of the ring reads again and resubscribes.
//! A subscriber's queue is bounded too: one that does not keep up is dropped, never
//! waited for, and subscribes again.

use std::{
    collections::VecDeque,
    sync::{
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    time::Duration,
};

use crate::{change::DurablePath, journal::Entry, sequences::FeedPosition};

/// One committed entry.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Committed {
    pub feed: FeedPosition,
    pub ordinal: u64,
    /// The operation's kind, when it had an operation ID.
    pub kind: Option<String>,
    /// Every file the entry created, replaced or removed.
    pub paths: Vec<DurablePath>,
}

/// The position is older than the ring (which includes every position from before a
/// restart) or was never issued: read again and subscribe from the position that read
/// returns.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Resync;

/// A stream ends (`None` from [`ChangeStream::recv`]) when the store is gone or when this
/// subscriber fell a whole ring behind and was dropped. Either way: subscribe again after
/// the last position received, and read again if that answers [`Resync`].
pub struct ChangeStream {
    backlog: VecDeque<Arc<Committed>>,
    live: Receiver<Arc<Committed>>,
}

impl ChangeStream {
    /// The next change if one is ready.
    pub fn try_recv(&mut self) -> Option<Arc<Committed>> {
        self.backlog
            .pop_front()
            .or_else(|| self.live.try_recv().ok())
    }

    /// Waits for the next change. `None` once the stream has ended.
    pub fn recv(&mut self) -> Option<Arc<Committed>> {
        self.backlog.pop_front().or_else(|| self.live.recv().ok())
    }

    /// Waits up to `timeout` for the next change.
    pub fn recv_timeout(&mut self, timeout: Duration) -> Option<Arc<Committed>> {
        let ready = self.backlog.pop_front();
        ready.or_else(|| self.live.recv_timeout(timeout).ok())
    }
}

struct Ring {
    /// The oldest position a subscription may start after.
    floor: u64,
    last: u64,
    entries: VecDeque<Arc<Committed>>,
    subscribers: Vec<SyncSender<Arc<Committed>>>,
}

pub(crate) struct Changes {
    capacity: usize,
    ring: Mutex<Ring>,
}

impl Changes {
    pub(crate) fn new(start: FeedPosition, capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            ring: Mutex::new(Ring {
                floor: start.0,
                last: start.0,
                entries: VecDeque::new(),
                subscribers: Vec::new(),
            }),
        }
    }

    pub(crate) fn publish(&self, entry: &Entry) {
        let change = Arc::new(Committed {
            feed: entry.header.feed,
            ordinal: entry.ordinal(),
            kind: entry.header.operation.as_ref().map(|o| o.kind.clone()),
            paths: entry.header.files.iter().map(|f| f.path.clone()).collect(),
        });
        let mut ring = self.ring.lock().unwrap_or_else(|e| e.into_inner());
        ring.last = change.feed.0;
        ring.entries.push_back(Arc::clone(&change));
        if ring.entries.len() > self.capacity {
            if let Some(dropped) = ring.entries.pop_front() {
                ring.floor = dropped.feed.0;
            }
        }
        // Never waits for a subscriber: a full queue, like a gone one, is dropped.
        ring.subscribers
            .retain(|subscriber| subscriber.try_send(Arc::clone(&change)).is_ok());
    }

    pub(crate) fn subscribe(&self, after: FeedPosition) -> Result<ChangeStream, Resync> {
        let mut ring = self.ring.lock().unwrap_or_else(|e| e.into_inner());
        if after.0 < ring.floor || after.0 > ring.last {
            return Err(Resync);
        }
        let backlog = ring.entries.iter().filter(|c| c.feed > after);
        let backlog = backlog.cloned().collect();
        let (sender, live) = mpsc::sync_channel(self.capacity);
        ring.subscribers.push(sender);
        Ok(ChangeStream { backlog, live })
    }
}
