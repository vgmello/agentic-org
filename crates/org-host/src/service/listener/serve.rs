//! Reading one connection: its requests, read in order and handled with at most
//! `max_in_flight` running at once.

use std::io;
use std::os::unix::net::UnixStream;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use org_proto::frame;
use org_proto::{ApiError, ErrorCode, Request, Response, UNREADABLE_REQUEST_ID};

use super::connection::Connection;
use super::outbound::{write_loop, Outbound};
use super::Handler;
#[cfg(test)]
use crate::peer::Accepted;
use crate::peer::Received;
use crate::sync::lock;

/// A reader that counts the bytes consumed from the stream, so each request line's starting
/// offset is known.
pub(super) struct Counted<R> {
    inner: R,
    consumed: u64,
}

impl<R: io::BufRead> io::Read for Counted<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.consumed += n as u64;
        Ok(n)
    }
}

impl<R: io::BufRead> io::BufRead for Counted<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.inner.fill_buf()
    }

    fn consume(&mut self, amount: usize) {
        self.consumed += amount as u64;
        self.inner.consume(amount);
    }
}

/// The calls a connection admitted as root may make: the greeting every connection
/// starts with, and `admin.reset`. Everything else is answered `not_authorized` before any
/// handler sees it.
fn root_may_call(request: &Request) -> bool {
    request.is::<org_proto::Hello>() || request.is::<org_host_control::admin::AdminReset>()
}

/// A connection's reader: reads requests until the client goes away or the stream is out of
/// step, runs each on its own thread, then waits for them and stops the writer.
pub(super) fn serve(
    stream: UnixStream,
    connection: Arc<Connection>,
    queue: Receiver<Outbound>,
    handler: Arc<dyn Handler>,
    max_in_flight: usize,
) {
    serve_with_poll(
        stream,
        connection,
        queue,
        handler,
        max_in_flight,
        org_socket::READ_TICK,
        org_socket::poll_readable,
    );
}

pub(super) fn serve_with_poll(
    stream: UnixStream,
    connection: Arc<Connection>,
    queue: Receiver<Outbound>,
    handler: Arc<dyn Handler>,
    max_in_flight: usize,
    tick: Duration,
    poll: impl FnMut(std::os::fd::RawFd, Duration) -> io::Result<bool>,
) {
    // The writer gets its own handle on the socket, never the `Connection`: that owns the
    // queue's sender, and a writer holding it would keep its own queue open forever.
    let closed = Arc::clone(&connection.closed);
    let writer = stream.try_clone().and_then(|out| {
        thread::Builder::new()
            .name("host-conn-out".into())
            .spawn(move || write_loop(&out, queue, &closed))
    });
    let in_flight = Arc::new(InFlight::new(max_in_flight));
    let mut reader = Counted {
        inner: org_socket::Ticked::with_poll(
            stream,
            tick,
            || connection.closed.load(Ordering::SeqCst),
            poll,
        ),
        consumed: 0,
    };
    loop {
        if connection.closed.load(Ordering::SeqCst) {
            break;
        }
        let sent_at = reader.consumed;
        let Ok(Some(line)) = frame::read_line(&mut reader) else {
            break;
        };
        let request = match crate::service::request::decode(&line) {
            Ok(request) => request,
            Err(_) => {
                connection.clear_recovery_permission();
                connection.send_response(Response::error(
                    crate::service::request::salvage_id(&line).unwrap_or(UNREADABLE_REQUEST_ID),
                    ApiError::new(ErrorCode::InvalidRequest, "the request couldn't be read"),
                ));
                continue;
            }
        };
        if connection.root_authority() && !root_may_call(&request) {
            connection.clear_recovery_permission();
            connection.send_response(Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::NotAuthorized,
                    "root is admitted on this socket for admin.reset only",
                ),
            ));
            continue;
        }
        let received = Received {
            sent_at,
            buffered_after: reader.inner.buffer().len(),
        };
        in_flight.acquire();
        if handler.inline(&request, &connection) {
            // Nothing more is read from the connection until it's answered.
            handler.handle(request, &connection, received);
            in_flight.release();
            continue;
        }
        let id = request.id;
        let worker = {
            let (handler, connection, in_flight) = (
                Arc::clone(&handler),
                Arc::clone(&connection),
                Arc::clone(&in_flight),
            );
            thread::Builder::new()
                .name("host-request".into())
                .spawn(move || {
                    let _done = Release(&in_flight);
                    handler.handle(request, &connection, received);
                })
        };
        if worker.is_err() {
            in_flight.release();
            connection.send_response(Response::error(
                id,
                ApiError::new(ErrorCode::StorageError, "the host is too busy to answer"),
            ));
        }
    }
    connection.closed.store(true, Ordering::SeqCst);
    handler.disconnected(&connection);
    in_flight.wait_idle();
    connection.finish();
    if let Ok(writer) = writer {
        let _ = writer.join();
    }
    connection.close();
}

/// Run the actual connection reader/writer on a connected pair when a test
/// environment cannot bind UNIX listeners. This does not exercise admission.
#[cfg(test)]
pub(in crate::service) fn serve_pair(stream: UnixStream, handler: Arc<dyn Handler>) {
    let (connection, queue) =
        Connection::new(stream.try_clone().unwrap(), Accepted::sample(&stream));
    serve(stream, connection, queue, handler, 4);
}

/// Counts a connection's running requests.
struct InFlight {
    count: Mutex<usize>,
    changed: Condvar,
    max: usize,
}

impl InFlight {
    fn new(max: usize) -> Self {
        Self {
            count: Mutex::new(0),
            changed: Condvar::new(),
            max: max.max(1),
        }
    }

    fn acquire(&self) {
        let mut count = lock(&self.count);
        while *count >= self.max {
            count = self.changed.wait(count).unwrap_or_else(|p| p.into_inner());
        }
        *count += 1;
    }

    fn release(&self) {
        *lock(&self.count) -= 1;
        self.changed.notify_all();
    }

    fn wait_idle(&self) {
        let mut count = lock(&self.count);
        while *count > 0 {
            count = self.changed.wait(count).unwrap_or_else(|p| p.into_inner());
        }
    }
}

/// Releases a request's slot even if its handler panics.
struct Release<'a>(&'a InFlight);

impl Drop for Release<'_> {
    fn drop(&mut self) {
        self.0.release();
    }
}
