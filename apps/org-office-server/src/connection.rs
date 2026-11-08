//! One client connection: a bounded outbound queue drained by its own writer thread.
//!
//! Nothing else ever writes to the socket, so responses and events never interleave
//! mid-line, and a client that stops reading can only fill its own queue. When that queue
//! is full the connection is closed rather than letting the server (or other clients) wait.

use std::io::BufWriter;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::Arc;

use org_proto::frame::{self, FrameError};
use org_proto::{ApiError, ErrorCode, Event, Response};
use serde_json::Value;

/// Messages a connection may have waiting to be written.
const OUTBOUND_QUEUE: usize = 256;

pub(crate) enum Outbound {
    Response(Response),
    /// `seq` is assigned by the writer, so it is gap-free in the order events are written.
    Event {
        event: String,
        data: Value,
    },
    /// Ends the writer. Needed because the writer holds the `Connection`, which owns the
    /// queue's sender: the queue can never close by itself.
    Close,
}

pub struct Connection {
    pub(crate) id: u64,
    outbound: SyncSender<Outbound>,
    stream: UnixStream,
    subscribed: AtomicBool,
}

impl Connection {
    /// Returns the connection and the receiving end for its writer thread.
    pub(crate) fn new(id: u64, stream: UnixStream) -> (Arc<Self>, Receiver<Outbound>) {
        let (outbound, queue) = mpsc::sync_channel(OUTBOUND_QUEUE);
        let connection = Arc::new(Self {
            id,
            outbound,
            stream,
            subscribed: AtomicBool::new(false),
        });
        (connection, queue)
    }

    /// Queues a response. If the queue is full the client isn't reading: close it.
    pub(crate) fn send_response(&self, response: Response) {
        self.enqueue(Outbound::Response(response));
    }

    /// Queues an event if this connection has subscribed.
    pub(crate) fn send_event(&self, event: &str, data: &Value) {
        if self.is_subscribed() {
            self.enqueue(Outbound::Event {
                event: event.to_owned(),
                data: data.clone(),
            });
        }
    }

    /// Tells the writer thread to stop once it has written everything queued before this.
    /// Blocks only while the queue is full; if the writer is gone it returns at once.
    pub(crate) fn finish(&self) {
        let _ = self.outbound.send(Outbound::Close);
    }

    pub(crate) fn mark_subscribed(&self) {
        self.subscribed.store(true, Ordering::SeqCst);
    }

    pub(crate) fn is_subscribed(&self) -> bool {
        self.subscribed.load(Ordering::SeqCst)
    }

    /// Closes the socket in both directions, which also ends both of its threads.
    pub(crate) fn close(&self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }

    fn enqueue(&self, message: Outbound) {
        match self.outbound.try_send(message) {
            Ok(()) => {}
            // Full: the client is not reading. Gone: the writer already stopped.
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => self.close(),
        }
    }
}

/// The writer thread's body: writes queued messages until the queue closes or a write fails.
pub(crate) fn write_loop(connection: &Connection, queue: Receiver<Outbound>) {
    let mut out = BufWriter::new(&connection.stream);
    let mut seq = 0u64;
    for message in queue {
        let written = match message {
            Outbound::Close => return,
            Outbound::Response(response) => send_response(&mut out, response),
            Outbound::Event { event, data } => {
                seq += 1;
                frame::write_message(&mut out, &Event { event, seq, data })
            }
        };
        match written {
            Ok(()) => {}
            Err(error) if error.closes_stream() => {
                connection.close();
                return;
            }
            // Only an event can be too large to send and be dropped here; nothing to answer.
            Err(_) => {}
        }
    }
}

/// Writes a response; if it is too large to send, answers that request with an error.
fn send_response(out: &mut impl std::io::Write, response: Response) -> Result<(), FrameError> {
    match frame::write_message(out, &response) {
        Err(FrameError::TooLargeToSend) => frame::write_message(
            out,
            &Response::error(
                response.id,
                ApiError::new(ErrorCode::StorageError, "the response is too large to send"),
            ),
        ),
        other => other,
    }
}
