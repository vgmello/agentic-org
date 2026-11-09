//! The office's socket: JSON lines (`org_proto::frame`), one thread per connection. It answers
//! `hello`, `org.snapshot`, `msg.bind`, `who`, `msg.send` / `msg.inbox` / `msg.read` /
//! `msg.wait`, `subscribe` and the organization commands, which only an app-bound connection may
//! send; every other method is `unknown_method`. A `msg.wait` is held on a thread of its own
//! (at most [`MAX_HELD`] per connection), so the connection keeps answering meanwhile. When the
//! server stops, every connection is closed and its thread joined BEFORE the office is, so no thread
//! touches a database that is closing.

use std::collections::HashMap;
use std::io::BufReader;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::bounded;
use crate::follow::{self, OrgFeed, Timing};
use crate::msg_calls;
use org_mailbox::{cancellation_pair, CancelHandle, Mailbox, MemberDirectory};
use org_office::rules::OrgCommand;
use org_proto::directory::{Who, WhoParams, WhoResult};
use org_proto::frame;
use org_proto::msg::{
    json_string_len, MsgBind, MsgBindParams, MsgBindResult, MsgWait, MAX_HANDLE_BYTES,
    OFFICE_SENDER,
};
use org_proto::org::{CommandAccepted, TeamCreate};
use org_proto::{AgentStatus, MemberKind};
use org_proto::{
    ApiError, AppToken, ErrorCode, Hello, HelloResult, OfficeHello, OfficeInfo, OfficeRole,
    OfficeStatus, OrgSnapshot, OrgSnapshotCall, Request, Response,
};

/// Reads the organization's snapshot (the office's `org_snapshot`), or says why it cannot.
pub type SnapshotFn = Box<dyn Fn() -> Result<OrgSnapshot, String> + Send + Sync>;

/// Runs one organization command (the office's `submit_org`): HQ's answer, or why it could not
/// answer at all (storage).
pub type SubmitFn =
    Box<dyn Fn(OrgCommand) -> Result<Result<CommandAccepted, ApiError>, String> + Send + Sync>;

/// Answers `who` (the office's `who`): the cards, the caller's mistake, or why it could not
/// answer at all (storage).
pub type WhoFn =
    Box<dyn Fn(&WhoParams) -> Result<Result<WhoResult, ApiError>, String> + Send + Sync>;

/// What the connections ask of the office.
pub struct OfficePorts {
    pub snapshot: SnapshotFn,
    pub submit: SubmitFn,
    pub who: WhoFn,
    /// The log, for `subscribe`.
    pub feed: Box<dyn OrgFeed>,
    /// Messaging: what `msg.send`, `msg.inbox`, `msg.read` and `msg.wait` run on.
    pub mailbox: Arc<Mailbox>,
    /// Who a `msg.bind` names.
    pub directory: Arc<dyn MemberDirectory + Send + Sync>,
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
    /// The app's token, given to the server on the launch pipe. Zeroed when `Shared` drops.
    app_token: AppToken,
    timing: Timing,
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
        app_token: AppToken,
    ) -> Self {
        Self {
            office_id: info.office_id.clone(),
            ports,
            app_token,
            timing: Timing::default(),
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
                        serve_connection(stream, &shared);
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
    // Stop: close every connection (which ends its blocked read), then wait for the threads.
    for stream in open.lock().unwrap_or_else(|p| p.into_inner()).values() {
        let _ = stream.shutdown(std::net::Shutdown::Both);
    }
    for thread in threads {
        let _ = thread.join();
    }
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

fn serve_connection(stream: UnixStream, shared: &Shared) {
    let Ok(out) = stream.try_clone() else {
        return;
    };
    // One writer at a time: answers and pushed events never interleave. Every frame has a
    // deadline (`bounded`), so a client that does not read cannot hold the thread.
    let out = Mutex::new(out);
    // Moves on whenever the connection's binding changes or the reader ends, which stops the
    // events of the binding before it.
    let generation = AtomicU64::new(0);
    // The events sent on this connection, whichever subscription sent them.
    let sent = AtomicU64::new(0);
    // The held `msg.wait` calls: each has a thread of its own, joined when the connection ends.
    let held = Held::default();
    // The subscription's thread is joined when the connection ends: it reads the office, which
    // must not be closing.
    std::thread::scope(|scope| {
        let _end_held_calls = CancelOnDrop(&held);
        let mut reader = BufReader::new(stream);
        let mut binding = Binding::Anonymous;
        let mut subscribed = false;
        // The last subscription's thread: a new one waits for it, so a client that binds and
        // subscribes in a loop never has more than two.
        let mut pusher: Option<std::thread::ScopedJoinHandle<'_, ()>> = None;
        // A line the framing refuses (too long, not JSON) ends the connection: the stream is out
        // of step and nothing in it can be trusted.
        while let Ok(Some(request)) = frame::read_message::<Request>(&mut reader) {
            // The member this connection acts as is checked on every call, not only at bind.
            if let Some(member) = binding.member() {
                if !request.is::<MsgBind>() && !still_bindable(shared, member) {
                    binding = Binding::Anonymous;
                    generation.fetch_add(1, Ordering::SeqCst);
                    subscribed = false;
                    held.cancel_all();
                }
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
                    Ok(start) => {
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
                        let mine = generation.load(Ordering::SeqCst);
                        let (feed, out, generation, sent) =
                            (&*shared.ports.feed, &out, &generation, &sent);
                        let timing = shared.timing;
                        pusher = Some(scope.spawn(move || {
                            follow::push(feed, out, generation, mine, start, sent, timing)
                        }));
                        continue;
                    }
                    Err(refusal) => Response::error(request.id, refusal),
                }
            } else {
                answer(&request, shared, &mut binding)
            };
            if !send(&out, request.id, &response, shared.timing) {
                break;
            }
        }
        generation.fetch_add(1, Ordering::SeqCst);
    });
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

fn answer(request: &Request, shared: &Shared, binding: &mut Binding) -> Response {
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
            Err(_) => internal(request.id),
        };
    }
    if request.is::<Who>() {
        return who(request, shared, binding);
    }
    if request.is::<Hello>() {
        return Response::ok::<Hello>(request.id, &shared.hello)
            .unwrap_or_else(|_| internal(request.id));
    }
    if request.is::<MsgBind>() {
        return bind_connection(request, shared, binding);
    }
    if let Some(response) = msg_calls::answer(request, &shared.ports.mailbox, binding.member()) {
        return response;
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

/// `msg.bind`, in the contract's order (`org_proto::msg::MsgBind`): the input alone, then the
/// member, then the credentials; it fails closed. Each call replaces the previous binding, and a
/// failed one leaves the connection unbound.
fn bind_connection(request: &Request, shared: &Shared, binding: &mut Binding) -> Response {
    *binding = Binding::Anonymous;
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
    if token != shared.app_token {
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
            Some(token) if token == shared.app_token => {}
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
    let (found, _) = shared.ports.directory.resolve_member(handle).ok()??;
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
    may.then_some(found)
}

/// Whether a connection bound as member `id` may still act as it. The bind's rules are not
/// checked once: an agent the owner has since made office-run, offboarded or removed must bind
/// again (and then fails the same rules).
fn still_bindable(shared: &Shared, id: &str) -> bool {
    bindable(shared, id).is_some_and(|member| member.id == id)
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
    use std::sync::atomic::AtomicBool;

    use super::*;

    fn shared(path: &Path) -> Arc<Shared> {
        Arc::new(Shared::new(
            &test_info(),
            path,
            ports(Box::new(|| Err("none".to_owned()))),
            token(),
        ))
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
            snapshot,
            submit: Box::new(|_| Err("none".to_owned())),
            who: Box::new(|_| Err("none".to_owned())),
            feed: Box::new(Arc::new(FakeFeed::default())),
            mailbox,
            directory,
        }
    }

    /// A mailbox and its directory over the testkit's in-memory store and the fixture members
    /// (ada and remy are agents, pat a person, plus the owner), at the fixture's time.
    fn messaging() -> (Arc<Mailbox>, Arc<org_mailbox::testkit::InMemoryDirectory>) {
        use org_mailbox::testkit::fixture::{
            members, ManualClock, INCARNATION, NOW_MS, OFFICE_ID, POSITION,
        };
        use org_mailbox::testkit::{InMemoryDirectory, InMemoryStore};
        use org_mailbox::{NoMessageEvents, OfficeIdentity};
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
            Arc::new(NoMessageEvents),
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

        fn push_with(&self, seq: u64, command_id: String) {
            self.log
                .lock()
                .unwrap()
                .push(org_proto::org::OrgChangedData {
                    position: org_proto::org::Position { epoch: 1, seq },
                    command_id,
                    changes: Vec::new(),
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
        let request = Request::new::<Hello>(1, &org_proto::Empty {}).unwrap();
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
                Err("none".to_owned())
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
        let hello = Request::new::<Hello>(2, &org_proto::Empty {}).unwrap();
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
                snapshot: Box::new(|| Err("none".to_owned())),
                submit: Box::new(move |_| {
                    counter.fetch_add(1, Ordering::SeqCst);
                    answer()
                }),
                who: Box::new(|_| Err("none".to_owned())),
                feed: Box::new(Arc::new(FakeFeed::default())),
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
        };
        assert_eq!(code_of(client.ask(garbled)), ErrorCode::NotAuthorized);
        // A method this office does not know is still just unknown.
        let unknown = |id| Request {
            id,
            method: "team.fly".to_owned(),
            params: serde_json::json!({}),
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
        };
        let unknown = |id| Request {
            id,
            method: "team.fly".to_owned(),
            params: serde_json::json!({}),
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
            client.ask(|id| Request::new::<Hello>(id, &org_proto::Empty {}).unwrap()),
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
                    snapshot: Box::new(|| Err("none".to_owned())),
                    submit: Box::new(|_| Err("none".to_owned())),
                    who: Box::new(|_| Err("none".to_owned())),
                    feed: Box::new(Arc::clone(&feed)),
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
            client.ask(|id| Request::new::<Hello>(id, &org_proto::Empty {}).unwrap()),
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
                snapshot: Box::new(|| Err("none".to_owned())),
                submit: Box::new(|_| Err("none".to_owned())),
                who: Box::new(move |params| {
                    counter.fetch_add(1, Ordering::SeqCst);
                    answer(params)
                }),
                feed: Box::new(Arc::new(FakeFeed::default())),
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
        let mut ports = ports(Box::new(|| Err("none".to_owned())));
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
    fn a_connection_is_unbound_when_its_member_stops_being_bindable() {
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

        // Made office-run after it bound: every call is `not_bound`, `who` and the snapshot
        // included, and its held wait ends.
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
            let inbox = client.call_later::<MsgInbox>(&inbox_params());
            // Three answers, in whatever order: all `not_bound`, the held wait's included.
            let mut got = std::collections::BTreeMap::new();
            for _ in 0..3 {
                let answer = client.answer();
                got.insert(answer.id, code_of(answer.outcome));
            }
            assert_eq!(
                got,
                [
                    (held, ErrorCode::NotBound),
                    (who, ErrorCode::NotBound),
                    (inbox, ErrorCode::NotBound)
                ]
                .into_iter()
                .collect(),
                "change {change}"
            );
            // It stays unbound until it binds again, which fails the same rules.
            assert_eq!(
                code_of(client.bind(Some("ada"), None)),
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
}
