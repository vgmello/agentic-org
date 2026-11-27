//! One started web process: its rendezvous, the descriptor handoff, its ready line and its
//! control connection.

use std::{
    ffi::OsStr,
    fs,
    io::{self, Write},
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
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use org_host_control::web::{WebFd, WebFirstLine, WebHandoff, WebReadyForSecrets, WEB_HANDOFF_FDS};
use org_platform_host::admission::{Child, ChildEnv, ImageRefusal, SpawnRefused};

use crate::{
    cloexec::SPAWN_LOCK,
    control_listener::{Handler, WebConnection},
    home,
    server_check::NoAdmission,
    sync::lock,
};

use super::control::{
    exited, failed, host_stopping, invalid, os_failed, refused, ControlEof, POLL,
};
use super::listener::bind;

use super::image::{Configuration, Image};
use super::stderr::{confined_by, not_the_reader, shown, with_refusal, Stderr, STDERR_GRACE};
use super::supervise::Serving;

/// How long a started child has to connect to the rendezvous, send its ready line
/// and acknowledge the handoff. [`Supervisor::start`](super::Supervisor::start) always uses this; tests pass a
/// longer bound to [`Running::start`] so a loaded machine cannot fail them.
pub(super) const WAIT: Duration = Duration::from_secs(5);

/// The web process's whole environment: nothing. Not one variable of the host's
/// reaches it, so neither `RUST_BACKTRACE` (a backtrace opens files, which a confined
/// process cannot do) nor a loader variable can. It needs none: its release, its
/// rendezvous and its instance come on the command line and the first line.
pub(super) fn web_env() -> ChildEnv {
    ChildEnv::new()
}

/// After `verify_web`: the status it read was the admitted child's only if that
/// child is still this process's unreaped child. A process id alone can be reused.
pub(super) fn still_running(child: &mut Child) -> io::Result<()> {
    match child.try_wait() {
        Ok(false) => Ok(()),
        Ok(true) => Err(failed(&format!(
            "the child exited after its sandbox was verified ({})",
            exited(child)
        ))),
        Err(error) => Err(os_failed("wait for the child")(error)),
    }
}

pub(super) struct Rendezvous {
    listener: UnixListener,
    path: PathBuf,
}

impl Rendezvous {
    pub(super) fn bind(runtime: &Path) -> io::Result<Self> {
        let directory = runtime.join("r");
        match fs::DirBuilder::new().mode(0o700).create(&directory) {
            Ok(()) => (),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e),
        }
        home::owner_only_dir(&directory).map_err(invalid)?;
        let path = directory.join(crate::secrets::generate_unique_name()?);
        org_proto::location::check_socket_path(&path, home::socket_path_limit()?)
            .map_err(invalid)?;
        let listener = {
            let _fence = lock(&SPAWN_LOCK);
            UnixListener::bind(&path)?
        };
        let bound = Self { listener, path };
        fs::set_permissions(&bound.path, fs::Permissions::from_mode(0o600))?;
        bound.listener.set_nonblocking(true)?;
        Ok(bound)
    }
    /// The child's connection, until `until` or until `stop` is set.
    pub(super) fn accept(
        &self,
        process: &mut Child,
        until: Instant,
        stop: &AtomicBool,
    ) -> io::Result<UnixStream> {
        loop {
            if stop.load(Ordering::SeqCst) {
                return Err(host_stopping());
            }
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

impl Drop for Rendezvous {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub(super) struct Running {
    // Drop order: kill and reap the child first, then close the control connection and wait
    // for its threads. A request of it still running (one waiting for a write lock of the
    // registry, say) is waited for only then: it never keeps the web process alive, past a
    // host's stop or into the next start (X5 review, third round, P1). The child makes no
    // request after it is killed; the request's answer is dropped.
    pub(super) process: Child,
    pub(super) control: WebConnection,
    pub(super) _stdin: fs::File,
    /// The layers of the report the host verified.
    pub(super) layers: org_platform::confinement::Layers,
    /// The child's standard error, for the reason a layer is missing.
    pub(super) captured: Arc<Stderr>,
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
    fn confined(&mut self) -> String {
        confined_by(&self.layers, || {
            self.captured.landlock_not_applied(STDERR_GRACE)
        })
    }
}

/// Why a running web process ended, pure: a refused `web.serving` wins over the EOF it
/// causes, then a reaped child's exit status (`exit`), then the control EOF.
pub(super) fn ended_cause(serving_refused: bool, exit: Option<String>, live: bool) -> String {
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
    /// Starts the web process of `config` through `image`, the host's one admission of it,
    /// serving on `held` when it is given (the listener setup bound) and on a listener it
    /// binds otherwise. `wait` bounds the steps from spawn through the handoff ACK; a start
    /// that waits for the web process ends as soon as `stop` is set, as a failed start.
    pub(super) fn start(
        config: &Configuration,
        image: &Image,
        held: Option<TcpListener>,
        handler: Arc<dyn Handler>,
        wait: Duration,
        stop: &AtomicBool,
    ) -> io::Result<Self> {
        config
            .check
            .may_spawn()
            .map_err(refused("the pre-spawn check"))?;
        // Validated configuration is frozen before bind. No port-zero substitution.
        let listener = match held {
            Some(listener) => listener,
            None => bind(&config.listen).map_err(os_failed("bind the browser listener"))?,
        };
        let bound = listener
            .local_addr()
            .map_err(os_failed("read the browser listener address"))?;
        if bound.ip() != config.listen.address() || bound.port() != config.listen.port() {
            return Err(failed(
                "the browser listener is not on its configured address and port",
            ));
        }
        let rendezvous =
            Rendezvous::bind(&config.runtime).map_err(os_failed("bind the rendezvous"))?;
        // The platform's admission starts the child with only its three pipes (holding
        // `SPAWN_LOCK` while it makes them and the process), checks its image before any
        // of it has run, and holds it until it is released. A system without an admission
        // adapter starts nothing. The admission is the host's one: built at its start, or
        // now if that build failed. A refused image keeps the check's words for the log.
        let checked = image.admission().map_err(|none| match none {
            NoAdmission::Refused(why) => refused("admission")(why),
            NoAdmission::Unavailable(error) => os_failed("spawn the web process suspended")(error),
        })?;
        let held = checked
            .start(&[OsStr::new("--serve")], &web_env(), &SPAWN_LOCK)
            .map_err(|error| match SpawnRefused::of(&error) {
                Some(refusal) => refused("admission")(config.check.why(&refusal.refusal)),
                None => os_failed("spawn the web process suspended")(error),
            })?;
        let pid = held.child().id();
        checked
            .recheck(held.child())
            .map_err(|refusal| refused("the pre-resume re-check")(config.check.why(&refusal)))?;
        let mut spawned = held
            .release()
            .map_err(os_failed("resume the web process"))?;
        let first = WebFirstLine {
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
        // Its standard output is drained and dropped: nothing of it is kept.
        let mut stdout = spawned.stdout;
        thread::Builder::new()
            .name("web-stdout".into())
            .spawn(move || {
                let _ = io::copy(&mut stdout, &mut io::sink());
            })
            .map_err(os_failed("start the stdout drain"))?;
        // Its standard error is drained too; only its start is kept, for the one line
        // of a child that could not confine itself.
        let captured = Stderr::capture(
            spawned.stderr,
            &config.instance,
            config.on_renewal_kept.clone(),
        )
        .map_err(os_failed("start the stderr drain"))?;
        let until = Instant::now() + wait;
        let mut stream = rendezvous
            .accept(&mut spawned.child, until, stop)
            .map_err(|error| with_refusal(error, &captured, &mut spawned.child))?;
        crate::supervisor::channels::reader_is_child(&checked, &spawned.child, &stream)
            .map_err(|_| not_the_reader(&captured, &mut spawned.child))?;
        let mut receiver = org_socket::rights::StreamReceiver::new(
            stream
                .try_clone()
                .map_err(os_failed("clone the control stream"))?,
        );
        let ready = line(&mut receiver, until, stop, "ready line")
            .map_err(|error| with_refusal(error, &captured, &mut spawned.child))?;
        let ready: WebReadyForSecrets = serde_json::from_slice(&ready)
            .map_err(|_| failed("the ready line does not match ready_for_secrets"))?;
        let enforcement = reported(&ready).map_err(failed)?;
        org_platform::confinement::verify_ready_report(&enforcement)
            .map_err(|error| failed(&error.to_string()))?;
        crate::supervisor::channels::reader_is_child(&checked, &spawned.child, &stream)
            .map_err(|_| failed("the ready-line peer is not the admitted child"))?;
        checked
            .recheck(&spawned.child)
            .map_err(|refusal| refused("the post-ready re-check")(config.check.why(&refusal)))?;
        // Admitted once more from the start: it must be the identity admitted before.
        checked
            .readmit(&spawned.child)
            .map_err(|refusal| match refusal {
                ImageRefusal::Readmitted => io::Error::other(
                    "web process admission refused at re-admission: the identity changed",
                ),
                refusal => refused("re-admission")(config.check.why(&refusal)),
            })?;
        let paths: Vec<_> = config.critical_paths.iter().map(PathBuf::as_path).collect();
        org_platform::confinement::verify_web(pid, &enforcement, &paths).map_err(|error| {
            failed(&format!(
                "verify the web process sandbox: {}",
                shown(&error.to_string())
            ))
        })?;
        // The child is held unreaped across that check; it must not have exited since.
        still_running(&mut spawned.child)?;
        // What the verified report says the child is confined by, for the log.
        let layers = org_platform::confinement::layers(&enforcement).map_err(|error| {
            failed(&format!(
                "name the web process sandbox: {}",
                shown(&error.to_string())
            ))
        })?;
        let (transfer, child) = transfer_pair().map_err(os_failed("create the transfer pair"))?;
        send_handoff(&mut stream, &child, &listener, until)
            .map_err(os_failed("send the descriptor handoff"))?;
        check_ack(&line(&mut receiver, until, stop, "handoff ACK")?)?;
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
            process: spawned.child,
            _stdin: spawned.stdin,
            layers,
            captured,
        })
    }
}

/// Accepts only the web process's `{"v":1,"event":"fds.received","of":"handoff"}` line.
/// Each refusal has a fixed, secret-free cause for the supervisor's log line.
pub(super) fn check_ack(bytes: &[u8]) -> io::Result<()> {
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
pub(super) fn transfer_pair() -> io::Result<(UnixDatagram, UnixDatagram)> {
    let (transfer, child) = {
        let _fence = lock(&SPAWN_LOCK);
        UnixDatagram::pair()?
    };
    org_socket::rights::set_buffer_sizes(
        transfer.as_fd(),
        org_office_launch::TRANSFER_SOCKET_BUFFER_BYTES,
    )?;
    org_socket::rights::set_buffer_sizes(
        child.as_fd(),
        org_office_launch::TRANSFER_SOCKET_BUFFER_BYTES,
    )?;
    child.shutdown(std::net::Shutdown::Write)?;
    transfer.shutdown(std::net::Shutdown::Read)?;
    Ok((transfer, child))
}

/// One [`WebHandoff`] line, then one `sendmsg` with the host.sock marker and
/// exactly [`WEB_HANDOFF_FDS`] in order: the transfer end, then the listener.
pub(super) fn send_handoff(
    stream: &mut UnixStream,
    child: &UnixDatagram,
    listener: &TcpListener,
    until: Instant,
) -> io::Result<()> {
    let fds = WEB_HANDOFF_FDS.map(|fd| match fd {
        WebFd::Transfer => child.as_fd(),
        WebFd::Listener => listener.as_fd(),
    });
    stream.set_write_timeout(Some(WAIT))?;
    org_proto::frame::write_message(stream, &WebHandoff).map_err(invalid)?;
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
//
// It waits until `until`, and ends as soon as `stop` is set: the host is stopping.
pub(super) fn line(
    receiver: &mut org_socket::rights::StreamReceiver,
    until: Instant,
    stop: &AtomicBool,
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
                if stop.load(Ordering::SeqCst) {
                    return Err(host_stopping());
                }
                let left = until
                    .checked_duration_since(Instant::now())
                    .ok_or_else(timed_out)?;
                let fd = receiver
                    .fd()
                    .ok_or_else(|| failed(&format!("the {what} stream is closed")))?;
                // In slices of `POLL`, so a stop is seen while the line is awaited.
                if !org_socket::poll_readable(fd.as_raw_fd(), left.min(POLL))
                    .map_err(os_failed("poll the control stream"))?
                    && left <= POLL
                {
                    return Err(timed_out());
                }
            }
            // The child's end of the control stream is gone. Linux reports that as a reset
            // (`ECONNRESET`), once, when the child closed or died with bytes of the host's
            // it had not read, and as the end of the stream otherwise; macOS reports the
            // end of the stream in both cases. A reset comes only after everything the
            // child had sent was read, and carries no bytes and no descriptors: it is the
            // same event, and this start fails as for any child that went away.
            Err(org_socket::rights::ReceiveError::Fatal(e))
                if matches!(
                    e.kind(),
                    io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset
                ) =>
            {
                let cause = failed(&format!("control EOF before the {what}"));
                return Err(io::Error::other(ControlEof(cause.to_string())));
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
pub(super) fn fatal_receive(what: &str, kind: io::ErrorKind) -> String {
    crate::host_log::untrusted(&format!(
        "web process start failed fatally while receiving the {what} ({kind:?}); the host exits"
    ))
}

/// What the web process's ready line reports about its confinement, as the platform
/// contract takes it. An [`Enforcement`](org_platform::confinement::Enforcement) says a
/// Landlock domain is in force in full, so a line that reports one enforced only in
/// part, or not at all, never becomes one: it is refused here. The line has no
/// seccomp field; where the system has seccomp the host observes it on the process
/// itself (Linux host design, section 3.3).
pub(super) fn reported(
    ready: &WebReadyForSecrets,
) -> Result<org_platform::confinement::Enforcement, &'static str> {
    use org_host_control::web::LandlockEnforcement::FullyEnforced;
    match ready.landlock {
        Some(status) if status.enforcement != FullyEnforced => {
            Err("the ready line reports landlock not fully enforced")
        }
        landlock => Ok(org_platform::confinement::Enforcement {
            landlock_abi: landlock.map(|status| status.abi),
            seccomp: false,
        }),
    }
}
