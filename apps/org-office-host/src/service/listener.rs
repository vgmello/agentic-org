//! The host's minimal `org-proto` listener: `host.sock`, one reader thread per connection,
//! a thread per request, and one writer thread per connection so responses and events never
//! interleave mid-line. Knows nothing about methods: every readable [`Request`] goes to the
//! [`Handler`]. It grows no router; it is replaced by the shared server crate once that exists.

use std::collections::HashMap;
use std::fs;
use std::io::{self, BufWriter};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use org_proto::frame::{self, FrameError};
use org_proto::{salvage_id, ApiError, ErrorCode, Event, Request, Response, UNREADABLE_REQUEST_ID};
use serde_json::Value;

use crate::cloexec::SPAWN_LOCK;
use crate::host_log;
use crate::peer::{peer_uid, Accepted, Received};
use crate::sync::lock;

/// How long the accept thread waits for a connection before checking whether to stop.
const ACCEPT_POLL: Duration = Duration::from_millis(100);

/// The pause after an accept error (e.g. out of fds), so a lasting one doesn't spin.
const ACCEPT_ERROR_PAUSE: Duration = Duration::from_millis(50);

/// Messages one connection may have waiting to be written. A client that lets this fill isn't
/// reading, and is disconnected rather than slowing anyone else down.
const OUTBOUND_QUEUE: usize = 256;

/// Answers requests through `connection`. Each call runs on its own thread, except those
/// [`Handler::inline`] picks, which run on the connection's reader thread.
pub(crate) trait Handler: Send + Sync + 'static {
    fn handle(&self, request: Request, connection: &Arc<Connection>, received: Received);

    /// Whether `request` is handled on the connection's reader thread, before anything more is
    /// read from the connection (the app-only calls: their check needs to know that nothing
    /// arrived after the request).
    fn inline(&self, request: &Request) -> bool {
        let _ = request;
        false
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ListenerConfig {
    /// Connections beyond this are closed on arrival.
    pub(crate) max_connections: usize,
    /// Requests one connection may have running at once; reading pauses beyond it.
    pub(crate) max_in_flight: usize,
    /// Only peers running as this user are served.
    pub(crate) expected_uid: u32,
}

/// Why the socket couldn't be bound.
#[derive(Debug)]
pub(crate) enum BindError {
    /// Something that isn't a socket is where the socket goes.
    NotASocket,
    Io(io::Error),
}

/// Binds `path` owner-only (0600), first removing a stale socket left there. Only a socket is
/// ever removed: the caller holds `host.lock`, so a socket there can only be a dead host's.
pub(crate) fn bind(path: &Path) -> Result<UnixListener, BindError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_socket() => fs::remove_file(path).map_err(BindError::Io)?,
        Ok(_) => return Err(BindError::NotASocket),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(BindError::Io(e)),
    }
    // See `cloexec`: the socket is made close-on-exec in a second step.
    let listener = {
        let _no_spawn_meanwhile = lock(&SPAWN_LOCK);
        UnixListener::bind(path).map_err(BindError::Io)?
    };
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(BindError::Io)?;
    Ok(listener)
}

/// Whether the peer runs as `expected_uid` (defence in depth: `host.sock` is already 0600 in
/// an owner-only folder). A peer whose uid can't be read is refused.
pub(crate) fn peer_allowed(stream: &UnixStream, expected_uid: u32) -> bool {
    matches!(peer_uid(stream), Ok(uid) if uid == expected_uid)
}

/// A running listener.
pub(crate) struct Listener {
    stopping: Arc<AtomicBool>,
    live: Arc<Mutex<HashMap<u64, Live>>>,
    accept: Option<JoinHandle<()>>,
}

struct Live {
    connection: Arc<Connection>,
    reader: JoinHandle<()>,
}

impl Listener {
    /// Serves `listener` until [`Listener::close`].
    pub(crate) fn start(
        listener: UnixListener,
        config: ListenerConfig,
        handler: Arc<dyn Handler>,
    ) -> io::Result<Self> {
        // Non-blocking, so the accept thread can poll and take `SPAWN_LOCK` only to accept.
        listener.set_nonblocking(true)?;
        let stopping = Arc::new(AtomicBool::new(false));
        let live = Arc::new(Mutex::new(HashMap::new()));
        let accept = {
            let (stopping, live) = (Arc::clone(&stopping), Arc::clone(&live));
            thread::Builder::new()
                .name("host-accept".into())
                .spawn(move || accept_loop(&listener, &config, &handler, &stopping, &live))?
        };
        Ok(Self {
            stopping,
            live,
            accept: Some(accept),
        })
    }

    /// Stops accepting (within [`ACCEPT_POLL`]) and closes every connection. Requests still
    /// running finish (their answers are dropped); [`Listener::join`] waits for them.
    pub(crate) fn close(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }
        for live in lock(&self.live).values() {
            live.connection.close();
        }
    }

    /// Waits for every connection's threads, including its running requests.
    pub(crate) fn join(&mut self) {
        let live: Vec<Live> = lock(&self.live).drain().map(|(_, live)| live).collect();
        for live in live {
            live.connection.close();
            let _ = live.reader.join();
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.close();
        self.join();
    }
}

fn accept_loop(
    listener: &UnixListener,
    config: &ListenerConfig,
    handler: &Arc<dyn Handler>,
    stopping: &AtomicBool,
    live: &Mutex<HashMap<u64, Live>>,
) {
    let mut next_id = 1u64;
    let mut failing = false;
    while !stopping.load(Ordering::SeqCst) {
        if !readable(listener, ACCEPT_POLL) {
            continue;
        }
        // See `cloexec`: on macOS an accepted socket is made close-on-exec in a second step,
        // so no spawn may happen in between. (Gone once servers are spawned with
        // `POSIX_SPAWN_CLOEXEC_DEFAULT` through `org-supervise`.)
        let accepted = {
            let _no_spawn_meanwhile = lock(&SPAWN_LOCK);
            listener.accept()
        };
        let stream = match accepted {
            Ok((stream, _)) => {
                failing = false;
                stream
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                continue
            }
            Err(e) => {
                if !failing {
                    host_log::log(&format!("accepting a connection failed: {e}"));
                    failing = true;
                }
                thread::sleep(ACCEPT_ERROR_PAUSE);
                continue;
            }
        };
        // macOS hands out accepted sockets non-blocking like the listener; the threads below
        // read and write them blocking.
        if stream.set_nonblocking(false).is_err() {
            continue;
        }
        if !peer_allowed(&stream, config.expected_uid) {
            host_log::log("refused a connection from another user");
            continue; // dropping `stream` closes it
        }
        let mut table = lock(live);
        table.retain(|_, live| !live.reader.is_finished());
        if table.len() >= config.max_connections {
            continue;
        }
        let Ok(for_writer) = stream.try_clone() else {
            continue;
        };
        let id = next_id;
        next_id += 1;
        let (connection, queue) = Connection::new(for_writer, Accepted::sample(&stream));
        let reader = {
            let (handler, connection) = (Arc::clone(handler), Arc::clone(&connection));
            let max_in_flight = config.max_in_flight;
            thread::Builder::new()
                .name(format!("host-conn-{id}"))
                .spawn(move || serve(stream, connection, queue, handler, max_in_flight))
        };
        if let Ok(reader) = reader {
            table.insert(id, Live { connection, reader });
        }
    }
}

/// A reader that counts the bytes consumed from the stream, so each request line's starting
/// offset is known.
struct Counted<R> {
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

/// Whether `listener` has a connection waiting, within `wait`.
fn readable(listener: &UnixListener, wait: Duration) -> bool {
    let mut fd = libc::pollfd {
        fd: listener.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let ms = libc::c_int::try_from(wait.as_millis()).unwrap_or(libc::c_int::MAX);
    // SAFETY: polls one valid pollfd, owned by this frame, for at most `ms`.
    unsafe { libc::poll(&mut fd, 1, ms) > 0 }
}

/// A connection's reader: reads requests until the client goes away or the stream is out of
/// step, runs each on its own thread, then waits for them and stops the writer.
fn serve(
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

fn serve_with_poll(
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
        let request = match serde_json::from_slice::<Request>(&line) {
            Ok(request) => request,
            Err(_) => {
                connection.send_response(Response::error(
                    salvage_id(&line).unwrap_or(UNREADABLE_REQUEST_ID),
                    ApiError::new(ErrorCode::InvalidRequest, "the request couldn't be read"),
                ));
                continue;
            }
        };
        let received = Received {
            sent_at,
            buffered_after: reader.inner.buffer().len(),
        };
        in_flight.acquire();
        if handler.inline(&request) {
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
    in_flight.wait_idle();
    connection.finish();
    if let Ok(writer) = writer {
        let _ = writer.join();
    }
    connection.close();
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

enum Outbound {
    Response(Response),
    /// A response line already encoded, holding a secret: written as is, then zeroed.
    Secret(SecretLine),
    /// `seq` is given by the writer, so it counts without gaps in the order events are written.
    Event {
        name: String,
        data: Value,
    },
    /// Ends the writer once everything queued before it is written.
    Close,
}

/// An encoded response line holding a secret (`office.app_token`). It never passes through a
/// `serde_json::Value` or the writer's buffer, and is zeroed when dropped (best effort: the
/// socket's kernel buffers still hold it until the client reads it).
pub(crate) struct SecretLine(Vec<u8>);

impl SecretLine {
    /// `bytes` is one JSON line, `\n` included.
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

impl Drop for SecretLine {
    fn drop(&mut self) {
        crate::secrets::zero(&mut self.0);
    }
}

/// One client connection, as handlers see it: a place to queue responses and events.
pub(crate) struct Connection {
    outbound: SyncSender<Outbound>,
    stream: UnixStream,
    /// What the host noted about the client when it accepted the connection (`app_auth`).
    accepted: Accepted,
    /// Ends the ticked reader even if shutdown misses a platform wakeup.
    closed: Arc<AtomicBool>,
}

impl Connection {
    fn new(stream: UnixStream, accepted: Accepted) -> (Arc<Self>, Receiver<Outbound>) {
        let (outbound, queue) = mpsc::sync_channel(OUTBOUND_QUEUE);
        let connection = Self {
            outbound,
            stream,
            accepted,
            closed: Arc::new(AtomicBool::new(false)),
        };
        (Arc::new(connection), queue)
    }

    /// Drain the refusal before closing, but stop even already-buffered requests.
    pub(crate) fn refuse_and_finish(&self, response: Response) {
        self.closed.store(true, Ordering::SeqCst);
        self.enqueue(Outbound::Response(response));
        self.enqueue(Outbound::Close);
    }

    /// What the host noted about the client when it accepted the connection.
    pub(crate) fn accepted(&self) -> Accepted {
        self.accepted
    }

    /// The connection's socket, to identify the client (`app_auth`).
    pub(crate) fn socket(&self) -> &UnixStream {
        &self.stream
    }

    /// Queues an encoded response line that holds a secret. Never blocks, like
    /// [`Connection::send_response`].
    pub(crate) fn send_secret_line(&self, line: SecretLine) {
        self.enqueue(Outbound::Secret(line));
    }

    /// Queues a response. Never blocks: a client whose queue is full is disconnected.
    pub(crate) fn send_response(&self, response: Response) {
        self.enqueue(Outbound::Response(response));
    }

    /// Queues an event. Never blocks, like [`Connection::send_response`].
    pub(crate) fn send_event(&self, name: &str, data: Value) {
        self.enqueue(Outbound::Event {
            name: name.to_owned(),
            data,
        });
    }

    /// Closes the socket both ways, which ends the connection's threads.
    pub(crate) fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        shut_down(&self.stream);
    }

    fn finish(&self) {
        let _ = self.outbound.send(Outbound::Close);
    }

    fn enqueue(&self, message: Outbound) {
        match self.outbound.try_send(message) {
            Ok(()) => {}
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => self.close(),
        }
    }
}

impl crate::broadcast::Subscriber for Connection {
    fn send_event(&self, name: &str, data: Value) {
        Connection::send_event(self, name, data);
    }
}

/// Shuts a socket down both ways: writing first, then reading, each on its own. Not
/// `Shutdown::Both`: on macOS that fails (`ENOTCONN`) once the client has shut down its own
/// writing, and then shuts nothing down, so a client that half-closed and reads to EOF would
/// wait forever (the socket stays open while anything holds a copy of it).
fn shut_down(stream: &UnixStream) {
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let _ = stream.shutdown(std::net::Shutdown::Read);
}

/// The writer: writes queued messages until [`Outbound::Close`], until every sender is gone,
/// or until a write fails (then it closes the socket, which also ends the reader).
fn write_loop(stream: &UnixStream, queue: Receiver<Outbound>, closed: &AtomicBool) {
    struct MarkClosed<'a>(&'a AtomicBool);
    impl Drop for MarkClosed<'_> {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let _closed = MarkClosed(closed);
    let mut out = BufWriter::new(stream);
    let mut seq = 0u64;
    for message in queue {
        let written = match message {
            Outbound::Close => return,
            Outbound::Response(response) => write_response(&mut out, response),
            Outbound::Secret(line) => {
                // Past the writer's buffer, straight to the socket, so no copy stays there.
                let written = io::Write::flush(&mut out).and_then(|()| {
                    let mut socket: &UnixStream = out.get_ref();
                    io::Write::write_all(&mut socket, &line.0)
                });
                if written.is_err() {
                    shut_down(stream);
                    return;
                }
                continue;
            }
            Outbound::Event { name, data } => {
                seq += 1;
                frame::write_message(
                    &mut out,
                    &Event {
                        event: name,
                        seq,
                        data,
                    },
                )
            }
        };
        if let Err(e) = written {
            if e.closes_stream() {
                shut_down(stream);
                return;
            }
        }
    }
}

/// Writes a response; one too large to send is answered with an error instead.
fn write_response(out: &mut impl io::Write, response: Response) -> Result<(), FrameError> {
    match frame::write_message(out, &response) {
        Err(FrameError::TooLargeToSend) => frame::write_message(
            out,
            &Response::error(
                response.id,
                ApiError::new(ErrorCode::StorageError, "the answer is too large to send"),
            ),
        ),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, Write};
    use std::path::PathBuf;

    #[test]
    fn refusal_marks_reader_closed_and_queues_response_before_close() {
        let (stream, _peer) = UnixStream::pair().unwrap();
        let (connection, queue) =
            Connection::new(stream.try_clone().unwrap(), Accepted::sample(&stream));
        connection.refuse_and_finish(Response::error(
            9,
            ApiError::new(ErrorCode::UnsupportedVersion, "unsupported"),
        ));
        assert!(connection.closed.load(Ordering::SeqCst));
        assert!(matches!(queue.recv().unwrap(),Outbound::Response(response) if response.id==9));
        assert!(matches!(queue.recv().unwrap(), Outbound::Close));
    }

    #[test]
    fn refusal_prevents_dispatch_of_an_already_buffered_suffix() {
        use std::sync::atomic::AtomicUsize;
        struct Closing(Arc<AtomicUsize>);
        impl Handler for Closing {
            fn inline(&self, _: &Request) -> bool {
                true
            }
            fn handle(&self, request: Request, connection: &Arc<Connection>, received: Received) {
                let call = self.0.fetch_add(1, Ordering::SeqCst);
                if call == 0 {
                    assert!(
                        received.buffered_after > 0,
                        "suffix must already be buffered"
                    );
                }
                connection.refuse_and_finish(Response::error(
                    request.id,
                    ApiError::new(ErrorCode::UnsupportedVersion, "unsupported"),
                ));
            }
        }
        let (stream, mut peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        peer.write_all(b"{\"id\":1,\"method\":\"hello\"}\n{\"id\":2,\"method\":\"suffix\"}\n")
            .unwrap();
        let (connection, queue) =
            Connection::new(stream.try_clone().unwrap(), Accepted::sample(&stream));
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        let worker =
            thread::spawn(move || serve(stream, connection, queue, Arc::new(Closing(observed)), 4));
        let mut reader = io::BufReader::new(peer);
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        assert_eq!(serde_json::from_str::<Response>(&line).unwrap().id, 1);
        line.clear();
        assert_eq!(reader.read_line(&mut line).unwrap(), 0);
        worker.join().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn ticked_reader_repolls_a_missed_eof_at_the_host_call_site() {
        let (stream, peer) = UnixStream::pair().unwrap();
        drop(peer);
        let (connection, queue) =
            Connection::new(stream.try_clone().unwrap(), Accepted::sample(&stream));
        let polls = std::cell::Cell::new(0);
        serve_with_poll(
            stream,
            Arc::clone(&connection),
            queue,
            Arc::new(Echo),
            1,
            org_socket::READ_TICK,
            |fd, _| {
                polls.set(polls.get() + 1);
                if polls.get() == 1 {
                    Ok(false)
                } else {
                    org_socket::poll_readable(fd, Duration::from_millis(1))
                }
            },
        );
        assert_eq!(polls.get(), 2);
        assert!(connection.closed.load(Ordering::SeqCst));
    }

    #[test]
    fn ticked_reader_observes_host_close_at_the_first_idle_tick() {
        let (stream, _peer) = UnixStream::pair().unwrap();
        let (connection, queue) =
            Connection::new(stream.try_clone().unwrap(), Accepted::sample(&stream));
        let polls = std::cell::Cell::new(0);
        serve_with_poll(
            stream,
            Arc::clone(&connection),
            queue,
            Arc::new(Echo),
            1,
            org_socket::READ_TICK,
            |_, _| {
                polls.set(polls.get() + 1);
                connection.close();
                Ok(false)
            },
        );
        assert_eq!(polls.get(), 1);
    }

    struct Echo;

    impl Handler for Echo {
        fn handle(&self, request: Request, connection: &Arc<Connection>, _received: Received) {
            connection.send_response(Response {
                id: request.id,
                outcome: org_proto::Outcome::Result(Value::String(request.method)),
            });
        }
    }

    fn start(dir: &Path, expected_uid: u32) -> (Listener, PathBuf) {
        let path = dir.join("t.sock");
        let config = ListenerConfig {
            max_connections: 2,
            max_in_flight: 4,
            expected_uid,
        };
        let listener = Listener::start(bind(&path).unwrap(), config, Arc::new(Echo));
        (listener.unwrap(), path)
    }

    fn short_tempdir() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("ohl")
            .tempdir_in("/tmp")
            .unwrap()
    }

    /// Connects with a read timeout, set at once: macOS refuses (EINVAL) to set one on a
    /// socket whose peer already closed, and a closed socket reads as EOF anyway.
    fn connect(path: &Path) -> UnixStream {
        let stream = UnixStream::connect(path).unwrap();
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        stream
    }

    fn read_line(stream: &UnixStream) -> Option<String> {
        let mut line = String::new();
        let n = io::BufReader::new(stream).read_line(&mut line).unwrap_or(0);
        (n > 0).then_some(line)
    }

    #[test]
    fn a_peer_of_another_uid_is_refused() {
        let (a, _b) = UnixStream::pair().unwrap();
        let me = crate::home::euid();
        assert_eq!(peer_uid(&a).unwrap(), me);
        assert!(peer_allowed(&a, me));
        assert!(!peer_allowed(&a, me.wrapping_add(1)));
    }

    #[test]
    fn the_listener_drops_a_peer_of_another_uid() {
        let dir = short_tempdir();
        let (_listener, path) = start(dir.path(), crate::home::euid().wrapping_add(1));
        let mut stream = connect(&path);
        let _ = stream.write_all(b"{\"id\":1,\"method\":\"hello\"}\n");
        assert_eq!(read_line(&stream), None);
    }

    #[test]
    fn unreadable_lines_get_invalid_request_and_the_connection_goes_on() {
        let dir = short_tempdir();
        let (_listener, path) = start(dir.path(), crate::home::euid());
        let mut stream = connect(&path);
        stream.write_all(b"{not json\n").unwrap();
        let reply: Response = serde_json::from_str(&read_line(&stream).unwrap()).unwrap();
        assert_eq!(reply.id, UNREADABLE_REQUEST_ID);
        assert!(
            matches!(reply.outcome, org_proto::Outcome::Error(ref e) if e.code == ErrorCode::InvalidRequest)
        );

        stream.write_all(b"{\"id\":7,\"method\":5}\n").unwrap();
        let reply: Response = serde_json::from_str(&read_line(&stream).unwrap()).unwrap();
        assert_eq!(reply.id, 7);

        stream.write_all(b"{\"id\":8,\"method\":\"x\"}\n").unwrap();
        let reply: Response = serde_json::from_str(&read_line(&stream).unwrap()).unwrap();
        assert_eq!(
            reply.outcome,
            org_proto::Outcome::Result(Value::String("x".into()))
        );
    }

    /// S1: a client that half-closes after its requests gets their answers, then EOF.
    #[test]
    fn a_half_closed_connection_is_answered_then_closed() {
        use std::io::Read;
        let dir = short_tempdir();
        let (_listener, path) = start(dir.path(), crate::home::euid());
        let mut stream = connect(&path);
        stream
            .write_all(b"{\"id\":1,\"method\":\"hello\"}\n")
            .unwrap();
        stream.shutdown(std::net::Shutdown::Write).unwrap();
        let mut all = String::new();
        stream
            .read_to_string(&mut all)
            .expect("EOF within the read timeout");
        assert_eq!(all.lines().count(), 1, "{all}");
    }

    #[test]
    fn connections_beyond_the_cap_are_closed() {
        let dir = short_tempdir();
        let (_listener, path) = start(dir.path(), crate::home::euid());
        let mut held = Vec::new();
        for _ in 0..2 {
            let mut s = connect(&path);
            s.write_all(b"{\"id\":1,\"method\":\"a\"}\n").unwrap();
            assert!(read_line(&s).is_some());
            held.push(s);
        }
        let mut third = connect(&path);
        let _ = third.write_all(b"{\"id\":1,\"method\":\"a\"}\n");
        assert_eq!(read_line(&third), None);
    }

    /// Records where each request began and what the accept noted.
    struct Recorder(Mutex<Vec<(String, u64, u64)>>);

    impl Handler for Recorder {
        fn handle(&self, request: Request, connection: &Arc<Connection>, received: Received) {
            lock(&self.0).push((
                request.method,
                received.sent_at,
                connection.accepted().buffered,
            ));
        }
    }

    /// The N0 rule's two halves: what the accept notes as already buffered, and each request's
    /// offset in the stream. A request that was already buffered when the connection was
    /// accepted starts before N0 (an app-only call there is refused); one sent after starts at or
    /// after it. (An integration test can't place a request before the host's accept reliably;
    /// here the "accept" is the sample taken after the first request was written.)
    #[test]
    fn the_accept_notes_what_was_buffered_and_requests_know_where_they_began() {
        let (client, server) = UnixStream::pair().unwrap();
        let first = b"{\"id\":1,\"method\":\"early\"}\n";
        let second = b"{\"id\":2,\"method\":\"after\"}\n";
        (&client).write_all(first).unwrap();
        let accepted = Accepted::sample(&server);
        assert_eq!(accepted.buffered, first.len() as u64);

        let recorder = Arc::new(Recorder(Mutex::new(Vec::new())));
        let (connection, queue) = Connection::new(server.try_clone().unwrap(), accepted);
        let handler: Arc<dyn Handler> = recorder.clone();
        let serving = thread::spawn(move || serve(server, connection, queue, handler, 4));
        (&client).write_all(second).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
        serving.join().unwrap();

        let mut seen = lock(&recorder.0).clone();
        seen.sort_by_key(|(_, at, _)| *at);
        let n0 = first.len() as u64;
        assert_eq!(
            seen,
            [
                ("early".to_owned(), 0, n0),
                ("after".to_owned(), first.len() as u64, n0),
            ]
        );
        // The early one began before N0; the later one at it.
        assert!(seen[0].1 < seen[0].2 && seen[1].1 >= seen[1].2);
    }

    /// A handler whose `app` calls (inline when `inline_app`) wait until the client has written
    /// its next line, then note how many bytes are waiting on the socket. Its other calls note
    /// that they ran.
    struct Probe {
        inline_app: bool,
        started: Mutex<mpsc::Sender<()>>,
        next_written: Mutex<mpsc::Receiver<()>>,
        next_handled: (Mutex<bool>, Condvar),
        waiting: Mutex<Option<u64>>,
    }

    impl Handler for Probe {
        fn inline(&self, request: &Request) -> bool {
            self.inline_app && request.method == "app"
        }

        fn handle(&self, request: Request, connection: &Arc<Connection>, _received: Received) {
            let (handled, changed) = &self.next_handled;
            if request.method != "app" {
                *lock(handled) = true;
                changed.notify_all();
                return;
            }
            lock(&self.started).send(()).unwrap();
            lock(&self.next_written).recv().unwrap();
            // A reader that isn't held back reads the next line and runs it at once: give it
            // the time to.
            let guard = lock(handled);
            drop(
                changed
                    .wait_timeout_while(guard, Duration::from_millis(500), |handled| !*handled)
                    .unwrap(),
            );
            *lock(&self.waiting) = Some(crate::peer::bytes_waiting(connection.socket()).unwrap());
        }
    }

    /// How many bytes were waiting on the socket while an `app` call was handled, once the
    /// client had written its next line.
    fn bytes_waiting_during_an_app_call(inline_app: bool) -> u64 {
        let (client, server) = UnixStream::pair().unwrap();
        let (started, has_started) = mpsc::channel();
        let (written, next_written) = mpsc::channel();
        let probe = Arc::new(Probe {
            inline_app,
            started: Mutex::new(started),
            next_written: Mutex::new(next_written),
            next_handled: (Mutex::new(false), Condvar::new()),
            waiting: Mutex::new(None),
        });
        let accepted = Accepted::sample(&server);
        let (connection, queue) = Connection::new(server.try_clone().unwrap(), accepted);
        let handler: Arc<dyn Handler> = probe.clone();
        let serving = thread::spawn(move || serve(server, connection, queue, handler, 4));
        (&client)
            .write_all(b"{\"id\":1,\"method\":\"app\"}\n")
            .unwrap();
        // The reader has read the call (alone: nothing else was written yet).
        has_started.recv_timeout(Duration::from_secs(5)).unwrap();
        (&client)
            .write_all(b"{\"id\":2,\"method\":\"next\"}\n")
            .unwrap();
        written.send(()).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
        serving.join().unwrap();
        let waiting = lock(&probe.waiting).expect("the app call ran");
        waiting
    }

    /// L1: the calls `Handler::inline` picks (the app-only calls) run on the connection's
    /// reader thread, which reads nothing more until they're answered: a line the client wrote
    /// meanwhile is still waiting on the socket, which is what their check (`app_auth`,
    /// "nothing after the request") relies on. A call run on its own thread lets the reader read
    /// on, so nothing is waiting.
    #[test]
    fn an_inline_call_runs_before_anything_more_is_read() {
        let next_line = b"{\"id\":2,\"method\":\"next\"}\n".len() as u64;
        assert_eq!(bytes_waiting_during_an_app_call(true), next_line);
        assert_eq!(bytes_waiting_during_an_app_call(false), 0);
    }

    #[test]
    fn bind_replaces_a_stale_socket_but_nothing_else() {
        let dir = short_tempdir();
        let path = dir.path().join("s.sock");
        drop(UnixListener::bind(&path).unwrap()); // leaves a stale socket file
        let listener = bind(&path).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        drop(listener);

        let file = dir.path().join("f.sock");
        fs::write(&file, b"not a socket").unwrap();
        assert!(matches!(bind(&file), Err(BindError::NotASocket)));
        assert_eq!(fs::read(&file).unwrap(), b"not a socket");
    }
}
