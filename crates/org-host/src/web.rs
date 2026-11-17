//! Host-owned browser process. Its private control authority is constructed only
//! after suspended-image, actual-reader, sandbox and descriptor-receipt checks.
use crate::{
    cloexec::SPAWN_LOCK,
    home,
    server_check::ServerCheck,
    service::listener::{Handler, WebConnection},
    spawn::{self, Process},
    sync::lock,
};
use org_host_control::{
    gateway::{
        GatewayFd, GatewayFirstLine, GatewayHandoff, GatewayReadyForSecrets, GATEWAY_HANDOFF_FDS,
    },
    listen::{ListenConfig, ListenMode},
    GatewayState,
};
use std::{
    ffi::OsStr,
    fs,
    io::{self, Read, Write},
    net::TcpListener,
    os::{
        fd::{AsFd, AsRawFd},
        unix::{
            fs::{DirBuilderExt, PermissionsExt},
            net::{UnixDatagram, UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(5);
/// Consecutive failed starts before the supervisor gives up.
const MAX_STARTS: usize = 5;
const POLL: Duration = Duration::from_millis(50);
/// How long a started web process may take to report `web.serving` (GW-01 r4).
const SERVING_DEADLINE: Duration = Duration::from_secs(10);
/// A web process that stayed `Running` this long was a healthy start: its later
/// exit begins a fresh run of consecutive failures instead of adding to one.
const STABLE_RUNNING: Duration = Duration::from_secs(60);
/// First restart delay; each further consecutive failure doubles it.
const RESTART_BACKOFF: Duration = Duration::from_secs(1);

/// Supervisor timing. Production uses [`Timing::PRODUCTION`]; tests shorten it.
#[derive(Clone, Copy)]
struct Timing {
    serving: Duration,
    stable: Duration,
    backoff: Duration,
}
impl Timing {
    const PRODUCTION: Self = Self {
        serving: SERVING_DEADLINE,
        stable: STABLE_RUNNING,
        backoff: RESTART_BACKOFF,
    };
}
fn unavailable() -> io::Error {
    io::Error::other("authenticated browser service unavailable")
}
fn invalid<T>(_: T) -> io::Error {
    unavailable()
}
/// The launch check refused the web process at `step`. Its cause names only the check
/// and the executable's identity, so it is kept for the supervisor's log line.
fn refused(step: &'static str) -> impl FnOnce(String) -> io::Error {
    move |why| io::Error::other(format!("web process admission refused at {step}: {why}"))
}
/// A failed start step with a fixed, secret-free cause for the supervisor's log line.
fn failed(cause: &str) -> io::Error {
    io::Error::other(format!("web process start failed: {cause}"))
}
/// A failed start step caused by an OS error. Only the error's kind is kept, never
/// its text, so the cause stays fixed and secret-free.
fn os_failed(cause: &'static str) -> impl FnOnce(io::Error) -> io::Error {
    move |error| failed(&format!("{cause} ({:?})", error.kind()))
}

pub(crate) struct Configuration {
    pub program: PathBuf,
    pub instance: String,
    pub runtime: PathBuf,
    pub listen: ListenConfig,
    pub check: ServerCheck,
    pub critical_paths: Vec<PathBuf>,
    pub release: Option<org_host_control::gateway::WebRelease>,
}
/// The selected release's web process image hash (`Err`: the release has none) and the
/// release identity bound into the web process's first line.
pub(crate) type SelectedRelease<E> = (Result<[u8; 20], E>, org_host_control::gateway::WebRelease);

/// The web process admission policy of a build (`debug`: `cfg!(debug_assertions)`) with app
/// team id `team_id` and the selected `release`, if any: the check every start uses and
/// the release binding of its first line. A malformed team id or a release without a
/// web process image hash refuses every start; a selected release pins its exact image.
pub(crate) fn admission<E>(
    debug: bool,
    team_id: Option<&str>,
    release: Option<SelectedRelease<E>>,
) -> (ServerCheck, Option<org_host_control::gateway::WebRelease>) {
    let check = ServerCheck::gateway_for_build(debug, team_id)
        .unwrap_or(ServerCheck::WebRefused("malformed app team id"));
    match release {
        None => (check, None),
        Some((Ok(hash), binding)) => (check.pinned(hash), Some(binding)),
        Some((Err(_), binding)) => (
            ServerCheck::WebRefused("the selected release has no agentic-org-web image hash"),
            Some(binding),
        ),
    }
}

pub(crate) struct Supervisor {
    stop: Arc<AtomicBool>,
    state: Arc<Mutex<GatewayState>>,
    thread: Option<JoinHandle<()>>,
}
impl Supervisor {
    pub(crate) fn start(
        config: Configuration,
        handler: Arc<dyn Handler>,
        state: Arc<Mutex<GatewayState>>,
        eligible: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> io::Result<Self> {
        // No bind, child or secret handoff for unverified TLS or a platform without a
        // web process confinement adapter.
        // Each refusal names its cause for the host log.
        org_socket::confinement::web_confinement_supported()?;
        if config.listen.scheme() != ListenMode::HttpLoopback {
            return Err(io::Error::other(
                "the web process requires a verified HTTP loopback listener",
            ));
        }
        config
            .check
            .may_spawn()
            .map_err(|_| io::Error::other(config.check.describe()))?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let observing = Arc::clone(&state);
        let thread = thread::Builder::new()
            .name("org-browser-supervisor".into())
            .spawn(move || {
                supervise(
                    &stopping,
                    &observing,
                    &*eligible,
                    Timing::PRODUCTION,
                    &|line| crate::host_log::log(line),
                    || Running::start(&config, Arc::clone(&handler)),
                )
            })
            .map_err(os_failed("start the supervisor thread"))?;
        Ok(Self {
            stop,
            state,
            thread: Some(thread),
        })
    }
}
impl Drop for Supervisor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
        *lock(&self.state) = GatewayState::Failed;
    }
}

/// A started web process the supervisor watches.
trait Serving {
    /// Whether the control connection is open and the child has not exited.
    fn serving(&mut self) -> bool;
    /// Whether the admitted web process reported, in order, that its HTTP server serves.
    fn reported(&mut self) -> bool;
    /// Why it stopped serving: fixed, secret-free text for the host log.
    fn ended(&mut self) -> String;
}

/// Starts the web process once `eligible`. It stays `Starting` until the admitted
/// web process reports `web.serving` within `deadline`, then publishes `Running`
/// only while that web process serves. A missing report, an exit or control EOF
/// publishes `Failed` and stops/reaps the child, before any bounded restart.
/// Only consecutive failed starts count toward [`MAX_STARTS`]: a web process that
/// stayed `Running` for `timing.stable` resets the count. No backoff follows
/// the final failed start. Every failed start and every exit is passed to `log`
/// with its cause, escaped, together with the restart it leads to.
fn supervise<S: Serving>(
    stopping: &AtomicBool,
    state: &Mutex<GatewayState>,
    eligible: &dyn Fn() -> bool,
    timing: Timing,
    log: &dyn Fn(&str),
    mut start: impl FnMut() -> io::Result<S>,
) {
    let mut failures = 0usize;
    loop {
        // First protected bootstrap starts the host before HQ exists. Wait without
        // binding or spending restart attempts; the same controller observes actual
        // completed provisioning.
        while !stopping.load(Ordering::SeqCst) && !eligible() {
            *lock(state) = GatewayState::Starting;
            thread::park_timeout(POLL);
        }
        if stopping.load(Ordering::SeqCst) {
            break;
        }
        *lock(state) = GatewayState::Starting;
        let mut stable = false;
        let reason = match start() {
            Ok(mut child) => {
                // HTTP initialization is still pending until the child reports it.
                let until = Instant::now() + timing.serving;
                while !stopping.load(Ordering::SeqCst)
                    && child.serving()
                    && !child.reported()
                    && Instant::now() < until
                {
                    thread::park_timeout(POLL);
                }
                let reason = if stopping.load(Ordering::SeqCst) {
                    String::new()
                } else if child.serving() && child.reported() {
                    *lock(state) = GatewayState::Running;
                    let running = Instant::now();
                    while !stopping.load(Ordering::SeqCst) && child.serving() {
                        thread::park_timeout(POLL);
                    }
                    stable = running.elapsed() >= timing.stable;
                    format!("the running web process stopped: {}", child.ended())
                } else if child.serving() {
                    "it did not report web.serving before the deadline".to_owned()
                } else {
                    format!("it stopped before serving: {}", child.ended())
                };
                *lock(state) = GatewayState::Failed;
                drop(child); // end sessions/control, then kill/reap before restart
                reason
            }
            Err(error) => error.to_string(),
        };
        *lock(state) = GatewayState::Failed;
        if stopping.load(Ordering::SeqCst) {
            break;
        }
        let reason = crate::host_log::untrusted(&reason);
        failures = if stable { 0 } else { failures + 1 };
        if failures >= MAX_STARTS {
            log(&format!(
                "authenticated browser restart limit reached after {failures} failed starts; last: {reason}"
            ));
            break;
        }
        // 1x after a stable run or the first failure, then 2x, 4x, 8x.
        let delay = timing.backoff * (1 << failures.saturating_sub(1));
        log(&format!(
            "authenticated browser restarting in {delay:?} because {reason}"
        ));
        let retry = Instant::now() + delay;
        while !stopping.load(Ordering::SeqCst) && Instant::now() < retry {
            thread::park_timeout(POLL);
        }
    }
    *lock(state) = GatewayState::Failed;
}

struct Rendezvous {
    listener: UnixListener,
    path: PathBuf,
}
impl Rendezvous {
    fn bind(runtime: &Path) -> io::Result<Self> {
        let directory = runtime.join("r");
        match fs::DirBuilder::new().mode(0o700).create(&directory) {
            Ok(()) => (),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e),
        }
        home::owner_only_dir(&directory).map_err(invalid)?;
        let path = directory.join(crate::secrets::generate_unique_name()?);
        org_proto::location::check_socket_path(&path).map_err(invalid)?;
        let listener = {
            let _fence = lock(&SPAWN_LOCK);
            UnixListener::bind(&path)?
        };
        let bound = Self { listener, path };
        fs::set_permissions(&bound.path, fs::Permissions::from_mode(0o600))?;
        bound.listener.set_nonblocking(true)?;
        Ok(bound)
    }
    fn accept(&self, process: &mut Process, until: Instant) -> io::Result<UnixStream> {
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    stream
                        .set_nonblocking(false)
                        .map_err(os_failed("make the control stream blocking"))?;
                    return Ok(stream);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock && Instant::now() < until => {
                    if process
                        .try_wait()
                        .map_err(os_failed("wait for the child"))?
                    {
                        return Err(failed(&format!(
                            "the child exited before connecting ({})",
                            exited(process)
                        )));
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    return Err(failed("the child did not connect before the deadline"))
                }
                Err(e) => return Err(os_failed("accept the rendezvous")(e)),
            }
        }
    }
}
/// How the reaped child ended, for a log line.
fn exited(process: &Process) -> String {
    process
        .ended()
        .unwrap_or_else(|| "status unknown".to_owned())
}
impl Drop for Rendezvous {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

struct Running {
    // Drop order: close control/revoke first, then kill/reap the child.
    control: WebConnection,
    process: Process,
    _stdin: fs::File,
}
impl Serving for Running {
    fn serving(&mut self) -> bool {
        self.control.live() && matches!(self.process.try_wait(), Ok(false))
    }
    fn reported(&mut self) -> bool {
        self.control.reported_serving()
    }
    fn ended(&mut self) -> String {
        let refused = self.control.serving_refused();
        let exit = matches!(self.process.try_wait(), Ok(true)).then(|| exited(&self.process));
        ended_cause(refused, exit, self.control.live())
    }
}
/// Why a running web process ended, pure: a refused `web.serving` wins over the EOF it
/// causes, then a reaped child's exit status (`exit`), then the control EOF.
fn ended_cause(serving_refused: bool, exit: Option<String>, live: bool) -> String {
    if serving_refused {
        "the host refused its web.serving as a protocol error".to_owned()
    } else if let Some(status) = exit {
        format!("the child exited ({status})")
    } else if !live {
        "its control connection reached EOF".to_owned()
    } else {
        "it stopped serving".to_owned()
    }
}
impl Running {
    fn start(config: &Configuration, handler: Arc<dyn Handler>) -> io::Result<Self> {
        config
            .check
            .may_spawn()
            .map_err(refused("the pre-spawn check"))?;
        // Validated configuration is frozen before bind. No port-zero substitution.
        let listener = {
            let _fence = lock(&SPAWN_LOCK);
            TcpListener::bind((config.listen.address(), config.listen.port()))
                .map_err(os_failed("bind the browser listener"))?
        };
        let bound = listener
            .local_addr()
            .map_err(os_failed("read the browser listener address"))?;
        if bound.port() != config.listen.port() {
            return Err(failed("the browser listener is not on its configured port"));
        }
        let rendezvous =
            Rendezvous::bind(&config.runtime).map_err(os_failed("bind the rendezvous"))?;
        let mut spawned = {
            let _fence = lock(&SPAWN_LOCK);
            spawn::spawn_suspended(&config.program, &[OsStr::new("--serve")], &[])
                .map_err(os_failed("spawn the web process suspended"))?
        };
        let pid = spawned.process.id();
        let admitted = config.check.admit(pid).map_err(refused("admission"))?;
        config
            .check
            .still(pid, admitted)
            .map_err(refused("the pre-resume re-check"))?;
        spawned
            .process
            .resume()
            .map_err(os_failed("resume the web process"))?;
        let first = GatewayFirstLine {
            rendezvous: rendezvous
                .path
                .to_str()
                .ok_or_else(|| failed("the rendezvous path is not UTF-8"))?
                .into(),
            instance_id: config.instance.clone(),
            release: config.release.clone(),
        };
        let mut first = serde_json::to_vec(&first).map_err(|_| failed("encode the first line"))?;
        if first.len() >= 1024 {
            return Err(failed("the first line exceeds 1023 bytes"));
        }
        first.push(b'\n');
        spawned
            .stdin
            .write_all(&first)
            .map_err(os_failed("write the first line"))?;
        // Drain bounded chunks without retaining any child output in logs.
        let mut stdout = spawned.stdout;
        thread::Builder::new()
            .name("web-stdout".into())
            .spawn(move || {
                let _ = io::copy(&mut stdout, &mut io::sink());
            })
            .map_err(os_failed("start the stdout drain"))?;
        let mut stderr = spawned.stderr;
        thread::Builder::new()
            .name("web-stderr".into())
            .spawn(move || {
                let mut bytes = [0u8; 4096];
                while matches!(stderr.read(&mut bytes), Ok(n) if n > 0) {}
            })
            .map_err(os_failed("start the stderr drain"))?;
        let until = Instant::now() + WAIT;
        let mut stream = rendezvous.accept(&mut spawned.process, until)?;
        crate::supervisor::channels::reader_is_child(&stream, pid, admitted)
            .map_err(|_| failed("the rendezvous peer is not the admitted child"))?;
        let mut receiver = org_socket::rights::StreamReceiver::new(
            stream
                .try_clone()
                .map_err(os_failed("clone the control stream"))?,
        );
        let ready: GatewayReadyForSecrets =
            serde_json::from_slice(&line(&mut receiver, until, "ready line")?)
                .map_err(|_| failed("the ready line does not match ready_for_secrets"))?;
        org_socket::confinement::verify_ready_report(ready.landlock.is_some())
            .map_err(|error| failed(&error.to_string()))?;
        crate::supervisor::channels::reader_is_child(&stream, pid, admitted)
            .map_err(|_| failed("the ready-line peer is not the admitted child"))?;
        config
            .check
            .still(pid, admitted)
            .map_err(refused("the post-ready re-check"))?;
        if config.check.admit(pid).map_err(refused("re-admission"))? != admitted {
            return Err(io::Error::other(
                "web process admission refused at re-admission: the identity changed",
            ));
        }
        let paths: Vec<_> = config.critical_paths.iter().map(PathBuf::as_path).collect();
        org_socket::confinement::verify_gateway(pid, &paths)
            .map_err(os_failed("verify the web process sandbox"))?;
        let (transfer, child) = transfer_pair().map_err(os_failed("create the transfer pair"))?;
        send_handoff(&mut stream, &child, &listener, until)
            .map_err(os_failed("send the descriptor handoff"))?;
        check_ack(&line(&mut receiver, until, "handoff ACK")?)?;
        drop(receiver);
        drop(child);
        drop(listener);
        drop(rendezvous);
        stream
            .set_write_timeout(None)
            .map_err(os_failed("clear the control write timeout"))?;
        let control = WebConnection::start(stream, transfer, config.listen.clone(), handler)
            .map_err(os_failed("start the control connection"))?;
        Ok(Self {
            control,
            process: spawned.process,
            _stdin: spawned.stdin,
        })
    }
}

/// Accepts only the web process's `{"v":1,"event":"fds.received","of":"handoff"}` line.
/// Each refusal has a fixed, secret-free cause for the supervisor's log line.
fn check_ack(bytes: &[u8]) -> io::Result<()> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Ack {
        v: u32,
        event: String,
        of: String,
    }
    let ack: Ack =
        serde_json::from_slice(bytes).map_err(|_| failed("the handoff ACK is malformed"))?;
    if ack.v != org_host_control::API_VERSION || ack.event != "fds.received" || ack.of != "handoff"
    {
        return Err(failed("the handoff ACK does not acknowledge the handoff"));
    }
    Ok(())
}

/// The host's transfer end (send-only) and the web process's end (receive-only).
/// The web process's write side is shut down before the end is handed off, so the
/// web process cannot send on its transfer endpoint.
fn transfer_pair() -> io::Result<(UnixDatagram, UnixDatagram)> {
    let (transfer, child) = {
        let _fence = lock(&SPAWN_LOCK);
        UnixDatagram::pair()?
    };
    org_socket::rights::set_buffer_sizes(
        transfer.as_fd(),
        org_office_launch::v3::TRANSFER_SOCKET_BUFFER_BYTES,
    )?;
    org_socket::rights::set_buffer_sizes(
        child.as_fd(),
        org_office_launch::v3::TRANSFER_SOCKET_BUFFER_BYTES,
    )?;
    child.shutdown(std::net::Shutdown::Write)?;
    transfer.shutdown(std::net::Shutdown::Read)?;
    Ok((transfer, child))
}

/// One [`GatewayHandoff`] line, then one `sendmsg` with the host.sock marker and
/// exactly [`GATEWAY_HANDOFF_FDS`] in order: the transfer end, then the listener.
fn send_handoff(
    stream: &mut UnixStream,
    child: &UnixDatagram,
    listener: &TcpListener,
    until: Instant,
) -> io::Result<()> {
    let fds = GATEWAY_HANDOFF_FDS.map(|fd| match fd {
        GatewayFd::Transfer => child.as_fd(),
        GatewayFd::Listener => listener.as_fd(),
    });
    stream.set_write_timeout(Some(WAIT))?;
    org_proto::frame::write_message(stream, &GatewayHandoff).map_err(invalid)?;
    org_socket::rights::send(
        stream.as_fd(),
        &[org_host_control::ops::HOST_SOCK_MARKER],
        &fds,
        until,
    )
    .map_err(invalid)
}

// recvmsg reads exactly through LF, never consuming a pipelined hello or silently
// discarding early descriptors. A fatal ancillary frame terminates the host as
// required by X-02; none of its possibly unreported descriptors may survive.
fn line(
    receiver: &mut org_socket::rights::StreamReceiver,
    until: Instant,
    what: &str,
) -> io::Result<Vec<u8>> {
    let timed_out = || failed(&format!("the {what} did not arrive before the deadline"));
    let mut bytes = Vec::new();
    loop {
        match receiver.receive_byte(0, &SPAWN_LOCK) {
            Ok((b'\n', _)) => return Ok(bytes),
            Ok((b, _)) if bytes.len() < 1023 => bytes.push(b),
            Ok(_) => return Err(failed(&format!("the {what} exceeds 1023 bytes"))),
            Err(org_socket::rights::ReceiveError::WouldBlock) => {
                let left = until
                    .checked_duration_since(Instant::now())
                    .ok_or_else(timed_out)?;
                let fd = receiver
                    .fd()
                    .ok_or_else(|| failed(&format!("the {what} stream is closed")))?;
                if !org_socket::poll_readable(fd.as_raw_fd(), left)
                    .map_err(os_failed("poll the control stream"))?
                {
                    return Err(timed_out());
                }
            }
            Err(org_socket::rights::ReceiveError::Fatal(e))
                if e.kind() == io::ErrorKind::UnexpectedEof =>
            {
                return Err(failed(&format!("control EOF before the {what}")))
            }
            Err(org_socket::rights::ReceiveError::Fatal(e)) => {
                crate::host_log::log(&fatal_receive(what, e.kind()));
                std::process::exit(1)
            }
        }
    }
}

/// The host's last log line before a fatal ancillary frame terminates it: the
/// step and the error's kind only, never its text, escaped for one log line.
fn fatal_receive(what: &str, kind: io::ErrorKind) -> String {
    crate::host_log::untrusted(&format!(
        "web process start failed fatally while receiving the {what} ({kind:?}); the host exits"
    ))
}

#[cfg(test)]
mod tests;
