//! The office's socket: JSON lines (`org_proto::frame`), one thread per connection. It answers
//! `hello`, `org.snapshot`, `msg.bind` (app-wide only) and the organization commands, which
//! only the app's connection may send; every other method is `unknown_method`. When the server
//! stops, every connection is closed and its thread joined BEFORE the office is, so no thread
//! touches a database that is closing.

use std::collections::HashMap;
use std::io::BufReader;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use org_office::rules::OrgCommand;
use org_proto::frame;
use org_proto::msg::{MsgBind, MsgBindParams, MsgBindResult, MAX_HANDLE_BYTES, OFFICE_SENDER};
use org_proto::org::{CommandAccepted, TeamCreate};
use org_proto::{
    ApiError, AppToken, ErrorCode, Hello, HelloResult, OfficeHello, OfficeInfo, OfficeRole,
    OfficeStatus, OrgSnapshot, OrgSnapshotCall, Request, Response, API_VERSION,
};

/// Reads the organization's snapshot (the office's `org_snapshot`), or says why it cannot.
pub type SnapshotFn = Box<dyn Fn() -> Result<OrgSnapshot, String> + Send + Sync>;

/// Runs one organization command (the office's `submit_org`): HQ's answer, or why it could not
/// answer at all (storage).
pub type SubmitFn =
    Box<dyn Fn(OrgCommand) -> Result<Result<CommandAccepted, ApiError>, String> + Send + Sync>;

/// What the connections ask of the office.
pub struct OfficePorts {
    pub snapshot: SnapshotFn,
    pub submit: SubmitFn,
}

/// Who a connection is, as far as the socket knows. Only the app is told apart so far: a client
/// that has not bound with the office's `app_token` is anonymous.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Binding {
    Anonymous,
    App,
}

impl Binding {
    /// Whether the connection is bound with the `app_token` (the app, or later a person or the
    /// owner bound with it): the only kind that may send organization commands.
    fn is_app_bound(self) -> bool {
        self == Binding::App
    }

    /// Whether the connection is bound at all.
    fn is_bound(self) -> bool {
        self != Binding::Anonymous
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
}

impl Shared {
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
            hello: HelloResult {
                api_version: API_VERSION,
                server_version: env!("CARGO_PKG_VERSION").to_owned(),
                host: "This Mac".to_owned(),
                // Part 1 implements only what every server of this api_version has.
                capabilities: Vec::new(),
                office: Some(OfficeHello {
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
                }),
            },
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
    let Ok(mut out) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    let mut binding = Binding::Anonymous;
    // A line the framing refuses (too long, not JSON) ends the connection: the stream is out of
    // step and nothing in it can be trusted.
    while let Ok(Some(request)) = frame::read_message::<Request>(&mut reader) {
        let response = answer(&request, shared, &mut binding);
        match frame::write_message(&mut out, &response) {
            Ok(()) => {}
            // The answer does not fit in a frame (nothing was written): say so, and carry on.
            Err(frame::FrameError::TooLargeToSend) => {
                let error = Response::error(
                    request.id,
                    ApiError::new(ErrorCode::StorageError, "the answer is too large to send"),
                );
                if frame::write_message(&mut out, &error).is_err() {
                    return;
                }
            }
            Err(_) => return,
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
    if request.is::<Hello>() {
        return Response::ok::<Hello>(request.id, &shared.hello)
            .unwrap_or_else(|_| internal(request.id));
    }
    if request.is::<MsgBind>() {
        return bind_connection(request, shared, binding);
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

/// `msg.bind`, app-wide only for now: with the office's `app_token` and no member. Binding as a
/// member is messaging's and arrives with it; until then every member bind fails closed
/// (`not_authorized`, the contract's answer to a lookup that fails) after the contract's
/// input-only checks. Each call replaces the previous binding, and a
/// failed one leaves the connection unbound.
fn bind_connection(request: &Request, shared: &Shared, binding: &mut Binding) -> Response {
    *binding = Binding::Anonymous;
    let params = match request.params::<MsgBind>() {
        Ok(params) => params,
        Err(_) => return bind_error(request.id, ErrorCode::InvalidRequest, "bad msg.bind params"),
    };
    let MsgBindParams { member, app_token } = params;
    // The contract's step 1, which needs no member lookup (`org_proto::msg::MsgBind`).
    if let Some(member) = &member {
        if member.len() > MAX_HANDLE_BYTES {
            return bind_error(request.id, ErrorCode::TooLarge, "the member is too long");
        }
        if member.eq_ignore_ascii_case(OFFICE_SENDER) {
            return bind_error(
                request.id,
                ErrorCode::InvalidRequest,
                "a connection cannot bind as the office",
            );
        }
        // Step 2: this office cannot resolve members yet (messaging's), which the contract
        // treats as a failed lookup: it fails closed, whatever else the call sends.
        return bind_error(
            request.id,
            ErrorCode::NotAuthorized,
            "this office cannot bind this member",
        );
    }
    let Some(token) = app_token else {
        return bind_error(
            request.id,
            ErrorCode::InvalidRequest,
            "bind with an app_token",
        );
    };
    if token != shared.app_token {
        return bind_error(request.id, ErrorCode::NotAuthorized, "wrong app token");
    }
    *binding = Binding::App;
    let result = MsgBindResult {
        member_id: None,
        office_id: shared.office_id.clone(),
    };
    Response::ok::<MsgBind>(request.id, &result).unwrap_or_else(|_| internal(request.id))
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
            office_id: "OFFICE1".into(),
            office_name: "HQ".into(),
            role: org_office::OfficeRole::Hq,
            org_id: "ORG1".into(),
            org_name: "Acme".into(),
            created_at: "2026-11-08T12:00:00.000Z".into(),
        }
    }

    fn ports(snapshot: SnapshotFn) -> OfficePorts {
        OfficePorts {
            snapshot,
            submit: Box::new(|_| Err("none".to_owned())),
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
        let shared = Arc::new(Shared::new(
            &test_info(),
            path,
            OfficePorts {
                snapshot: Box::new(|| Err("none".to_owned())),
                submit: Box::new(move |_| {
                    counter.fetch_add(1, Ordering::SeqCst);
                    answer()
                }),
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
        assert_eq!(value["office_id"], "OFFICE1");
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
            code_of(client.bind(Some("owner"), Some(token()))),
            ErrorCode::NotAuthorized
        );
        // The input-only checks come first: the office is never a member, and a member over
        // the limit is too large, whatever else the call sends.
        for office in ["office", "OFFICE", "Office"] {
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
        let longest = "x".repeat(MAX_HANDLE_BYTES);
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
}
