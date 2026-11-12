//! The office's socket: JSON lines (`org_proto::frame`), one thread per connection. It answers
//! `hello`, `org.snapshot`, `msg.bind`, `who`, `msg.send` / `msg.inbox` / `msg.read` /
//! `msg.wait`, `subscribe` and the organization commands, which only an app-bound connection may
//! send, and the work writes made as a bound member; other methods are `unknown_method`. A `msg.wait` is held on a thread of its own
//! (at most [`MAX_HELD`] per connection), so the connection keeps answering meanwhile. When the
//! server stops, every connection is closed and its thread joined BEFORE the office is, so no thread
//! touches a database that is closing.

use std::collections::HashMap;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::bounded;
use crate::follow::{self, OrgFeed, Timing};
use crate::live::LiveEvents;
use crate::msg_calls;
use org_mailbox::{cancellation_pair, CancelHandle, Mailbox, MemberDirectory};
use org_office::rules::OrgCommand;
use org_proto::directory::{
    StatusSet, StatusSetParams, StatusSetResult, Who, WhoParams, WhoResult,
};
use org_proto::frame;
use org_proto::msg::{
    json_string_len, MsgBind, MsgBindParams, MsgBindResult, MsgWait, MAX_HANDLE_BYTES,
    OFFICE_SENDER,
};
use org_proto::org::{CommandAccepted, TeamCreate};
use org_proto::work::{TaskCreate, WorkAccepted};
use org_proto::{AgentStatus, MemberKind};
use org_proto::{
    ApiError, AppToken, ErrorCode, Hello, HelloResult, OfficeHello, OfficeInfo, OfficeRole,
    OfficeStatus, OrgSnapshot, OrgSnapshotCall, Request, Response,
};
use org_work::{Caller, WorkCall};

/// Reads the organization's snapshot (the office's `org_snapshot`), or says why it cannot.
/// Availability is distinct from storage failure at the organization read boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotFailure {
    Unavailable,
    Storage,
}

pub type SnapshotFn = Box<dyn Fn() -> Result<OrgSnapshot, SnapshotFailure> + Send + Sync>;

/// Runs one organization command (the office's `submit_org`): HQ's answer, or why it could not
/// answer at all (storage).
pub type SubmitFn =
    Box<dyn Fn(OrgCommand) -> Result<Result<CommandAccepted, ApiError>, String> + Send + Sync>;

/// Reads a retained org operation for today's implicit owner connection.
pub type OperationFn = Box<
    dyn Fn(&str) -> Result<Result<org_proto::OperationOutcome, ApiError>, String> + Send + Sync,
>;

/// Answers `who` (the office's `who`): the cards, the caller's mistake, or why it could not
/// answer at all (storage).
pub type WhoFn =
    Box<dyn Fn(&WhoParams) -> Result<Result<WhoResult, ApiError>, String> + Send + Sync>;

/// Sets or clears the given member's status line (the office's `set_status`): the result, the
/// caller's mistake, or why it could not answer at all (storage).
pub type StatusFn = Box<
    dyn Fn(&str, &StatusSetParams) -> Result<Result<StatusSetResult, ApiError>, String>
        + Send
        + Sync,
>;

pub type WorkFn =
    Box<dyn Fn(Caller, WorkCall) -> Result<Result<WorkAccepted, ApiError>, String> + Send + Sync>;

/// What the connections ask of the office.
pub struct OfficePorts {
    pub snapshot: SnapshotFn,
    pub submit: SubmitFn,
    pub work: WorkFn,
    pub operation_get: OperationFn,
    pub who: WhoFn,
    pub status: StatusFn,
    /// The log, for `subscribe`.
    pub feed: Box<dyn OrgFeed>,
    /// Live events for `subscribe` (messages, status lines).
    pub live: Arc<LiveEvents>,
    /// Messaging: what `msg.send`, `msg.inbox`, `msg.read` and `msg.wait` run on.
    pub mailbox: Arc<Mailbox>,
    /// Who a `msg.bind` names.
    pub directory: Arc<dyn MemberDirectory + Send + Sync>,
}

/// Launch v3 retains its grant key; grant verification is enabled separately in 2a.5.
pub(super) enum LaunchAuth {
    Legacy(AppToken),
    Grants { _key: org_proto::GrantKey },
}

impl From<AppToken> for LaunchAuth {
    fn from(token: AppToken) -> Self {
        Self::Legacy(token)
    }
}

impl LaunchAuth {
    fn accepts(&self, token: &AppToken) -> bool {
        matches!(self, Self::Legacy(expected) if token == expected)
    }
}

/// Who a connection is, as far as the socket knows.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Binding {
    /// Not bound, or the last bind failed.
    Anonymous,
    /// Bound app-wide with the office's `app_token`, as no member.
    App,
    /// Bound as a member: `id` is the member's canonical id; `with_app_token` when a person or
    /// the owner bound with the `app_token` (an agent binds without one).
    Member { id: String, with_app_token: bool },
}

impl Binding {
    /// Whether the connection is bound with the `app_token` (the app, or a person or the owner
    /// bound with it): the only kind that may send organization commands.
    fn is_app_bound(&self) -> bool {
        matches!(
            self,
            Binding::App
                | Binding::Member {
                    with_app_token: true,
                    ..
                }
        )
    }

    /// Whether the connection is bound at all.
    fn is_bound(&self) -> bool {
        *self != Binding::Anonymous
    }

    /// The member the connection acts as, if it is bound as one.
    fn member(&self) -> Option<&str> {
        match self {
            Binding::Member { id, .. } => Some(id),
            _ => None,
        }
    }
}

/// Most `msg.wait` calls held at once on one connection; more get `unavailable`.
pub(crate) const MAX_HELD: usize = org_proto::msg::MAX_HELD_CALLS_PER_CONNECTION;

/// What other threads may do to a connection: end its held calls and its subscription. The
/// watcher does it when the connection's member stops being bindable, without waiting for the
/// connection's next call.
#[derive(Default)]
struct Control {
    held: Held,
    /// Moves on whenever the connection's binding changes or the reader ends, which stops the
    /// events of the binding before it.
    generation: AtomicU64,
    /// Set by the watcher when it has ended this connection's held calls and subscription
    /// because its member can no longer be bound: the connection thread refuses the next call
    /// with `not_bound`, then closes. If it remains idle, the next reader tick ends it and
    /// removes its registration.
    revoked: std::sync::atomic::AtomicBool,
}

/// The member-bound connections, by connection number: who each acts as, and how to reach it.
type Bound = Mutex<HashMap<u64, (String, Arc<Control>)>>;

/// Keeps `conn`'s entry in [`Bound`] equal to the member it acts as. Called whenever its binding
/// may have changed; removes the entry when the connection is not bound as a member, and when
/// the connection ends.
struct Registration<'a> {
    bound: &'a Bound,
    conn: u64,
    control: Arc<Control>,
    member: Option<String>,
}

impl Registration<'_> {
    /// `true` when the connection is newly registered as `member`: it may have become unbindable
    /// before the watcher could see it, so the caller looks once.
    fn sync(&mut self, member: Option<&str>) -> bool {
        if self.member.as_deref() == member {
            return false;
        }
        let mut bound = self.bound.lock().unwrap_or_else(|p| p.into_inner());
        // Under the lock the watcher acts under: a decision about the member this connection
        // had must not unbind the one it has now.
        match member {
            Some(_) if self.control.revoked.load(Ordering::SeqCst) => return false,
            Some(member) => {
                bound.insert(self.conn, (member.to_owned(), Arc::clone(&self.control)));
            }
            None => {
                // A lookup-failure unbind must not erase a watcher revocation that raced
                // the call's liveness check. The reader still needs to observe that flag.
                bound.remove(&self.conn);
            }
        }
        self.member = member.map(str::to_owned);
        member.is_some()
    }
}

impl Drop for Registration<'_> {
    fn drop(&mut self) {
        self.bound
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.conn);
    }
}

/// The held calls of a connection, so a re-bind or the end of the connection can end them.
#[derive(Default)]
struct Held {
    next: Mutex<(u64, HashMap<u64, CancelHandle>)>,
}

impl Held {
    /// Room for one more: its number and a way to cancel it, or `None` when `MAX_HELD` are held.
    fn start(&self, cancel: CancelHandle) -> Option<u64> {
        let mut state = self.next.lock().unwrap_or_else(|p| p.into_inner());
        if state.1.len() >= MAX_HELD {
            return None;
        }
        state.0 += 1;
        let number = state.0;
        state.1.insert(number, cancel);
        Some(number)
    }

    fn finish(&self, number: u64) {
        self.next
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .1
            .remove(&number);
    }

    /// Ends every held call (each answers `not_bound`; one that had already chosen a message may
    /// still answer with it, under its own id, before or after the bind's answer: the contract
    /// fixes no order) and gives their places back at once, so
    /// a client that binds again can wait again. The cancelled calls are already ending; their
    /// threads are bounded by the write deadline of the answer they still send.
    fn cancel_all(&self) {
        let ended = std::mem::take(&mut self.next.lock().unwrap_or_else(|p| p.into_inner()).1);
        for handle in ended.values() {
            handle.cancel();
        }
    }
}

/// Ends the held calls when the connection's thread stops for any reason, a panic included:
/// `thread::scope` joins the held calls' threads first, and they would otherwise wait out their
/// timeouts.
struct CancelOnDrop<'a>(&'a Held);

impl Drop for CancelOnDrop<'_> {
    fn drop(&mut self) {
        self.0.cancel_all();
    }
}

/// A held call's place, given back however its thread ends (a panic included) and only after its
/// answer is written, so a client that does not read cannot make threads pile up past the cap.
struct Place<'a>(&'a Held, u64);

impl Drop for Place<'_> {
    fn drop(&mut self) {
        self.0.finish(self.1);
    }
}

/// Most connections served at once; more are closed at once.
pub(crate) const MAX_CONNECTIONS: usize = 64;

/// What every connection shares.
pub struct Shared {
    pub hello: HelloResult,
    office_id: String,
    ports: OfficePorts,
    /// Typed launch authority, retained and zeroed when `Shared` drops.
    auth: LaunchAuth,
    timing: Timing,
    /// The connections bound as a member, for the watcher.
    bound: Bound,
}

impl Shared {
    /// The same, with other timings (tests).
    #[cfg(test)]
    fn with_timing(mut self, timing: Timing) -> Self {
        self.timing = timing;
        self
    }

    /// The `hello` this office answers with.
    pub fn new(
        info: &org_office::OfficeInfo,
        socket: &Path,
        ports: OfficePorts,
        auth: impl Into<LaunchAuth>,
    ) -> Self {
        Self {
            office_id: info.office_id.clone(),
            ports,
            auth: auth.into(),
            timing: Timing::default(),
            bound: Mutex::default(),
            // Part 1 implements only what every server of this api_version has: no capabilities.
            hello: HelloResult::office(
                env!("CARGO_PKG_VERSION"),
                "This Mac",
                OfficeHello {
                    organization_name: info.org_name.clone(),
                    office: OfficeInfo {
                        id: info.office_id.clone(),
                        organization_id: info.org_id.clone(),
                        name: info.office_name.clone(),
                        role: match info.role {
                            org_office::OfficeRole::Hq => OfficeRole::Hq,
                        },
                        host: "This Mac".to_owned(),
                        endpoint: Some(socket.display().to_string()),
                        status: OfficeStatus::Active,
                        status_reason: None,
                        next_restart_at: None,
                    },
                },
            ),
        }
    }
}

/// Binds `path` (owner-only), replacing a stale socket file a crashed server left: the caller
/// holds the office lock, so no other server of this office is alive.
pub fn bind(path: &Path) -> std::io::Result<UnixListener> {
    match std::fs::symlink_metadata(path) {
        // Only a stale socket (a crashed server's) is replaced; anything else is not ours.
        Ok(metadata) if metadata.file_type().is_socket() => {
            // A socket someone still answers on is a live server, not a stale file.
            if UnixStream::connect(path).is_ok() {
                return Err(std::io::Error::other("a server is already on this socket"));
            }
            std::fs::remove_file(path)?
        }
        Ok(_) => return Err(std::io::Error::other("the socket path is not a socket")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

/// Accepts connections until `stop` says to quit, then closes every connection and waits for
/// its thread, so nothing is still reading the office when the caller closes it.
pub fn run(listener: &UnixListener, shared: &Arc<Shared>, stop: &dyn Fn() -> bool) {
    let stopping = Arc::new(std::sync::atomic::AtomicBool::new(false));
    // Ends the held calls of a connection whose member stops being bindable, at once.
    let watching = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let watcher = {
        let (shared, watching) = (Arc::clone(shared), Arc::clone(&watching));
        std::thread::Builder::new()
            .name("org-office-watch".into())
            .spawn(move || watch(&shared, &watching))
            .ok()
    };
    let live = Arc::new(AtomicUsize::new(0));
    // A handle on each connection, to close it at shutdown.
    let open: Arc<Mutex<HashMap<u64, UnixStream>>> = Arc::default();
    let mut threads: Vec<JoinHandle<()>> = Vec::new();
    let mut next_id = 0u64;
    while !stop() {
        match listener.accept() {
            Ok((stream, _)) => {
                if live.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                    live.fetch_sub(1, Ordering::SeqCst);
                    continue; // dropped: closed at once
                }
                next_id += 1;
                let id = next_id;
                // A connection that can't be registered can't be closed at shutdown: refuse it.
                let Ok(handle) = stream.try_clone() else {
                    live.fetch_sub(1, Ordering::SeqCst);
                    continue;
                };
                open.lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .insert(id, handle);
                let shared = Arc::clone(shared);
                let stopping = Arc::clone(&stopping);
                let slot = Slot(Arc::clone(&live));
                let registered = Registered {
                    registry: Arc::clone(&open),
                    id,
                };
                // If the thread cannot start, the closure (and so the slot and the registry
                // entry) is dropped with the error: both are given back, the connection closes.
                if let Ok(thread) = std::thread::Builder::new()
                    .name("org-office-conn".into())
                    .spawn(move || {
                        // Both given back however the thread ends, panics included: the slot,
                        // and the handle that would otherwise keep the socket open.
                        let _slot = slot;
                        let _registered = registered;
                        let _ = stream.set_nonblocking(false);
                        serve_connection(stream, &shared, id, &stopping);
                    })
                {
                    threads.retain(|t| !t.is_finished());
                    threads.push(thread);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            // A connection that died before accept: carry on.
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    // The tick also observes stop if a platform shutdown misses its reader wakeup.
    stopping.store(true, Ordering::SeqCst);
    // Stop: close every connection, then wait for the threads.
    for stream in open.lock().unwrap_or_else(|p| p.into_inner()).values() {
        let _ = stream.shutdown(std::net::Shutdown::Both);
    }
    for thread in threads {
        let _ = thread.join();
    }
    watching.store(false, Ordering::SeqCst);
    if let Some(watcher) = watcher {
        let _ = watcher.join();
    }
}

/// Whenever the organization's log has moved (and the first time), looks at each member-bound
/// connection's member: one that is no longer bindable (offboarded, made office-run, removed) has
/// its held `msg.wait` calls ended with `not_bound` and its subscription ended, without waiting
/// for the connection's next call. An idle revoked connection closes at its next reader tick;
/// a call that arrives first receives `not_bound`, then the connection closes. A lookup that
/// fails ends nothing. Organization changes are what can unbind a member, so a quiet log costs
/// one cheap head read per tick.
fn watch(shared: &Shared, watching: &std::sync::atomic::AtomicBool) {
    let mut last_head = None;
    while watching.load(Ordering::SeqCst) {
        // Before looking, so a change while looking wakes the wait below at once.
        let token = shared.ports.feed.token();
        let head = shared.ports.feed.head().ok();
        if head.is_none() || head != last_head {
            // A panic in a lookup must not end the watching for good.
            let pass = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| revoke(shared)));
            match pass {
                // Settled: this head is done with.
                Ok(false) => last_head = head,
                // A lookup could not answer: look again on the next tick.
                Ok(true) => {}
                Err(_) => {
                    eprintln!("org-server: the member watcher failed once; it carries on");
                }
            }
        }
        shared.ports.feed.wait(token, shared.timing.idle);
    }
}

/// One pass of the watcher. `true` when a lookup could not answer, so the pass must be repeated.
fn revoke(shared: &Shared) -> bool {
    let mut undecided = false;
    let members: std::collections::HashSet<String> = shared
        .bound
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .values()
        .map(|(member, _)| member.clone())
        .collect();
    // One lookup per member, however many connections it has.
    for member in members {
        match lookup(shared, &member) {
            Lookup::Failed => {
                undecided = true;
                continue;
            }
            Lookup::Bindable(found) if found.id == member => continue,
            Lookup::Bindable(_) | Lookup::Not => {}
        }
        // Confirmed under the registry's lock, on the connections that STILL act as this member:
        // one that bound as someone else, or bound again after the member was restored, is left
        // alone.
        let bound = shared.bound.lock().unwrap_or_else(|p| p.into_inner());
        if !is_gone(shared, &member) {
            continue;
        }
        for (_, control) in bound.values().filter(|(m, _)| *m == member) {
            // `revoked` first: a wait that starts after the cancel below sees it.
            control.revoked.store(true, Ordering::SeqCst);
            control.generation.fetch_add(1, Ordering::SeqCst);
            control.held.cancel_all();
        }
    }
    undecided
}

/// A connection's entry in the registry; dropping it removes the entry (and so the extra handle
/// on the socket), even if the connection's thread panics.
struct Registered {
    registry: Arc<Mutex<HashMap<u64, UnixStream>>>,
    id: u64,
}

impl Drop for Registered {
    fn drop(&mut self) {
        self.registry
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.id);
    }
}

/// One of the connection slots; dropping it gives the slot back, even if the thread panics.
struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn serve_connection(
    stream: UnixStream,
    shared: &Shared,
    conn: u64,
    stopping: &std::sync::atomic::AtomicBool,
) {
    serve_connection_with_poll(
        stream,
        shared,
        conn,
        stopping,
        org_socket::READ_TICK,
        org_socket::poll_readable,
    );
}

fn serve_connection_with_poll(
    stream: UnixStream,
    shared: &Shared,
    conn: u64,
    stopping: &std::sync::atomic::AtomicBool,
    tick: Duration,
    poll: impl FnMut(std::os::fd::RawFd, Duration) -> std::io::Result<bool>,
) {
    let Ok(out) = stream.try_clone() else {
        return;
    };
    // One writer at a time: answers and pushed events never interleave. Every frame has a
    // deadline (`bounded`), so a client that does not read cannot hold the thread.
    let out = Mutex::new(out);
    // What the watcher may do to this connection, and its place in the registry.
    let control = Arc::new(Control::default());
    let generation = &control.generation;
    let held = &control.held;
    let mut registration = Registration {
        bound: &shared.bound,
        conn,
        control: Arc::clone(&control),
        member: None,
    };
    // The events sent on this connection, whichever subscription sent them.
    let sent = AtomicU64::new(0);
    // The subscription's thread is joined when the connection ends: it reads the office, which
    // must not be closing.
    std::thread::scope(|scope| {
        let _end_held_calls = CancelOnDrop(held);
        let mut reader = org_socket::Ticked::with_poll(
            stream,
            tick,
            || stopping.load(Ordering::SeqCst) || control.revoked.load(Ordering::SeqCst),
            poll,
        );
        let mut binding = Binding::Anonymous;
        let mut subscribed = false;
        // The last subscription's thread: a new one waits for it, so a client that binds and
        // subscribes in a loop never has more than two.
        let mut pusher: Option<std::thread::ScopedJoinHandle<'_, ()>> = None;
        // A line the framing refuses (too long, not JSON) ends the connection: the stream is out
        // of step and nothing in it can be trusted.
        while let Ok(Some(request)) = frame::read_message::<Request>(&mut reader) {
            // The member this connection acts as is checked on every call, not only at bind, and
            // the watcher may have ended the connection's held calls already.
            let member_lookup = if control.revoked.load(Ordering::SeqCst) {
                Some(Lookup::Not)
            } else {
                binding
                    .member()
                    .map(|member| lookup_binding(shared, member))
            };
            if matches!(member_lookup, Some(Lookup::Not)) || control.revoked.load(Ordering::SeqCst)
            {
                refuse_revoked(&out, request.id, &control, shared.timing);
                break;
            }
            if !request.is::<MsgBind>() && matches!(member_lookup, Some(Lookup::Failed)) {
                binding = Binding::Anonymous;
                registration.sync(None);
                generation.fetch_add(1, Ordering::SeqCst);
                subscribed = false;
                held.cancel_all();
            }
            if request.is::<Hello>() {
                let response = hello_response(&request, shared);
                let unsupported = matches!(
                    &response.outcome,
                    org_proto::Outcome::Error(error) if error.code == ErrorCode::UnsupportedVersion
                );
                if !send(&out, request.id, &response, shared.timing) || unsupported {
                    break;
                }
                continue;
            }
            if request.is::<MsgBind>() {
                generation.fetch_add(1, Ordering::SeqCst);
                subscribed = false;
                // A bind, failed or not, ends the held calls of the binding before it.
                held.cancel_all();
            }
            if request.is::<MsgWait>() {
                match msg_calls::begin_wait(&request, binding.member()) {
                    Ok(wait) => {
                        let (cancel, cancellation) = cancellation_pair();
                        let Some(number) = held.start(cancel) else {
                            let busy = Response::error(
                                request.id,
                                ApiError::new(
                                    ErrorCode::Unavailable,
                                    "too many calls are held on this connection",
                                ),
                            );
                            if !send(&out, request.id, &busy, shared.timing) {
                                break;
                            }
                            continue;
                        };
                        // The watcher sets `revoked` before it cancels, so a wait that began
                        // after its cancel still finds the flag and ends at once.
                        if control.revoked.load(Ordering::SeqCst) {
                            held.cancel_all();
                        }
                        let (out, held, mailbox, timing) =
                            (&out, &held, &shared.ports.mailbox, shared.timing);
                        let id = request.id;
                        let started = std::thread::Builder::new()
                            .name("org-office-wait".into())
                            .spawn_scoped(scope, move || {
                                let _place = Place(held, number);
                                // A panic in the mailbox answers this call alone.
                                let answer =
                                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                        msg_calls::finish_wait(id, mailbox, wait, &cancellation)
                                    }))
                                    .unwrap_or_else(|_| internal(id));
                                send(out, id, &answer, timing);
                            });
                        if started.is_err() {
                            held.finish(number);
                            let busy = Response::error(
                                request.id,
                                ApiError::new(ErrorCode::Unavailable, "the office is busy"),
                            );
                            if !send(out, request.id, &busy, timing) {
                                break;
                            }
                        }
                        continue;
                    }
                    Err(refusal) => {
                        if !send(&out, request.id, &refusal, shared.timing) {
                            break;
                        }
                        continue;
                    }
                }
            }
            let response = if request.is::<org_proto::Subscribe>() {
                match follow::begin(
                    &request,
                    &*shared.ports.feed,
                    binding.is_app_bound(),
                    subscribed,
                ) {
                    Ok(position) => {
                        // Live events start from here: what is published from now on.
                        let start = follow::Start {
                            position,
                            live: shared.ports.live.head(),
                        };
                        // The generation this subscription belongs to, taken before the answer
                        // goes out: a watcher's bump after this ends it.
                        let mine = generation.load(Ordering::SeqCst);
                        // The answer first, then the events.
                        let answer =
                            Response::ok::<org_proto::Subscribe>(request.id, &org_proto::Empty {})
                                .unwrap_or_else(|_| internal(request.id));
                        if !send(&out, request.id, &answer, shared.timing) {
                            break;
                        }
                        subscribed = true;
                        if let Some(old) = pusher.take() {
                            let _ = old.join();
                        }
                        let (feed, live) = (&*shared.ports.feed, &*shared.ports.live);
                        let wire = follow::Wire {
                            out: &out,
                            generation,
                            mine,
                            seq: &sent,
                            timing: shared.timing,
                        };
                        pusher = Some(scope.spawn(move || follow::push(feed, live, &wire, start)));
                        continue;
                    }
                    Err(refusal) => Response::error(request.id, refusal),
                }
            } else {
                answer(&request, shared, &mut binding)
            };
            // The watcher finds this connection by the member it now acts as. Registered before
            // the answer goes out, so a client that has its bind's answer is already watched.
            if registration.sync(binding.member()) {
                // Registered after the member may already have gone: the watcher cannot have
                // seen this entry, so look once.
                if let Some(member) = binding.member() {
                    if is_gone(shared, member) {
                        control.held.cancel_all();
                        control.generation.fetch_add(1, Ordering::SeqCst);
                        control.revoked.store(true, Ordering::SeqCst);
                    }
                }
            }
            // A watcher can revoke the old binding while msg.bind resolves the new one.
            // Registration keeps that revocation sticky; never acknowledge a fresh bind.
            if request.is::<MsgBind>() && control.revoked.load(Ordering::SeqCst) {
                refuse_revoked(&out, request.id, &control, shared.timing);
                break;
            }
            if !send(&out, request.id, &response, shared.timing) {
                break;
            }
        }
        generation.fetch_add(1, Ordering::SeqCst);
    });
}

fn refuse_revoked(out: &Mutex<UnixStream>, id: u64, control: &Control, timing: Timing) {
    control.generation.fetch_add(1, Ordering::SeqCst);
    control.held.cancel_all();
    let refusal = bind_error(
        id,
        ErrorCode::NotBound,
        "this connection's member binding was revoked",
    );
    let _ = send(out, id, &refusal, timing);
}

/// Writes `response` to the client; `false` when the connection is gone.
fn send(out: &Mutex<UnixStream>, id: u64, response: &Response, timing: Timing) -> bool {
    let stream = out.lock().unwrap_or_else(|p| p.into_inner());
    match bounded::write_message(&stream, response, timing.write) {
        Ok(()) => true,
        // The answer does not fit in a frame (nothing was written): say so, and carry on.
        Err(frame::FrameError::TooLargeToSend) => {
            let error = Response::error(
                id,
                ApiError::new(ErrorCode::StorageError, "the answer is too large to send"),
            );
            bounded::write_message(&stream, &error, timing.write).is_ok()
        }
        // A frame may be half written: nothing more goes out on this connection, not even an
        // event from the follower once it gets the lock.
        Err(_) => {
            let _ = stream.shutdown(std::net::Shutdown::Both);
            false
        }
    }
}

/// X-02 §3, B1-07.1: this office socket supports only v1 until instance-aware
/// launch is available. Every call therefore stays at v1, including legacy-first
/// calls and repeated hellos; the wider SUPPORTED_API is not active here yet.
fn hello_response(request: &Request, shared: &Shared) -> Response {
    match request.params::<Hello>() {
        Ok(params) if params.api_range().contains(org_proto::ApiRange::V1.min) => {
            Response::ok::<Hello>(request.id, &shared.hello)
                .unwrap_or_else(|_| internal(request.id))
        }
        Ok(_) => Response::error(
            request.id,
            ApiError::new(
                ErrorCode::UnsupportedVersion,
                "the office supports only API version 1",
            ),
        ),
        Err(_) => Response::error(
            request.id,
            ApiError::new(ErrorCode::InvalidRequest, "invalid hello parameters"),
        ),
    }
}

fn answer(request: &Request, shared: &Shared, binding: &mut Binding) -> Response {
    if request.is::<org_proto::OperationGet>() {
        let params = match request.params::<org_proto::OperationGet>() {
            Ok(params) if org_proto::ulid_time_ms(&params.id).is_some() => params,
            _ => {
                return Response::error(
                    request.id,
                    ApiError::new(
                        ErrorCode::InvalidRequest,
                        "invalid operation lookup parameters",
                    ),
                )
            }
        };
        if params.kind != org_proto::OperationKind::Org {
            return Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::Unavailable,
                    "this operation kind is not available yet",
                ),
            );
        }
        // Before 2a.5 every supported connection acts as the owner (X-02 §4.2).
        // Unverified request.principal must never become a stored-principal key.
        return match (shared.ports.operation_get)(&params.id) {
            Ok(Ok(outcome)) => Response::ok::<org_proto::OperationGet>(request.id, &outcome)
                .unwrap_or_else(|_| internal(request.id)),
            Ok(Err(refusal)) => Response::error(request.id, refusal),
            Err(_) => internal(request.id),
        };
    }
    if request.is::<OrgSnapshotCall>() {
        // The whole organization: for a bound connection (the app; agents bind with messaging),
        // like `who` and `subscribe`.
        if !binding.is_bound() {
            return Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::NotAuthorized,
                    "bind before reading the organization",
                ),
            );
        }
        return match (shared.ports.snapshot)() {
            Ok(snapshot) => Response::ok::<OrgSnapshotCall>(request.id, &snapshot)
                .unwrap_or_else(|_| internal(request.id)),
            Err(SnapshotFailure::Unavailable) => Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::Unavailable,
                    "organization snapshot is not available yet",
                ),
            ),
            Err(SnapshotFailure::Storage) => internal(request.id),
        };
    }
    if request.is::<Who>() {
        return who(request, shared, binding);
    }
    if request.is::<StatusSet>() {
        return status_set(request, shared, binding);
    }
    if request.is::<MsgBind>() {
        return bind_connection(request, shared, binding);
    }
    if let Some(response) = msg_calls::answer(request, &shared.ports.mailbox, binding.member()) {
        return response;
    }
    match WorkCall::parse(request) {
        Ok(None) => {}
        parsed => {
            let Binding::Member { id, with_app_token } = binding else {
                return bind_error(
                    request.id,
                    ErrorCode::NotBound,
                    "bind as a member to change work",
                );
            };
            if WorkCall::is_app_only(request) && !*with_app_token {
                return Response::error(
                    request.id,
                    ApiError::new(
                        ErrorCode::NotAuthorized,
                        format!("`{}` is for the app", request.method),
                    ),
                );
            }
            let call = match parsed {
                Err(refusal) => return Response::error(request.id, refusal),
                Ok(Some(call)) => call,
                Ok(None) => unreachable!("unknown work method fell through"),
            };
            let caller = if *with_app_token {
                Caller::app(id.as_str())
            } else {
                Caller::agent(id.as_str())
            };
            return match (shared.ports.work)(caller, call) {
                Ok(Ok(accepted)) => Response::ok::<TaskCreate>(request.id, &accepted)
                    .unwrap_or_else(|_| internal(request.id)),
                Ok(Err(refusal)) => Response::error(request.id, refusal),
                Err(_) => internal(request.id),
            };
        }
    }
    // An organization command: read to tell it from an unknown method, but the caller is checked
    // BEFORE anything else is said about it (a bad param is not told to an anonymous client).
    match OrgCommand::parse(request) {
        Ok(None) => {}
        Ok(Some(_)) | Err(_) if !binding.is_app_bound() => {
            return Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::NotAuthorized,
                    "organization commands are for the app",
                ),
            );
        }
        Err(refusal) => return Response::error(request.id, refusal),
        Ok(Some(command)) => {
            return match (shared.ports.submit)(command) {
                // Every organization command answers with `CommandAccepted`; any command's call
                // type serializes the same way.
                Ok(Ok(accepted)) => Response::ok::<TeamCreate>(request.id, &accepted)
                    .unwrap_or_else(|_| internal(request.id)),
                Ok(Err(refusal)) => Response::error(request.id, refusal),
                Err(_) => internal(request.id),
            };
        }
    }
    Response::error(
        request.id,
        ApiError::new(
            ErrorCode::UnknownMethod,
            format!("this office does not know `{}`", request.method),
        ),
    )
}

/// `who`: the directory, for a bound connection (`not_bound` otherwise, as that call's contract
/// says). The binding is checked before the params are read.
fn who(request: &Request, shared: &Shared, binding: &Binding) -> Response {
    if !binding.is_bound() {
        return Response::error(
            request.id,
            ApiError::new(ErrorCode::NotBound, "bind before asking who"),
        );
    }
    let Ok(params) = request.params::<Who>() else {
        return bind_error(
            request.id,
            ErrorCode::InvalidRequest,
            "the params of `who` are not valid",
        );
    };
    match (shared.ports.who)(&params) {
        Ok(Ok(result)) => {
            Response::ok::<Who>(request.id, &result).unwrap_or_else(|_| internal(request.id))
        }
        Ok(Err(refusal)) => Response::error(request.id, refusal),
        Err(_) => internal(request.id),
    }
}

/// `status.set`: an agent-bound connection's own status line. The member is the binding's, never
/// the call's. Unbound or app-wide is `not_bound`; a person or the owner (bound with the app
/// token) is `not_authorized`. The binding is checked before the params are read.
fn status_set(request: &Request, shared: &Shared, binding: &Binding) -> Response {
    let member = match binding {
        Binding::Member {
            id,
            with_app_token: false,
        } => id,
        Binding::Member { .. } => {
            return bind_error(
                request.id,
                ErrorCode::NotAuthorized,
                "only an agent sets its status",
            )
        }
        Binding::Anonymous | Binding::App => {
            return bind_error(
                request.id,
                ErrorCode::NotBound,
                "bind as an agent to set a status",
            )
        }
    };
    let Ok(params) = request.params::<StatusSet>() else {
        return bind_error(
            request.id,
            ErrorCode::InvalidRequest,
            "the params of `status.set` are not valid",
        );
    };
    match (shared.ports.status)(member, &params) {
        Ok(Ok(result)) => {
            Response::ok::<StatusSet>(request.id, &result).unwrap_or_else(|_| internal(request.id))
        }
        Ok(Err(refusal)) => Response::error(request.id, refusal),
        Err(_) => internal(request.id),
    }
}

/// `msg.bind`, in the contract's order (`org_proto::msg::MsgBind`): the input alone, then the
/// member, then the credentials; it fails closed. Each call replaces the previous binding, and a
/// failed one leaves the connection unbound.
fn bind_connection(request: &Request, shared: &Shared, binding: &mut Binding) -> Response {
    *binding = Binding::Anonymous;
    if matches!(shared.auth, LaunchAuth::Grants { .. }) {
        return bind_error(
            request.id,
            ErrorCode::NotAuthorized,
            "grant authorization is not active",
        );
    }
    let params = match request.params::<MsgBind>() {
        Ok(params) => params,
        Err(_) => return bind_error(request.id, ErrorCode::InvalidRequest, "bad msg.bind params"),
    };
    let MsgBindParams { member, app_token } = params;
    let bound = match member {
        Some(member) => bind_member(request.id, shared, &member, app_token),
        None => bind_app(request.id, shared, app_token),
    };
    match bound {
        Ok((now, member_id)) => {
            *binding = now;
            let result = MsgBindResult {
                member_id,
                office_id: shared.office_id.clone(),
            };
            Response::ok::<MsgBind>(request.id, &result).unwrap_or_else(|_| internal(request.id))
        }
        Err(refusal) => refusal,
    }
}

/// App-wide: the `app_token` and no member.
fn bind_app(
    id: u64,
    shared: &Shared,
    app_token: Option<AppToken>,
) -> Result<(Binding, Option<String>), Response> {
    let Some(token) = app_token else {
        return Err(bind_error(
            id,
            ErrorCode::InvalidRequest,
            "bind with an app_token",
        ));
    };
    if !shared.auth.accepts(&token) {
        return Err(bind_error(id, ErrorCode::NotAuthorized, "wrong app token"));
    }
    Ok((Binding::App, None))
}

/// As a member: an agent of this office that may message (no token), or a person or the owner
/// (with the `app_token`). An office-run agent needs a session token, which this office does not
/// issue yet: it cannot be bound.
fn bind_member(
    id: u64,
    shared: &Shared,
    member: &str,
    app_token: Option<AppToken>,
) -> Result<(Binding, Option<String>), Response> {
    // Step 1, which needs no lookup.
    if json_string_len(member) > MAX_HANDLE_BYTES {
        return Err(bind_error(
            id,
            ErrorCode::TooLarge,
            "the member is too long",
        ));
    }
    let handle = org_office::directory::trim_address(member);
    if handle.eq_ignore_ascii_case(OFFICE_SENDER) {
        return Err(bind_error(
            id,
            ErrorCode::InvalidRequest,
            "a connection cannot bind as the office",
        ));
    }
    // Step 2: the member. A lookup that fails, an unknown member and one that cannot message are
    // all `not_authorized`, and say no more.
    let refused = || {
        bind_error(
            id,
            ErrorCode::NotAuthorized,
            "this office cannot bind this member",
        )
    };
    let Some(found) = bindable(shared, handle) else {
        return Err(refused());
    };
    let is_person = found.kind == MemberKind::Person;
    // Step 3: the credentials.
    if is_person {
        match app_token {
            Some(token) if shared.auth.accepts(&token) => {}
            _ => return Err(refused()),
        }
    } else if app_token.is_some() {
        return Err(bind_error(
            id,
            ErrorCode::InvalidRequest,
            "an agent binds without an app_token",
        ));
    }
    let member_id = found.id;
    Ok((
        Binding::Member {
            id: member_id.clone(),
            with_app_token: is_person,
        },
        Some(member_id),
    ))
}

/// The member `handle` names if a connection may act as it: a person or the owner, or an agent
/// of this office that is active or paused and not office-run (an office-run agent needs a session
/// token, which this office does not issue yet). `None` for anything else and for a lookup that
/// fails: it fails closed. Used at bind and again on every later call of the connection.
fn bindable(shared: &Shared, handle: &str) -> Option<org_proto::Member> {
    match lookup(shared, handle) {
        Lookup::Bindable(member) => Some(*member),
        Lookup::Not | Lookup::Failed => None,
    }
}

/// What a look at a member found.
enum Lookup {
    Bindable(Box<org_proto::Member>),
    /// The member is there to see and may not be bound (or is not there).
    Not,
    /// The directory could not say. A bind or a call fails closed on it; the watcher does not
    /// end a connection on it (the next look, or the connection's next call, decides).
    Failed,
}

fn lookup(shared: &Shared, handle: &str) -> Lookup {
    let found = match shared.ports.directory.resolve_member(handle) {
        Ok(Some((found, _))) => found,
        Ok(None) => return Lookup::Not,
        Err(_) => return Lookup::Failed,
    };
    let may = match found.kind {
        MemberKind::Person => true,
        MemberKind::Agent => {
            found.office_id.as_deref() == Some(shared.office_id.as_str())
                && found.agent.as_ref().is_some_and(|profile| {
                    matches!(profile.status, AgentStatus::Active | AgentStatus::Paused)
                        && profile.run_mode.is_none()
                })
        }
        _ => false,
    };
    if may {
        Lookup::Bindable(Box::new(found))
    } else {
        Lookup::Not
    }
}

/// Resolves an existing binding by identity: another member's matching handle cannot keep it.
fn lookup_binding(shared: &Shared, id: &str) -> Lookup {
    match lookup(shared, id) {
        Lookup::Bindable(member) if member.id != id => Lookup::Not,
        found => found,
    }
}

/// Whether the member `id` is certainly no longer bindable. A directory that could not answer is
/// not certain.
fn is_gone(shared: &Shared, id: &str) -> bool {
    matches!(lookup_binding(shared, id), Lookup::Not)
}

fn bind_error(id: u64, code: ErrorCode, message: &str) -> Response {
    Response::error(id, ApiError::new(code, message))
}

fn internal(id: u64) -> Response {
    Response::error(
        id,
        ApiError::new(ErrorCode::StorageError, "the office could not answer"),
    )
}

#[cfg(test)]
mod tests {
    mod work;
    use std::io::BufReader;
    use std::sync::atomic::AtomicBool;

    use super::*;

    #[test]
    fn grants_bind_refuses_before_params_or_directory_and_clears_each_prior_binding() {
        struct ForbiddenDirectory;
        impl MemberDirectory for ForbiddenDirectory {
            fn resolve_member(
                &self,
                _: &str,
            ) -> Result<
                Option<(org_proto::Member, org_proto::org::Position)>,
                org_mailbox::StoreError,
            > {
                panic!("grant bind must never query the legacy directory");
            }
        }
        let mut ports = ports(Box::new(|| Err(SnapshotFailure::Storage)));
        ports.directory = Arc::new(ForbiddenDirectory);
        let shared = Shared::new(
            &test_info(),
            Path::new("/unused"),
            ports,
            LaunchAuth::Grants {
                _key: org_proto::GrantKey::from_bytes([7; 32]),
            },
        );
        assert!(
            matches!(&shared.auth, LaunchAuth::Grants { _key } if _key == &org_proto::GrantKey::from_bytes([7;32]))
        );
        for params in [
            serde_json::json!({"app_token":"a7".repeat(32)}),
            serde_json::json!({"app_token":"11".repeat(32)}),
            serde_json::json!({}),
            serde_json::json!({"app_token":"not hex"}),
            serde_json::json!({"member":"ada"}),
            serde_json::json!({"member":"pat","app_token":"a7".repeat(32)}),
            serde_json::json!({"member":7}),
            serde_json::json!({"member":"office"}),
        ] {
            for mut binding in [
                Binding::Anonymous,
                Binding::App,
                Binding::Member {
                    id: "ada".into(),
                    with_app_token: false,
                },
            ] {
                let request = Request {
                    id: 1,
                    method: "msg.bind".into(),
                    params: params.clone(),
                    principal: None,
                };
                assert_eq!(
                    code_of(answer(&request, &shared, &mut binding).outcome),
                    ErrorCode::NotAuthorized
                );
                assert_eq!(binding, Binding::Anonymous);
            }
        }
    }

    #[test]
    fn grants_mode_keeps_v1_hello_and_closes_the_tokenless_legacy_agent_path() {
        let legacy = shared(Path::new("/unused"));
        let request = Request::new::<MsgBind>(
            1,
            &MsgBindParams {
                member: Some("ada".into()),
                app_token: None,
            },
        )
        .unwrap();
        assert!(matches!(
            answer(&request, &legacy, &mut Binding::Anonymous).outcome,
            org_proto::Outcome::Result(_)
        ));
        let grants = Shared::new(
            &test_info(),
            Path::new("/unused"),
            ports(Box::new(|| Err(SnapshotFailure::Storage))),
            LaunchAuth::Grants {
                _key: org_proto::GrantKey::from_bytes([7; 32]),
            },
        );
        assert_eq!(
            code_of(answer(&request, &grants, &mut Binding::Anonymous).outcome),
            ErrorCode::NotAuthorized
        );
        assert_eq!(
            serde_json::to_value(&grants.hello).unwrap(),
            serde_json::to_value(&legacy.hello).unwrap()
        );
    }

    #[test]
    fn grants_authority_refuses_every_bind_on_the_live_socket_and_keeps_api_one() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let shared = Arc::new(Shared::new(
            &test_info(),
            &path,
            ports(Box::new(|| Err(SnapshotFailure::Storage))),
            LaunchAuth::Grants {
                _key: org_proto::GrantKey::from_bytes([7; 32]),
            },
        ));
        let (stop, server) = start(&path, &shared);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut client = Client::connect(&path);
            assert!(
                matches!(client.call::<Hello>(&Default::default()), org_proto::Outcome::Result(value)
                if value == serde_json::to_value(&shared.hello).unwrap() && value["api_version"] == 1)
            );
            for params in [
                serde_json::json!({"app_token":"a7".repeat(32)}),
                serde_json::json!({"app_token":"11".repeat(32)}),
                serde_json::json!({}),
                serde_json::json!({"app_token":"not hex"}),
                serde_json::json!({"member":"ada"}),
                serde_json::json!({"member":"pat","app_token":"a7".repeat(32)}),
                serde_json::json!({"member":7}),
                serde_json::json!({"member":"office"}),
            ] {
                assert_eq!(
                    code_of(client.ask(|id| Request {
                        id,
                        method: "msg.bind".into(),
                        params,
                        principal: None,
                    })),
                    ErrorCode::NotAuthorized
                );
                assert_eq!(
                    code_of(client.call::<MsgInbox>(&inbox_params())),
                    ErrorCode::NotBound
                );
                assert_eq!(code_of(client.team_create()), ErrorCode::NotAuthorized);
            }
        }));
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
        result.unwrap();
    }

    #[test]
    fn an_unbind_preserves_a_racing_watcher_revocation() {
        let control = Arc::new(Control::default());
        let bound = Mutex::new(HashMap::from([(
            1,
            (
                org_mailbox::testkit::fixture::ADA_ID.to_owned(),
                Arc::clone(&control),
            ),
        )]));
        let mut registration = Registration {
            bound: &bound,
            conn: 1,
            control: Arc::clone(&control),
            member: Some(org_mailbox::testkit::fixture::ADA_ID.to_owned()),
        };
        // The watcher wins after a call's liveness check but before a lookup-failure unbind.
        control.revoked.store(true, Ordering::SeqCst);
        registration.sync(None);
        assert!(bound.lock().unwrap().is_empty());
        assert!(control.revoked.load(Ordering::SeqCst));
    }

    #[test]
    fn ticked_reader_repolls_a_missed_eof_at_the_server_call_site() {
        let (stream, peer) = UnixStream::pair().unwrap();
        drop(peer);
        let shared = shared(Path::new("/unused-test-socket"));
        let polls = std::cell::Cell::new(0);
        serve_connection_with_poll(
            stream,
            &shared,
            1,
            &AtomicBool::new(false),
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
        assert!(shared.bound.lock().unwrap().is_empty());
    }

    #[test]
    fn ticked_reader_observes_server_stop_without_a_shutdown_wakeup() {
        let (stream, _peer) = UnixStream::pair().unwrap();
        let shared = shared(Path::new("/unused-test-socket"));
        let stopping = AtomicBool::new(false);
        let polls = std::cell::Cell::new(0);
        serve_connection_with_poll(
            stream,
            &shared,
            1,
            &stopping,
            org_socket::READ_TICK,
            |_, _| {
                polls.set(polls.get() + 1);
                stopping.store(true, Ordering::SeqCst);
                Ok(false)
            },
        );
        assert_eq!(polls.get(), 1);
    }

    #[test]
    fn ticked_reader_clears_a_revoked_idle_member_registration() {
        let (stream, mut peer) = UnixStream::pair().unwrap();
        let shared = shared(Path::new("/unused-test-socket"));
        let request = Request::new::<MsgBind>(
            1,
            &MsgBindParams {
                member: Some(org_mailbox::testkit::fixture::ADA_HANDLE.to_owned()),
                app_token: None,
            },
        )
        .unwrap();
        frame::write_message(&mut peer, &request).unwrap();
        let polls = std::cell::Cell::new(0);
        serve_connection_with_poll(
            stream,
            &shared,
            1,
            &AtomicBool::new(false),
            org_socket::READ_TICK,
            |fd, _| {
                polls.set(polls.get() + 1);
                if polls.get() == 1 {
                    org_socket::poll_readable(fd, Duration::from_millis(1))
                } else {
                    let bound = shared.bound.lock().unwrap();
                    let (member, control) = bound.get(&1).expect("bind must register the member");
                    assert_eq!(member, org_mailbox::testkit::fixture::ADA_ID);
                    control.revoked.store(true, Ordering::SeqCst);
                    Ok(false)
                }
            },
        );
        assert_eq!(polls.get(), 2);
        assert!(shared.bound.lock().unwrap().is_empty());
    }

    #[test]
    fn a_revoked_connection_answers_a_call_that_arrives_before_the_tick_then_closes() {
        let (stream, mut peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let shared = shared(Path::new("/unused-test-socket"));
        let bind = Request::new::<MsgBind>(
            1,
            &MsgBindParams {
                member: Some(org_mailbox::testkit::fixture::ADA_HANDLE.to_owned()),
                app_token: None,
            },
        )
        .unwrap();
        frame::write_message(&mut peer, &bind).unwrap();
        let mut polls = 0;
        serve_connection_with_poll(
            stream,
            &shared,
            1,
            &AtomicBool::new(false),
            org_socket::READ_TICK,
            |fd, _| {
                polls += 1;
                assert!(
                    polls <= 2,
                    "revoked connection stayed open after its refusal"
                );
                if polls == 2 {
                    let bound = shared.bound.lock().unwrap();
                    let (member, control) = bound.get(&1).expect("member must be registered");
                    assert_eq!(member, org_mailbox::testkit::fixture::ADA_ID);
                    control.revoked.store(true, Ordering::SeqCst);
                    let call = Request::new::<MsgInbox>(2, &inbox_params()).unwrap();
                    frame::write_message(&mut peer, &call).unwrap();
                }
                org_socket::poll_readable(fd, Duration::from_millis(1))
            },
        );
        assert_eq!(polls, 2);
        assert!(shared.bound.lock().unwrap().is_empty());
        let mut responses = BufReader::new(peer);
        let bound = frame::read_message::<Response>(&mut responses)
            .unwrap()
            .unwrap();
        assert_eq!(bound.id, 1);
        assert!(matches!(bound.outcome, org_proto::Outcome::Result(_)));
        let refused = frame::read_message::<Response>(&mut responses)
            .unwrap()
            .unwrap();
        assert_eq!(refused.id, 2);
        assert_eq!(code_of(refused.outcome), ErrorCode::NotBound);
        assert!(frame::read_message::<Response>(&mut responses)
            .unwrap()
            .is_none());
    }

    #[test]
    fn a_rebind_cannot_revive_a_connection_revoked_during_its_directory_lookup() {
        struct RevokingDirectory {
            inner: Arc<org_mailbox::testkit::InMemoryDirectory>,
            revoke: Mutex<Option<Arc<Control>>>,
        }
        impl MemberDirectory for RevokingDirectory {
            fn resolve_member(
                &self,
                name: &str,
            ) -> Result<Option<(org_proto::Member, org_proto::Position)>, org_mailbox::StoreError>
            {
                if name == org_mailbox::testkit::fixture::REMY_HANDLE {
                    if let Some(control) = self.revoke.lock().unwrap().take() {
                        control.revoked.store(true, Ordering::SeqCst);
                    }
                }
                self.inner.resolve_member(name)
            }
        }
        let (mailbox, inner) = messaging();
        let directory = Arc::new(RevokingDirectory {
            inner,
            revoke: Mutex::new(None),
        });
        let mut ports = ports(Box::new(|| Err(SnapshotFailure::Storage)));
        ports.mailbox = mailbox;
        ports.directory = directory.clone();
        let shared = Shared::new(
            &test_info(),
            Path::new("/unused-test-socket"),
            ports,
            token(),
        );
        let (stream, mut peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let bind = |id, member: &str| {
            Request::new::<MsgBind>(
                id,
                &MsgBindParams {
                    member: Some(member.to_owned()),
                    app_token: None,
                },
            )
            .unwrap()
        };
        frame::write_message(
            &mut peer,
            &bind(1, org_mailbox::testkit::fixture::ADA_HANDLE),
        )
        .unwrap();
        let mut polls = 0;
        serve_connection_with_poll(
            stream,
            &shared,
            1,
            &AtomicBool::new(false),
            org_socket::READ_TICK,
            |fd, _| {
                polls += 1;
                assert!(polls <= 2, "the rebind erased a watcher revocation");
                if polls == 2 {
                    let control = Arc::clone(&shared.bound.lock().unwrap().get(&1).unwrap().1);
                    *directory.revoke.lock().unwrap() = Some(control);
                    frame::write_message(
                        &mut peer,
                        &bind(2, org_mailbox::testkit::fixture::REMY_HANDLE),
                    )
                    .unwrap();
                }
                org_socket::poll_readable(fd, Duration::from_millis(1))
            },
        );
        assert_eq!(polls, 2);
        assert!(shared.bound.lock().unwrap().is_empty());
        let mut responses = BufReader::new(peer);
        let first = frame::read_message::<Response>(&mut responses)
            .unwrap()
            .unwrap();
        assert_eq!(first.id, 1);
        assert!(matches!(first.outcome, org_proto::Outcome::Result(_)));
        let refused = frame::read_message::<Response>(&mut responses)
            .unwrap()
            .unwrap();
        assert_eq!(refused.id, 2);
        assert_eq!(code_of(refused.outcome), ErrorCode::NotBound);
        assert!(frame::read_message::<Response>(&mut responses)
            .unwrap()
            .is_none());
    }

    #[test]
    fn hello_cannot_bypass_a_removed_members_per_call_revocation() {
        use org_mailbox::testkit::fixture::POSITION;
        // X-02 §3 leaves v1 binding revocation intact. No watcher runs here:
        // the hello call itself must discover the missing member and close.
        let (mailbox, directory) = messaging();
        let mut ports = ports(Box::new(|| Err(SnapshotFailure::Storage)));
        ports.mailbox = mailbox;
        ports.directory = directory.clone();
        let shared = Shared::new(
            &test_info(),
            Path::new("/unused-test-socket"),
            ports,
            token(),
        );
        let (stream, mut peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        frame::write_message(
            &mut peer,
            &Request::new::<MsgBind>(
                1,
                &MsgBindParams {
                    member: Some("ada".into()),
                    app_token: None,
                },
            )
            .unwrap(),
        )
        .unwrap();
        let stopping = AtomicBool::new(false);
        let mut polls = 0;
        serve_connection_with_poll(
            stream,
            &shared,
            1,
            &stopping,
            org_socket::READ_TICK,
            |fd, _| {
                polls += 1;
                if polls == 2 {
                    directory.remove(ADA_ID, POSITION);
                    frame::write_message(
                        &mut peer,
                        &Request::new::<Hello>(2, &Default::default()).unwrap(),
                    )
                    .unwrap();
                } else if polls > 2 {
                    // Bound a broken implementation too, then inspect its actual reply.
                    stopping.store(true, Ordering::SeqCst);
                    return Ok(false);
                }
                org_socket::poll_readable(fd, Duration::from_millis(1))
            },
        );
        let mut responses = BufReader::new(peer);
        let bound = frame::read_message::<Response>(&mut responses)
            .unwrap()
            .unwrap();
        assert_eq!(bound.id, 1);
        assert!(matches!(bound.outcome, org_proto::Outcome::Result(_)));
        let hello = frame::read_message::<Response>(&mut responses)
            .unwrap()
            .unwrap();
        assert_eq!(hello.id, 2);
        assert_eq!(code_of(hello.outcome), ErrorCode::NotBound);
        assert!(frame::read_message::<Response>(&mut responses)
            .unwrap()
            .is_none());
        assert_eq!(polls, 2);
        assert!(shared.bound.lock().unwrap().is_empty());
    }

    #[test]
    fn hello_cannot_bypass_a_failed_lookup_reset_and_cancellation() {
        // X-02 §3 preserves legacy binding failure handling before hello.
        // Drive the real connection directly, without the background watcher.
        let (mailbox, memory) = messaging();
        let directory = Arc::new(FlakyDirectory {
            inner: memory,
            failing: AtomicBool::new(false),
        });
        let feed = Arc::new(FakeFeed::default());
        let mut ports = ports(Box::new(|| Err(SnapshotFailure::Storage)));
        ports.mailbox = mailbox.clone();
        ports.directory = directory.clone();
        ports.feed = Box::new(feed.clone());
        let shared = Shared::new(
            &test_info(),
            Path::new("/unused-test-socket"),
            ports,
            token(),
        );
        let (stream, mut peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        frame::write_message(
            &mut peer,
            &Request::new::<MsgBind>(
                1,
                &MsgBindParams {
                    member: Some("pat".into()),
                    app_token: Some(token()),
                },
            )
            .unwrap(),
        )
        .unwrap();
        let stopping = AtomicBool::new(false);
        let mut polls = 0;
        let mut control = None;
        let mut before = 0;
        let mut ready = false;
        let mut observed = None;
        serve_connection_with_poll(
            stream,
            &shared,
            1,
            &stopping,
            org_socket::READ_TICK,
            |fd, _| {
                polls += 1;
                match polls {
                    2 => frame::write_message(
                        &mut peer,
                        &Request::new::<org_proto::Subscribe>(2, &Default::default()).unwrap(),
                    )
                    .unwrap(),
                    3 => frame::write_message(
                        &mut peer,
                        &Request::new::<org_proto::msg::MsgWait>(3, &wait_params()).unwrap(),
                    )
                    .unwrap(),
                    4 => {
                        let deadline = std::time::Instant::now() + Duration::from_secs(5);
                        while std::time::Instant::now() < deadline {
                            if mailbox.held_waits() == 1 && feed.waiting.load(Ordering::SeqCst) == 1
                            {
                                ready = true;
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        let Some(current) = shared
                            .bound
                            .lock()
                            .unwrap()
                            .get(&1)
                            .map(|(_, control)| Arc::clone(control))
                        else {
                            stopping.store(true, Ordering::SeqCst);
                            return Ok(false);
                        };
                        before = current.generation.load(Ordering::SeqCst);
                        control = Some(current);
                        directory.failing.store(true, Ordering::SeqCst);
                        frame::write_message(
                            &mut peer,
                            &Request::new::<Hello>(4, &Default::default()).unwrap(),
                        )
                        .unwrap();
                    }
                    5 => {
                        let current = control.as_ref().unwrap();
                        observed = Some((
                            current.generation.load(Ordering::SeqCst),
                            shared.bound.lock().unwrap().get(&1).is_none(),
                            current.held.next.lock().unwrap().1.is_empty(),
                        ));
                        // Restore lookup before the next call: otherwise that call
                        // could mask a hello that failed to clear the old binding.
                        directory.failing.store(false, Ordering::SeqCst);
                        frame::write_message(
                            &mut peer,
                            &Request::new::<Who>(5, &Default::default()).unwrap(),
                        )
                        .unwrap();
                    }
                    6.. => {
                        stopping.store(true, Ordering::SeqCst);
                        return Ok(false);
                    }
                    _ => {}
                }
                org_socket::poll_readable(fd, Duration::from_millis(1))
            },
        );
        let mut reader = BufReader::new(peer);
        let mut responses = Vec::new();
        while let Some(response) = frame::read_message::<Response>(&mut reader).unwrap() {
            responses.push(response);
        }
        responses.sort_by_key(|response| response.id);
        assert_eq!(responses.len(), 5);
        for id in [1, 2, 4] {
            assert_eq!(responses[id - 1].id, id as u64);
            assert!(matches!(
                responses[id - 1].outcome,
                org_proto::Outcome::Result(_)
            ));
        }
        assert!(
            ready,
            "subscription and held wait must be active before hello"
        );
        assert_eq!(
            observed,
            Some((before + 1, true, true)),
            "hello must reset the generation, registration and held calls before the next request"
        );
        assert_eq!(code_of(responses[2].outcome.clone()), ErrorCode::NotBound);
        assert_eq!(responses[4].id, 5);
        assert_eq!(code_of(responses[4].outcome.clone()), ErrorCode::NotBound);
        assert_eq!(mailbox.held_waits(), 0);
    }

    fn shared(path: &Path) -> Arc<Shared> {
        Arc::new(Shared::new(
            &test_info(),
            path,
            ports(Box::new(|| Err(SnapshotFailure::Storage))),
            token(),
        ))
    }

    #[test]
    fn org_operation_get_for_an_absent_row_is_not_found_on_a_legacy_connection() {
        let dir = tempfile::TempDir::new().unwrap();
        let shared = shared(&dir.path().join("office.sock"));
        let request = Request::new::<org_proto::OperationGet>(
            1,
            &org_proto::OperationGetParams {
                kind: org_proto::OperationKind::Org,
                id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            },
        )
        .unwrap();
        let response = answer(&request, &shared, &mut Binding::Anonymous);
        assert_eq!(code_of(response.outcome), ErrorCode::NotFound);
    }

    #[test]
    fn operation_get_decodes_kind_and_id_before_dispatch_and_sanitizes_storage_errors() {
        use serde_json::json;
        let dir = tempfile::TempDir::new().unwrap();
        let mut ports = ports(Box::new(|| Err(SnapshotFailure::Storage)));
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        ports.operation_get = Box::new(move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            Err("SECRET SQL /private/organization".into())
        });
        let shared = Shared::new(&test_info(), &dir.path().join("s.sock"), ports, token());
        for params in [
            json!({"kind":"unknown","id":"01ARZ3NDEKTSV4RRFFQ69G5FAV"}),
            json!({"kind":"org","id":"bad"}),
            json!({"kind":"work","id":"bad"}),
            json!({"kind":"org"}),
            json!({"kind":"org","id":7}),
        ] {
            let mut request = Request::new::<org_proto::OperationGet>(
                1,
                &org_proto::OperationGetParams {
                    kind: org_proto::OperationKind::Org,
                    id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
                },
            )
            .unwrap();
            request.params = params;
            assert_eq!(
                code_of(answer(&request, &shared, &mut Binding::Anonymous).outcome),
                ErrorCode::InvalidRequest
            );
        }
        for kind in [
            org_proto::OperationKind::Work,
            org_proto::OperationKind::Msg,
        ] {
            let request = Request::new::<org_proto::OperationGet>(
                1,
                &org_proto::OperationGetParams {
                    kind,
                    id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
                },
            )
            .unwrap();
            assert_eq!(
                code_of(answer(&request, &shared, &mut Binding::Anonymous).outcome),
                ErrorCode::Unavailable
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let request = Request::new::<org_proto::OperationGet>(
            1,
            &org_proto::OperationGetParams {
                kind: org_proto::OperationKind::Org,
                id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            },
        )
        .unwrap();
        let response = answer(&request, &shared, &mut Binding::Anonymous);
        assert_eq!(code_of(response.outcome.clone()), ErrorCode::StorageError);
        assert!(!serde_json::to_string(&response).unwrap().contains("SECRET"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    fn token() -> AppToken {
        AppToken::from_bytes([7; org_proto::APP_TOKEN_BYTES])
    }

    fn test_info() -> org_office::OfficeInfo {
        org_office::OfficeInfo {
            office_id: org_mailbox::testkit::fixture::OFFICE_ID.into(),
            office_name: "HQ".into(),
            role: org_office::OfficeRole::Hq,
            org_id: "ORG1".into(),
            org_name: "Acme".into(),
            created_at: "2026-11-08T12:00:00.000Z".into(),
        }
    }

    fn ports(snapshot: SnapshotFn) -> OfficePorts {
        let (mailbox, directory) = messaging();
        OfficePorts {
            work: Box::new(|_, _| Err("none".into())),
            operation_get: Box::new(|_| {
                Ok(Err(ApiError::new(
                    ErrorCode::NotFound,
                    "operation not found",
                )))
            }),
            snapshot,
            submit: Box::new(|_| Err("none".to_owned())),
            who: Box::new(|_| Err("none".to_owned())),
            status: Box::new(|_, _| Err("none".to_owned())),
            feed: Box::new(Arc::new(FakeFeed::default())),
            live: Arc::new(LiveEvents::new(Box::new(|| {}))),
            mailbox,
            directory,
        }
    }

    #[test]
    fn operation_get_returns_stored_outcomes_for_each_legacy_binding() {
        use serde_json::json;
        for outcome in [
            org_proto::OperationOutcome::Committed {
                result: json!({"status":"committed","position":{"epoch":1,"seq":2}}),
            },
            org_proto::OperationOutcome::Rejected {
                refusal: ApiError::new(ErrorCode::InvalidName, "stored \"refusal\""),
            },
            org_proto::OperationOutcome::Unknown,
        ] {
            let dir = tempfile::TempDir::new().unwrap();
            let mut ports = ports(Box::new(|| Err(SnapshotFailure::Storage)));
            let stored = outcome.clone();
            ports.operation_get = Box::new(move |_| Ok(Ok(stored.clone())));
            let shared = Shared::new(&test_info(), &dir.path().join("s.sock"), ports, token());
            let request = Request::new::<org_proto::OperationGet>(
                1,
                &org_proto::OperationGetParams {
                    kind: org_proto::OperationKind::Org,
                    id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
                },
            )
            .unwrap();
            for mut binding in [
                Binding::Anonymous,
                Binding::App,
                Binding::Member {
                    id: "agent".into(),
                    with_app_token: false,
                },
                Binding::Member {
                    id: "owner".into(),
                    with_app_token: true,
                },
            ] {
                let response = answer(&request, &shared, &mut binding);
                if matches!(outcome, org_proto::OperationOutcome::Unknown) {
                    assert_eq!(code_of(response.outcome), ErrorCode::StorageError);
                } else {
                    assert_eq!(
                        response.into_result::<org_proto::OperationGet>().unwrap(),
                        outcome
                    );
                }
            }
        }
    }

    /// A mailbox and its directory over the testkit's in-memory store and the fixture members
    /// (ada and remy are agents, pat a person, plus the owner), at the fixture's time.
    fn messaging() -> (Arc<Mailbox>, Arc<org_mailbox::testkit::InMemoryDirectory>) {
        messaging_with(Arc::new(LiveEvents::new(Box::new(|| {}))))
    }

    /// The same, with the mailbox's events going to `live`.
    fn messaging_with(
        live: Arc<LiveEvents>,
    ) -> (Arc<Mailbox>, Arc<org_mailbox::testkit::InMemoryDirectory>) {
        use org_mailbox::testkit::fixture::{
            members, ManualClock, INCARNATION, NOW_MS, OFFICE_ID, POSITION,
        };
        use org_mailbox::testkit::{InMemoryDirectory, InMemoryStore};
        use org_mailbox::OfficeIdentity;
        let directory = Arc::new(InMemoryDirectory::new(members(), POSITION));
        let mailbox = Mailbox::new(
            Arc::new(InMemoryStore::new()),
            directory.clone(),
            OfficeIdentity {
                office_id: OFFICE_ID.to_owned(),
                incarnation: INCARNATION.to_owned(),
                is_hq: true,
            },
            Arc::new(ManualClock::new(NOW_MS)),
            live,
        );
        (Arc::new(mailbox), directory)
    }

    /// A log the test appends to: `push` is a commit. The genesis is at (1, 1).
    #[derive(Default)]
    struct FakeFeed {
        log: Mutex<Vec<org_proto::org::OrgChangedData>>,
        token: Mutex<u64>,
        woken: std::sync::Condvar,
        /// Makes every read of the log fail.
        broken: std::sync::atomic::AtomicBool,
        /// Followers asleep in `wait` now, and the most there have been at once.
        waiting: AtomicUsize,
        peak_waiting: AtomicUsize,
    }

    impl FakeFeed {
        fn push(&self, seq: u64) {
            self.push_with(seq, format!("cmd-{seq}"));
        }

        /// An event whose frame is about `size` bytes.
        fn push_big(&self, seq: u64, size: usize) {
            self.push_with(seq, "x".repeat(size));
        }

        /// Wakes the followers without a commit, as a live event's publish does.
        fn wake(&self) {
            *self.token.lock().unwrap() += 1;
            self.woken.notify_all();
        }

        fn push_with(&self, seq: u64, command_id: String) {
            self.log
                .lock()
                .unwrap()
                .push(org_proto::org::OrgChangedData {
                    position: org_proto::org::Position { epoch: 1, seq },
                    command_id,
                    changes: Vec::new(),
                    actor: None,
                });
            *self.token.lock().unwrap() += 1;
            self.woken.notify_all();
        }
    }

    impl OrgFeed for Arc<FakeFeed> {
        fn head(&self) -> Result<org_proto::org::Position, String> {
            let seq = self
                .log
                .lock()
                .unwrap()
                .last()
                .map_or(1, |d| d.position.seq);
            Ok(org_proto::org::Position { epoch: 1, seq })
        }
        fn can_resume_from(&self, after: org_proto::org::Position) -> Result<bool, String> {
            let head = self.head()?;
            Ok(after.epoch == 1 && (1..=head.seq).contains(&after.seq))
        }
        fn changes_after(
            &self,
            after: org_proto::org::Position,
            limit: usize,
        ) -> Result<Vec<org_proto::org::OrgChangedData>, String> {
            if self.broken.load(Ordering::SeqCst) {
                return Err("log unreadable".to_owned());
            }
            Ok(self
                .log
                .lock()
                .unwrap()
                .iter()
                .filter(|d| (d.position.epoch, d.position.seq) > (after.epoch, after.seq))
                .take(limit)
                .cloned()
                .collect())
        }
        fn token(&self) -> u64 {
            *self.token.lock().unwrap()
        }
        fn wait(&self, seen: u64, timeout: Duration) -> u64 {
            let now = self.waiting.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak_waiting.fetch_max(now, Ordering::SeqCst);
            let guard = self.token.lock().unwrap();
            let token = *self
                .woken
                .wait_timeout_while(guard, timeout, |t| *t == seen)
                .unwrap()
                .0;
            self.waiting.fetch_sub(1, Ordering::SeqCst);
            token
        }
    }

    /// Whether `stream` is served: it answers hello. The wait is generous because a loaded machine
    /// (a full workspace run) can take seconds to schedule 64 connection threads; a connection
    /// the server refuses is closed at once, so that case does not wait for it.
    fn hello_ok(stream: &mut UnixStream) -> bool {
        let request = Request::new::<Hello>(1, &org_proto::HelloParams::default()).unwrap();
        if frame::write_message(&mut *stream, &request).is_err() {
            return false;
        }
        stream
            .set_read_timeout(Some(Duration::from_secs(60)))
            .unwrap();
        matches!(
            frame::read_message::<Response>(&mut BufReader::new(&*stream)),
            Ok(Some(_))
        )
    }

    #[test]
    fn only_so_many_connections_are_served_and_a_closed_one_gives_its_slot_back() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let listener = bind(&path).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let server = {
            let (shared, stop) = (shared(&path), Arc::clone(&stop));
            std::thread::spawn(move || run(&listener, &shared, &|| stop.load(Ordering::SeqCst)))
        };

        let mut held: Vec<UnixStream> = (0..MAX_CONNECTIONS)
            .map(|_| UnixStream::connect(&path).unwrap())
            .collect();
        // All of them are served (each answers hello), so all slots are taken.
        for stream in &mut held {
            assert!(hello_ok(stream));
        }
        // One more is accepted and closed at once, with no answer.
        let mut extra = UnixStream::connect(&path).unwrap();
        assert!(
            !hello_ok(&mut extra),
            "a connection over the limit was served"
        );
        // A slot comes back when a client goes away.
        drop(held.pop());
        let deadline = std::time::Instant::now() + Duration::from_secs(120);
        let mut again = loop {
            let mut stream = UnixStream::connect(&path).unwrap();
            if hello_ok(&mut stream) {
                break stream;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the slot of a closed connection never came back"
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(hello_ok(&mut again));
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    fn shared_with(path: &Path, snapshot: SnapshotFn) -> Arc<Shared> {
        Arc::new(Shared::new(&test_info(), path, ports(snapshot), token()))
    }

    fn start(path: &Path, shared: &Arc<Shared>) -> (Arc<AtomicBool>, std::thread::JoinHandle<()>) {
        let listener = bind(path).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let server = {
            let (shared, stop) = (Arc::clone(shared), Arc::clone(&stop));
            std::thread::spawn(move || run(&listener, &shared, &|| stop.load(Ordering::SeqCst)))
        };
        (stop, server)
    }

    /// A connection bound as the app (it reads the organization), with nothing left unread.
    fn connect_bound(path: &Path) -> UnixStream {
        let mut stream = UnixStream::connect(path).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let params = org_proto::msg::MsgBindParams {
            member: None,
            app_token: Some(token()),
        };
        let bind = Request::new::<org_proto::msg::MsgBind>(100, &params).unwrap();
        frame::write_message(&mut stream, &bind).unwrap();
        let answer = frame::read_message::<Response>(&mut BufReader::new(&stream))
            .unwrap()
            .unwrap();
        assert!(matches!(answer.outcome, org_proto::Outcome::Result(_)));
        stream
    }

    fn snapshot_request(id: u64) -> Request {
        Request::new::<org_proto::OrgSnapshotCall>(id, &org_proto::Empty {}).unwrap()
    }

    #[test]
    fn run_does_not_return_while_a_connection_is_still_working_and_does_when_it_is_done() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        // A snapshot that blocks until released: a connection thread busy inside the office.
        let (release, hold) = std::sync::mpsc::channel::<()>();
        let (entered, inside) = std::sync::mpsc::channel::<()>();
        let hold = Mutex::new(hold);
        let shared = shared_with(
            &path,
            Box::new(move || {
                let _ = entered.send(());
                let _ = hold.lock().unwrap().recv();
                Err(SnapshotFailure::Storage)
            }),
        );
        let (stop, server) = start(&path, &shared);
        let mut client = connect_bound(&path);
        frame::write_message(&mut client, &snapshot_request(1)).unwrap();
        inside.recv_timeout(Duration::from_secs(5)).unwrap();

        stop.store(true, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            !server.is_finished(),
            "run returned with a connection still inside the office"
        );

        release.send(()).unwrap();
        server.join().unwrap();
        assert_eq!(Arc::strong_count(&shared), 1);
    }

    #[test]
    fn a_connection_thread_that_panics_closes_its_socket_and_the_server_goes_on() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let shared = shared_with(&path, Box::new(|| panic!("a bug in a handler")));
        let (stop, server) = start(&path, &shared);
        let mut client = connect_bound(&path);
        frame::write_message(&mut client, &snapshot_request(1)).unwrap();
        // The thread panics: the client sees the end of the stream, it does not hang.
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = BufReader::new(&client);
        assert!(matches!(
            frame::read_message::<Response>(&mut reader),
            Ok(None) | Err(_)
        ));
        // The panicked connection's slot and handle are gone, and a new one is served.
        let mut next = UnixStream::connect(&path).unwrap();
        assert!(hello_ok(&mut next));
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn an_answer_too_large_for_a_frame_is_an_error_not_a_dropped_connection() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let shared = shared_with(
            &path,
            Box::new(|| {
                let teams = (0..30_000)
                    .map(|n| org_proto::org::Team {
                        id: format!("team-{n}"),
                        name: "x".repeat(60),
                        owner_team_id: None,
                        lead_member_id: None,
                        version: 1,
                    })
                    .collect();
                let mut snapshot: OrgSnapshot = serde_json::from_value(serde_json::json!({
                    "position": {"epoch": 1, "seq": 1},
                    "teams": [], "members": [], "memberships": []
                }))
                .unwrap();
                snapshot.teams = teams;
                Ok(snapshot)
            }),
        );
        let (stop, server) = start(&path, &shared);
        let mut client = connect_bound(&path);
        client
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        frame::write_message(&mut client, &snapshot_request(1)).unwrap();
        let mut reader = BufReader::new(&client);
        let answer = frame::read_message::<Response>(&mut reader)
            .unwrap()
            .unwrap();
        assert!(
            matches!(answer.outcome, org_proto::Outcome::Error(_)),
            "{answer:?}"
        );
        // And the connection still works.
        let hello = Request::new::<Hello>(2, &org_proto::HelloParams::default()).unwrap();
        frame::write_message(&mut &client, &hello).unwrap();
        let next = frame::read_message::<Response>(&mut reader)
            .unwrap()
            .unwrap();
        assert!(matches!(next.outcome, org_proto::Outcome::Result(_)));
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn a_live_socket_is_not_replaced_and_a_stale_one_is() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let live = bind(&path).unwrap();
        assert!(bind(&path).is_err(), "a live server's socket was taken");
        drop(live); // the file stays, nobody answers
        assert!(bind(&path).is_ok());
    }

    // ---- binding and organization commands ---------------------------------------------

    use std::sync::atomic::AtomicUsize;

    use org_proto::msg::{MsgBind, MsgBindParams};
    use org_proto::org::{CommandStatus, Position, TeamCreate, TeamCreateParams};

    /// A server whose `submit` counts the commands it gets and answers with `answer`.
    fn serving(
        path: &Path,
        answer: impl Fn() -> Result<Result<CommandAccepted, ApiError>, String> + Send + Sync + 'static,
    ) -> (
        Arc<AtomicUsize>,
        Arc<AtomicBool>,
        std::thread::JoinHandle<()>,
    ) {
        let submitted = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&submitted);
        let (mailbox, directory) = messaging();
        let shared = Arc::new(Shared::new(
            &test_info(),
            path,
            OfficePorts {
                work: Box::new(|_, _| Err("none".into())),
                operation_get: Box::new(|_| {
                    Ok(Err(ApiError::new(
                        ErrorCode::NotFound,
                        "operation not found",
                    )))
                }),
                snapshot: Box::new(|| Err(SnapshotFailure::Storage)),
                submit: Box::new(move |_| {
                    counter.fetch_add(1, Ordering::SeqCst);
                    answer()
                }),
                who: Box::new(|_| Err("none".to_owned())),
                status: Box::new(|_, _| Err("none".to_owned())),
                feed: Box::new(Arc::new(FakeFeed::default())),
                live: Arc::new(LiveEvents::new(Box::new(|| {}))),
                mailbox,
                directory,
            },
            token(),
        ));
        let (stop, server) = start(path, &shared);
        (submitted, stop, server)
    }

    fn committed() -> Result<Result<CommandAccepted, ApiError>, String> {
        Ok(Ok(CommandAccepted {
            status: CommandStatus::Committed,
            position: Some(Position { epoch: 1, seq: 2 }),
        }))
    }

    struct Client {
        stream: UnixStream,
        reader: BufReader<UnixStream>,
        next: u64,
    }

    impl Client {
        fn connect(path: &Path) -> Self {
            let stream = UnixStream::connect(path).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let reader = BufReader::new(stream.try_clone().unwrap());
            Self {
                stream,
                reader,
                next: 0,
            }
        }

        fn ask(&mut self, build: impl FnOnce(u64) -> Request) -> org_proto::Outcome {
            self.next += 1;
            frame::write_message(&mut self.stream, &build(self.next)).unwrap();
            let response = frame::read_message::<Response>(&mut self.reader)
                .unwrap()
                .unwrap();
            assert_eq!(response.id, self.next);
            response.outcome
        }

        fn bind(
            &mut self,
            member: Option<&str>,
            app_token: Option<AppToken>,
        ) -> org_proto::Outcome {
            let params = MsgBindParams {
                member: member.map(str::to_owned),
                app_token,
            };
            self.ask(|id| Request::new::<MsgBind>(id, &params).unwrap())
        }

        /// The next message of either kind (an answer has an `id`, an event an `event`).
        fn next_message(&mut self) -> serde_json::Value {
            frame::read_message::<serde_json::Value>(&mut self.reader)
                .unwrap()
                .expect("the connection ended")
        }

        /// The next message if one arrives within `wait`.
        fn message_within(&mut self, wait: Duration) -> Option<serde_json::Value> {
            self.stream.set_read_timeout(Some(wait)).unwrap();
            let got = frame::read_message::<serde_json::Value>(&mut self.reader);
            self.stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            got.ok().flatten()
        }

        /// Nothing arrives within `wait`.
        fn nothing_within(&mut self, wait: Duration) -> bool {
            self.stream.set_read_timeout(Some(wait)).unwrap();
            let got = frame::read_message::<serde_json::Value>(&mut self.reader);
            self.stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            matches!(got, Err(frame::FrameError::Io(e)) if matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ))
        }

        /// The server closed the connection (not merely: nothing arrived in time).
        fn connection_ended(&mut self) -> bool {
            match frame::read_message::<serde_json::Value>(&mut self.reader) {
                Ok(None) => true,
                Err(frame::FrameError::Io(e)) => !matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ),
                _ => false,
            }
        }

        fn subscribe(&mut self, from_seq: Option<u64>) -> org_proto::Outcome {
            let params = org_proto::SubscribeParams {
                from_position: from_seq.map(|seq| org_proto::org::Position { epoch: 1, seq }),
                from_work_position: None,
            };
            self.ask(|id| Request::new::<org_proto::Subscribe>(id, &params).unwrap())
        }

        fn team_create(&mut self) -> org_proto::Outcome {
            self.ask(|id| {
                Request::new::<TeamCreate>(
                    id,
                    &TeamCreateParams {
                        command_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
                        team_id: "T1".into(),
                        name: "Platform".into(),
                        owner_team_id: None,
                    },
                )
                .unwrap()
            })
        }
    }

    fn code_of(outcome: org_proto::Outcome) -> ErrorCode {
        match outcome {
            org_proto::Outcome::Error(e) => e.code,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[test]
    fn an_organization_command_from_a_connection_that_is_not_the_apps_is_refused_before_it_is_read()
    {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (submitted, stop, server) = serving(&path, committed);
        let mut client = Client::connect(&path);

        assert_eq!(code_of(client.team_create()), ErrorCode::NotAuthorized);
        // Params that do not decode are not told apart from good ones to a client that may not
        // send the command at all.
        let garbled = |id| Request {
            id,
            method: "team.create".to_owned(),
            params: serde_json::json!({"nope": 1}),
            principal: None,
        };
        assert_eq!(code_of(client.ask(garbled)), ErrorCode::NotAuthorized);
        // A method this office does not know is still just unknown.
        let unknown = |id| Request {
            id,
            method: "team.fly".to_owned(),
            params: serde_json::json!({}),
            principal: None,
        };
        assert_eq!(code_of(client.ask(unknown)), ErrorCode::UnknownMethod);

        assert_eq!(submitted.load(Ordering::SeqCst), 0);
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn a_connection_bound_with_the_app_token_runs_commands_and_a_wrong_token_binds_nothing() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (submitted, stop, server) = serving(&path, committed);
        let mut client = Client::connect(&path);

        let wrong = AppToken::from_bytes([8; org_proto::APP_TOKEN_BYTES]);
        assert_eq!(
            code_of(client.bind(None, Some(wrong))),
            ErrorCode::NotAuthorized
        );
        assert_eq!(code_of(client.team_create()), ErrorCode::NotAuthorized);

        let bound = client.bind(None, Some(token()));
        let org_proto::Outcome::Result(value) = bound else {
            panic!("{bound:?}")
        };
        assert_eq!(value["office_id"], org_mailbox::testkit::fixture::OFFICE_ID);
        assert!(value.get("member_id").is_none());
        let org_proto::Outcome::Result(accepted) = client.team_create() else {
            panic!("the bound app's command was refused")
        };
        assert_eq!(accepted["status"], "committed");
        assert_eq!(accepted["position"]["seq"], 2);
        assert_eq!(submitted.load(Ordering::SeqCst), 1);

        // Another connection is not bound by it.
        let mut other = Client::connect(&path);
        assert_eq!(code_of(other.team_create()), ErrorCode::NotAuthorized);
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn a_failed_bind_unbinds_the_connection() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (submitted, stop, server) = serving(&path, committed);
        let mut client = Client::connect(&path);
        assert!(matches!(
            client.bind(None, Some(token())),
            org_proto::Outcome::Result(_)
        ));

        // Each bind replaces the last, and a failed one leaves the connection unbound.
        let wrong = AppToken::from_bytes([9; org_proto::APP_TOKEN_BYTES]);
        assert_eq!(
            code_of(client.bind(None, Some(wrong))),
            ErrorCode::NotAuthorized
        );
        assert_eq!(code_of(client.team_create()), ErrorCode::NotAuthorized);
        assert_eq!(submitted.load(Ordering::SeqCst), 0);
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn binds_this_office_does_not_do_are_refused_and_bind_nothing() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (_submitted, stop, server) = serving(&path, committed);
        let mut client = Client::connect(&path);

        // Neither a member nor a token; a member (messaging's, later); garbled params. Each one
        // is tried from an app-bound connection too, and leaves it unbound.
        let garbled = |id| Request {
            id,
            method: "msg.bind".to_owned(),
            params: serde_json::json!({"app_token": "not hex"}),
            principal: None,
        };
        assert_eq!(code_of(client.bind(None, None)), ErrorCode::InvalidRequest);
        client.bind(None, Some(token()));
        assert_eq!(code_of(client.bind(None, None)), ErrorCode::InvalidRequest);
        assert_eq!(code_of(client.team_create()), ErrorCode::NotAuthorized);
        client.bind(None, Some(token()));
        assert_eq!(
            code_of(client.bind(Some("ana"), None)),
            ErrorCode::NotAuthorized
        );
        assert_eq!(code_of(client.team_create()), ErrorCode::NotAuthorized);
        client.bind(None, Some(token()));
        assert_eq!(
            code_of(client.bind(Some("nobody"), Some(token()))),
            ErrorCode::NotAuthorized
        );
        // The input-only checks come first: the office is never a member, and a member over
        // the limit is too large, whatever else the call sends.
        for office in ["office", "OFFICE", "Office", " office ", "\toffice\n"] {
            client.bind(None, Some(token()));
            assert_eq!(
                code_of(client.bind(Some(office), Some(token()))),
                ErrorCode::InvalidRequest,
                "{office}"
            );
            assert_eq!(code_of(client.team_create()), ErrorCode::NotAuthorized);
        }
        client.bind(None, Some(token()));
        let long = "x".repeat(MAX_HANDLE_BYTES + 1);
        assert_eq!(code_of(client.bind(Some(&long), None)), ErrorCode::TooLarge);
        assert_eq!(code_of(client.team_create()), ErrorCode::NotAuthorized);
        // The limit is on the JSON-encoded string, its quotes included.
        let barely_too_long = "x".repeat(MAX_HANDLE_BYTES - 1);
        assert_eq!(
            code_of(client.bind(Some(&barely_too_long), None)),
            ErrorCode::TooLarge
        );
        let longest = "x".repeat(MAX_HANDLE_BYTES - 2);
        assert_eq!(
            code_of(client.bind(Some(&longest), None)),
            ErrorCode::NotAuthorized
        );
        assert_eq!(code_of(client.team_create()), ErrorCode::NotAuthorized);
        client.bind(None, Some(token()));
        assert_eq!(code_of(client.ask(garbled)), ErrorCode::InvalidRequest);
        assert_eq!(code_of(client.team_create()), ErrorCode::NotAuthorized);
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn what_the_office_answers_is_what_the_app_gets() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let calls = Arc::new(AtomicUsize::new(0));
        let answers = Arc::clone(&calls);
        let (_submitted, stop, server) = serving(&path, move || {
            match answers.fetch_add(1, Ordering::SeqCst) {
                0 => Ok(Err(ApiError::new(ErrorCode::AlreadyExists, "taken"))),
                _ => Err("disk on fire at /secret/path".to_owned()),
            }
        });
        let mut client = Client::connect(&path);
        client.bind(None, Some(token()));

        // A refusal keeps its code; a storage failure is a fixed text, not the cause.
        assert_eq!(code_of(client.team_create()), ErrorCode::AlreadyExists);
        let failed = client.team_create();
        let org_proto::Outcome::Error(error) = failed else {
            panic!("{failed:?}")
        };
        assert_eq!(error.code, ErrorCode::StorageError);
        assert!(!error.message.contains("secret"), "{}", error.message);
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn the_bound_app_is_told_a_garbled_command_and_an_unknown_method_apart() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (submitted, stop, server) = serving(&path, committed);
        let mut client = Client::connect(&path);
        client.bind(None, Some(token()));

        let garbled = |id| Request {
            id,
            method: "team.create".to_owned(),
            params: serde_json::json!({"nope": 1}),
            principal: None,
        };
        let unknown = |id| Request {
            id,
            method: "team.fly".to_owned(),
            params: serde_json::json!({}),
            principal: None,
        };
        assert_eq!(code_of(client.ask(garbled)), ErrorCode::InvalidRequest);
        assert_eq!(code_of(client.ask(unknown)), ErrorCode::UnknownMethod);
        // Neither reached the office, and the app is still bound.
        assert_eq!(submitted.load(Ordering::SeqCst), 0);
        assert!(matches!(
            client.team_create(),
            org_proto::Outcome::Result(_)
        ));
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn every_organization_command_is_refused_to_a_connection_that_is_not_the_apps() {
        use org_proto::org::*;
        use org_proto::Call;
        let methods = [
            TeamCreate::METHOD,
            TeamRename::METHOD,
            TeamMove::METHOD,
            TeamSetLead::METHOD,
            TeamDelete::METHOD,
            TeamAddMember::METHOD,
            TeamRemoveMember::METHOD,
            TeamSetTitle::METHOD,
            MemberAddPerson::METHOD,
            MemberAddAgent::METHOD,
            MemberUpdateAgent::METHOD,
            MemberSetAgentStatus::METHOD,
            MemberOffboard::METHOD,
            MemberSetRunMode::METHOD,
            MemberUpdatePerson::METHOD,
            MemberMove::METHOD,
            MemberRemove::METHOD,
            MemberSetReportsTo::METHOD,
            ProjectAddTeam::METHOD,
            ProjectRemoveTeam::METHOD,
        ];
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (submitted, stop, server) = serving(&path, committed);
        let mut client = Client::connect(&path);

        for method in methods {
            let request = |id| Request {
                id,
                method: method.to_owned(),
                params: serde_json::json!({}),
                principal: None,
            };
            assert_eq!(
                code_of(client.ask(request)),
                ErrorCode::NotAuthorized,
                "{method}"
            );
        }

        assert_eq!(submitted.load(Ordering::SeqCst), 0);
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn snapshot_pending_is_unavailable_only_after_binding() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let shared = shared_with(&path, Box::new(|| Err(SnapshotFailure::Unavailable)));
        let (stop, server) = start(&path, &shared);
        let mut client = Client::connect(&path);
        assert_eq!(
            code_of(client.ask(snapshot_request)),
            ErrorCode::NotAuthorized
        );
        client.bind(None, Some(token()));
        assert_eq!(
            code_of(client.ask(snapshot_request)),
            ErrorCode::Unavailable
        );
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn the_organization_is_read_only_by_a_bound_connection() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        // The fake office cannot read (`Err`), so reaching it shows as a storage error.
        let (_submitted, stop, server) = serving(&path, committed);
        let mut client = Client::connect(&path);

        let snapshot = |id| snapshot_request(id);
        assert_eq!(code_of(client.ask(snapshot)), ErrorCode::NotAuthorized);
        // Hello stays open: a client learns what it is talking to before it binds.
        assert!(matches!(
            client.ask(|id| Request::new::<Hello>(id, &org_proto::HelloParams::default()).unwrap()),
            org_proto::Outcome::Result(_)
        ));
        client.bind(None, Some(token()));
        assert_eq!(code_of(client.ask(snapshot)), ErrorCode::StorageError);
        // A failed bind takes the right to read away too.
        client.bind(None, None);
        assert_eq!(code_of(client.ask(snapshot)), ErrorCode::NotAuthorized);
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    // ---- subscribe -------------------------------------------------------------------

    fn following(path: &Path) -> (Arc<FakeFeed>, Arc<AtomicBool>, std::thread::JoinHandle<()>) {
        following_with(path, Timing::default())
    }

    fn following_with(
        path: &Path,
        timing: Timing,
    ) -> (Arc<FakeFeed>, Arc<AtomicBool>, std::thread::JoinHandle<()>) {
        let feed = Arc::new(FakeFeed::default());
        let (mailbox, directory) = messaging();
        let shared = Arc::new(
            Shared::new(
                &test_info(),
                path,
                OfficePorts {
                    work: Box::new(|_, _| Err("none".into())),
                    operation_get: Box::new(|_| {
                        Ok(Err(ApiError::new(
                            ErrorCode::NotFound,
                            "operation not found",
                        )))
                    }),
                    snapshot: Box::new(|| Err(SnapshotFailure::Storage)),
                    submit: Box::new(|_| Err("none".to_owned())),
                    who: Box::new(|_| Err("none".to_owned())),
                    status: Box::new(|_, _| Err("none".to_owned())),
                    feed: Box::new(Arc::clone(&feed)),
                    live: Arc::new(LiveEvents::new(Box::new(|| {}))),
                    mailbox,
                    directory,
                },
                token(),
            )
            .with_timing(timing),
        );
        let (stop, server) = start(path, &shared);
        (feed, stop, server)
    }

    /// The event's `(seq, position.seq, command_id)`.
    fn event_of(message: &serde_json::Value) -> (u64, u64, String) {
        assert_eq!(message["event"], "org.changed", "{message}");
        (
            message["seq"].as_u64().unwrap(),
            message["data"]["position"]["seq"].as_u64().unwrap(),
            message["data"]["command_id"].as_str().unwrap().to_owned(),
        )
    }

    #[test]
    fn subscribing_needs_a_binding_and_says_nothing_else_to_a_connection_without_one() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (feed, stop, server) = following(&path);
        let mut client = Client::connect(&path);

        assert_eq!(code_of(client.subscribe(None)), ErrorCode::NotAuthorized);
        // Params that do not decode are not told apart either.
        let garbled = |id| Request {
            id,
            method: "subscribe".to_owned(),
            params: serde_json::json!({"from_position": "nope"}),
            principal: None,
        };
        assert_eq!(code_of(client.ask(garbled)), ErrorCode::NotAuthorized);
        feed.push(2);
        assert!(client.nothing_within(Duration::from_millis(400)));
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn live_events_start_after_the_answer_and_count_from_one_on_the_connection() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (feed, stop, server) = following(&path);
        feed.push(2); // before the subscription: not sent
        let mut client = Client::connect(&path);
        client.bind(None, Some(token()));

        assert!(matches!(
            client.subscribe(None),
            org_proto::Outcome::Result(_)
        ));
        assert!(client.nothing_within(Duration::from_millis(300)));
        feed.push(3);
        feed.push(4);

        assert_eq!(event_of(&client.next_message()), (1, 3, "cmd-3".into()));
        assert_eq!(event_of(&client.next_message()), (2, 4, "cmd-4".into()));
        // The connection still answers, between events.
        assert!(matches!(
            client.ask(|id| Request::new::<Hello>(id, &org_proto::HelloParams::default()).unwrap()),
            org_proto::Outcome::Result(_)
        ));
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn resuming_sends_what_the_client_missed_in_order_then_goes_live_without_a_repeat() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (feed, stop, server) = following(&path);
        // More than one page of log after the position the client holds.
        for seq in 2..=252 {
            feed.push(seq);
        }
        let mut client = Client::connect(&path);
        client.bind(None, Some(token()));

        // The answer comes first, then every missed event, then live ones.
        let answer = client.subscribe(Some(2));
        assert!(
            matches!(answer, org_proto::Outcome::Result(_)),
            "{answer:?}"
        );
        for expected in 3..=252u64 {
            let (seq, position, _) = event_of(&client.next_message());
            assert_eq!((seq, position), (expected - 2, expected));
        }
        feed.push(253);
        assert_eq!(
            event_of(&client.next_message()),
            (251, 253, "cmd-253".into())
        );
        assert!(client.nothing_within(Duration::from_millis(300)));
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn a_position_the_log_cannot_resume_from_is_snapshot_required_and_subscribes_nothing() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (feed, stop, server) = following(&path);
        feed.push(2);
        let mut client = Client::connect(&path);
        client.bind(None, Some(token()));

        assert_eq!(
            code_of(client.subscribe(Some(9))),
            ErrorCode::SnapshotRequired
        );
        assert_eq!(
            code_of(client.subscribe(Some(0))),
            ErrorCode::SnapshotRequired
        );
        let work = |id| {
            Request::new::<org_proto::Subscribe>(
                id,
                &org_proto::SubscribeParams {
                    from_position: None,
                    from_work_position: Some(org_proto::work::WorkPosition::default()),
                },
            )
            .unwrap()
        };
        assert_eq!(code_of(client.ask(work)), ErrorCode::SnapshotRequired);
        // None of them subscribed: nothing is pushed, and a good subscribe still works.
        feed.push(3);
        assert!(client.nothing_within(Duration::from_millis(300)));
        assert!(matches!(
            client.subscribe(Some(2)),
            org_proto::Outcome::Result(_)
        ));
        assert_eq!(event_of(&client.next_message()).1, 3);
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn a_second_subscribe_is_refused_and_does_not_double_the_events() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (feed, stop, server) = following(&path);
        let mut client = Client::connect(&path);
        client.bind(None, Some(token()));
        assert!(matches!(
            client.subscribe(None),
            org_proto::Outcome::Result(_)
        ));

        assert_eq!(code_of(client.subscribe(None)), ErrorCode::InvalidRequest);
        feed.push(2);
        assert_eq!(event_of(&client.next_message()).1, 2);
        assert!(
            client.nothing_within(Duration::from_millis(300)),
            "an event twice"
        );
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn a_bind_ends_the_subscription_and_the_client_subscribes_again() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (feed, stop, server) = following(&path);
        let mut client = Client::connect(&path);
        client.bind(None, Some(token()));
        assert!(matches!(
            client.subscribe(None),
            org_proto::Outcome::Result(_)
        ));
        feed.push(2);
        assert_eq!(event_of(&client.next_message()).1, 2);

        // A failed bind unbinds the connection: no event reaches it any more.
        client.bind(None, None);
        feed.push(3);
        assert!(client.nothing_within(Duration::from_millis(400)));
        // Bound again, it subscribes again and resumes from what it last had.
        client.bind(None, Some(token()));
        assert!(matches!(
            client.subscribe(Some(2)),
            org_proto::Outcome::Result(_)
        ));
        // `seq` counts on the connection, not per subscription.
        assert_eq!(event_of(&client.next_message()), (2, 3, "cmd-3".into()));
        // A successful re-bind ends it too.
        client.bind(None, Some(token()));
        feed.push(4);
        assert!(client.nothing_within(Duration::from_millis(400)));
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn a_log_that_cannot_be_read_closes_the_subscribers_connection_instead_of_leaving_it_silent() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (feed, stop, server) = following(&path);
        let mut client = Client::connect(&path);
        client.bind(None, Some(token()));
        // Subscribing reads the log too, so the failure is switched on after it.
        assert!(matches!(
            client.subscribe(None),
            org_proto::Outcome::Result(_)
        ));
        feed.broken.store(true, Ordering::SeqCst);
        feed.push(2);

        assert!(client.connection_ended());
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn stopping_with_a_subscribed_connection_open_returns_and_closes_it() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (_feed, stop, server) = following(&path);
        let mut client = Client::connect(&path);
        client.bind(None, Some(token()));
        assert!(matches!(
            client.subscribe(None),
            org_proto::Outcome::Result(_)
        ));

        let started = std::time::Instant::now();
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();

        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(client.connection_ended());
    }

    #[test]
    fn a_commit_wakes_the_subscriber_without_waiting_for_the_idle_poll() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        // The poll is far longer than the delivery is allowed to take: only the commit's wake-up
        // can deliver in time. (Stopping waits for the poll, so it is not hours.)
        let timing = Timing {
            idle: Duration::from_secs(6),
            ..Timing::default()
        };
        let (feed, stop, server) = following_with(&path, timing);
        let mut client = Client::connect(&path);
        client.bind(None, Some(token()));
        assert!(matches!(
            client.subscribe(None),
            org_proto::Outcome::Result(_)
        ));
        std::thread::sleep(Duration::from_millis(300)); // the follower is asleep in `wait`

        let started = std::time::Instant::now();
        feed.push(2);

        assert_eq!(event_of(&client.next_message()).1, 2);
        assert!(started.elapsed() < Duration::from_secs(3));
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn a_client_that_does_not_read_is_dropped_after_the_write_deadline() {
        use std::io::Read;
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let timing = Timing {
            write: Duration::from_millis(500),
            ..Timing::default()
        };
        let (feed, stop, server) = following_with(&path, timing);
        let mut client = Client::connect(&path);
        client.bind(None, Some(token()));
        assert!(matches!(
            client.subscribe(None),
            org_proto::Outcome::Result(_)
        ));

        // Events far bigger than the socket's buffers, to a client that reads nothing.
        client
            .stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        for seq in 2..=5 {
            feed.push_big(seq, 400_000);
        }
        std::thread::sleep(Duration::from_secs(3));

        // The server gave up on it and closed: draining ends in EOF. (Without a deadline the
        // drain would unblock the server and the stream would never end.)
        let mut sink = Vec::new();
        let ended = client.stream.read_to_end(&mut sink);
        assert!(ended.is_ok(), "{ended:?}");
        assert!(
            sink.len() < 4 * 400_000,
            "everything was sent: {}",
            sink.len()
        );
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn a_client_that_reads_a_trickle_is_dropped_by_the_deadline_of_the_whole_frame() {
        use std::io::Read;
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let timing = Timing {
            write: Duration::from_secs(1),
            ..Timing::default()
        };
        let (feed, stop, server) = following_with(&path, timing);
        let mut client = Client::connect(&path);
        client.bind(None, Some(token()));
        assert!(matches!(
            client.subscribe(None),
            org_proto::Outcome::Result(_)
        ));
        feed.push_big(2, 900_000);

        // Some progress every 100 ms (each write call would succeed within a second), but at
        // this pace the frame would take over twenty: the frame's deadline ends it.
        let started = std::time::Instant::now();
        let mut buffer = [0u8; 4096];
        let mut total = 0usize;
        let ended = loop {
            std::thread::sleep(Duration::from_millis(100));
            match client.stream.read(&mut buffer) {
                Ok(0) => break true,
                Ok(n) => total += n,
                Err(_) => break false,
            }
            if started.elapsed() > Duration::from_secs(15) {
                break false;
            }
        };
        assert!(ended, "still connected after {total} bytes");
        assert!(total < 900_000, "the whole frame went out: {total}");
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn binding_and_subscribing_in_a_loop_never_leaves_more_than_two_followers_on_a_connection() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let timing = Timing {
            idle: Duration::from_millis(300),
            ..Timing::default()
        };
        let (feed, stop, server) = following_with(&path, timing);
        let mut client = Client::connect(&path);

        for _ in 0..8 {
            client.bind(None, Some(token()));
            assert!(matches!(
                client.subscribe(None),
                org_proto::Outcome::Result(_)
            ));
        }

        // The one being replaced, and the new one: a new subscription waits for the old to end.
        let peak = feed.peak_waiting.load(Ordering::SeqCst);
        assert!(peak <= 2, "{peak} followers at once");
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn a_rebind_in_the_middle_of_a_replay_leaves_no_gap_in_the_numbers() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (feed, stop, server) = following(&path);
        for seq in 2..=2001 {
            feed.push(seq);
        }
        let mut client = Client::connect(&path);
        client.bind(None, Some(token()));
        assert!(matches!(
            client.subscribe(Some(1)),
            org_proto::Outcome::Result(_)
        ));
        // Read part of the replay, then bind again while the follower is still sending.
        let mut last = (0, 0);
        for _ in 0..50 {
            let (n, position, _) = event_of(&client.next_message());
            last = (n, position);
        }
        client.stream.set_nonblocking(false).unwrap();
        let rebind = Request::new::<MsgBind>(
            900,
            &MsgBindParams {
                member: None,
                app_token: Some(token()),
            },
        )
        .unwrap();
        frame::write_message(&mut client.stream, &rebind).unwrap();
        // Events already on the wire come first; the bind's answer ends them.
        loop {
            let message = client.next_message();
            if message.get("id") == Some(&serde_json::json!(900)) {
                break;
            }
            let (n, position, _) = event_of(&message);
            assert_eq!(n, last.0 + 1, "a number was skipped");
            last = (n, position);
        }
        // Subscribing again from the last position continues the numbers, with no gap.
        client.next = 900;
        assert!(matches!(
            client.subscribe(Some(last.1)),
            org_proto::Outcome::Result(_)
        ));
        let (n, position, _) = event_of(&client.next_message());
        assert_eq!((n, position), (last.0 + 1, last.1 + 1));
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    // ---- who -------------------------------------------------------------------------

    /// A server whose `who` answers from `answer` and counts its calls.
    fn asking(
        path: &Path,
        answer: impl Fn(&WhoParams) -> Result<Result<WhoResult, ApiError>, String>
            + Send
            + Sync
            + 'static,
    ) -> (
        Arc<AtomicUsize>,
        Arc<AtomicBool>,
        std::thread::JoinHandle<()>,
    ) {
        let asked = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&asked);
        let (mailbox, directory) = messaging();
        let shared = Arc::new(Shared::new(
            &test_info(),
            path,
            OfficePorts {
                work: Box::new(|_, _| Err("none".into())),
                operation_get: Box::new(|_| {
                    Ok(Err(ApiError::new(
                        ErrorCode::NotFound,
                        "operation not found",
                    )))
                }),
                snapshot: Box::new(|| Err(SnapshotFailure::Storage)),
                submit: Box::new(|_| Err("none".to_owned())),
                who: Box::new(move |params| {
                    counter.fetch_add(1, Ordering::SeqCst);
                    answer(params)
                }),
                status: Box::new(|_, _| Err("none".to_owned())),
                feed: Box::new(Arc::new(FakeFeed::default())),
                live: Arc::new(LiveEvents::new(Box::new(|| {}))),
                mailbox,
                directory,
            },
            token(),
        ));
        let (stop, server) = start(path, &shared);
        (asked, stop, server)
    }

    fn who_ask(client: &mut Client, params: serde_json::Value) -> org_proto::Outcome {
        client.ask(|id| Request {
            id,
            method: "who".to_owned(),
            params,
            principal: None,
        })
    }

    #[test]
    fn who_is_for_a_bound_connection_and_an_unbound_one_is_not_bound_before_its_params_are_read() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (asked, stop, server) = asking(&path, |_| Ok(Ok(WhoResult { members: vec![] })));
        let mut client = Client::connect(&path);

        assert_eq!(
            code_of(who_ask(&mut client, serde_json::json!({}))),
            ErrorCode::NotBound
        );
        assert_eq!(
            code_of(who_ask(&mut client, serde_json::json!({"kind": 7}))),
            ErrorCode::NotBound
        );
        assert_eq!(asked.load(Ordering::SeqCst), 0);

        client.bind(None, Some(token()));
        let org_proto::Outcome::Result(value) = who_ask(&mut client, serde_json::json!({})) else {
            panic!("a bound connection was refused")
        };
        assert_eq!(value["members"], serde_json::json!([]));
        // Params that do not decode are told to a bound connection.
        assert_eq!(
            code_of(who_ask(&mut client, serde_json::json!({"kind": 7}))),
            ErrorCode::InvalidRequest
        );
        assert_eq!(asked.load(Ordering::SeqCst), 1);
        // A failed bind takes the right away.
        client.bind(None, None);
        assert_eq!(
            code_of(who_ask(&mut client, serde_json::json!({}))),
            ErrorCode::NotBound
        );
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn what_who_answers_is_what_the_caller_gets() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let (_asked, stop, server) = asking(&path, move |params| {
            match seen.fetch_add(1, Ordering::SeqCst) {
                0 => {
                    assert_eq!(params.member.as_deref(), Some("pixel"));
                    assert_eq!(params.team.as_deref(), Some("t1"));
                    Ok(Err(ApiError::new(ErrorCode::NotFound, "no such member")))
                }
                _ => Err("disk on fire at /secret/path".to_owned()),
            }
        });
        let mut client = Client::connect(&path);
        client.bind(None, Some(token()));

        // A refusal keeps its code; a storage failure is a fixed text, not the cause.
        assert_eq!(
            code_of(who_ask(
                &mut client,
                serde_json::json!({"member": "pixel", "team": "t1"})
            )),
            ErrorCode::NotFound
        );
        let org_proto::Outcome::Error(error) = who_ask(&mut client, serde_json::json!({})) else {
            panic!("expected an error")
        };
        assert_eq!(error.code, ErrorCode::StorageError);
        assert!(!error.message.contains("secret"), "{}", error.message);
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    // ---- messaging: member binds and the msg.* calls ---------------------------------------

    use org_mailbox::testkit::fixture::{
        msg_id, ADA_ID, FAR_HANDLE, GONE_HANDLE, ODD_HANDLE, OFFICE_ID, OWNER_ID, PAT_ID, REMY_ID,
    };
    use org_proto::directory::StatusSet;
    use org_proto::msg::{
        MsgInbox, MsgInboxParams, MsgRead, MsgReadParams, MsgSend, MsgSendParams, MsgWaitParams,
        KIND_MSG,
    };

    /// A server over the fixture members that accepts any organization command, and the mailbox
    /// it runs on.
    fn messaging_server(
        path: &Path,
    ) -> (Arc<Mailbox>, Arc<AtomicBool>, std::thread::JoinHandle<()>) {
        let (mailbox, _, stop, server) = messaging_server_with_directory(path);
        (mailbox, stop, server)
    }

    /// The same, with the directory the test may change under the server.
    fn messaging_server_with_directory(
        path: &Path,
    ) -> (
        Arc<Mailbox>,
        Arc<org_mailbox::testkit::InMemoryDirectory>,
        Arc<AtomicBool>,
        std::thread::JoinHandle<()>,
    ) {
        let (mailbox, directory) = messaging();
        let mut ports = ports(Box::new(|| Err(SnapshotFailure::Storage)));
        ports.submit = Box::new(|_| committed());
        ports.mailbox = Arc::clone(&mailbox);
        ports.directory = directory.clone();
        let shared = Arc::new(Shared::new(&test_info(), path, ports, token()));
        let (stop, server) = start(path, &shared);
        (mailbox, directory, stop, server)
    }

    /// Waits for `condition`, and fails (instead of hanging the suite) if it never comes.
    fn until(what: &str, condition: impl Fn() -> bool) {
        let limit = std::time::Instant::now() + Duration::from_secs(20);
        while !condition() {
            assert!(std::time::Instant::now() < limit, "never happened: {what}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn send_params(n: u32, to: &str, body: &str) -> MsgSendParams {
        MsgSendParams {
            id: msg_id(n),
            to: to.to_owned(),
            kind: KIND_MSG.to_owned(),
            priority: None,
            subject: None,
            body: body.to_owned(),
            meta: Default::default(),
            reply_to: None,
        }
    }

    fn inbox_params() -> MsgInboxParams {
        MsgInboxParams {
            include_all: false,
            from: None,
            thread_id: None,
            cursor: None,
            limit: None,
        }
    }

    impl Client {
        fn call<C: org_proto::Call>(&mut self, params: &C::Params) -> org_proto::Outcome {
            self.ask(|id| Request::new::<C>(id, params).unwrap())
        }

        /// Sends a call without waiting for its answer; the id it will answer with.
        fn call_later<C: org_proto::Call>(&mut self, params: &C::Params) -> u64 {
            self.next += 1;
            frame::write_message(
                &mut self.stream,
                &Request::new::<C>(self.next, params).unwrap(),
            )
            .unwrap();
            self.next
        }

        /// The next answer, whichever call it is for.
        fn answer(&mut self) -> Response {
            frame::read_message::<Response>(&mut self.reader)
                .unwrap()
                .expect("the connection ended")
        }

        fn bound_as(path: &Path, member: &str) -> Self {
            let mut client = Client::connect(path);
            let token = (member == "pat" || member == "owner").then(token);
            assert!(
                matches!(
                    client.bind(Some(member), token),
                    org_proto::Outcome::Result(_)
                ),
                "{member} binds"
            );
            client
        }
    }

    #[test]
    fn an_agent_binds_without_a_token_by_handle_or_id_and_is_told_who_it_is() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (_mailbox, stop, server) = messaging_server(&path);
        let mut client = Client::connect(&path);
        for name in ["ada", "ADA", " ada ", ADA_ID] {
            let org_proto::Outcome::Result(value) = client.bind(Some(name), None) else {
                panic!("{name} should bind")
            };
            assert_eq!(value["member_id"], ADA_ID, "{name}");
            assert_eq!(value["office_id"], OFFICE_ID);
        }
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn a_bind_that_the_contract_refuses_says_which_way_and_leaves_the_connection_unbound() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (_mailbox, stop, server) = messaging_server(&path);
        let mut client = Client::connect(&path);
        let unbound = |client: &mut Client| {
            code_of(client.call::<MsgInbox>(&inbox_params())) == ErrorCode::NotBound
        };

        // An agent with a token is a mistake in the call, not a refusal of the member.
        assert_eq!(
            code_of(client.bind(Some("ada"), Some(token()))),
            ErrorCode::InvalidRequest
        );
        assert!(unbound(&mut client));
        // Every `not_authorized` bind of a member says the same words, so the text tells a
        // person's handle from an unknown one no more than the code does.
        let text_of = |outcome| match outcome {
            org_proto::Outcome::Error(e) => e.message,
            other => panic!("expected an error, got {other:?}"),
        };
        assert_eq!(
            text_of(client.bind(Some("pat"), None)),
            text_of(client.bind(Some("nobody"), None))
        );
        // A person or the owner needs the token: none, or a wrong one.
        for who in ["pat", "owner"] {
            assert_eq!(
                code_of(client.bind(Some(who), None)),
                ErrorCode::NotAuthorized,
                "{who}"
            );
            let wrong = AppToken::from_bytes([8; org_proto::APP_TOKEN_BYTES]);
            assert_eq!(
                code_of(client.bind(Some(who), Some(wrong))),
                ErrorCode::NotAuthorized,
                "{who}"
            );
            assert!(unbound(&mut client));
        }
        // An unknown member, an offboarded or unknown-status agent, and one of another office
        // fail closed, the same way.
        for name in ["nobody", GONE_HANDLE, ODD_HANDLE, FAR_HANDLE, "", "   "] {
            assert_eq!(
                code_of(client.bind(Some(name), None)),
                ErrorCode::NotAuthorized,
                "{name:?}"
            );
            assert!(unbound(&mut client), "{name:?}");
        }
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn an_office_run_agent_cannot_bind_yet_and_a_member_removed_after_binding_is_not_bound() {
        use org_mailbox::testkit::fixture::{members, POSITION};
        use org_proto::RunMode;
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (_mailbox, directory, stop, server) = messaging_server_with_directory(&path);
        let mut client = Client::connect(&path);

        // An agent with a run mode (`Unknown` included) is office-run and needs a session token,
        // which this office does not issue yet: it fails closed.
        let mut run = members().into_iter().find(|m| m.id == ADA_ID).unwrap();
        for mode in [RunMode::AlwaysOn, RunMode::OnCall, RunMode::Unknown] {
            run.agent.as_mut().unwrap().run_mode = Some(mode);
            directory.upsert(run.clone(), POSITION);
            assert_eq!(
                code_of(client.bind(Some("ada"), None)),
                ErrorCode::NotAuthorized,
                "{mode:?}"
            );
        }
        run.agent.as_mut().unwrap().run_mode = None;
        directory.upsert(run, POSITION);
        assert!(matches!(
            client.bind(Some("ada"), None),
            org_proto::Outcome::Result(_)
        ));
        assert!(matches!(
            client.call::<MsgInbox>(&inbox_params()),
            org_proto::Outcome::Result(_)
        ));
        // Moved out of the organization after it bound: its next call is `not_bound`.
        directory.remove(ADA_ID, POSITION);
        assert_eq!(
            code_of(client.call::<MsgInbox>(&inbox_params())),
            ErrorCode::NotBound
        );
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn a_connection_closes_when_its_member_stops_being_bindable() {
        use org_mailbox::testkit::fixture::{members, POSITION};
        use org_proto::{AgentStatus, RunMode};
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (_mailbox, directory, stop, server) = messaging_server_with_directory(&path);
        let ada = || members().into_iter().find(|m| m.id == ADA_ID).unwrap();
        let wait = MsgWaitParams {
            timeout_ms: 30_000,
            from: None,
            urgent_only: false,
        };

        // A confirmed revocation found by a call closes just as the watcher path does.
        for change in 0..3 {
            let mut client = Client::bound_as(&path, "ada");
            let held = client.call_later::<org_proto::msg::MsgWait>(&wait);
            until("a wait is held", || _mailbox.held_waits() > 0);
            let mut changed = ada();
            match change {
                0 => changed.agent.as_mut().unwrap().run_mode = Some(RunMode::OnCall),
                1 => changed.agent.as_mut().unwrap().status = AgentStatus::Offboarded,
                _ => directory.remove(ADA_ID, POSITION),
            }
            if change < 2 {
                directory.upsert(changed, POSITION);
            }
            let who = client.call_later::<org_proto::directory::Who>(&Default::default());
            // The call and held wait both receive `not_bound`, in either order, then EOF.
            let mut got = std::collections::BTreeMap::new();
            for _ in 0..2 {
                let answer = client.answer();
                got.insert(answer.id, code_of(answer.outcome));
            }
            assert_eq!(
                got,
                [(held, ErrorCode::NotBound), (who, ErrorCode::NotBound),]
                    .into_iter()
                    .collect(),
                "change {change}"
            );
            assert!(frame::read_message::<Response>(&mut client.reader)
                .unwrap()
                .is_none());
            // A new connection still cannot bind the revoked member.
            let mut refused = Client::connect(&path);
            assert_eq!(
                code_of(refused.bind(Some("ada"), None)),
                ErrorCode::NotAuthorized
            );
            directory.upsert(ada(), POSITION);
        }
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn a_removed_members_id_that_is_another_members_handle_does_not_keep_the_connection_bound() {
        use org_mailbox::testkit::fixture::POSITION;
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (_mailbox, directory, stop, server) = messaging_server_with_directory(&path);
        // The fixture's shadow agent has pat's id as its handle: once pat is gone, pat's id
        // names the shadow, who is bindable but not pat.
        let mut client = Client::bound_as(&path, "pat");
        directory.remove(PAT_ID, POSITION);
        assert_eq!(
            code_of(who_ask(&mut client, serde_json::json!({}))),
            ErrorCode::NotBound
        );
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn a_person_bound_with_the_token_acts_as_that_person_and_may_send_organization_commands() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (_mailbox, stop, server) = messaging_server(&path);
        let mut client = Client::connect(&path);
        let org_proto::Outcome::Result(value) = client.bind(Some("owner"), Some(token())) else {
            panic!("the owner binds with the token")
        };
        assert_eq!(value["member_id"], OWNER_ID);
        assert!(matches!(
            client.team_create(),
            org_proto::Outcome::Result(_)
        ));
        // An agent is not app-bound.
        client.bind(Some("ada"), None);
        assert_eq!(code_of(client.team_create()), ErrorCode::NotAuthorized);
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn a_message_goes_from_the_bound_member_to_the_other_and_reads_once() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (_mailbox, stop, server) = messaging_server(&path);
        let mut ada = Client::bound_as(&path, "ada");
        let mut remy = Client::bound_as(&path, "remy");

        let org_proto::Outcome::Result(sent) = ada.call::<MsgSend>(&send_params(1, "remy", "hi"))
        else {
            panic!("the send is accepted")
        };
        assert_eq!(sent["id"], msg_id(1));
        // The sender sees nothing in its own inbox; the recipient sees the header.
        let org_proto::Outcome::Result(own) = ada.call::<MsgInbox>(&inbox_params()) else {
            panic!("inbox")
        };
        assert_eq!(own["headers"].as_array().unwrap().len(), 0);
        let org_proto::Outcome::Result(theirs) = remy.call::<MsgInbox>(&inbox_params()) else {
            panic!("inbox")
        };
        assert_eq!(theirs["headers"][0]["from_member_id"], ADA_ID);
        assert_eq!(theirs["headers"][0]["to_member_id"], REMY_ID);

        let read = MsgReadParams { id: None };
        let org_proto::Outcome::Result(got) = remy.call::<MsgRead>(&read) else {
            panic!("read")
        };
        assert_eq!(got["message"]["body"], "hi");
        let org_proto::Outcome::Result(again) = remy.call::<MsgRead>(&read) else {
            panic!("read")
        };
        assert!(again["message"].is_null() || again.get("message").is_none());
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn who_a_call_acts_as_is_the_binding_and_never_a_field_of_the_call() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (_mailbox, stop, server) = messaging_server(&path);
        let mut ada = Client::bound_as(&path, "ada");
        // A `from` in the params is not read: the decoder refuses unknown fields or ignores
        // them, and the message is ada's either way.
        let forged = Request {
            id: 0,
            method: "msg.send".to_owned(),
            params: serde_json::json!({
                "id": msg_id(2), "to": "remy", "kind": "msg", "body": "x",
                "from": "pat", "from_member_id": PAT_ID
            }),
            principal: None,
        };
        let sent = ada.ask(|id| Request { id, ..forged });
        let mut remy = Client::bound_as(&path, "remy");
        let org_proto::Outcome::Result(theirs) = remy.call::<MsgInbox>(&inbox_params()) else {
            panic!("inbox")
        };
        let headers = theirs["headers"].as_array().unwrap();
        match sent {
            // Accepted: the extra fields were not read, and the message is ada's.
            org_proto::Outcome::Result(_) => {
                assert_eq!(headers.len(), 1);
                assert_eq!(headers[0]["from_member_id"], ADA_ID);
            }
            // Refused: nothing was sent.
            org_proto::Outcome::Error(e) => {
                assert_eq!(e.code, ErrorCode::InvalidRequest);
                assert!(headers.is_empty());
            }
        }
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn messaging_needs_a_member_binding_and_an_app_wide_one_is_not_a_member() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (_mailbox, stop, server) = messaging_server(&path);
        let mut client = Client::connect(&path);
        let wait = MsgWaitParams {
            timeout_ms: 10,
            from: None,
            urgent_only: false,
        };
        let calls = |client: &mut Client| {
            [
                code_of(client.call::<MsgSend>(&send_params(3, "remy", "x"))),
                code_of(client.call::<MsgInbox>(&inbox_params())),
                code_of(client.call::<MsgRead>(&MsgReadParams { id: None })),
                code_of(client.call::<org_proto::msg::MsgWait>(&wait)),
            ]
        };
        assert_eq!(calls(&mut client), [ErrorCode::NotBound; 4]);
        client.bind(None, Some(token()));
        assert_eq!(calls(&mut client), [ErrorCode::NotBound; 4]);
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn a_bad_param_is_told_in_fixed_words_that_do_not_echo_the_body() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (_mailbox, stop, server) = messaging_server(&path);
        let mut ada = Client::bound_as(&path, "ada");
        let org_proto::Outcome::Error(error) = ada.ask(|id| Request {
            id,
            method: "msg.send".to_owned(),
            params: serde_json::json!({
                "id": msg_id(4), "to": "remy", "kind": "msg", "body": "x",
                "priority": "SECRET-BODY"
            }),
            principal: None,
        }) else {
            panic!("a refusal")
        };
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert!(!error.message.contains("SECRET"), "{}", error.message);
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn a_held_wait_is_answered_when_a_message_arrives_while_the_connection_keeps_answering() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (mailbox, stop, server) = messaging_server(&path);
        let mut remy = Client::bound_as(&path, "remy");
        let mut ada = Client::bound_as(&path, "ada");
        let wait = MsgWaitParams {
            timeout_ms: 30_000,
            from: None,
            urgent_only: false,
        };
        let held = remy.call_later::<org_proto::msg::MsgWait>(&wait);
        // The wait is registered before anything is sent to it.
        until("a wait is held", || mailbox.held_waits() > 0);
        // The same connection still answers.
        assert!(matches!(
            remy.call::<MsgInbox>(&inbox_params()),
            org_proto::Outcome::Result(_)
        ));
        ada.call::<MsgSend>(&send_params(5, "remy", "wake up"));
        let answer = remy.answer();
        assert_eq!(answer.id, held);
        let org_proto::Outcome::Result(value) = answer.outcome else {
            panic!("the wait ends with the message")
        };
        assert_eq!(value["message"]["body"], "wake up");
        assert_eq!(mailbox.held_waits(), 0);
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn a_bind_ends_the_held_wait_of_the_binding_before_it_with_not_bound() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (mailbox, stop, server) = messaging_server(&path);
        let mut remy = Client::bound_as(&path, "remy");
        let wait = MsgWaitParams {
            timeout_ms: 30_000,
            from: None,
            urgent_only: false,
        };
        let held = remy.call_later::<org_proto::msg::MsgWait>(&wait);
        until("a wait is held", || mailbox.held_waits() > 0);
        // A failed bind counts too.
        let rebind = remy.call_later::<MsgBind>(&MsgBindParams {
            member: Some("nobody".into()),
            app_token: None,
        });
        let mut answers = [remy.answer(), remy.answer()];
        answers.sort_by_key(|answer| answer.id);
        assert_eq!(answers[0].id, held);
        assert_eq!(code_of(answers[0].outcome.clone()), ErrorCode::NotBound);
        assert_eq!(answers[1].id, rebind);
        assert_eq!(
            code_of(answers[1].outcome.clone()),
            ErrorCode::NotAuthorized
        );
        assert_eq!(mailbox.held_waits(), 0);
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn a_connection_that_closes_with_waits_held_leaves_none_behind_and_the_server_stops() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (mailbox, stop, server) = messaging_server(&path);
        let mut remy = Client::bound_as(&path, "remy");
        let wait = MsgWaitParams {
            timeout_ms: 30_000,
            from: None,
            urgent_only: false,
        };
        remy.call_later::<org_proto::msg::MsgWait>(&wait);
        remy.call_later::<org_proto::msg::MsgWait>(&wait);
        until("two waits are held", || mailbox.held_waits() >= 2);
        drop(remy);
        until("no wait is held", || mailbox.held_waits() == 0);
        // And one still held when the server stops does not keep it from stopping.
        let mut again = Client::bound_as(&path, "remy");
        again.call_later::<org_proto::msg::MsgWait>(&wait);
        until("a wait is held", || mailbox.held_waits() > 0);
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
        assert_eq!(mailbox.held_waits(), 0);
    }

    #[test]
    fn rebinding_gives_the_held_places_back_at_once() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (mailbox, stop, server) = messaging_server(&path);
        let mut remy = Client::bound_as(&path, "remy");
        let wait = MsgWaitParams {
            timeout_ms: 30_000,
            from: None,
            urgent_only: false,
        };
        for _ in 0..MAX_HELD {
            remy.call_later::<org_proto::msg::MsgWait>(&wait);
        }
        until("all are held", || mailbox.held_waits() >= MAX_HELD);
        // Bind again and wait at once: the places of the cancelled calls are free already.
        let rebind = remy.call_later::<MsgBind>(&MsgBindParams {
            member: Some("remy".into()),
            app_token: None,
        });
        let fresh = remy.call_later::<org_proto::msg::MsgWait>(&wait);
        let mut ended = 0;
        let mut rebound = false;
        while ended < MAX_HELD || !rebound {
            let answer = remy.answer();
            if answer.id == rebind {
                rebound = true;
            } else {
                assert_ne!(answer.id, fresh, "the new wait is held, not answered");
                assert_eq!(code_of(answer.outcome), ErrorCode::NotBound);
                ended += 1;
            }
        }
        until("only the new wait is held", || mailbox.held_waits() == 1);
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn the_places_of_held_calls_are_counted_freed_and_cancelled_with_their_guard() {
        let held = Held::default();
        let mut cancellations = Vec::new();
        for _ in 0..MAX_HELD {
            let (handle, cancellation) = cancellation_pair();
            cancellations.push(cancellation);
            assert!(held.start(handle).is_some());
        }
        let (handle, _extra) = cancellation_pair();
        assert!(held.start(handle).is_none(), "the cap");
        {
            // The guard ends them all, as when the connection's thread panics.
            let _guard = CancelOnDrop(&held);
        }
        assert!(cancellations.iter().all(|c| c.is_cancelled()));
        let (handle, _fresh) = cancellation_pair();
        let number = held.start(handle).expect("the places are free again");
        {
            let _place = Place(&held, number);
        }
        // A place given back is taken again.
        for _ in 0..MAX_HELD {
            let (handle, _) = cancellation_pair();
            assert!(held.start(handle).is_some());
        }
    }

    #[test]
    fn at_most_sixteen_waits_are_held_on_a_connection() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (mailbox, stop, server) = messaging_server(&path);
        let mut remy = Client::bound_as(&path, "remy");
        let wait = MsgWaitParams {
            timeout_ms: 30_000,
            from: None,
            urgent_only: false,
        };
        for _ in 0..MAX_HELD {
            remy.call_later::<org_proto::msg::MsgWait>(&wait);
        }
        until("all are held", || mailbox.held_waits() >= MAX_HELD);
        let extra = remy.call_later::<org_proto::msg::MsgWait>(&wait);
        let answer = remy.answer();
        assert_eq!(answer.id, extra);
        assert_eq!(code_of(answer.outcome), ErrorCode::Unavailable);
        assert_eq!(mailbox.held_waits(), MAX_HELD);
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    // ---- status.set ------------------------------------------------------------------------

    type StatusLog = Arc<Mutex<Vec<(String, StatusSetParams)>>>;

    /// A server whose `status.set` records who asked and echoes the text back.
    fn status_server(path: &Path) -> (StatusLog, Arc<AtomicBool>, std::thread::JoinHandle<()>) {
        let log: StatusLog = Arc::default();
        let (mailbox, directory) = messaging();
        let mut ports = ports(Box::new(|| Err(SnapshotFailure::Storage)));
        ports.mailbox = mailbox;
        ports.directory = directory;
        let recorded = Arc::clone(&log);
        ports.status = Box::new(move |member, params| {
            recorded
                .lock()
                .unwrap()
                .push((member.to_owned(), params.clone()));
            match params.text.as_deref() {
                Some("storage") => Err("disk on fire at /secret/path".to_owned()),
                Some("refuse") => Ok(Err(ApiError::new(ErrorCode::TooLarge, "too long"))),
                text => Ok(Ok(StatusSetResult {
                    status: text.map(str::to_owned),
                    expires_at: text.map(|_| "2026-11-08T20:00:00.000Z".to_owned()),
                })),
            }
        });
        let shared = Arc::new(Shared::new(&test_info(), path, ports, token()));
        let (stop, server) = start(path, &shared);
        (log, stop, server)
    }

    fn status_params(text: &str) -> StatusSetParams {
        StatusSetParams {
            text: Some(text.to_owned()),
            ttl_ms: None,
        }
    }

    #[test]
    fn an_agent_sets_its_own_status_and_the_member_is_the_binding_not_the_call() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (log, stop, server) = status_server(&path);
        let mut ada = Client::bound_as(&path, "ada");
        let org_proto::Outcome::Result(value) = ada.call::<StatusSet>(&status_params("Drafting"))
        else {
            panic!("an agent sets its status")
        };
        assert_eq!(value["status"], "Drafting");
        // A `member` in the call is not read.
        let forged = ada.ask(|id| Request {
            id,
            method: "status.set".to_owned(),
            params: serde_json::json!({"text": "Hijack", "member": PAT_ID, "member_id": PAT_ID}),
            principal: None,
        });
        let _ = forged;
        let seen: Vec<String> = log.lock().unwrap().iter().map(|(m, _)| m.clone()).collect();
        assert!(
            !seen.is_empty() && seen.iter().all(|m| m == ADA_ID),
            "{seen:?}"
        );
        // A refusal keeps its code; a storage failure is a fixed text.
        assert_eq!(
            code_of(ada.call::<StatusSet>(&status_params("refuse"))),
            ErrorCode::TooLarge
        );
        let org_proto::Outcome::Error(error) = ada.call::<StatusSet>(&status_params("storage"))
        else {
            panic!("storage fails")
        };
        assert_eq!(error.code, ErrorCode::StorageError);
        assert!(!error.message.contains("secret"), "{}", error.message);
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn only_an_agent_bound_connection_may_set_a_status_and_the_binding_is_checked_before_the_params(
    ) {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (log, stop, server) = status_server(&path);
        let garbage = |client: &mut Client| {
            code_of(client.ask(|id| Request {
                id,
                method: "status.set".to_owned(),
                params: serde_json::json!({"text": ["not a string"]}),
                principal: None,
            }))
        };
        let mut client = Client::connect(&path);
        // Unbound and app-wide: not_bound, whatever the params.
        assert_eq!(garbage(&mut client), ErrorCode::NotBound);
        client.bind(None, Some(token()));
        assert_eq!(garbage(&mut client), ErrorCode::NotBound);
        assert_eq!(
            code_of(client.call::<StatusSet>(&status_params("x"))),
            ErrorCode::NotBound
        );
        // A person or the owner bound with the token: not_authorized.
        for who in ["pat", "owner"] {
            client.bind(Some(who), Some(token()));
            assert_eq!(garbage(&mut client), ErrorCode::NotAuthorized, "{who}");
            assert_eq!(
                code_of(client.call::<StatusSet>(&status_params("x"))),
                ErrorCode::NotAuthorized,
                "{who}"
            );
        }
        // An agent with garbage params: invalid_request in fixed words.
        client.bind(Some("ada"), None);
        let org_proto::Outcome::Error(error) = client.ask(|id| Request {
            id,
            method: "status.set".to_owned(),
            params: serde_json::json!({"text": ["SECRET-STATUS"]}),
            principal: None,
        }) else {
            panic!("a refusal")
        };
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert!(!error.message.contains("SECRET"), "{}", error.message);
        assert!(log.lock().unwrap().is_empty(), "nothing reached the office");
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    // ---- live events on subscribe ----------------------------------------------------------

    /// A server whose mailbox publishes to `live`, which subscribers read.
    fn events_server(
        path: &Path,
        live: &Arc<LiveEvents>,
    ) -> (Arc<Mailbox>, Arc<AtomicBool>, std::thread::JoinHandle<()>) {
        let (mailbox, directory) = messaging_with(Arc::clone(live));
        let mut ports = ports(Box::new(|| Err(SnapshotFailure::Storage)));
        ports.mailbox = Arc::clone(&mailbox);
        ports.directory = directory;
        ports.live = Arc::clone(live);
        let timing = Timing {
            idle: Duration::from_millis(20),
            ..Timing::default()
        };
        let shared = Arc::new(Shared::new(&test_info(), path, ports, token()).with_timing(timing));
        let (stop, server) = start(path, &shared);
        (mailbox, stop, server)
    }

    /// The next event, skipping nothing: its name and data.
    fn next_event(client: &mut Client) -> (String, serde_json::Value) {
        let message = client.next_message();
        (
            message["event"].as_str().expect("an event").to_owned(),
            message["data"].clone(),
        )
    }

    #[test]
    fn an_app_subscriber_is_told_of_a_message_and_of_its_reading_after_it_subscribed() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let live = Arc::new(LiveEvents::new(Box::new(|| {})));
        let (_mailbox, stop, server) = events_server(&path, &live);
        let mut ada = Client::bound_as(&path, "ada");
        let mut remy = Client::bound_as(&path, "remy");
        // Sent BEFORE the app subscribed: never told.
        ada.call::<MsgSend>(&send_params(60, "remy", "earlier"));

        let mut app = Client::connect(&path);
        app.bind(None, Some(token()));
        assert!(matches!(app.subscribe(None), org_proto::Outcome::Result(_)));
        ada.call::<MsgSend>(&send_params(61, "remy", "now"));
        let (name, data) = next_event(&mut app);
        assert_eq!(name, "message.received");
        assert_eq!(data["header"]["id"], msg_id(61));
        assert_eq!(data["header"]["from_member_id"], ADA_ID);
        assert_eq!(data["header"]["to_member_id"], REMY_ID);
        assert!(
            !data.to_string().contains("now"),
            "no body in an event: {data}"
        );

        // Reading it is a state change.
        remy.call::<MsgRead>(&MsgReadParams {
            id: Some(msg_id(61)),
        });
        let (name, data) = next_event(&mut app);
        assert_eq!(name, "message.state_changed");
        assert_eq!(data["id"], msg_id(61));
        assert_eq!(data["state"], "read");
        assert!(
            app.nothing_within(Duration::from_millis(200)),
            "nothing else"
        );
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn a_rebind_ends_the_live_events_too_and_a_new_subscriber_starts_from_now() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let live = Arc::new(LiveEvents::new(Box::new(|| {})));
        let (_mailbox, stop, server) = events_server(&path, &live);
        let mut ada = Client::bound_as(&path, "ada");
        let mut app = Client::connect(&path);
        app.bind(None, Some(token()));
        app.subscribe(None);
        // A bind ends the subscription: nothing more arrives until it subscribes again.
        app.bind(None, Some(token()));
        ada.call::<MsgSend>(&send_params(62, "remy", "missed"));
        assert!(app.nothing_within(Duration::from_millis(300)));
        assert!(matches!(app.subscribe(None), org_proto::Outcome::Result(_)));
        assert!(
            app.nothing_within(Duration::from_millis(300)),
            "no replay of what was missed"
        );
        ada.call::<MsgSend>(&send_params(63, "remy", "seen"));
        let (name, data) = next_event(&mut app);
        assert_eq!(
            (name.as_str(), &data["header"]["id"]),
            ("message.received", &msg_id(63).into())
        );
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    /// A server whose followers sleep `idle` between looks unless woken, over `feed`, with `live`
    /// waking that feed on every publish (as the real office does).
    fn waking_server(
        path: &Path,
        live: &Arc<LiveEvents>,
        feed: &Arc<FakeFeed>,
        timing: Timing,
    ) -> (Arc<AtomicBool>, std::thread::JoinHandle<()>) {
        let (mailbox, directory) = messaging_with(Arc::clone(live));
        let mut ports = ports(Box::new(|| Err(SnapshotFailure::Storage)));
        ports.mailbox = mailbox;
        ports.directory = directory;
        ports.live = Arc::clone(live);
        ports.feed = Box::new(Arc::clone(feed));
        let shared = Arc::new(Shared::new(&test_info(), path, ports, token()).with_timing(timing));
        start(path, &shared)
    }

    #[test]
    fn a_published_event_wakes_a_follower_that_would_otherwise_sleep_its_whole_idle_time() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let feed = Arc::new(FakeFeed::default());
        let live = {
            let feed = Arc::clone(&feed);
            Arc::new(LiveEvents::new(Box::new(move || feed.wake())))
        };
        let timing = Timing {
            idle: Duration::from_secs(5),
            ..Timing::default()
        };
        let (stop, server) = waking_server(&path, &live, &feed, timing);
        let mut ada = Client::bound_as(&path, "ada");
        let mut app = Client::connect(&path);
        app.bind(None, Some(token()));
        app.subscribe(None);
        // Let the follower go to sleep on its 5 s idle, then publish.
        std::thread::sleep(Duration::from_millis(300));
        let started = std::time::Instant::now();
        ada.call::<MsgSend>(&send_params(64, "remy", "wake"));
        let (name, _) = next_event(&mut app);
        assert_eq!(name, "message.received");
        // Without the wake it would sleep out the rest of its 5 s idle.
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    fn presence(member: &str, status: &str) -> org_proto::directory::MemberPresenceChangedData {
        org_proto::directory::MemberPresenceChangedData {
            member_id: member.to_owned(),
            presence: org_proto::directory::MemberPresence::Offline,
            last_seen_at: "2026-11-09T12:00:00.000Z".to_owned(),
            status: Some(status.to_owned()),
            status_expires_at: None,
            current_task_id: None,
        }
    }

    #[test]
    fn presence_goes_at_once_then_at_most_once_a_gap_and_the_last_state_is_never_lost() {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let feed = Arc::new(FakeFeed::default());
        let live = {
            let feed = Arc::clone(&feed);
            Arc::new(LiveEvents::new(Box::new(move || feed.wake())))
        };
        let gap = Duration::from_millis(600);
        let timing = Timing {
            idle: Duration::from_millis(20),
            presence_gap: gap,
            ..Timing::default()
        };
        let (stop, server) = waking_server(&path, &live, &feed, timing);
        let mut app = Client::connect(&path);
        app.bind(None, Some(token()));
        app.subscribe(None);

        let started = std::time::Instant::now();
        live.publish_presence(presence("A", "one"));
        let (name, first) = next_event(&mut app);
        assert_eq!(
            (name.as_str(), first["status"].as_str()),
            ("member.presence_changed", Some("one"))
        );
        assert!(started.elapsed() < gap / 2, "the first goes at once");

        // Inside A's gap: A's two updates coalesce to the latest and wait; B is not held.
        live.publish_presence(presence("A", "two"));
        live.publish_presence(presence("A", "three"));
        live.publish_presence(presence("B", "other"));
        let (_, second) = next_event(&mut app);
        let at_second = started.elapsed();
        let (_, third) = next_event(&mut app);
        let at_third = started.elapsed();
        assert_eq!(
            (second["member_id"].as_str(), second["status"].as_str()),
            (Some("B"), Some("other"))
        );
        assert!(at_second < gap / 2, "B goes at once: {at_second:?}");
        assert_eq!(
            (third["member_id"].as_str(), third["status"].as_str()),
            (Some("A"), Some("three"))
        );
        assert!(
            at_third >= gap,
            "A's held one waited out the gap: {at_third:?}"
        );
        assert!(
            app.nothing_within(Duration::from_millis(300)),
            "nothing else"
        );
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn a_subscriber_that_fell_more_than_a_ring_behind_is_disconnected_to_list_again() {
        use std::io::Read;
        let live = LiveEvents::new(Box::new(|| {}));
        for n in 0..crate::live::CAPACITY + 5 {
            live.publish_live(crate::live::Live::StateChanged(
                org_proto::msg::MessageStateChangedData {
                    id: format!("M{n}"),
                    thread_id: "T".into(),
                    to_member_id: "X".into(),
                    state: org_proto::msg::MessageLifecycleState::Read,
                    triage_gen: 0,
                    snoozed_until: None,
                    snooze_after_task: false,
                    reason: None,
                    expired_waiting_person: false,
                    updated_at: "2026-11-09T12:00:00.000Z".into(),
                },
            ));
        }
        // A pusher that starts before the first of them: the earliest are already gone.
        let (server_end, mut client_end) = UnixStream::pair().unwrap();
        client_end
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let out = Mutex::new(server_end);
        let generation = AtomicU64::new(0);
        let seq = AtomicU64::new(0);
        let wire = follow::Wire {
            out: &out,
            generation: &generation,
            mine: 0,
            seq: &seq,
            timing: Timing::default(),
        };
        let feed = Arc::new(FakeFeed::default());
        follow::push(
            &feed,
            &live,
            &wire,
            follow::Start {
                position: org_proto::org::Position { epoch: 1, seq: 1 },
                live: crate::live::Head {
                    messages: 0,
                    presence: 0,
                },
            },
        );
        // It returned, having sent nothing and closed the connection.
        let mut rest = Vec::new();
        assert_eq!(client_end.read_to_end(&mut rest).unwrap(), 0);
        assert_eq!(seq.load(Ordering::SeqCst), 0);
    }

    // ---- the watcher: a member that stops being bindable loses its held calls at once ------

    /// A server for the watcher tests: the mailbox, the directory the test changes, the feed it
    /// wakes, the live events, and how to stop it.
    type UnbindServer = (
        Arc<Mailbox>,
        Arc<org_mailbox::testkit::InMemoryDirectory>,
        Arc<FakeFeed>,
        Arc<LiveEvents>,
        Arc<AtomicBool>,
        std::thread::JoinHandle<()>,
    );

    /// A server whose followers and watcher sleep `idle` unless woken: the test changes the
    /// directory under it, then wakes the feed as an organization change does.
    fn unbind_server(path: &Path) -> UnbindServer {
        let feed = Arc::new(FakeFeed::default());
        let live = {
            let feed = Arc::clone(&feed);
            Arc::new(LiveEvents::new(Box::new(move || feed.wake())))
        };
        let (mailbox, directory) = messaging_with(Arc::clone(&live));
        let mut ports = ports(Box::new(|| Err(SnapshotFailure::Storage)));
        ports.mailbox = Arc::clone(&mailbox);
        ports.directory = directory.clone();
        ports.live = Arc::clone(&live);
        ports.feed = Box::new(Arc::clone(&feed));
        let timing = Timing {
            idle: Duration::from_secs(3),
            ..Timing::default()
        };
        let shared = Arc::new(Shared::new(&test_info(), path, ports, token()).with_timing(timing));
        let (stop, server) = start(path, &shared);
        (mailbox, directory, feed, live, stop, server)
    }

    fn wait_params() -> MsgWaitParams {
        MsgWaitParams {
            timeout_ms: 30_000,
            from: None,
            urgent_only: false,
        }
    }

    #[test]
    fn a_held_wait_ends_not_bound_at_once_when_its_member_is_offboarded_made_office_run_or_removed()
    {
        use org_mailbox::testkit::fixture::{members, POSITION};
        use org_proto::{AgentStatus, RunMode};
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (mailbox, directory, feed, _live, stop, server) = unbind_server(&path);
        let ada = || members().into_iter().find(|m| m.id == ADA_ID).unwrap();
        for change in 0..3 {
            let mut idle_connection = Client::bound_as(&path, "ada");
            let held = idle_connection.call_later::<org_proto::msg::MsgWait>(&wait_params());
            // Another member's wait, which must stay.
            let mut remy = Client::bound_as(&path, "remy");
            remy.call_later::<org_proto::msg::MsgWait>(&wait_params());
            until("both waits are held", || mailbox.held_waits() >= 2);

            let mut changed = ada();
            match change {
                0 => changed.agent.as_mut().unwrap().status = AgentStatus::Offboarded,
                1 => changed.agent.as_mut().unwrap().run_mode = Some(RunMode::OnCall),
                _ => directory.remove(ADA_ID, POSITION),
            }
            if change < 2 {
                directory.upsert(changed, POSITION);
            }
            let started = std::time::Instant::now();
            // The organization changed: the idle connection sends nothing, and still hears.
            feed.push(11 + change as u64);
            let answer = idle_connection.answer();
            assert_eq!(answer.id, held, "change {change}");
            assert_eq!(
                code_of(answer.outcome),
                ErrorCode::NotBound,
                "change {change}"
            );
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "{:?}",
                started.elapsed()
            );
            // Remy's wait is still held.
            std::thread::sleep(Duration::from_millis(150));
            assert_eq!(mailbox.held_waits(), 1, "change {change}");
            drop(remy);
            until("remy's wait is gone with its connection", || {
                mailbox.held_waits() == 0
            });
            directory.upsert(ada(), POSITION);
        }
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn a_connection_that_bound_as_someone_else_since_is_left_alone() {
        use org_mailbox::testkit::fixture::{members, POSITION};
        use org_proto::AgentStatus;
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (mailbox, directory, feed, _live, stop, server) = unbind_server(&path);
        let mut client = Client::bound_as(&path, "ada");
        // It acts as remy now; ada is no longer its member.
        client.bind(Some("remy"), None);
        client.call_later::<org_proto::msg::MsgWait>(&wait_params());
        until("remy's wait is held", || mailbox.held_waits() == 1);
        let mut ada = members().into_iter().find(|m| m.id == ADA_ID).unwrap();
        ada.agent.as_mut().unwrap().status = AgentStatus::Offboarded;
        directory.upsert(ada, POSITION);
        feed.push(12);
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(mailbox.held_waits(), 1, "remy's wait was not touched");
        stop.store(true, Ordering::SeqCst);
        drop(client);
        server.join().unwrap();
    }

    #[test]
    fn a_subscription_ends_when_its_person_stops_being_bindable() {
        use org_mailbox::testkit::fixture::POSITION;
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let (_mailbox, directory, feed, live, stop, server) = unbind_server(&path);
        let mut ada = Client::bound_as(&path, "ada");
        // pat, bound with the token, follows.
        let mut pat = Client::connect(&path);
        assert!(matches!(
            pat.bind(Some("pat"), Some(token())),
            org_proto::Outcome::Result(_)
        ));
        assert!(matches!(pat.subscribe(None), org_proto::Outcome::Result(_)));
        ada.call::<MsgSend>(&send_params(70, "remy", "seen"));
        assert_eq!(next_event(&mut pat).0, "message.received");

        directory.remove(PAT_ID, POSITION);
        feed.push(13);
        std::thread::sleep(Duration::from_millis(300));
        // Whatever happens now is not pushed to pat.
        ada.call::<MsgSend>(&send_params(71, "remy", "unseen"));
        live.publish_presence(org_proto::directory::MemberPresenceChangedData {
            member_id: "X".into(),
            presence: org_proto::directory::MemberPresence::Offline,
            last_seen_at: "2026-11-09T12:00:00.000Z".into(),
            status: None,
            status_expires_at: None,
            current_task_id: None,
        });
        // The organization change that removed pat may still be pushed to it (it was committed
        // before the subscription ended); nothing of what happened after is.
        while let Some(message) = pat.message_within(Duration::from_millis(500)) {
            assert_eq!(message["event"], "org.changed", "{message}");
        }
        stop.store(true, Ordering::SeqCst);
        server.join().unwrap();
    }

    /// A directory that can be made to fail, over the testkit's in-memory one.
    struct FlakyDirectory {
        inner: Arc<org_mailbox::testkit::InMemoryDirectory>,
        failing: AtomicBool,
    }

    impl MemberDirectory for FlakyDirectory {
        fn resolve_member(
            &self,
            id_or_handle: &str,
        ) -> Result<Option<(org_proto::Member, org_proto::Position)>, org_mailbox::StoreError>
        {
            if self.failing.load(Ordering::SeqCst) {
                return Err(org_mailbox::StoreError::new("the directory is down"));
            }
            self.inner.resolve_member(id_or_handle)
        }
    }

    #[test]
    fn a_directory_failure_preserves_connections_but_restoring_a_revoked_member_does_not() {
        use org_mailbox::testkit::fixture::POSITION;
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let feed = Arc::new(FakeFeed::default());
        let live = {
            let feed = Arc::clone(&feed);
            Arc::new(LiveEvents::new(Box::new(move || feed.wake())))
        };
        let (mailbox, memory) = messaging_with(Arc::clone(&live));
        let flaky = Arc::new(FlakyDirectory {
            inner: Arc::clone(&memory),
            failing: AtomicBool::new(false),
        });
        let mut ports = ports(Box::new(|| Err(SnapshotFailure::Storage)));
        ports.mailbox = Arc::clone(&mailbox);
        ports.directory = flaky.clone();
        ports.live = Arc::clone(&live);
        ports.feed = Box::new(Arc::clone(&feed));
        let timing = Timing {
            idle: Duration::from_secs(3),
            ..Timing::default()
        };
        let shared =
            Arc::new(Shared::new(&test_info(), path.as_path(), ports, token()).with_timing(timing));
        let (stop, server) = start(&path, &shared);

        // The directory is down for a moment: nothing is ended.
        let mut pat = Client::connect(&path);
        pat.bind(Some("pat"), Some(token()));
        pat.subscribe(None);
        let mut ada = Client::bound_as(&path, "ada");
        let mut lookup_failure = Client::bound_as(&path, "remy");
        ada.call_later::<org_proto::msg::MsgWait>(&wait_params());
        until("a wait is held", || mailbox.held_waits() == 1);
        flaky.failing.store(true, Ordering::SeqCst);
        feed.push(21);
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(mailbox.held_waits(), 1, "a failed lookup ends nothing");
        assert_eq!(
            code_of(lookup_failure.call::<org_proto::directory::Who>(&Default::default())),
            ErrorCode::NotBound
        );
        assert!(matches!(
            lookup_failure.call::<Hello>(&org_proto::HelloParams::default()),
            org_proto::Outcome::Result(_)
        ));

        // Once the watcher revokes pat, restoring the member cannot revive that connection.
        flaky.failing.store(false, Ordering::SeqCst);
        assert!(matches!(
            lookup_failure.bind(Some("remy"), None),
            org_proto::Outcome::Result(_)
        ));
        let pat_member = memory
            .resolve_member(PAT_ID)
            .unwrap()
            .map(|(member, _)| member)
            .unwrap();
        memory.remove(PAT_ID, POSITION);
        feed.push(22);
        until("pat is revoked or already closed", || {
            let bound = shared.bound.lock().unwrap();
            match bound.values().find(|(member, _)| member == PAT_ID) {
                Some((_, control)) => control.revoked.load(Ordering::SeqCst),
                None => true,
            }
        });
        memory.upsert(pat_member, POSITION);
        // Changes already on the wire may precede EOF, but the old connection must close.
        while let Some(message) = frame::read_message::<serde_json::Value>(&mut pat.reader).unwrap()
        {
            assert_eq!(message["event"], "org.changed");
        }
        assert!(
            shared
                .bound
                .lock()
                .unwrap()
                .values()
                .all(|(member, _)| member != PAT_ID),
            "the revoked connection must release its registration"
        );
        // The restored person can bind and subscribe on a fresh connection.
        let mut restored = Client::bound_as(&path, "pat");
        assert!(matches!(
            restored.subscribe(None),
            org_proto::Outcome::Result(_)
        ));
        stop.store(true, Ordering::SeqCst);
        drop((pat, ada));
        server.join().unwrap();
    }

    #[test]
    fn a_pass_that_could_not_decide_is_repeated_on_a_quiet_log() {
        use org_mailbox::testkit::fixture::POSITION;
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let feed = Arc::new(FakeFeed::default());
        let live = {
            let feed = Arc::clone(&feed);
            Arc::new(LiveEvents::new(Box::new(move || feed.wake())))
        };
        let (mailbox, memory) = messaging_with(Arc::clone(&live));
        let flaky = Arc::new(FlakyDirectory {
            inner: Arc::clone(&memory),
            failing: AtomicBool::new(false),
        });
        let mut ports = ports(Box::new(|| Err(SnapshotFailure::Storage)));
        ports.mailbox = Arc::clone(&mailbox);
        ports.directory = flaky.clone();
        ports.live = Arc::clone(&live);
        ports.feed = Box::new(Arc::clone(&feed));
        // A short idle: the retry comes without any commit.
        let timing = Timing {
            idle: Duration::from_millis(100),
            ..Timing::default()
        };
        let shared =
            Arc::new(Shared::new(&test_info(), path.as_path(), ports, token()).with_timing(timing));
        let (stop, server) = start(&path, &shared);

        let mut ada = Client::bound_as(&path, "ada");
        let held = ada.call_later::<org_proto::msg::MsgWait>(&wait_params());
        until("a wait is held", || mailbox.held_waits() == 1);
        // Ada is removed in the same breath as the directory fails: the one pass for this commit
        // cannot decide.
        memory.remove(ADA_ID, POSITION);
        flaky.failing.store(true, Ordering::SeqCst);
        feed.push(31);
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(
            mailbox.held_waits(),
            1,
            "nothing is ended while it cannot tell"
        );
        // The directory is back; no new commit. The watcher looks again by itself.
        flaky.failing.store(false, Ordering::SeqCst);
        let answer = ada.answer();
        assert_eq!(answer.id, held);
        assert_eq!(code_of(answer.outcome), ErrorCode::NotBound);
        stop.store(true, Ordering::SeqCst);
        drop(ada);
        server.join().unwrap();
    }
}
