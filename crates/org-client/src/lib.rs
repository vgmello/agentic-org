//! # org-client: connect to the host and its offices
//!
//! Opens a connection to this machine's host or to an office server (local Unix sockets,
//! found through `org_proto::location`) and speaks `org-proto`. Used by the desktop app's
//! Tauri shell and by `org-agent`. Knows nothing about how an office stores its data.
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

mod error;

use std::collections::HashMap;
use std::io::{ErrorKind, Write};
use std::ops::Deref;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::UnixStream;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use org_proto::frame;
use org_proto::location::{self, check_socket_path};
use org_proto::{
    salvage_id, Call, CallError, Empty, Event, Hello, HelloResult, ListOfficesResult,
    ListOrganizationsResult, OfficeInfo, OfficeList, Organization, OrganizationCreate,
    OrganizationCreateParams, OrganizationList, Position, Request, Response, ServerMessage,
    Subscribe, SubscribeParams, API_VERSION, MAX_FRAME_BYTES,
};

pub use error::ClientError;

/// How long a call waits for its answer unless told otherwise.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// What `organization.create` gets by default. The host may hold a create up to 30 s while it
/// starts up (and answers `not_ready` when it would take longer), so the short default would give
/// up on a call that is going to succeed.
pub const CREATE_TIMEOUT: Duration = Duration::from_secs(60);

/// The timeout [`Client::call`] gives a call by default: [`CREATE_TIMEOUT`] for
/// `organization.create` (the one call that is slow by nature), [`DEFAULT_TIMEOUT`] for every
/// other. A caller that knows better
/// uses [`Client::call_with_timeout`].
pub fn default_timeout(method: &str) -> Duration {
    match method {
        "organization.create" => CREATE_TIMEOUT,
        _ => DEFAULT_TIMEOUT,
    }
}

/// Events waiting to be read. A reader that falls this far behind loses the connection
/// instead of making the app buffer without limit. The bound is a count, not bytes: an event
/// line is at most `MAX_FRAME_BYTES`, so a server that only sends maximum-size events can make
/// the queue hold about 256 MiB of text (and its parsed form is larger still). The server is the
/// user's own office on a 0600 socket, so this is a limit on a bug, not on an attacker.
const EVENT_QUEUE: usize = 256;

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
        self.closed.store(true, Ordering::SeqCst);
        // After the peer has half-closed, macOS refuses `shutdown(Both)` (ENOTCONN) and also
        // `shutdown(Read)`; `shutdown(Write)` still works, and that is what wakes a writer.
        if self.stream.shutdown(std::net::Shutdown::Both).is_err() {
            let _ = self.stream.shutdown(std::net::Shutdown::Read);
            let _ = self.stream.shutdown(std::net::Shutdown::Write);
        }
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
    reader: Option<JoinHandle<()>>,
}

/// The socket of this machine's host: `<office home>/host.sock`, with the office home found
/// the way the server finds it (`ORG_OFFICE_HOME`, else the user's Application Support).
pub fn default_host_socket_path() -> Result<PathBuf, ClientError> {
    host_socket_in(&location::default_office_home().ok_or(ClientError::NoOfficeHome)?)
}

fn host_socket_in(home: &Path) -> Result<PathBuf, ClientError> {
    if !home.is_absolute() {
        // The server's working directory is `/`: a relative home would name another socket.
        return Err(ClientError::NoOfficeHome);
    }
    let socket = location::host_socket_path(home);
    check_socket_path(&socket).map_err(ClientError::SocketPathTooLong)?;
    Ok(socket)
}

/// Where to connect for `office`. The host's answer is untrusted input from a local
/// process, so nothing it says can point outside `<home>/run/`.
fn office_socket(office: &OfficeInfo, home: &Path) -> Result<PathBuf, ClientError> {
    if !home.is_absolute() {
        return Err(ClientError::NoOfficeHome);
    }
    let run = home.join("run");
    let socket = match &office.endpoint {
        Some(endpoint) => {
            let endpoint = PathBuf::from(endpoint);
            if !is_socket_directly_in(&endpoint, &run) {
                return Err(ClientError::OfficeNotReachable);
            }
            endpoint
        }
        None => {
            location::office_socket_path(home, &office.id).ok_or(ClientError::OfficeNotReachable)?
        }
    };
    check_socket_path(&socket).map_err(ClientError::SocketPathTooLong)?;
    check_not_redirected(&run, &socket)?;
    Ok(socket)
}

/// Whether `path` is `<dir>/<name>.sock` with a plain name: no `..`, no extra directories.
fn is_socket_directly_in(path: &Path, dir: &Path) -> bool {
    let Ok(rest) = path.strip_prefix(dir) else {
        return false;
    };
    let mut parts = rest.components();
    matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(name)), None)
            if Path::new(name).extension().is_some_and(|ext| ext == "sock")
    )
}

/// The path rules above are lexical. This looks at the file system: `run` must be a real
/// directory (not a link to somewhere else) and the socket, if it exists, a real socket
/// (not a link or a file), so nothing planted in the office home can redirect the connection.
/// (It is a check, not a guarantee: the path is looked up again when connecting.)
fn check_not_redirected(run: &Path, socket: &Path) -> Result<(), ClientError> {
    match std::fs::symlink_metadata(run) {
        Ok(metadata) if metadata.file_type().is_dir() => {}
        // Missing is fine: the connection then fails as "not running".
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        _ => return Err(ClientError::OfficeNotReachable),
    }
    match std::fs::symlink_metadata(socket) {
        Ok(metadata) if metadata.file_type().is_socket() => Ok(()),
        Ok(_) => Err(ClientError::OfficeNotReachable),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(ClientError::Io(e)),
    }
}

impl Client {
    /// Connects to this machine's host (see [`default_host_socket_path`]).
    pub fn connect_host() -> Result<Self, ClientError> {
        Self::connect(default_host_socket_path()?)
    }

    /// Connects to the host of the office home `home` (an absolute path).
    pub fn connect_host_in(home: &Path) -> Result<Self, ClientError> {
        Self::connect(host_socket_in(home)?)
    }

    /// Connects to an office server on this machine, as the host listed it: by its
    /// `endpoint` if it gave one and that is a socket inside `<home>/run/`, else by the
    /// rule in `org_proto::location`. `home` must be absolute.
    pub fn connect_office(office: &OfficeInfo, home: &Path) -> Result<Self, ClientError> {
        Self::connect(office_socket(office, home)?)
    }

    /// Connects to the server listening on `socket`.
    pub fn connect(socket: impl AsRef<Path>) -> Result<Self, ClientError> {
        let socket = socket.as_ref();
        check_socket_path(socket).map_err(ClientError::SocketPathTooLong)?;
        let stream = UnixStream::connect(socket).map_err(|e| match e.kind() {
            ErrorKind::NotFound | ErrorKind::ConnectionRefused => ClientError::NotRunning(e),
            _ => ClientError::Io(e),
        })?;
        let io = ClientError::Io;
        let reader_stream = stream.try_clone().map_err(io)?;
        let writer_stream = stream.try_clone().map_err(io)?;

        let shared = Arc::new(Shared {
            closed: AtomicBool::new(false),
            next_id: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
            events: Arc::new(Mutex::new(None)),
            next_subscription: AtomicU64::new(1),
            queued_bytes: AtomicUsize::new(0),
            stream,
        });
        let (outbound, queue) = mpsc::sync_channel(WRITE_QUEUE);
        let reader = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("org-client-reader".into())
                .spawn(move || read_loop(&shared, reader_stream))
                .map_err(ClientError::Io)?
        };
        let writer = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("org-client-writer".into())
                .spawn(move || write_loop(&shared, writer_stream, queue))
        };
        // Not joined on drop: the writer ends once the socket is shut down and its queue is
        // gone, and `drop` must not wait on a write.
        if let Err(error) = writer {
            // The reader is already running and would never end: stop it.
            shared.close();
            let _ = reader.join();
            return Err(ClientError::Io(error));
        }
        Ok(Self {
            shared,
            outbound: Some(outbound),
            reader: Some(reader),
        })
    }

    /// Calls `C` and waits for the answer up to [`default_timeout`] for its method
    /// ([`DEFAULT_TIMEOUT`], or longer for the calls that are slow by nature).
    pub fn call<C: Call>(&self, params: &C::Params) -> Result<C::Result, ClientError> {
        self.call_with_timeout::<C>(params, default_timeout(C::METHOD))
    }

    /// Calls `C`. `timeout` bounds the whole call, sending the request and then waiting for
    /// the answer. A request that can't be fully sent by then closes the connection, because
    /// part of it may be on the wire.
    pub fn call_with_timeout<C: Call>(
        &self,
        params: &C::Params,
        timeout: Duration,
    ) -> Result<C::Result, ClientError> {
        self.call_tracked::<C>(params, timeout, &mut false)
    }

    /// [`Client::call_with_timeout`], also saying whether the request was handed to the writer
    /// and not withdrawn again (`sent`), i.e. whether the server may have seen any of it.
    fn call_tracked<C: Call>(
        &self,
        params: &C::Params,
        timeout: Duration,
        sent: &mut bool,
    ) -> Result<C::Result, ClientError> {
        let started = Instant::now();
        // A timeout too large to add to a clock reading means "very long".
        let deadline = started
            .checked_add(timeout)
            .unwrap_or_else(|| started + Duration::from_secs(60 * 60 * 24 * 365));
        let id = self.shared.next_id.fetch_add(1, Ordering::SeqCst);
        if id > MAX_REQUEST_ID {
            return Err(ClientError::Protocol(
                "this connection has used up its request ids; reconnect".into(),
            ));
        }
        let request =
            Request::new::<C>(id, params).map_err(|e| ClientError::Protocol(e.to_string()))?;
        let mut line =
            serde_json::to_vec(&request).map_err(|e| ClientError::Protocol(e.to_string()))?;
        if line.len() > MAX_FRAME_BYTES {
            // Nothing is sent, so the connection stays in step.
            return Err(ClientError::RequestTooLarge);
        }
        line.push(b'\n');
        let size = line.len();

        // Register before sending, so the answer can't beat the registration.
        let (answer_to, answer) = mpsc::sync_channel(1);
        lock(&self.shared.pending).insert(id, answer_to);
        // Registered first, then checked: either the reader's cleanup sees this call or this
        // check sees `closed`, so a call on a dead connection never waits out its timeout.
        if self.shared.closed.load(Ordering::SeqCst) {
            lock(&self.shared.pending).remove(&id);
            return Err(ClientError::Disconnected);
        }

        let state = Arc::new(AtomicU8::new(QUEUED));
        // Wait for room, but only until this call's deadline: a burst of large requests is no
        // reason to give the connection up. A genuinely stuck write is dealt with by the call
        // that is waiting on it (see `settle`), and the connection closes then.
        let mut waiting = Some(Outgoing {
            line,
            deadline,
            state: Arc::clone(&state),
        });
        loop {
            let message = waiting
                .take()
                .expect("the message is put back before looping");
            let outbound = match (&self.outbound, self.shared.closed.load(Ordering::SeqCst)) {
                (Some(outbound), false) => outbound,
                _ => {
                    lock(&self.shared.pending).remove(&id);
                    return Err(ClientError::Disconnected);
                }
            };
            // Reserve room in the byte budget, never going over it (not even for a moment).
            let reserved = self
                .shared
                .queued_bytes
                .try_update(Ordering::SeqCst, Ordering::SeqCst, |queued| {
                    (queued + size <= MAX_QUEUED_BYTES).then_some(queued + size)
                })
                .is_ok();
            if reserved {
                match outbound.try_send(message) {
                    Ok(()) => {
                        *sent = true;
                        break;
                    }
                    Err(TrySendError::Full(message)) => {
                        self.shared.queued_bytes.fetch_sub(size, Ordering::SeqCst);
                        waiting = Some(message);
                    }
                    Err(TrySendError::Disconnected(_)) => {
                        self.shared.queued_bytes.fetch_sub(size, Ordering::SeqCst);
                        lock(&self.shared.pending).remove(&id);
                        return Err(ClientError::Disconnected);
                    }
                }
            } else {
                waiting = Some(message);
            }
            if Instant::now() >= deadline {
                lock(&self.shared.pending).remove(&id);
                return Err(ClientError::Timeout);
            }
            std::thread::sleep(Duration::from_millis(1));
        }

        let mut outcome = answer.recv_timeout(deadline.saturating_duration_since(Instant::now()));
        // On timeout the entry is still there: drop it, so a late answer is ignored.
        let entry = lock(&self.shared.pending).remove(&id);
        if matches!(outcome, Err(mpsc::RecvTimeoutError::Timeout)) {
            // An answer that came in between the wait ending and the entry going is still an
            // answer: a call that succeeded is not reported as timed out (the caller might
            // retry a create that went through). If the entry was already gone, the reader has
            // taken it and is about to send: wait a moment for that.
            let late = if entry.is_none() {
                answer.recv_timeout(Duration::from_millis(100)).ok()
            } else {
                answer.try_recv().ok()
            };
            if let Some(late) = late {
                outcome = Ok(late);
            }
        }
        if !matches!(outcome, Ok(Ok(_))) && self.settle(&state) {
            // Withdrawn before the writer started on it: the server never saw any of it.
            *sent = false;
        }
        match outcome {
            Ok(Ok(response)) => response.into_result::<C>().map_err(|error| match error {
                CallError::Api(error) => ClientError::Api(error),
                CallError::Decode(_) => {
                    ClientError::undecodable(&format!("the result of `{}`", C::METHOD))
                }
            }),
            Ok(Err(error)) => Err(error),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(ClientError::Timeout),
            // A connection closed at or after the call's deadline (the close above, or the
            // writer giving up a half-sent request) means this call ran out of time: say so,
            // whichever of the two got there first.
            Err(mpsc::RecvTimeoutError::Disconnected) if Instant::now() >= deadline => {
                Err(ClientError::Timeout)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(ClientError::Disconnected),
        }
    }

    /// This call is out of time (or the connection is going away). What happens to its request
    /// depends on how far the writer got:
    /// - not started: withdraw it, so it is never sent (nothing is on the wire);
    /// - being written: part of it may be on the wire. The writer decides: it finishes, or it
    ///   notices the deadline and gives the request up itself, usually within milliseconds, so
    ///   a healthy connection is not closed just because the deadline fell while the writer was
    ///   busy. Only if the write is still stuck after [`WRITE_GRACE`] does this call close the
    ///   connection, which also wakes the blocked write (a peer that takes a few bytes at a
    ///   time keeps one `write()` going well past any deadline).
    ///
    /// Returns whether the request was withdrawn without any of it being written.
    ///
    /// Known limit: the writer thread is in `WRITING` from the moment it picks a request up
    /// until the whole request is out. A write that is still going `WRITE_GRACE` after the
    /// deadline (a peer that takes a large request slowly, or a writer that is not scheduled
    /// for that long under heavy load or a stopped process) looks the same as a stuck write,
    /// and the caller then closes the connection, healthy or not. Callers must be ready to
    /// reconnect.
    fn settle(&self, state: &AtomicU8) -> bool {
        match state.compare_exchange(QUEUED, ABANDONED, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) | Err(ABANDONED) => return true,
            Err(WRITING) => {}
            Err(_) => return false, // written
        }
        let give_up_at = Instant::now() + WRITE_GRACE;
        while Instant::now() < give_up_at {
            let now = state.load(Ordering::SeqCst);
            if now != WRITING || self.shared.closed.load(Ordering::SeqCst) {
                return now == ABANDONED;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        if state.load(Ordering::SeqCst) == WRITING {
            self.shared.close();
        }
        false
    }

    /// `hello`, then checks the protocol version. A server of another version is refused:
    /// use this first on every new connection.
    pub fn hello(&self) -> Result<HelloResult, ClientError> {
        let hello = self.call::<Hello>(&Empty {})?;
        if hello.api_version != API_VERSION {
            return Err(ClientError::VersionMismatch {
                client: API_VERSION,
                server: hello.api_version,
            });
        }
        Ok(hello)
    }

    /// `organization.create` (host): creates an organization and starts its HQ office, waiting
    /// up to [`CREATE_TIMEOUT`] (the host may hold a create while it starts up). Check
    /// the HQ office's `status` in the result: the organization exists even if its server
    /// hasn't started.
    pub fn create_organization(&self, name: &str) -> Result<Organization, ClientError> {
        self.call::<OrganizationCreate>(&OrganizationCreateParams { name: name.into() })
    }

    /// `organization.list` (host): every organization with an office on this machine.
    pub fn list_organizations(&self) -> Result<Vec<Organization>, ClientError> {
        let ListOrganizationsResult { organizations } = self.call::<OrganizationList>(&Empty {})?;
        Ok(organizations)
    }

    /// `office.list` (host): every office on this machine, with how to reach it.
    pub fn list_offices(&self) -> Result<Vec<OfficeInfo>, ClientError> {
        let ListOfficesResult { offices } = self.call::<OfficeList>(&Empty {})?;
        Ok(offices)
    }

    /// Subscribes to server events. The [`Subscription`] ends when the connection closes, and
    /// holds at most 256 events: a reader that falls further behind loses the connection
    /// (calls then fail with `Disconnected`). So do not leave it unread for long, and note
    /// that more than 256 events arriving before the server's answer to `subscribe` also
    /// closes it.
    ///
    /// Once at a time: while a subscription is alive, a second call fails with
    /// [`ClientError::AlreadySubscribed`] and leaves the first as it was. Dropping the
    /// subscription releases it. If the call times out the server may or may not have
    /// subscribed, so the connection is closed: reconnect. (A timeout while the request was
    /// still waiting its turn behind another call's write is different: nothing was sent, so
    /// the connection is left alone.)
    ///
    /// This follows the live stream from now on. To resume after a snapshot without missing
    /// a change, use [`Client::subscribe_from`].
    pub fn subscribe(&self) -> Result<Subscription, ClientError> {
        self.subscribe_from(None)
    }

    /// Like [`Client::subscribe`], but resumes from `from_position` (the position of an
    /// `org.snapshot`): the server first sends every change after it, in order, then follows
    /// live. If the server can no longer resume from that position it answers
    /// `snapshot_required` (a [`ClientError::Api`] with that code): take a fresh snapshot and
    /// subscribe from its position. `None` follows live only.
    pub fn subscribe_from(
        &self,
        from_position: Option<Position>,
    ) -> Result<Subscription, ClientError> {
        self.subscribe_from_with_timeout(from_position, DEFAULT_TIMEOUT)
    }

    /// [`Client::subscribe_from`] with a timeout of your own. If the timeout passes after the
    /// request was sent, the server may or may not have subscribed, so the connection is closed;
    /// if it passes while the request was still waiting its turn (nothing sent), the connection
    /// is left alone.
    pub fn subscribe_from_with_timeout(
        &self,
        from_position: Option<Position>,
        timeout: Duration,
    ) -> Result<Subscription, ClientError> {
        let (events, received) = mpsc::sync_channel(EVENT_QUEUE);
        let id = self.shared.next_subscription.fetch_add(1, Ordering::SeqCst);
        {
            // Install the channel first: an event may arrive before the subscribe answer.
            let mut installed = lock(&self.shared.events);
            if installed.is_some() {
                return Err(ClientError::AlreadySubscribed);
            }
            *installed = Some((id, events));
        }
        // From here the `Subscription` owns the slot, and releases it when dropped.
        let subscription = Subscription {
            events: received,
            id,
            slot: Arc::clone(&self.shared.events),
        };
        let mut sent = false;
        let params = SubscribeParams {
            from_position,
            from_work_position: None,
        };
        match self.call_tracked::<Subscribe>(&params, timeout, &mut sent) {
            Ok(Empty {}) => Ok(subscription),
            Err(error) => {
                if sent && matches!(error, ClientError::Timeout) {
                    self.shared.close();
                }
                // `subscription` drops here and releases the slot.
                Err(error)
            }
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.shared.close();
        // Let the writer thread end once its queue is empty.
        self.outbound.take();
        if let Some(reader) = self.reader.take() {
            // The shutdown above ends the reader's read. On macOS it has been seen not to wake
            // it (in about one drop in a hundred), and an unconditional join would then freeze
            // whoever dropped the client, usually a UI thread. Wait a moment, then let the
            // thread go: it ends when the server closes its side.
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

/// The writer thread: writes queued requests, each within its own deadline.
fn write_loop(shared: &Shared, mut stream: UnixStream, queue: Receiver<Outgoing>) {
    for message in queue {
        shared
            .queued_bytes
            .fetch_sub(message.line.len(), Ordering::SeqCst);
        if shared.closed.load(Ordering::SeqCst) {
            return;
        }
        // Too late already: the caller has given up, and nothing of it has been sent.
        if Instant::now() >= message.deadline {
            let _ = message.state.compare_exchange(
                QUEUED,
                ABANDONED,
                Ordering::SeqCst,
                Ordering::SeqCst,
            );
            continue;
        }
        // The caller may have withdrawn it in the meantime.
        if message
            .state
            .compare_exchange(QUEUED, WRITING, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            continue;
        }
        match write_before(&mut stream, &message.line, message.deadline) {
            Ok(Written::Whole) => message.state.store(WRITTEN, Ordering::SeqCst),
            // Never started after all: nothing is on the wire, so the stream is in step.
            Ok(Written::NotStarted) => message.state.store(ABANDONED, Ordering::SeqCst),
            // The write failed part-way: the stream can't be trusted.
            Err(_) => {
                shared.close();
                return;
            }
        }
    }
}

enum Written {
    Whole,
    NotStarted,
}

/// Writes all of `bytes`, unless `deadline` has already passed before the first byte
/// (`NotStarted`: nothing is on the wire, so the stream is still in step).
///
/// Once started, a request is FINISHED even if its deadline passes meanwhile: stopping would
/// leave half a frame on the wire and the connection could not be used again. A write that
/// is merely slow therefore costs the caller its timeout but not the connection. A write that
/// is stuck is cut by the caller (see [`Client::settle`]): after its deadline plus
/// [`WRITE_GRACE`] it closes the connection, which wakes the blocked `write()`. (A single
/// `write()` can outlast any deadline against a peer that takes a few bytes at a time.)
fn write_before(
    stream: &mut UnixStream,
    bytes: &[u8],
    deadline: Instant,
) -> std::io::Result<Written> {
    let mut rest = bytes;
    while !rest.is_empty() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() && rest.len() == bytes.len() {
            return Ok(Written::NotStarted);
        }
        // A write that has started but is still not done `WRITE_GRACE` after the deadline is
        // given up here, which closes the connection (part of the request is on the wire).
        // This does not wait for a caller to notice: a caller whose call was answered early
        // (a server may answer before it has read the whole request) never will.
        if remaining.is_zero() && Instant::now() >= deadline + WRITE_GRACE {
            return Err(ErrorKind::TimedOut.into());
        }
        // Past the deadline, keep going in short steps; the check above (or the caller, in
        // `settle`) cuts it if it sticks.
        let step = if remaining.is_zero() {
            WRITE_GRACE
        } else {
            remaining
        };
        stream.set_write_timeout(Some(step))?;
        match stream.write(rest) {
            Ok(0) => return Err(ErrorKind::WriteZero.into()),
            Ok(n) => rest = &rest[n..],
            // No progress this step; try again (the caller cuts a write that stays stuck).
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(Written::Whole)
}

/// Makes sure the connection is closed however the reader ends, a panic included.
struct CloseOnExit<'a>(&'a Shared);

impl Drop for CloseOnExit<'_> {
    fn drop(&mut self) {
        self.0.close();
    }
}

/// Reads server messages until the connection ends, then fails everything still waiting.
fn read_loop(shared: &Shared, stream: UnixStream) {
    let _close = CloseOnExit(shared);
    let mut reader = std::io::BufReader::new(stream);
    while let Ok(Some(line)) = frame::read_line(&mut reader) {
        match serde_json::from_slice::<ServerMessage>(&line) {
            Ok(ServerMessage::Response(response)) => {
                if let Some(waiting) = lock(&shared.pending).remove(&response.id) {
                    let _ = waiting.send(Ok(response));
                }
                // No one waiting: a late answer to a call that timed out, or id 0 (the
                // server couldn't read our request and had no id to answer under).
            }
            Ok(ServerMessage::Event(event)) => {
                let mut installed = lock(&shared.events);
                match installed.as_ref().map(|(_, events)| events.try_send(event)) {
                    Some(Err(TrySendError::Full(_))) => {
                        // The app isn't keeping up. Give the connection up rather than buffer.
                        drop(installed);
                        return;
                    }
                    // The receiver is gone: release the subscription.
                    Some(Err(TrySendError::Disconnected(_))) => *installed = None,
                    Some(Ok(())) | None => {}
                }
            }
            // A message shape from a newer server.
            Ok(ServerMessage::Unknown(_)) => {}
            Err(_) => {
                // A known shape that doesn't decode: fail just the call it answered, if
                // its id can be read; the stream itself is still in step.
                let waiting = salvage_id(&line).and_then(|id| lock(&shared.pending).remove(&id));
                if let Some(waiting) = waiting {
                    let _ = waiting.send(Err(ClientError::undecodable("the answer")));
                }
            }
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixListener;

    use super::*;

    /// A server that accepts one connection and never reads or answers, until dropped.
    struct StuckServer {
        _dir: tempfile::TempDir,
        path: PathBuf,
        _release: mpsc::Sender<()>,
    }

    fn stuck_server() -> StuckServer {
        let dir = tempfile::Builder::new().prefix("oc").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let (release, held) = mpsc::channel::<()>();
        std::thread::spawn(move || {
            let connection = listener.accept();
            let _ = held.recv(); // returns when the test drops `release`
            drop(connection);
        });
        StuckServer {
            _dir: dir,
            path,
            _release: release,
        }
    }

    #[test]
    fn a_client_can_be_shared_between_threads() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Client>();
    }

    #[test]
    fn a_request_has_ten_seconds_unless_its_kind_is_slow_by_nature() {
        assert_eq!(DEFAULT_TIMEOUT, Duration::from_secs(10));
        assert_eq!(default_timeout("hello"), DEFAULT_TIMEOUT);
        assert_eq!(default_timeout("organization.list"), DEFAULT_TIMEOUT);
        assert_eq!(default_timeout(OrganizationCreate::METHOD), CREATE_TIMEOUT);
        // The host may hold a create 30 s at startup.
        assert!(CREATE_TIMEOUT >= Duration::from_secs(45));
    }

    #[test]
    fn the_write_queue_never_holds_more_than_its_byte_budget_and_does_fill_it() {
        let server = stuck_server();
        let client = Arc::new(Client::connect(&server.path).unwrap());
        // Each request is nearly the largest a frame may be, and nothing is ever read, so the
        // writer sticks on the first and the rest queue up to the budget.
        let name = "x".repeat(MAX_FRAME_BYTES - 1024);
        let callers: Vec<_> = (0..40)
            .map(|_| {
                let client = Arc::clone(&client);
                let name = name.clone();
                std::thread::spawn(move || {
                    client.call_with_timeout::<OrganizationCreate>(
                        &OrganizationCreateParams { name },
                        Duration::from_secs(60),
                    )
                })
            })
            .collect();
        // Watch the counter until the budget is reached (as long as a loaded machine needs),
        // then cut the connection so the callers end at once.
        let mut most = 0;
        let give_up = Instant::now() + Duration::from_secs(20);
        while most + 2 * MAX_FRAME_BYTES < MAX_QUEUED_BYTES && Instant::now() < give_up {
            most = most.max(client.shared.queued_bytes.load(Ordering::SeqCst));
            std::thread::sleep(Duration::from_millis(1));
        }
        // Keep watching a little longer: it must never go over, even once full.
        let settle = Instant::now() + Duration::from_millis(300);
        while Instant::now() < settle {
            most = most.max(client.shared.queued_bytes.load(Ordering::SeqCst));
            std::thread::sleep(Duration::from_millis(1));
        }
        drop(server);
        for caller in callers {
            let outcome = caller.join().unwrap();
            assert!(
                matches!(
                    outcome,
                    Err(ClientError::Timeout | ClientError::Disconnected)
                ),
                "{outcome:?}"
            );
        }
        assert!(
            most <= MAX_QUEUED_BYTES,
            "{most} bytes were queued; the budget is {MAX_QUEUED_BYTES}"
        );
        assert!(
            most + 2 * MAX_FRAME_BYTES >= MAX_QUEUED_BYTES,
            "only {most} bytes were ever queued: the budget was not what held the callers back"
        );
    }

    #[test]
    fn request_ids_stop_before_they_pass_two_to_the_fifty_third() {
        let server = stuck_server();
        let client = Client::connect(&server.path).unwrap();
        client
            .shared
            .next_id
            .store(MAX_REQUEST_ID, Ordering::SeqCst);
        // The last id is still usable (it times out: nobody answers)...
        let last = client.call_with_timeout::<Hello>(&Empty {}, Duration::from_millis(50));
        assert!(matches!(last, Err(ClientError::Timeout)), "{last:?}");
        // ...and the next call is refused rather than sent with an id the peer may misread.
        let next = client.call_with_timeout::<Hello>(&Empty {}, Duration::from_millis(50));
        assert!(matches!(next, Err(ClientError::Protocol(_))), "{next:?}");
    }
}
