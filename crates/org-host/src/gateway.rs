//! Host-owned browser process. Its private control authority is constructed only
//! after suspended-image, actual-reader, sandbox and descriptor-receipt checks.
use crate::{
    cloexec::SPAWN_LOCK,
    home,
    server_check::ServerCheck,
    service::listener::{GatewayConnection, Handler},
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
const MAX_STARTS: usize = 5;
const POLL: Duration = Duration::from_millis(50);
fn unavailable() -> io::Error {
    io::Error::other("authenticated browser service unavailable")
}
fn invalid<T>(_: T) -> io::Error {
    unavailable()
}

pub(crate) struct Configuration {
    pub program: PathBuf,
    pub instance: String,
    pub runtime: PathBuf,
    pub listen: ListenConfig,
    pub check: ServerCheck,
    pub critical_paths: Vec<PathBuf>,
    pub release: Option<org_host_control::gateway::GatewayRelease>,
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
        // No bind, child or secret handoff for unverified TLS or an unsupported OS.
        if !cfg!(target_os = "macos") || config.listen.scheme() != ListenMode::HttpLoopback {
            return Err(unavailable());
        }
        config.check.may_spawn().map_err(invalid)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let observing = Arc::clone(&state);
        let thread = thread::Builder::new()
            .name("org-browser-supervisor".into())
            .spawn(move || {
                supervise(&stopping, &observing, &*eligible, || {
                    Running::start(&config, Arc::clone(&handler))
                })
            })?;
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

/// A started gateway the supervisor watches.
trait Serving {
    /// Whether the control connection is open and the child has not exited.
    fn serving(&mut self) -> bool;
}

/// Starts the gateway once `eligible`, publishes `Running` only while an admitted
/// gateway serves, and publishes `Failed` as soon as it exits or its control
/// connection reaches EOF, before any bounded restart.
fn supervise<S: Serving>(
    stopping: &AtomicBool,
    state: &Mutex<GatewayState>,
    eligible: &dyn Fn() -> bool,
    mut start: impl FnMut() -> io::Result<S>,
) {
    for attempt in 0..MAX_STARTS {
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
        match start() {
            Ok(mut child) => {
                *lock(state) = GatewayState::Running;
                while !stopping.load(Ordering::SeqCst) && child.serving() {
                    thread::park_timeout(POLL);
                }
                *lock(state) = GatewayState::Failed;
                drop(child); // end sessions/control, then kill/reap before restart
            }
            Err(_) => crate::host_log::log("authenticated browser startup failed"),
        }
        *lock(state) = GatewayState::Failed;
        let retry = Instant::now() + Duration::from_secs(1 << attempt);
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
        let random = crate::secrets::generate_db_key()?;
        let suffix: String = random.expose_bytes()[..8]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let path = directory.join(suffix);
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
                    stream.set_nonblocking(false)?;
                    return Ok(stream);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock && Instant::now() < until => {
                    if process.try_wait()? {
                        return Err(unavailable());
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(e) => return Err(e),
            }
        }
    }
}
impl Drop for Rendezvous {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

struct Running {
    // Drop order: close control/revoke first, then kill/reap the child.
    control: GatewayConnection,
    process: Process,
    _stdin: fs::File,
}
impl Serving for Running {
    fn serving(&mut self) -> bool {
        self.control.live() && matches!(self.process.try_wait(), Ok(false))
    }
}
impl Running {
    fn start(config: &Configuration, handler: Arc<dyn Handler>) -> io::Result<Self> {
        config.check.may_spawn().map_err(invalid)?;
        // Validated configuration is frozen before bind. No port-zero substitution.
        let listener = {
            let _fence = lock(&SPAWN_LOCK);
            TcpListener::bind((config.listen.address(), config.listen.port()))?
        };
        if listener.local_addr()?.port() != config.listen.port() {
            return Err(unavailable());
        }
        let rendezvous = Rendezvous::bind(&config.runtime)?;
        let mut spawned = {
            let _fence = lock(&SPAWN_LOCK);
            spawn::spawn_suspended(&config.program, &[OsStr::new("--serve")], &[])?
        };
        let admitted = config.check.admit(spawned.process.id()).map_err(invalid)?;
        config
            .check
            .still(spawned.process.id(), admitted)
            .map_err(invalid)?;
        spawned.process.resume()?;
        let first = GatewayFirstLine {
            rendezvous: rendezvous.path.to_str().ok_or_else(unavailable)?.into(),
            instance_id: config.instance.clone(),
            release: config.release.clone(),
        };
        let mut first = serde_json::to_vec(&first).map_err(invalid)?;
        if first.len() >= 1024 {
            return Err(unavailable());
        }
        first.push(b'\n');
        spawned.stdin.write_all(&first)?;
        // Drain bounded chunks without retaining any child output in logs.
        let mut stdout = spawned.stdout;
        thread::Builder::new()
            .name("gateway-stdout".into())
            .spawn(move || {
                let _ = io::copy(&mut stdout, &mut io::sink());
            })?;
        let mut stderr = spawned.stderr;
        thread::Builder::new()
            .name("gateway-stderr".into())
            .spawn(move || {
                let mut bytes = [0u8; 4096];
                while matches!(stderr.read(&mut bytes), Ok(n) if n > 0) {}
            })?;
        let until = Instant::now() + WAIT;
        let mut stream = rendezvous.accept(&mut spawned.process, until)?;
        crate::supervisor::channels::reader_is_child(&stream, spawned.process.id(), admitted)?;
        let mut receiver = org_socket::rights::StreamReceiver::new(stream.try_clone()?);
        let ready: GatewayReadyForSecrets =
            serde_json::from_slice(&line(&mut receiver, until)?).map_err(invalid)?;
        if ready.landlock.is_some() {
            return Err(unavailable());
        }
        crate::supervisor::channels::reader_is_child(&stream, spawned.process.id(), admitted)?;
        config
            .check
            .still(spawned.process.id(), admitted)
            .map_err(invalid)?;
        if config.check.admit(spawned.process.id()).map_err(invalid)? != admitted {
            return Err(unavailable());
        }
        let paths: Vec<_> = config.critical_paths.iter().map(PathBuf::as_path).collect();
        org_socket::confinement::verify_gateway(spawned.process.id(), &paths)?;
        let (transfer, child) = transfer_pair()?;
        send_handoff(&mut stream, &child, &listener, until)?;
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Ack {
            v: u32,
            event: String,
            of: String,
        }
        let ack: Ack = serde_json::from_slice(&line(&mut receiver, until)?).map_err(invalid)?;
        if ack.v != org_host_control::API_VERSION
            || ack.event != "fds.received"
            || ack.of != "handoff"
        {
            return Err(unavailable());
        }
        drop(receiver);
        drop(child);
        drop(listener);
        drop(rendezvous);
        stream.set_write_timeout(None)?;
        let control = GatewayConnection::start(stream, transfer, config.listen.clone(), handler)?;
        Ok(Self {
            control,
            process: spawned.process,
            _stdin: spawned.stdin,
        })
    }
}

/// The host's transfer end (send-only) and the gateway's end (receive-only).
/// The gateway's write side is shut down before the end is handed off, so the
/// gateway cannot send on its transfer endpoint.
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
fn line(receiver: &mut org_socket::rights::StreamReceiver, until: Instant) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    loop {
        match receiver.receive_byte(0, &SPAWN_LOCK) {
            Ok((b'\n', _)) => return Ok(bytes),
            Ok((b, _)) if bytes.len() < 1023 => bytes.push(b),
            Ok(_) => return Err(unavailable()),
            Err(org_socket::rights::ReceiveError::WouldBlock) => {
                let left = until
                    .checked_duration_since(Instant::now())
                    .ok_or_else(unavailable)?;
                if !org_socket::poll_readable(
                    receiver.fd().ok_or_else(unavailable)?.as_raw_fd(),
                    left,
                )? {
                    return Err(unavailable());
                }
            }
            Err(org_socket::rights::ReceiveError::Fatal(e))
                if e.kind() == io::ErrorKind::UnexpectedEof =>
            {
                return Err(unavailable())
            }
            Err(org_socket::rights::ReceiveError::Fatal(_)) => std::process::exit(1),
        }
    }
}

#[cfg(test)]
mod tests;
