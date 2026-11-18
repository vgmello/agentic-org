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
use org_proto::{ApiError, ErrorCode, Event, Request, Response, UNREADABLE_REQUEST_ID};
use serde_json::Value;

use crate::cloexec::SPAWN_LOCK;
use crate::host_log;
use crate::peer::{Accepted, Received};
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

    /// Authority ends at reader EOF, before draining outstanding responses.
    fn disconnected(&self, _connection: &Arc<Connection>) {}

    /// Whether `request` is handled on the connection's reader thread, before anything more is
    /// read from the connection, including handshake decisions before later pipelined calls.
    fn inline(&self, request: &Request, connection: &Connection) -> bool {
        let _ = (request, connection);
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

/// A running listener.
pub(crate) struct Listener {
    stopping: Arc<AtomicBool>,
    live: Arc<Mutex<HashMap<u64, Live>>>,
    accept: Option<JoinHandle<()>>,
}

struct Gateway {
    transfer: Arc<std::os::unix::net::UnixDatagram>,
    listen: org_host_control::listen::ListenConfig,
    /// A successful `tls.fetch` answered on this connection.
    fetched: AtomicBool,
    /// The web process's one `web.serving` was accepted.
    serving: AtomicBool,
    /// The host refused a `web.serving` as a protocol error and closed control.
    serving_refused: AtomicBool,
}

/// How the host takes a `web.serving` notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ServingReport {
    /// Not the admitted web process connection.
    NotWeb,
    /// Accepted: the first one, after hello and a successful `tls.fetch`.
    Accepted,
    /// Repeated, or before hello/`tls.fetch`: a protocol error.
    OutOfOrder,
}

/// Lifetime of the sole authenticated web process control connection. Constructed only by
/// the web process supervisor, after its image/reader/sandbox admission and handoff ACK.
/// This is the only production path that gives a connection web process privilege:
/// host.sock and setup connections stay `ChannelKind::Local`.
pub(crate) struct WebConnection {
    connection: Arc<Connection>,
    thread: Option<JoinHandle<()>>,
}
impl WebConnection {
    pub(crate) fn start(
        stream: UnixStream,
        transfer: std::os::unix::net::UnixDatagram,
        listen: org_host_control::listen::ListenConfig,
        handler: Arc<dyn Handler>,
    ) -> io::Result<Self> {
        let (mut connection, queue) =
            Connection::new(stream.try_clone()?, Accepted::sample(&stream));
        Arc::get_mut(&mut connection)
            .ok_or_else(|| io::Error::other("shared admission"))?
            .gateway = Some(Gateway {
            transfer: Arc::new(transfer),
            listen,
            fetched: AtomicBool::new(false),
            serving: AtomicBool::new(false),
            serving_refused: AtomicBool::new(false),
        });
        let serving = Arc::clone(&connection);
        let thread = thread::Builder::new()
            .name("web-control".into())
            .spawn(move || serve(stream, serving, queue, handler, 4))?;
        Ok(Self {
            connection,
            thread: Some(thread),
        })
    }
    pub(crate) fn live(&self) -> bool {
        !self.connection.closed.load(Ordering::SeqCst)
    }
    /// The host refused the web process's `web.serving` as a protocol error.
    pub(crate) fn serving_refused(&self) -> bool {
        self.connection
            .gateway
            .as_ref()
            .is_some_and(|g| g.serving_refused.load(Ordering::SeqCst))
    }
    /// The web process reported, in order, that its HTTP server is serving.
    pub(crate) fn reported_serving(&self) -> bool {
        self.connection
            .gateway
            .as_ref()
            .is_some_and(|g| g.serving.load(Ordering::SeqCst))
    }
}
impl Drop for WebConnection {
    fn drop(&mut self) {
        self.connection.close();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A served `Local` connection for tests, built like an accepted host.sock
/// connection. It has no web process privilege.
#[cfg(test)]
pub(crate) struct LocalConnection {
    connection: Arc<Connection>,
    thread: Option<JoinHandle<()>>,
}
#[cfg(test)]
impl LocalConnection {
    pub(crate) fn start(stream: UnixStream, handler: Arc<dyn Handler>) -> io::Result<Self> {
        if !org_socket::peer_allowed(&stream, org_socket::effective_uid()) {
            return Err(io::Error::other("local peer refused"));
        }
        let (connection, queue) = Connection::new(stream.try_clone()?, Accepted::sample(&stream));
        let serving = Arc::clone(&connection);
        let thread = thread::Builder::new()
            .name("local-control".into())
            .spawn(move || serve(stream, serving, queue, handler, 1))?;
        Ok(Self {
            connection,
            thread: Some(thread),
        })
    }
    pub(crate) fn live(&self) -> bool {
        !self.connection.closed.load(Ordering::SeqCst)
    }
}
#[cfg(test)]
impl Drop for LocalConnection {
    fn drop(&mut self) {
        self.connection.close();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
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

    /// [`Listener::join`], until `until` at the latest: a connection whose running request
    /// has not ended by then (one waiting for a write lock of the registry, say) is left to
    /// end on its own, its answer dropped. Says whether every connection ended.
    pub(crate) fn join_by(&mut self, until: std::time::Instant) -> bool {
        let live: Vec<Live> = lock(&self.live).drain().map(|(_, live)| live).collect();
        for live in &live {
            live.connection.close();
        }
        // Every connection is waited for, until the same bound.
        let left = live
            .into_iter()
            .map(|live| crate::sync::join_by(live.reader, until))
            .filter(|ended| !ended)
            .count();
        left == 0
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
                continue;
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
        let Some(admitted) = admission(
            org_socket::peer_allowed(&stream, config.expected_uid),
            || root_peer(&stream),
        ) else {
            host_log::log("refused a connection from another user");
            continue; // dropping `stream` closes it
        };
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
        if admitted == Admitted::Root {
            connection.mark_root_authority();
            host_log::log("admitted root on host.sock, for admin.reset only");
        }
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

/// Who a peer of `host.sock` was admitted as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Admitted {
    /// The host's own user: every call is judged by its own rule.
    User,
    /// Root, on a system that names a peer by its user alone: the local authority that
    /// may reset the administrator (SIGN-33, ADR-0017 decision 11). It is admitted for
    /// that call and the greeting, and for nothing else ([`root_may_call`]).
    Root,
}

/// The admission decision, pure: `own_user` is whether the kernel names the host's own
/// user as the peer, and `root` asks whether it names root (asked only otherwise). Any
/// other user is not admitted at all.
fn admission(own_user: bool, root: impl FnOnce() -> bool) -> Option<Admitted> {
    if own_user {
        Some(Admitted::User)
    } else if root() {
        Some(Admitted::Root)
    } else {
        None
    }
}

/// Whether the kernel names root as the peer of `stream`, through the platform's peer
/// identity. Only a system that names a peer by its user alone reports a user at all
/// (`Identity::uid`): where a peer is identified by its image, root has no route here,
/// and a system that cannot identify a peer admits nobody this way.
fn root_peer(stream: &UnixStream) -> bool {
    crate::peer::identity(stream).is_ok_and(|identity| identity.uid == Some(0))
}

/// The calls a connection admitted as root may make: the greeting every connection
/// starts with, and `admin.reset`. Everything else is answered `not_authorized` before any
/// handler sees it.
fn root_may_call(request: &Request) -> bool {
    request.is::<org_proto::Hello>() || request.is::<org_host_control::admin::AdminReset>()
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
        let request = match super::request::decode(&line) {
            Ok(request) => request,
            Err(_) => {
                connection.clear_recovery_permission();
                connection.send_response(Response::error(
                    super::request::salvage_id(&line).unwrap_or(UNREADABLE_REQUEST_ID),
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

enum Outbound {
    Response(Response),
    /// A response line already encoded, holding a secret: written as is, then zeroed.
    Secret(SecretLine),
    Descriptor {
        response: Response,
        descriptor: std::os::fd::OwnedFd,
        receipt: Receiver<()>,
        gateway: Option<Arc<std::os::unix::net::UnixDatagram>>,
        office: String,
        channel: String,
    },
    /// `seq` is given by the writer, so it counts without gaps in the order events are written.
    Event {
        name: String,
        data: Value,
    },
    /// Ends the writer once everything queued before it is written.
    Close,
}

/// An encoded verified-session response line holding a secret. It never passes through a
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
    hello_completed: AtomicBool,
    /// The peer is root, admitted as the local authority for `admin.reset` alone.
    root_authority: AtomicBool,
    descriptor_receipts:
        Mutex<HashMap<String, (mpsc::Sender<()>, crate::supervisor::channels::ChannelSlot)>>,
    received_channels: Mutex<HashMap<String, crate::supervisor::channels::ChannelSlot>>,
    gateway: Option<Gateway>,
    pub(crate) recovery_permission: Mutex<Option<crate::recovery::Permission>>,
}

#[cfg(test)]
impl Connection {
    /// A connection as the listener makes one for an accepted `stream`, for the tests of
    /// the calls that judge a connection (`admin`). Nothing serves it.
    pub(crate) fn accepted_for_tests(stream: &UnixStream) -> Arc<Self> {
        Self::new(stream.try_clone().unwrap(), Accepted::sample(stream)).0
    }
}

/// Capacity is a definite pre-enqueue refusal; all other delivery failures stay fatal.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DescriptorError {
    Capacity,
    Transport,
}

impl Connection {
    fn new(stream: UnixStream, accepted: Accepted) -> (Arc<Self>, Receiver<Outbound>) {
        let (outbound, queue) = mpsc::sync_channel(OUTBOUND_QUEUE);
        let connection = Self {
            outbound,
            stream,
            accepted,
            closed: Arc::new(AtomicBool::new(false)),
            hello_completed: AtomicBool::new(false),
            root_authority: AtomicBool::new(false),
            descriptor_receipts: Mutex::default(),
            received_channels: Mutex::default(),
            gateway: None,
            recovery_permission: Mutex::default(),
        };
        (Arc::new(connection), queue)
    }

    pub(super) fn liveness(&self) -> std::sync::Weak<AtomicBool> {
        Arc::downgrade(&self.closed)
    }

    /// Privileged inline handlers must observe EOF without consuming another
    /// request while waiting for OS authentication. Any pipelined byte denies.
    pub(crate) fn recovery_peer_idle(&self) -> bool {
        if self.closed.load(Ordering::SeqCst) {
            return false;
        }
        let mut byte = 0u8;
        // SAFETY: live socket and one writable byte, non-consuming/non-blocking.
        let n = unsafe {
            libc::recv(
                self.stream.as_raw_fd(),
                (&mut byte as *mut u8).cast(),
                1,
                libc::MSG_PEEK | libc::MSG_DONTWAIT,
            )
        };
        n < 0 && std::io::Error::last_os_error().kind() == io::ErrorKind::WouldBlock
    }
    pub(crate) fn clear_recovery_permission(&self) {
        *lock(&self.recovery_permission) = None;
    }
    pub(crate) fn hello_completed(&self) -> bool {
        self.hello_completed.load(Ordering::SeqCst)
    }
    /// Set once, when the connection is accepted, and never cleared.
    pub(crate) fn mark_root_authority(&self) {
        self.root_authority.store(true, Ordering::SeqCst);
    }
    /// Whether the kernel named root as this connection's peer when it was accepted, on a
    /// system where that makes it the local authority for `admin.reset`.
    pub(crate) fn root_authority(&self) -> bool {
        self.root_authority.load(Ordering::SeqCst)
    }
    pub(crate) fn channel_kind(&self) -> org_office_launch::ChannelKind {
        if self.gateway.is_some() {
            org_office_launch::ChannelKind::Web
        } else {
            org_office_launch::ChannelKind::Local
        }
    }
    pub(crate) fn gateway_listen(&self) -> Option<&org_host_control::listen::ListenConfig> {
        self.gateway.as_ref().map(|g| &g.listen)
    }
    /// Records a successful `tls.fetch` answer on the web process connection.
    pub(crate) fn note_listener_fetched(&self) {
        if let Some(web) = &self.gateway {
            web.fetched.store(true, Ordering::SeqCst);
        }
    }
    /// Takes the web process's `web.serving`: accepted once, after hello and `tls.fetch`.
    pub(crate) fn report_serving(&self) -> ServingReport {
        let Some(web) = &self.gateway else {
            return ServingReport::NotWeb;
        };
        if self.hello_completed()
            && web.fetched.load(Ordering::SeqCst)
            && web
                .serving
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            ServingReport::Accepted
        } else {
            ServingReport::OutOfOrder
        }
    }

    pub(crate) fn complete_hello(&self) {
        self.hello_completed.store(true, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> Arc<Self> {
        let (stream, _peer) = UnixStream::pair().unwrap();
        Self::new(stream.try_clone().unwrap(), Accepted::sample(&stream)).0
    }
    #[cfg(test)]
    pub(super) fn record_received_channel_for_test(&self, channel: &str) {
        self.record_received_slot_for_test(
            channel,
            crate::supervisor::channels::ChannelSlot::for_test(),
        );
    }

    #[cfg(test)]
    pub(super) fn record_received_slot_for_test(
        &self,
        channel: &str,
        slot: crate::supervisor::channels::ChannelSlot,
    ) {
        lock(&self.received_channels).insert(channel.into(), slot);
    }

    /// Refuses a web process's `web.serving` as a protocol error, noting it so the
    /// supervisor logs that cause rather than only the control EOF that follows.
    pub(crate) fn refuse_serving(&self, response: Response) {
        if let Some(web) = &self.gateway {
            web.serving_refused.store(true, Ordering::SeqCst);
        }
        self.refuse_and_finish(response);
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
    pub(crate) fn send_descriptor(
        &self,
        response: Response,
        channel: String,
        descriptor: std::os::fd::OwnedFd,
        office: &str,
        witness: crate::supervisor::channels::ChannelSlot,
    ) -> Result<(), DescriptorError> {
        let (sent, receipt) = mpsc::channel();
        {
            // One order for every paired access. Pending reservations are never pruned.
            let mut pending = lock(&self.descriptor_receipts);
            let mut acknowledged = lock(&self.received_channels);
            acknowledged.retain(|_, witness| witness.is_live());
            if pending.contains_key(&channel) || acknowledged.contains_key(&channel) {
                return Err(DescriptorError::Transport);
            }
            if pending.len() >= 16 || pending.len() + acknowledged.len() >= 512 {
                return Err(DescriptorError::Capacity);
            }
            if self.closed.load(Ordering::SeqCst) {
                return Err(DescriptorError::Transport);
            }
            pending.insert(channel.clone(), (sent, witness));
        }
        self.enqueue(Outbound::Descriptor {
            response,
            descriptor,
            receipt,
            gateway: self.gateway.as_ref().map(|g| Arc::clone(&g.transfer)),
            office: office.into(),
            channel,
        });
        if self.closed.load(Ordering::SeqCst) {
            return Err(DescriptorError::Transport);
        }
        Ok(())
    }
    pub(crate) fn descriptor_received(&self, channel: &str) -> bool {
        let mut pending = lock(&self.descriptor_receipts);
        let mut acknowledged = lock(&self.received_channels);
        let Some((receipt, witness)) = pending.remove(channel) else {
            return false;
        };
        if receipt.send(()).is_err() {
            return false;
        }
        if witness.is_live() {
            acknowledged.insert(channel.into(), witness);
        }
        true
    }
    pub(crate) fn channel_received(&self, channel: &str) -> bool {
        lock(&self.received_channels)
            .get(channel)
            .is_some_and(|witness| witness.is_live())
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
            Outbound::Descriptor {
                response,
                descriptor,
                receipt,
                gateway,
                office,
                channel,
            } => {
                use std::os::fd::AsFd;
                let sent = write_response(&mut out, response)
                    .and_then(|()| io::Write::flush(&mut out).map_err(FrameError::Io));
                let until = std::time::Instant::now() + Duration::from_secs(5);
                let delivered = if sent.is_err() {
                    false
                } else if let Some(gateway) = gateway {
                    org_office_launch::encode_transfer_record(
                        &org_office_launch::TransferRecord::Channel {
                            kind: org_office_launch::ChannelKind::Web,
                            office_id: office,
                            channel_id: channel,
                        },
                    )
                    .ok()
                    .is_some_and(|header| {
                        org_socket::rights::send(
                            gateway.as_fd(),
                            &header,
                            &[descriptor.as_fd()],
                            until,
                        )
                        .is_ok()
                    })
                } else {
                    org_socket::rights::send(
                        out.get_ref().as_fd(),
                        &[org_host_control::ops::HOST_SOCK_MARKER],
                        &[descriptor.as_fd()],
                        until,
                    )
                    .is_ok()
                };
                if sent.is_err()
                    || !delivered
                    || receipt.recv_timeout(Duration::from_secs(5)).is_err()
                {
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
    fn web_serving_is_taken_only_from_the_web_after_hello_and_tls_fetch() {
        let plain = Connection::for_test();
        plain.complete_hello();
        plain.note_listener_fetched();
        assert!(matches!(plain.report_serving(), ServingReport::NotWeb));
        let (host, _peer) = UnixStream::pair().unwrap();
        let (transfer, _received) = std::os::unix::net::UnixDatagram::pair().unwrap();
        let (mut connection, _queue) =
            Connection::new(host.try_clone().unwrap(), Accepted::sample(&host));
        Arc::get_mut(&mut connection).unwrap().gateway = Some(Gateway {
            transfer: Arc::new(transfer),
            listen: serde_json::from_str(r#"{"address":"127.0.0.1","port":48123,"scheme":"http_loopback","origin":"http://127.0.0.1:48123","hsts":"off"}"#).unwrap(),
            fetched: AtomicBool::new(false),
            serving: AtomicBool::new(false),
            serving_refused: AtomicBool::new(false),
        });
        connection.note_listener_fetched();
        assert!(
            matches!(connection.report_serving(), ServingReport::OutOfOrder),
            "tls.fetch without hello"
        );
        connection.complete_hello();
        assert!(matches!(
            connection.report_serving(),
            ServingReport::Accepted
        ));
        assert!(matches!(
            connection.report_serving(),
            ServingReport::OutOfOrder
        ));
    }

    #[test]
    fn gateway_descriptor_uses_datagram_and_receipt_never_stream_rights() {
        use std::os::fd::AsFd;
        let (host, peer) = UnixStream::pair().unwrap();
        let (transfer, received) = std::os::unix::net::UnixDatagram::pair().unwrap();
        let (mut connection, queue) =
            Connection::new(host.try_clone().unwrap(), Accepted::sample(&host));
        Arc::get_mut(&mut connection).unwrap().gateway = Some(Gateway {
            transfer: Arc::new(transfer),
            listen: serde_json::from_str(r#"{"address":"127.0.0.1","port":48123,"scheme":"http_loopback","origin":"http://127.0.0.1:48123","hsts":"off"}"#).unwrap(),
            fetched: AtomicBool::new(false),
            serving: AtomicBool::new(false),
            serving_refused: AtomicBool::new(false),
        });
        assert_eq!(
            connection.channel_kind(),
            org_office_launch::ChannelKind::Web
        );
        assert!(connection.gateway_listen().is_some());
        let closed = Arc::clone(&connection.closed);
        let writer = std::thread::spawn(move || write_loop(&host, queue, &closed));
        let (channel, mut worker) = UnixStream::pair().unwrap();
        connection
            .send_descriptor(
                Response::ok::<org_host_control::ops::OfficeOpenChannel>(
                    3,
                    &org_host_control::ops::OfficeOpenChannelResult {
                        channel_id: "channel".into(),
                    },
                )
                .unwrap(),
                "channel".into(),
                channel.into(),
                "office",
                crate::supervisor::channels::ChannelSlot::for_test(),
            )
            .unwrap();
        connection.send_response(Response {
            id: 4,
            outcome: org_proto::Outcome::Result(Value::Bool(true)),
        });
        let mut stream = org_socket::rights::StreamReceiver::new(peer);
        let until = std::time::Instant::now() + Duration::from_secs(3);
        let mut line = Vec::new();
        loop {
            match stream.receive_byte(0, &SPAWN_LOCK) {
                Ok((b'\n', fds)) => {
                    assert!(fds.is_empty());
                    break;
                }
                Ok((byte, fds)) => {
                    assert!(fds.is_empty());
                    line.push(byte);
                }
                Err(org_socket::rights::ReceiveError::WouldBlock)
                    if std::time::Instant::now() < until =>
                {
                    std::thread::yield_now()
                }
                Err(error) => panic!("{error}"),
            }
        }
        assert_eq!(serde_json::from_slice::<Response>(&line).unwrap().id, 3);
        let mut datagrams = org_socket::rights::DatagramReceiver::new(received);
        let mut header = [0; org_office_launch::TRANSFER_HEADER_MAX_BYTES];
        let mut record = loop {
            match datagrams.receive(&mut header, 1, &SPAWN_LOCK) {
                Ok(record) => break record,
                Err(org_socket::rights::ReceiveError::WouldBlock)
                    if std::time::Instant::now() < until =>
                {
                    std::thread::yield_now()
                }
                Err(error) => panic!("{error}"),
            }
        };
        assert!(
            matches!(org_office_launch::decode_transfer_record(&header[..record.bytes]).unwrap(),
            org_office_launch::TransferRecord::Channel { kind:org_office_launch::ChannelKind::Web, office_id, channel_id } if office_id=="office" && channel_id=="channel")
        );
        let mut channel =
            org_socket::rights::connected_unix_stream(record.descriptors.pop().unwrap()).unwrap();
        org_socket::rights::set_buffer_sizes(channel.as_fd(), 4096).unwrap();
        channel.write_all(b"through").unwrap();
        let mut bytes = [0; 7];
        std::io::Read::read_exact(&mut worker, &mut bytes).unwrap();
        assert_eq!(&bytes, b"through");
        assert!(matches!(
            stream.receive_byte(0, &SPAWN_LOCK),
            Err(org_socket::rights::ReceiveError::WouldBlock)
        ));
        assert!(!connection.channel_received("channel"));
        assert!(!connection.descriptor_received("foreign"));
        assert!(connection.descriptor_received("channel"));
        assert!(connection.channel_received("channel"));
        assert!(!connection.descriptor_received("channel"));
        connection.finish();
        writer.join().unwrap();
    }

    #[test]
    fn local_descriptor_response_is_atomic_and_kept_until_correlated_receipt() {
        let (host, peer) = UnixStream::pair().unwrap();
        let (connection, queue) =
            Connection::new(host.try_clone().unwrap(), Accepted::sample(&host));
        let closed = Arc::clone(&connection.closed);
        let writer = std::thread::spawn(move || write_loop(&host, queue, &closed));
        let (client, mut worker) = UnixStream::pair().unwrap();
        let result = org_host_control::ops::OfficeOpenChannelResult {
            channel_id: "channel".into(),
        };
        connection
            .send_descriptor(
                Response::ok::<org_host_control::ops::OfficeOpenChannel>(3, &result).unwrap(),
                "channel".into(),
                client.into(),
                "office",
                crate::supervisor::channels::ChannelSlot::for_test(),
            )
            .unwrap();
        connection.send_response(Response {
            id: 4,
            outcome: org_proto::Outcome::Result(Value::Bool(true)),
        });
        let mut receiver = org_socket::rights::StreamReceiver::new(peer);
        let receive = |receiver: &mut org_socket::rights::StreamReceiver, count| {
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            loop {
                match receiver.receive_byte(count, &SPAWN_LOCK) {
                    Ok(value) => return value,
                    Err(org_socket::rights::ReceiveError::WouldBlock)
                        if std::time::Instant::now() < deadline =>
                    {
                        std::thread::yield_now()
                    }
                    Err(error) => panic!("{error}"),
                }
            }
        };
        let mut line = Vec::new();
        loop {
            let (byte, fds) = receive(&mut receiver, 0);
            assert!(fds.is_empty());
            if byte == b'\n' {
                break;
            }
            line.push(byte);
        }
        let response: Response = serde_json::from_slice(&line).unwrap();
        assert_eq!(response.id, 3);
        assert_eq!(
            response
                .into_result::<org_host_control::ops::OfficeOpenChannel>()
                .unwrap(),
            result
        );
        let (marker, mut descriptors) = receive(&mut receiver, 1);
        assert_eq!(marker, org_host_control::ops::HOST_SOCK_MARKER);
        let mut channel =
            org_socket::rights::connected_unix_stream(descriptors.pop().unwrap()).unwrap();
        channel.write_all(b"real channel").unwrap();
        let mut expected = [0; 12];
        std::io::Read::read_exact(&mut worker, &mut expected).unwrap();
        assert_eq!(&expected, b"real channel");
        // The next queued response is withheld until descriptor ownership is acknowledged.
        assert!(matches!(
            receiver.receive_byte(0, &SPAWN_LOCK),
            Err(org_socket::rights::ReceiveError::WouldBlock)
        ));
        assert!(!connection.descriptor_received("other-channel"));
        assert!(connection.descriptor_received("channel"));
        assert!(connection.channel_received("channel"));
        assert!(!connection.descriptor_received("channel"));
        line.clear();
        loop {
            let (byte, fds) = receive(&mut receiver, 0);
            assert!(fds.is_empty());
            if byte == b'\n' {
                break;
            }
            line.push(byte);
        }
        assert_eq!(serde_json::from_slice::<Response>(&line).unwrap().id, 4);
        connection.finish();
        writer.join().unwrap();
    }

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
            fn inline(&self, _: &Request, _: &Connection) -> bool {
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

    /// SIGN-33: the host's own user is admitted as itself, root as the local authority
    /// (asked only when the peer is not the host's user), and nobody else at all.
    #[test]
    fn only_the_hosts_user_and_root_are_admitted_and_root_is_asked_last() {
        let unasked = || -> bool { panic!("asked about root for the host's own user") };
        assert_eq!(admission(true, unasked), Some(Admitted::User));
        assert_eq!(admission(false, || true), Some(Admitted::Root));
        assert_eq!(admission(false, || false), None);
        // This process's own peer is its own user: the platform never names it root unless
        // the tests run as root, and where a peer is identified by its image, never.
        let (stream, _peer) = UnixStream::pair().unwrap();
        let named_root = crate::peer::identity(&stream).is_ok_and(|i| i.uid == Some(0));
        assert_eq!(root_peer(&stream), named_root);
        assert!(!named_root || crate::home::euid() == 0);
    }

    /// A connection admitted as root reaches a handler with the greeting and
    /// `admin.reset` only: every other method is answered `not_authorized` before any
    /// handler sees it. The same requests on the host's own user's connection all reach
    /// the handler, which judges each by its own rule.
    #[test]
    fn root_is_admitted_for_admin_reset_and_the_greeting_and_nothing_else() {
        const METHODS: [&str; 14] = [
            "hello",
            "instance.status",
            "admin.link.initial",
            "admin.reset",
            "tls.fetch",
            "web.serving",
            "organization.provision",
            "operation.get",
            "recovery.release",
            "recovery.acknowledge",
            "session.grant",
            "office.app_token",
            "office.restart",
            "no.such.method",
        ];
        for root in [true, false] {
            let (host, peer) = UnixStream::pair().unwrap();
            let (connection, queue) =
                Connection::new(host.try_clone().unwrap(), Accepted::sample(&host));
            assert!(!connection.root_authority());
            if root {
                connection.mark_root_authority();
            }
            let serving = Arc::clone(&connection);
            let server = thread::spawn(move || serve(host, serving, queue, Arc::new(Echo), 4));
            peer.set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut reader = io::BufReader::new(peer.try_clone().unwrap());
            for (id, method) in METHODS.iter().enumerate() {
                writeln!(
                    &peer,
                    r#"{{"id":{},"method":"{method}","params":{{}}}}"#,
                    id + 1
                )
                .unwrap();
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let response: Response = serde_json::from_str(&line).unwrap();
                assert_eq!(response.id, id as u64 + 1);
                let reached = match response.outcome {
                    org_proto::Outcome::Result(value) => {
                        assert_eq!(value, Value::String((*method).into()));
                        true
                    }
                    org_proto::Outcome::Error(error) => {
                        assert_eq!(error.code, ErrorCode::NotAuthorized, "{method}");
                        assert!(error.message.contains("admin.reset only"), "{method}");
                        false
                    }
                };
                let allowed = !root || matches!(*method, "hello" | "admin.reset");
                assert_eq!(reached, allowed, "root={root} {method}");
            }
            drop(reader);
            drop(peer);
            server.join().unwrap();
        }
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

    fn loopback_listen() -> org_host_control::listen::ListenConfig {
        serde_json::from_str(r#"{"address":"127.0.0.1","port":48123,"scheme":"http_loopback","origin":"http://127.0.0.1:48123","hsts":"off"}"#).unwrap()
    }

    #[test]
    fn only_the_admitted_web_connection_is_web_kind() {
        let (host, peer) = UnixStream::pair().unwrap();
        let (transfer, _child) = std::os::unix::net::UnixDatagram::pair().unwrap();
        let web = WebConnection::start(host, transfer, loopback_listen(), Arc::new(Echo)).unwrap();
        assert_eq!(
            web.connection.channel_kind(),
            org_office_launch::ChannelKind::Web
        );
        assert!(web.connection.gateway_listen().is_some());
        assert!(web.live());

        // Every connection the host.sock listener accepts is built by `Connection::new`,
        // which carries no web process.
        let (accepted, _accepted_peer) = UnixStream::pair().unwrap();
        let (plain, _queue) =
            Connection::new(accepted.try_clone().unwrap(), Accepted::sample(&accepted));
        assert_eq!(plain.channel_kind(), org_office_launch::ChannelKind::Local);

        // Peer EOF ends the web process control connection.
        drop(peer);
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while web.live() && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!web.live());
    }

    #[test]
    fn host_sock_connections_are_local() {
        let dir = short_tempdir();
        let (listener, path) = start(dir.path(), org_socket::effective_uid());
        let _client = UnixStream::connect(&path).unwrap();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let kinds: Vec<_> = lock(&listener.live)
                .values()
                .map(|live| live.connection.channel_kind())
                .collect();
            if !kinds.is_empty() {
                assert!(kinds
                    .iter()
                    .all(|k| *k == org_office_launch::ChannelKind::Local));
                break;
            }
            assert!(std::time::Instant::now() < until, "no accepted connection");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
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
        use std::io::Read;

        let me = crate::home::euid();
        for closed in [false, true] {
            let (mut stream, peer) = UnixStream::pair().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let _peer = if closed {
                drop(peer);
                assert_eq!(stream.read(&mut [0]).unwrap(), 0);
                None
            } else {
                Some(peer)
            };
            assert!(org_socket::peer_allowed(&stream, me));
            assert!(!org_socket::peer_allowed(&stream, me.wrapping_add(1)));
            println!("pair peer_closed={closed} uid={me}: match=true mismatch=false");
        }
    }

    #[test]
    fn expected_uid_is_checked_before_host_dispatch() {
        use std::io::Read;
        use std::sync::atomic::AtomicUsize;

        struct Counted(Arc<AtomicUsize>);
        impl Handler for Counted {
            fn handle(&self, request: Request, connection: &Arc<Connection>, _: Received) {
                self.0.fetch_add(1, Ordering::SeqCst);
                connection.send_response(Response {
                    id: request.id,
                    outcome: org_proto::Outcome::Result(Value::String(request.method)),
                });
            }
        }

        let uid = crate::home::euid();
        for expected_uid in [uid, uid.wrapping_add(1)] {
            let dir = short_tempdir();
            let path = dir.path().join("uid.sock");
            let calls = Arc::new(AtomicUsize::new(0));
            let mut listener = Listener::start(
                bind(&path).unwrap(),
                ListenerConfig {
                    max_connections: 2,
                    max_in_flight: 4,
                    expected_uid,
                },
                Arc::new(Counted(Arc::clone(&calls))),
            )
            .unwrap();
            let mut stream = connect(&path);
            let _ = stream.write_all(b"{\"id\":1,\"method\":\"hello\"}\n");
            if expected_uid == uid {
                let response: Response =
                    serde_json::from_str(&read_line(&stream).expect("matching peer served"))
                        .unwrap();
                assert_eq!(response.id, 1);
                assert_eq!(
                    response.outcome,
                    org_proto::Outcome::Result(Value::String("hello".into()))
                );
                println!("accepted uid={uid} expected={expected_uid}: served");
            } else {
                let result = stream.read(&mut [0]);
                assert!(
                    matches!(result, Ok(0))
                        || matches!(result, Err(ref error) if error.kind() == io::ErrorKind::ConnectionReset),
                    "wrong UID reached dispatch or was not closed: {result:?}"
                );
                println!("accepted uid={uid} expected={expected_uid}: {result:?}");
            }
            listener.close();
            listener.join();
            assert_eq!(
                calls.load(Ordering::SeqCst),
                usize::from(expected_uid == uid)
            );
        }
    }

    /// X5 review V2: a stopping host waits for a request still running only until its
    /// bound. A request that waits for something else (here a lock the test holds, as a
    /// write lock of the registry would be held) is left running, and the wait ends at the
    /// bound.
    #[test]
    fn a_bounded_join_leaves_a_request_that_does_not_end_by_its_bound() {
        struct Waits(Arc<Mutex<()>>, Arc<AtomicBool>);
        impl Handler for Waits {
            fn handle(&self, _: Request, _: &Arc<Connection>, _: Received) {
                self.1.store(true, Ordering::SeqCst);
                drop(lock(&self.0));
            }
        }
        let dir = short_tempdir();
        let path = dir.path().join("w.sock");
        let held = Arc::new(Mutex::new(()));
        let entered = Arc::new(AtomicBool::new(false));
        let holding = lock(&held);
        let mut listener = Listener::start(
            bind(&path).unwrap(),
            ListenerConfig {
                max_connections: 2,
                max_in_flight: 4,
                expected_uid: crate::home::euid(),
            },
            Arc::new(Waits(Arc::clone(&held), Arc::clone(&entered))),
        )
        .unwrap();
        let mut stream = connect(&path);
        stream
            .write_all(b"{\"id\":1,\"method\":\"slow\"}\n")
            .unwrap();
        let until = std::time::Instant::now() + Duration::from_secs(10);
        while !entered.load(Ordering::SeqCst) {
            assert!(std::time::Instant::now() < until, "the request never ran");
            std::thread::sleep(Duration::from_millis(5));
        }
        listener.close();
        let asked = std::time::Instant::now();
        assert!(!listener.join_by(asked + Duration::from_millis(300)));
        let took = asked.elapsed();
        assert!(took >= Duration::from_millis(300), "{took:?}");
        assert!(took < Duration::from_secs(2), "{took:?}");
        // The request, left to run, ends once its lock is free.
        drop(holding);
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
        fn inline(&self, request: &Request, _: &Connection) -> bool {
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

#[cfg(test)]
mod recovery_eof_tests {
    use super::*;
    #[test]
    fn inline_presence_wait_observes_peer_eof_without_reading_or_authorizing_pipelined_bytes() {
        use std::io::{Read, Write};
        let (stream, mut peer) = UnixStream::pair().unwrap();
        let (connection, _queue) =
            Connection::new(stream.try_clone().unwrap(), Accepted::sample(&stream));
        assert!(connection.recovery_peer_idle());
        peer.write_all(b"x").unwrap();
        assert!(!connection.recovery_peer_idle());
        // Peek neither swallows the following request nor changes shared flags.
        let mut read = stream;
        let mut byte = [0];
        read.read_exact(&mut byte).unwrap();
        assert_eq!(byte, [b'x']);
        assert!(connection.recovery_peer_idle());
        peer.shutdown(std::net::Shutdown::Write).unwrap();
        assert!(!connection.recovery_peer_idle());
        assert!(!connection.closed.load(Ordering::SeqCst));
    }
}

#[cfg(test)]
mod channel_lifecycle_tests {
    use super::*;
    use crate::supervisor::channels::{
        tests::{await_closed, worker_fixture},
        ChannelSlot,
    };
    use org_host_control::{ops, Password};
    use org_proto::Outcome;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixDatagram;
    use std::time::Instant;

    fn response(id: u64) -> Response {
        Response {
            id,
            outcome: Outcome::Result(serde_json::json!({})),
        }
    }
    fn queued_descriptor(
        connection: &Connection,
        id: &str,
        witness: ChannelSlot,
    ) -> Result<(), DescriptorError> {
        let (descriptor, _peer) = UnixStream::pair().unwrap();
        connection.send_descriptor(response(1), id.into(), descriptor.into(), "office", witness)
    }
    #[test]
    fn concurrent_pending_reservations_count_with_live_acknowledged_records_and_capacity_is_definite(
    ) {
        let (host, _peer) = UnixStream::pair().unwrap();
        let (connection, queue) =
            Connection::new(host.try_clone().unwrap(), Accepted::sample(&host));
        for n in 0..496 {
            lock(&connection.received_channels).insert(format!("ack-{n}"), ChannelSlot::for_test());
        }
        let barrier = Arc::new(std::sync::Barrier::new(33));
        let threads: Vec<_> = (0..32)
            .map(|n| {
                let connection = Arc::clone(&connection);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    let id = format!("pending-{n}");
                    let result = queued_descriptor(&connection, &id, ChannelSlot::for_test());
                    (id, result)
                })
            })
            .collect();
        barrier.wait();
        let mut admitted = Vec::new();
        for thread in threads {
            let (id, result) = thread.join().unwrap();
            match result {
                Ok(()) => admitted.push(id),
                Err(error) => assert_eq!(error, DescriptorError::Capacity),
            }
        }
        assert_eq!(admitted.len(), 16);
        assert_eq!(lock(&connection.descriptor_receipts).len(), 16);
        assert_eq!(lock(&connection.received_channels).len(), 496);
        assert!(!connection.closed.load(Ordering::SeqCst));
        let closed = lock(&connection.descriptor_receipts)[&admitted[0]]
            .1
            .clone();
        closed.close_for_test();
        assert_eq!(
            queued_descriptor(&connection, "cannot-prune-pending", ChannelSlot::for_test()),
            Err(DescriptorError::Capacity)
        );
        assert_eq!(
            queued_descriptor(&connection, &admitted[0], ChannelSlot::for_test()),
            Err(DescriptorError::Transport)
        );
        assert!(
            !Connection::for_test().descriptor_received(&admitted[0]),
            "foreign connection ACK"
        );
        // A late, exact ACK still resolves ownership even after physical EOF.
        assert!(connection.descriptor_received(&admitted[0]));
        assert!(!connection.channel_received(&admitted[0]));
        assert!(!connection.descriptor_received(&admitted[0]));
        assert!(queued_descriptor(&connection, "replacement", ChannelSlot::for_test()).is_ok());
        for id in &admitted[1..] {
            assert!(connection.descriptor_received(id));
        }
        assert_eq!(lock(&connection.received_channels).len(), 511);
        assert_eq!(lock(&connection.descriptor_receipts).len(), 1);
        assert_eq!(
            queued_descriptor(&connection, "combined-513", ChannelSlot::for_test()),
            Err(DescriptorError::Capacity)
        );
        // Losing the actual queued ownership waiter cannot turn a late ACK into authority.
        drop(queue);
        assert!(!connection.descriptor_received("replacement"));
        assert!(!connection.channel_received("replacement"));
    }

    #[test]
    fn missing_native_ownership_ack_times_out_and_late_exact_ack_cannot_grant() {
        let (host, peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(8))).unwrap();
        let (connection, queue) =
            Connection::new(host.try_clone().unwrap(), Accepted::sample(&host));
        let closed = Arc::clone(&connection.closed);
        let writer = thread::spawn(move || write_loop(&host, queue, &closed));
        let witness = ChannelSlot::for_test();
        let (descriptor, _worker) = UnixStream::pair().unwrap();
        connection
            .send_descriptor(
                response(1),
                "no-ownership-ack".into(),
                descriptor.into(),
                "office",
                witness,
            )
            .unwrap();
        let mut wire = Wire {
            writer: peer.try_clone().unwrap(),
            reader: org_socket::rights::StreamReceiver::new(peer),
            gateway: None,
            id: 1,
        };
        assert_eq!(wire.response().id, 1);
        let descriptor = wire.descriptor("no-ownership-ack");
        let until = Instant::now() + Duration::from_secs(8);
        while !connection.closed.load(Ordering::SeqCst) {
            assert!(
                Instant::now() < until,
                "writer did not enforce ownership deadline"
            );
            thread::sleep(Duration::from_millis(10));
        }
        writer.join().unwrap();
        assert!(!connection.descriptor_received("no-ownership-ack"));
        assert!(!connection.channel_received("no-ownership-ack"));
        drop(descriptor);
    }

    struct LiveHandler {
        auth: Arc<super::super::authentication::Authentication>,
        endpoint: Arc<crate::supervisor::channels::Endpoint>,
    }
    impl Handler for LiveHandler {
        fn inline(&self, _: &Request, _: &Connection) -> bool {
            true
        }
        fn disconnected(&self, connection: &Arc<Connection>) {
            self.auth.disconnect(connection);
        }
        fn handle(&self, request: Request, connection: &Arc<Connection>, _: Received) {
            if request.is::<ops::FdsReceived>() {
                super::super::acknowledge_descriptors(&request, connection);
            } else if request.is::<ops::OfficeOpenChannel>() {
                let p = request.params::<ops::OfficeOpenChannel>().unwrap();
                match self.auth.open_channel(
                    connection,
                    &p.session,
                    Arc::clone(&self.endpoint),
                    "org",
                ) {
                    Ok((id, socket, witness)) => super::super::deliver_channel(
                        connection, request.id, id, socket, "office", witness,
                    ),
                    Err(error) => connection.send_response(Response::error(request.id, error)),
                }
            } else if request.is::<ops::SessionGrant>() {
                let p = request.params::<ops::SessionGrant>().unwrap();
                match self
                    .auth
                    .grant(connection, &p.session, &p.office_id, &p.channel_id)
                {
                    Ok(result) => super::super::secret_response(connection, request.id, &result),
                    Err(error) => connection.send_response(Response::error(request.id, error)),
                }
            } else {
                let p = request.params::<ops::SessionRenew>().unwrap();
                let response = match self.auth.authorize(connection, &p.session) {
                    Ok(_) => response(request.id),
                    Err(error) => Response::error(request.id, error),
                };
                connection.send_response(response);
            }
        }
    }
    struct Wire {
        writer: UnixStream,
        reader: org_socket::rights::StreamReceiver,
        gateway: Option<org_socket::rights::DatagramReceiver>,
        id: u64,
    }
    impl Wire {
        fn request(&mut self, method: &str, params: Value) {
            self.id += 1;
            frame::write_message(
                &mut self.writer,
                &serde_json::json!({"id":self.id,"method":method,"params":params}),
            )
            .unwrap();
        }
        fn byte(&mut self, count: usize) -> (u8, Vec<std::os::fd::OwnedFd>) {
            let until = Instant::now() + Duration::from_secs(3);
            loop {
                match self.reader.receive_byte(count, &SPAWN_LOCK) {
                    Ok(result) => return result,
                    Err(org_socket::rights::ReceiveError::WouldBlock) if Instant::now() < until => {
                        thread::yield_now()
                    }
                    Err(error) => panic!("{error}"),
                }
            }
        }
        fn response(&mut self) -> Response {
            let mut line = Vec::new();
            loop {
                let (b, fds) = self.byte(0);
                assert!(fds.is_empty());
                if b == b'\n' {
                    break;
                }
                line.push(b);
            }
            let response: Response = serde_json::from_slice(&line).unwrap();
            assert_eq!(response.id, self.id);
            response
        }
        fn descriptor(&mut self, id: &str) -> UnixStream {
            let mut fds = if let Some(gateway) = &mut self.gateway {
                let mut buffer = [0; org_office_launch::TRANSFER_HEADER_MAX_BYTES];
                let until = Instant::now() + Duration::from_secs(3);
                let received = loop {
                    match gateway.receive(&mut buffer, 1, &SPAWN_LOCK) {
                        Ok(received) => break received,
                        Err(org_socket::rights::ReceiveError::WouldBlock)
                            if Instant::now() < until =>
                        {
                            thread::yield_now()
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                assert!(
                    matches!(org_office_launch::decode_transfer_record(&buffer[..received.bytes]).unwrap(),
                    org_office_launch::TransferRecord::Channel { kind: org_office_launch::ChannelKind::Web, office_id, channel_id } if office_id == "office" && channel_id == id)
                );
                received.descriptors
            } else {
                let (marker, fds) = self.byte(1);
                assert_eq!(marker, ops::HOST_SOCK_MARKER);
                fds
            };
            assert_eq!(fds.len(), 1);
            let stream = org_socket::rights::connected_unix_stream(fds.pop().unwrap()).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            stream
        }
    }
    struct Control {
        _root: tempfile::TempDir,
        worker: crate::supervisor::channels::tests::WorkerFixture,
        connection: Arc<Connection>,
        wire: Wire,
        session: org_host_control::SessionHandle,
        thread: Option<JoinHandle<()>>,
    }
    impl Control {
        fn new(gateway: bool) -> Self {
            let worker = worker_fixture("office");
            let (_root, auth, _) = super::super::authentication::tests::fixture();
            let auth = Arc::new(auth);
            let (host, peer) = UnixStream::pair().unwrap();
            let (mut connection, queue) =
                Connection::new(host.try_clone().unwrap(), Accepted::sample(&host));
            let datagrams = if gateway {
                let (transfer, peer) = UnixDatagram::pair().unwrap();
                Arc::get_mut(&mut connection).unwrap().gateway = Some(Gateway {
                transfer: Arc::new(transfer),
                listen: serde_json::from_str(r#"{"address":"127.0.0.1","port":48123,"scheme":"http_loopback","origin":"http://127.0.0.1:48123","hsts":"off"}"#).unwrap(),
                fetched: AtomicBool::new(false),
                serving: AtomicBool::new(false),
                serving_refused: AtomicBool::new(false),
            });
                Some(org_socket::rights::DatagramReceiver::new(peer))
            } else {
                None
            };
            let signed_in = auth
                .verify(
                    &connection,
                    "OWNER",
                    &Password::new("synthetic test password".into()).unwrap(),
                )
                .unwrap();
            super::super::authentication::tests::link_owner(&auth, &connection, &signed_in.session);
            let serving = Arc::clone(&connection);
            let handler = Arc::new(LiveHandler {
                auth: Arc::clone(&auth),
                endpoint: Arc::clone(&worker.endpoint),
            });
            let thread = thread::spawn(move || serve(host, serving, queue, handler, 4));
            let wire = Wire {
                writer: peer.try_clone().unwrap(),
                reader: org_socket::rights::StreamReceiver::new(peer),
                gateway: datagrams,
                id: 0,
            };
            Self {
                _root,
                worker,
                connection,
                wire,
                session: signed_in.session,
                thread: Some(thread),
            }
        }
        fn closed_cycle(&mut self, cycle: usize) -> String {
            self.wire.request(
                "office.open_channel",
                serde_json::json!({"session":self.session,"office_id":"office"}),
            );
            let result = self
                .wire
                .response()
                .into_result::<ops::OfficeOpenChannel>()
                .unwrap();
            let mut channel = self.wire.descriptor(&result.channel_id);
            let witness = lock(&self.connection.descriptor_receipts)[&result.channel_id]
                .1
                .clone();
            assert!(!self.connection.channel_received(&result.channel_id));
            if cycle.is_multiple_of(2) {
                // A worker-side protocol close is observable before the listener drops its
                // retained sender FD on client ACK. Merely dropping our copy cannot be.
                channel.write_all(b"not-json\n").unwrap();
                drop(channel);
                await_closed(&witness); // Late client ACK after authenticated worker close.
            } else {
                channel.write_all(b"actual transferred RPC").unwrap();
                let mut bytes = [0; 22];
                channel.read_exact(&mut bytes).unwrap();
                assert_eq!(&bytes, b"actual transferred RPC");
                self.wire
                    .request("fds.received", serde_json::json!({"of":result.channel_id}));
                assert!(self
                    .wire
                    .response()
                    .into_result::<ops::FdsReceived>()
                    .is_ok());
                self.wire.request("session.grant", serde_json::json!({"session":self.session,"office_id":"office","channel_id":result.channel_id}));
                assert!(self
                    .wire
                    .response()
                    .into_result::<ops::SessionGrant>()
                    .is_ok());
                drop(channel);
                await_closed(&witness);
            }
            if cycle.is_multiple_of(2) {
                self.wire
                    .request("fds.received", serde_json::json!({"of":result.channel_id}));
                assert!(self
                    .wire
                    .response()
                    .into_result::<ops::FdsReceived>()
                    .is_ok());
            }
            assert!(!self.connection.channel_received(&result.channel_id));
            self.wire.request("session.grant", serde_json::json!({"session":self.session,"office_id":"office","channel_id":result.channel_id}));
            assert!(self
                .wire
                .response()
                .into_result::<ops::SessionGrant>()
                .is_err());
            assert!(lock(&self.connection.received_channels).len() <= 1);
            result.channel_id
        }
        fn capacity_preserves_channel(&mut self) {
            // The same control connection survives genuine listener capacity refusal.
            lock(&self.connection.received_channels).clear();
            self.wire.request(
                "office.open_channel",
                serde_json::json!({"session":self.session,"office_id":"office"}),
            );
            let held = self
                .wire
                .response()
                .into_result::<ops::OfficeOpenChannel>()
                .unwrap();
            let mut held_channel = self.wire.descriptor(&held.channel_id);
            self.wire
                .request("fds.received", serde_json::json!({"of":held.channel_id}));
            assert!(self
                .wire
                .response()
                .into_result::<ops::FdsReceived>()
                .is_ok());
            for n in 0..511 {
                lock(&self.connection.received_channels)
                    .insert(format!("live-budget-{n}"), ChannelSlot::for_test());
            }
            self.wire.request(
                "office.open_channel",
                serde_json::json!({"session":self.session,"office_id":"office"}),
            );
            let error = self
                .wire
                .response()
                .into_result::<ops::OfficeOpenChannel>()
                .unwrap_err();
            assert!(
                matches!(error, org_proto::CallError::Api(error) if error.code == ErrorCode::Unavailable)
            );
            self.wire
                .request("session.renew", serde_json::json!({"session":self.session}));
            assert!(self
                .wire
                .response()
                .into_result::<ops::SessionRenew>()
                .is_ok());
            assert!(!self.connection.closed.load(Ordering::SeqCst));
            held_channel.write_all(b"still live").unwrap();
            let mut bytes = [0; 10];
            held_channel.read_exact(&mut bytes).unwrap();
            assert_eq!(&bytes, b"still live");
            self.wire.request("session.grant", serde_json::json!({"session":self.session,"office_id":"office","channel_id":held.channel_id}));
            assert!(self
                .wire
                .response()
                .into_result::<ops::SessionGrant>()
                .is_ok());
        }
        fn bad_receipt(&mut self, receipt: &str) {
            // Duplicates and foreign receipts still terminate the actual control reader.
            self.wire
                .request("fds.received", serde_json::json!({"of":receipt}));
            assert!(self
                .wire
                .response()
                .into_result::<ops::FdsReceived>()
                .is_err());
            assert!(self.connection.closed.load(Ordering::SeqCst));
        }
    }
    impl Drop for Control {
        fn drop(&mut self) {
            self.connection.close();
            self.thread.take().unwrap().join().unwrap();
        }
    }
    fn live_control_cycles(gateway: bool) {
        let mut control = Control::new(gateway);
        let mut last_channel = String::new();
        for cycle in 0..520 {
            last_channel = control.closed_cycle(cycle);
        }
        assert_eq!(control.worker.transferred.load(Ordering::SeqCst), 520);
        control.capacity_preserves_channel();
        control.bad_receipt(if gateway { &last_channel } else { "foreign" });
    }
    #[test]
    fn native_live_control_exceeds_512_closed_channels_without_losing_receipt_or_authority_checks()
    {
        live_control_cycles(false);
    }
    #[test]
    fn gateway_live_control_exceeds_512_closed_channels_without_losing_receipt_or_authority_checks()
    {
        live_control_cycles(true);
    }
}
