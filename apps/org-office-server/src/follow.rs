//! `subscribe`: a connection that follows the organization's log. After the `subscribe` answer a
//! thread of its own pushes `org.changed` events, in log order, each read from the log (so a
//! commit is never missed and never sent twice), and sleeps until the next commit.
//!
//! A subscription belongs to the binding that made it: any later `msg.bind` on the connection
//! (failed or not) ends it, and the client subscribes again once it is bound. Events go out
//! through the connection's one writer lock, so they never interleave with an answer.

use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use org_proto::org::{OrgChangedData, Position};
use org_proto::{ApiError, ErrorCode, Event, OrgChanged, Request, Subscribe, SubscribeParams};

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
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            idle: Duration::from_millis(200),
            write: Duration::from_secs(10),
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
    /// A number that changes when a command commits; read it before reading the log.
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

/// Pushes the events after `start` until the connection's generation moves on from `mine` (a
/// re-bind, or the reader is gone) or a write fails. A failure to read the log or to write an
/// event closes the connection: the client reconnects and resumes from the last position it got.
pub fn push(
    feed: &dyn OrgFeed,
    out: &Mutex<UnixStream>,
    generation: &AtomicU64,
    mine: u64,
    start: Position,
    seq: &AtomicU64,
    timing: Timing,
) {
    let mut last = start;
    loop {
        if generation.load(Ordering::SeqCst) != mine {
            return;
        }
        // Before reading, so a commit while reading wakes the next wait at once.
        let token = feed.token();
        loop {
            let Ok(batch) = feed.changes_after(last, PAGE) else {
                close(out);
                return;
            };
            if batch.is_empty() {
                break;
            }
            for data in batch {
                let stream = out.lock().unwrap_or_else(|p| p.into_inner());
                // Checked under the writer lock: once a re-bind has moved the generation on, not
                // one more event goes out, and none takes a number it does not send.
                if generation.load(Ordering::SeqCst) != mine {
                    return;
                }
                // Counts per connection, so a client that subscribes again sees no restart and
                // no gap (the number is taken here, under the lock, right before the write).
                let number = seq.load(Ordering::SeqCst) + 1;
                let Ok(event) = Event::new::<OrgChanged>(number, &data) else {
                    let _ = stream.shutdown(std::net::Shutdown::Both);
                    return;
                };
                if crate::bounded::write_message(&stream, &event, timing.write).is_err() {
                    let _ = stream.shutdown(std::net::Shutdown::Both);
                    return;
                }
                seq.store(number, Ordering::SeqCst);
                last = data.position;
            }
        }
        feed.wait(token, timing.idle);
    }
}

fn close(out: &Mutex<UnixStream>) {
    let stream = out.lock().unwrap_or_else(|p| p.into_inner());
    let _ = stream.shutdown(std::net::Shutdown::Both);
}
