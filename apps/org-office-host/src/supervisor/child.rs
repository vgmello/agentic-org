//! One `org-server` process, launched with v3 (X-02 §5.1): spawned suspended, admitted, given
//! a rendezvous and its first line, authenticated and handed its keys and transfer socket there
//! (`rendezvous`). Its events come only from the rendezvous; its stdout is untrusted and drained
//! unread; its stderr goes to the office's log. Ending the rendezvous stops it.

use std::ffi::{OsStr, OsString};
use std::io::{self, BufReader, Read};
use std::net::Shutdown;
use std::os::unix::net::{UnixDatagram, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use org_office_launch::v3::{
    self, CreateLine, Handoff, HandoffFd, HostFirstLine, OfficeLine, RendezvousEvent,
};
use org_office_launch::{ServerEvent, CREATE_FLAG, DISCARD_FLAG, OFFICE_FLAG};
use org_proto::location::{self, OFFICE_HOME_ENV, OFFICE_RUNTIME_DIR_ENV};
use org_proto::{DbKey, GrantKey, RecoveryCode};

use super::rendezvous::{self, Admitted, Rendezvous};
use crate::cloexec::SPAWN_LOCK;
use crate::host_log;
use crate::logs::RotatingLog;
use crate::peer::AuditToken;
use crate::secrets::generate_grant_key;
use crate::server_check::ServerCheck;
use crate::spawn::{spawn_suspended, Process, Spawned};
use crate::sync::lock;

/// How often a stop polls whether the child has exited.
const EXIT_POLL: Duration = Duration::from_millis(10);

/// How long a server has to connect to its rendezvous and send `ready_for_secrets`.
const RENDEZVOUS_WAIT: Duration = Duration::from_secs(10);

/// What an office's supervision thread hears: its child's events, the end of its rendezvous,
/// or a stop request. Child messages carry the child's generation, so a late line from
/// a child that was already replaced is ignored.
#[derive(Debug)]
pub(crate) enum Signal {
    Event {
        generation: u64,
        event: ServerEvent,
    },
    /// The child's rendezvous ended (it exited, never connected, or faulted).
    Closed {
        generation: u64,
    },
    /// The host is shutting down: stop the child by this deadline.
    Stop {
        deadline: Instant,
    },
    /// `office.restart`: start the office again now, if it isn't running.
    Restart,
}

/// The environment a server gets, besides `ORG_OFFICE_HOME`: only these of the host's own, so
/// nothing else in the login session reaches it (the development key folder included: servers
/// never touch a key store, they get their key on the first line). SQLCipher needs only the
/// user's identity and home; `TMPDIR` is the user's temporary folder.
const PASSED_ENV: [&str; 6] = ["HOME", "USER", "LOGNAME", "PATH", "TMPDIR", "LANG"];

/// How to start a server: the program and the office home it is given.
#[derive(Debug, Clone)]
pub(crate) struct Spawner {
    pub(crate) program: PathBuf,
    pub(crate) home: PathBuf,
    pub(crate) runtime_dir: Option<PathBuf>,
    /// The instance every first line names.
    pub(crate) instance_id: String,
    /// Who may get a rendezvous: every child is checked before it's made (`server_check`).
    pub(crate) check: Arc<ServerCheck>,
}

/// Why a server wasn't started.
#[derive(Debug)]
pub(crate) enum SpawnError {
    /// It couldn't be spawned (or its pipes' threads couldn't start).
    Io(io::Error),
    /// It was spawned, but isn't the signed office server (or no server may be started): it
    /// was killed and reaped, and got nothing (`server_check`).
    Untrusted(String),
}

impl std::fmt::Display for SpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpawnError::Io(e) => write!(f, "{e}"),
            SpawnError::Untrusted(why) => write!(f, "refused to start it: {why}"),
        }
    }
}

impl From<io::Error> for SpawnError {
    fn from(e: io::Error) -> Self {
        SpawnError::Io(e)
    }
}

impl From<SpawnError> for io::Error {
    fn from(e: SpawnError) -> Self {
        match e {
            SpawnError::Io(e) => e,
            SpawnError::Untrusted(_) => io::Error::other(e.to_string()),
        }
    }
}

/// Which server to start, with the keys its handoff carries (owned: they are zeroed once sent,
/// or when the spawn fails). Every start and create gets a fresh grant key at the spawn.
pub(crate) enum Mode {
    /// Create an office.
    Create(Box<NewOffice>),
    /// Serve the office with this id, with its database key.
    Office(String, DbKey),
    /// Remove an unfinished office, then exit: its database key, or `None` when the host has
    /// none.
    Discard(String, Option<DbKey>),
}

/// A `--create`: its first line (the spawn fills in the instance and the rendezvous), its new
/// database key and its recovery code.
pub(crate) struct NewOffice {
    pub(crate) line: CreateLine,
    pub(crate) db_key: DbKey,
    pub(crate) recovery_code: RecoveryCode,
}

/// A running server process. Dropping it kills and reaps the process if it's still running,
/// so a server is never left behind unsupervised.
pub(crate) struct RunningChild {
    process: Process,
    /// Its rendezvous connection, whose end is the server's (X-02 §5.1 step 9).
    rendezvous: Option<UnixStream>,
    /// The host's end of its transfer socket, held for its whole life (closing it ends the
    /// server on macOS); `office.open_channel` sends on it (P1b).
    transfer: Option<UnixDatagram>,
    /// This start's grant key (X-02 §4.1), never logged; `session.grant` signs with it (P2b).
    grant_key: Option<GrantKey>,
    pub(crate) generation: u64,
    pub(crate) started: Instant,
}

impl Spawner {
    pub(crate) fn socket_path(&self, office_id: &str) -> Option<PathBuf> {
        match &self.runtime_dir {
            Some(runtime) => location::office_socket_in(runtime, office_id),
            None => location::office_socket_path(&self.home, office_id),
        }
    }

    /// The folder its rendezvous goes in.
    fn runtime(&self) -> PathBuf {
        self.runtime_dir
            .clone()
            .unwrap_or_else(|| self.home.join("run"))
    }

    /// Starts a server (X-02 §5.1). Its events go to `signals` tagged with `generation`; its
    /// stderr goes to `log`.
    ///
    /// It is started suspended (`spawn::spawn_suspended`) and admitted (`server_check`) before
    /// any of its image runs; a child that fails is killed still suspended, having got nothing.
    /// Only then is its rendezvous made, and it is resumed, its token checked again, and its
    /// first line written. Its keys go only to the rendezvous peer that passes `rendezvous`'s
    /// checks; any failure there kills it. A child that exits before it connects is reported
    /// as closed.
    pub(crate) fn spawn(
        &self,
        mode: Mode,
        generation: u64,
        signals: Sender<Signal>,
        log: Arc<Mutex<RotatingLog>>,
    ) -> Result<RunningChild, SpawnError> {
        self.check.may_spawn().map_err(|why| {
            host_log::log(&format!("didn't start an office server: {why}"));
            SpawnError::Untrusted(why)
        })?;
        let args: Vec<&OsStr> = match &mode {
            Mode::Create(_) => vec![OsStr::new(CREATE_FLAG)],
            Mode::Office(id, _) => vec![OsStr::new(OFFICE_FLAG), OsStr::new(id)],
            Mode::Discard(id, _) => vec![OsStr::new(DISCARD_FLAG), OsStr::new(id)],
        };
        let mut env: Vec<(OsString, OsString)> = PASSED_ENV
            .iter()
            .filter_map(|name| std::env::var_os(name).map(|value| ((*name).into(), value)))
            .collect();
        env.push((OFFICE_HOME_ENV.into(), self.home.clone().into_os_string()));
        if let Some(runtime) = &self.runtime_dir {
            env.push((
                OFFICE_RUNTIME_DIR_ENV.into(),
                runtime.clone().into_os_string(),
            ));
        }
        let spawned = {
            // See `cloexec`: no fd of ours may be half-made while a child is spawned.
            let _one_at_a_time = lock(&SPAWN_LOCK);
            spawn_suspended(&self.program, &args, &env)?
        };
        let Spawned {
            mut process,
            mut stdin,
            stdout,
            stderr,
        } = spawned;
        let pid = process.id();
        // Suspended: check the image, before any of it has run.
        let admitted = match self.check.admit(pid) {
            Ok(admitted) => admitted,
            Err(why) => {
                process.kill();
                host_log::log(&format!(
                    "refused office server {pid}: {why}; it was stopped before it ran and got \
                     nothing"
                ));
                return Err(SpawnError::Untrusted(why));
            }
        };
        let mut running = RunningChild {
            process,
            rendezvous: None,
            transfer: None,
            grant_key: None,
            generation,
            started: Instant::now(),
        };
        // Only after the admission: nothing the child reached before it leads to the keys.
        let listener = Rendezvous::bind(&self.runtime())?;
        running.process.resume()?;
        thread::Builder::new()
            .name(format!("office-{pid}-out"))
            .spawn(move || drain(stdout))?;
        thread::Builder::new()
            .name(format!("office-{pid}-err"))
            .spawn(move || copy_stderr(stderr, &log))?;
        // Still the image that was checked (no exec since), right before the line.
        if let Err(why) = self.check.still(pid, admitted) {
            host_log::log(&format!(
                "refused office server {pid}: {why}; it was stopped and got nothing"
            ));
            running.kill();
            return Err(SpawnError::Untrusted(why));
        }
        let (line, handoff, grant_key) = launch(mode, &self.instance_id, listener.path())?;
        running.grant_key = grant_key;
        // A child that dies at once makes this fail with a broken pipe; its missing rendezvous
        // then reports it. The error never holds a secret: the line has none.
        if let Err(e) = v3::write_first_line(&mut stdin, &line) {
            host_log::log(&format!("couldn't write to office server {pid}: {e}"));
        }
        drop(stdin);
        let still = || self.check.still(pid, admitted);
        match running.hand_off(listener, admitted, &still, handoff) {
            Ok(Some(events)) => {
                thread::Builder::new()
                    .name(format!("office-{pid}-events"))
                    .spawn(move || read_events(events, generation, &signals))?;
            }
            Ok(None) => drop(signals.send(Signal::Closed { generation })),
            Err(why) if running.has_exited() => {
                host_log::log(&format!(
                    "office server {pid} exited during its handoff: {why}"
                ));
                drop(signals.send(Signal::Closed { generation }));
            }
            Err(why) => {
                host_log::log(&format!(
                    "refused office server {pid}: {why}; it was killed"
                ));
                running.kill();
                return Err(SpawnError::Untrusted(why));
            }
        }
        Ok(running)
    }
}

/// The first line and the handoff for `mode`, and the grant key the host keeps (start and create).
fn launch(
    mode: Mode,
    instance_id: &str,
    rendezvous: &Path,
) -> io::Result<(HostFirstLine, Handoff, Option<GrantKey>)> {
    let path = rendezvous
        .to_str()
        .ok_or_else(|| io::Error::other("the rendezvous path isn't UTF-8"))?;
    let office = |office_id| OfficeLine {
        instance_id: instance_id.to_owned(),
        office_id,
        rendezvous: path.to_owned(),
    };
    // The handoff gets its own copy; the host's is zeroed when the server is gone.
    let copy = |key: &GrantKey| GrantKey::from_bytes(*key.expose_bytes());
    Ok(match mode {
        Mode::Create(new) => {
            let NewOffice {
                mut line,
                db_key,
                recovery_code,
            } = *new;
            line.office = office(std::mem::take(&mut line.office.office_id));
            let kept = generate_grant_key()?;
            let handoff = Handoff::Create {
                db_key,
                grant_key: copy(&kept),
                recovery_code,
                fds: vec![HandoffFd::Transfer],
            };
            (HostFirstLine::Create(line), handoff, Some(kept))
        }
        Mode::Office(office_id, db_key) => {
            let kept = generate_grant_key()?;
            let handoff = Handoff::Start {
                db_key,
                grant_key: copy(&kept),
                fds: vec![HandoffFd::Transfer],
            };
            (HostFirstLine::Start(office(office_id)), handoff, Some(kept))
        }
        Mode::Discard(office_id, db_key) => {
            let handoff = Handoff::Discard {
                db_key,
                fds: Vec::new(),
            };
            (HostFirstLine::Discard(office(office_id)), handoff, None)
        }
    })
}

impl RunningChild {
    /// X-02 §5.1 steps 4 and 7 on `listener`: the events' reader once handed off, `None` if the
    /// child exited before it connected, or why it must be killed.
    fn hand_off(
        &mut self,
        listener: Rendezvous,
        token: AuditToken,
        still: &dyn Fn() -> Result<(), String>,
        handoff: Handoff,
    ) -> Result<Option<BufReader<UnixStream>>, String> {
        let deadline = Instant::now() + RENDEZVOUS_WAIT;
        let process = &mut self.process;
        let Some(stream) = listener.accept_one(deadline, || process.try_wait().unwrap_or(true))?
        else {
            return Ok(None);
        };
        rendezvous::authenticate(&stream, &Admitted { token, still }, deadline)?;
        self.rendezvous = Some(
            stream
                .try_clone()
                .map_err(|e| format!("its rendezvous connection: {e}"))?,
        );
        let handed = rendezvous::hand_off(stream, handoff, deadline)?;
        self.transfer = handed.transfer;
        Ok(Some(handed.events))
    }

    /// Ends the rendezvous both ways: the server's signal to stop (X-02 §5.1 step 9).
    fn end_rendezvous(&mut self) {
        if let Some(stream) = self.rendezvous.take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }

    /// Asks the server to stop (ends its rendezvous), waits for it until `deadline`, then
    /// SIGKILLs it. Always reaps it.
    pub(crate) fn stop(&mut self, deadline: Instant) {
        self.end_rendezvous();
        if !self.wait_until(deadline) {
            self.kill();
        }
    }

    /// Whether the process has exited (it is reaped if so). Doesn't wait.
    pub(crate) fn has_exited(&mut self) -> bool {
        self.process.try_wait().unwrap_or(false)
    }

    /// SIGKILLs and reaps the process.
    pub(crate) fn kill(&mut self) {
        self.end_rendezvous();
        self.process.kill();
    }

    /// Waits for the process to exit until `deadline`; reaps it if it did.
    fn wait_until(&mut self, deadline: Instant) -> bool {
        loop {
            match self.process.try_wait() {
                Ok(true) => return true,
                Ok(false) if Instant::now() < deadline => thread::sleep(EXIT_POLL),
                Ok(false) | Err(_) => return false,
            }
        }
    }
}

impl Drop for RunningChild {
    fn drop(&mut self) {
        self.kill();
    }
}

/// The rendezvous reader: forwards the server's events until EOF or an error that leaves the
/// connection out of step, then ends the rendezvous. Lines it can't use are logged and skipped.
/// A receipt here is for descriptors never sent (the handoff's was read already): a fault.
fn read_events(mut events: BufReader<UnixStream>, generation: u64, signals: &Sender<Signal>) {
    loop {
        match v3::read_server_event(&mut events) {
            Ok(Some(RendezvousEvent::Server(event))) => {
                if signals.send(Signal::Event { generation, event }).is_err() {
                    break;
                }
            }
            Ok(Some(_)) => {
                host_log::log("an office server acknowledged descriptors it wasn't sent");
                break;
            }
            Ok(None) => break,
            Err(e) if e.closes_stream() => {
                host_log::log(&format!("an office server's rendezvous failed: {e}"));
                break;
            }
            Err(e) => host_log::log(&format!("ignored a line from an office server: {e}")),
        }
    }
    let _ = events.get_ref().shutdown(Shutdown::Both);
    let _ = signals.send(Signal::Closed { generation });
}

/// The stdout drain: stdout is untrusted (a helper that resumed the child early may hold it),
/// so it is read to the end, unparsed, so the server never blocks on a full pipe.
fn drain(mut stdout: impl Read) {
    let mut buf = [0u8; 4096];
    loop {
        match stdout.read(&mut buf) {
            Ok(0) => return,
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }
    }
}

/// The stderr copier: drains stderr to the end (so the server never blocks on a full pipe),
/// writing what it can to the log.
fn copy_stderr(mut stderr: impl Read, log: &Mutex<RotatingLog>) {
    let mut buf = [0u8; 8192];
    let mut warned = false;
    loop {
        match stderr.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => {
                if let Err(e) = lock(log).write(&buf[..n]) {
                    if !warned {
                        host_log::log(&format!("couldn't write an office server's log: {e}"));
                        warned = true;
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }
    }
}
