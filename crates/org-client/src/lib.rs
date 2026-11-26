//! # org-client: connect to the host and its offices
//!
//! Opens a connection to this machine's host or to an office server (local Unix sockets,
//! found through `org_proto::location`) and speaks `org-proto`. Used by `agent-os`. Knows nothing about how an office stores its data.
//!
//! A [`Client`] is one long-lived connection that many threads may share. Calls are
//! numbered from 1 and answered by `id`, several may be in flight at once, and every call
//! has a timeout that bounds the whole call, sending included (the protocol's "client
//! duties"). Events (after [`Client::subscribe`]) arrive on a bounded channel.
//!
//! Limits worth knowing:
//! - A call's time is its timeout plus, for the one call whose request is mid-write at its
//!   deadline, up to `WRITE_GRACE` (100 ms) and about a millisecond of polling for queue room.
//! - A call with a huge timeout (even `Duration::MAX`) whose request is stuck mid-write holds
//!   the connection: nothing else can be written until it gives up. Use sensible timeouts.
//! - A [`ClientError::Timeout`] can also report a connection that closed just before the
//!   call's deadline, if the caller only noticed after it.
//!
//! Callers never write to the socket: a call encodes its request and hands it to a writer
//! thread, then waits for its answer. If the request is still being written when the call's
//! time is up, the call closes the connection, which also wakes the blocked write. (A
//! per-write kernel timeout can't be relied on: macOS restarts it whenever the peer takes
//! any bytes, so a server that reads a few bytes at a time would stretch it without end.)
//!
//! A connection that goes wrong is closed, never limped along: a request left half-written
//! may have put half a frame on the wire, a flood of unread events fills the channel, and an
//! unreadable frame leaves the stream out of step. Calls after that fail at once with
//! [`ClientError::Disconnected`].
//!
//! Trust: the peer is a process of the same user that owns the office home (the home is
//! `0700`). The client checks paths, not the peer's identity; it does not authenticate the
//! process on the other end of a socket.
//!
//! The client is blocking and thread-based, like `org_proto::frame`.

#![forbid(unsafe_code)]

pub mod agent;
mod call;
mod connect;
mod error;
mod helper;
mod paths;
mod protocol;
mod transport;

use helper::Helper;
use std::collections::HashMap;
use std::ops::Deref;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use org_proto::{Event, Response};

pub use call::{CallMeta, CallOutcome};
pub use error::ClientError;
pub use paths::{default_host_socket_path, default_office_home};

/// How long a call waits for its answer unless told otherwise.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// The timeout [`Client::call`] gives a call by default: [`DEFAULT_TIMEOUT`] for every
/// method. No call this client makes is slow by nature (an organization is created
/// through the browser API, not through this client). A caller that knows better uses
/// [`Client::call_with_timeout`].
pub fn default_timeout(_method: &str) -> Duration {
    DEFAULT_TIMEOUT
}

/// Events waiting to be read. A reader that falls this far behind loses the connection
/// instead of making the app buffer without limit. The bound is a count, not bytes: an event
/// line is at most `MAX_FRAME_BYTES`, so a server that only sends maximum-size events can make
/// the queue hold about 256 MiB of text (and its parsed form is larger still). The server is the
/// user's own office on a 0600 socket, so this is a limit on a bug, not on an attacker.
const EVENT_QUEUE: usize = 256;

/// How often [`Client::call_unless`] checks whether to give up.
const STOP_POLL: Duration = Duration::from_millis(50);

/// How long dropping a `Client` waits for its reader thread to end before letting it go.
const DROP_JOIN_WAIT: Duration = Duration::from_millis(500);

/// Requests waiting for the writer. A call that finds the queue full waits for room until its
/// own deadline, then times out; the queue being full never closes the connection.
const WRITE_QUEUE: usize = 1024;

/// Bytes of requests waiting for the writer, across the queue. The same limit by size.
const MAX_QUEUED_BYTES: usize = 16 * 1024 * 1024;

/// How long a call whose request is mid-write at its deadline lets the writer settle it before
/// closing the connection itself.
const WRITE_GRACE: Duration = Duration::from_millis(100);

/// Request ids stay below 2^53, so a JavaScript number can hold them (see `org_proto`).
const MAX_REQUEST_ID: u64 = (1 << 53) - 1;

// Where a request is on its way to the wire.
const QUEUED: u8 = 0;
const WRITING: u8 = 1;
const WRITTEN: u8 = 2;
const ABANDONED: u8 = 3;

type Pending = HashMap<u64, SyncSender<Result<Response, ClientError>>>;

/// The subscription's sender, with the id of the [`Subscription`] that owns it. Shared on its
/// own (not through `Shared`) so a live `Subscription` does not keep the connection's socket
/// open after the client is dropped.
type EventSlot = Mutex<Option<(u64, SyncSender<Event>)>>;

struct Shared {
    /// Set when the connection ends or is given up, before pending calls are failed.
    closed: AtomicBool,
    next_id: AtomicU64,
    pending: Mutex<Pending>,
    events: Arc<EventSlot>,
    next_subscription: AtomicU64,
    queued_bytes: AtomicUsize,
    /// To cut the connection from any thread.
    stream: UnixStream,
}

impl Shared {
    /// Gives the connection up: later calls fail at once, waiting ones are woken, the peer is
    /// told to stop, and a write blocked on the socket is woken.
    fn close(&self) {
        self.close_with(|stream| {
            // After the peer has half-closed, macOS refuses shutdown(Both) and Read.
            if stream.shutdown(std::net::Shutdown::Both).is_err() {
                let _ = stream.shutdown(std::net::Shutdown::Read);
                let _ = stream.shutdown(std::net::Shutdown::Write);
            }
        });
    }

    // The callback lets the unit test hold shutdown in progress explicitly.
    fn close_with(&self, shutdown: impl FnOnce(&UnixStream)) {
        // One owner completes shutdown before notifying waiting calls. The half-close
        // test relies on that completion signal before its peer begins reading EOF.
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        shutdown(&self.stream);
        // Dropping the senders wakes every waiting call with `Disconnected`.
        lock(&self.pending).clear();
        *lock(&self.events) = None;
    }
}

/// An encoded request line on its way to the writer thread.
struct Outgoing {
    line: Vec<u8>,
    /// When the caller stops waiting for it.
    deadline: Instant,
    /// `QUEUED` -> `WRITING` -> `WRITTEN` (the writer), or `QUEUED` -> `ABANDONED` (the
    /// caller, which ran out of time before the writer started on it).
    state: Arc<AtomicU8>,
}

/// Server events, from [`Client::subscribe`]. Use it like a `Receiver<Event>` (`recv`,
/// `recv_timeout`, `try_recv`, `iter`, ...). Dropping it releases the subscription on this side,
/// so `subscribe` can be called again. The protocol has no unsubscribe: the server keeps
/// publishing to the connection until it closes, so a second `subscribe_from` can see events
/// the old subscription already delivered (take a new snapshot and use positions to drop
/// duplicates), and what the server does with a repeated `subscribe` is its own contract.
///
/// **A `msg.bind` on the connection ends the subscription on the server** (failed or not):
/// no event follows the bind's answer, and nothing on this side says so. Bind first, then
/// subscribe, and if you ever bind again, drop this `Subscription` and `subscribe_from` again
/// from the last position you got.
#[must_use = "dropping the Subscription ends the subscription"]
pub struct Subscription {
    events: Receiver<Event>,
    id: u64,
    slot: Arc<EventSlot>,
}

impl std::fmt::Debug for Subscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Subscription")
            .field("id", &self.id)
            .finish()
    }
}

impl Deref for Subscription {
    type Target = Receiver<Event>;

    fn deref(&self) -> &Receiver<Event> {
        &self.events
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let mut slot = lock(&self.slot);
        // Only release our own: a newer subscription may already own the slot.
        if matches!(&*slot, Some((id, _)) if *id == self.id) {
            *slot = None;
        }
    }
}

pub struct Client {
    shared: Arc<Shared>,
    /// `None` only while dropping.
    outbound: Option<SyncSender<Outgoing>>,
    reader: Option<Helper>,
    // Kept for explicit test teardown; normal Drop still detaches it.
    _writer: Option<Helper>,
}

impl Drop for Client {
    fn drop(&mut self) {
        self.shared.close();
        // Let the writer thread end once its queue is empty.
        self.outbound.take();
        if let Some(reader) = self.reader.take() {
            // Shutdown normally wakes the reader. If macOS misses that wakeup, Ticked
            // observes `closed` within READ_TICK. Keep this short UI-facing join bound;
            // a reader that outlives it releases its remaining resources at the next tick.
            let give_up_at = Instant::now() + DROP_JOIN_WAIT;
            while !reader.is_finished() && Instant::now() < give_up_at {
                std::thread::sleep(Duration::from_millis(5));
            }
            if reader.is_finished() {
                let _ = reader.join();
            }
        }
    }
}

/// The scoped thread's result; a panic in it is passed on.
fn join_scoped<T>(handle: std::thread::ScopedJoinHandle<'_, T>) -> T {
    match handle.join() {
        Ok(value) => value,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod lifetime_tests;

#[cfg(test)]
mod tests;
