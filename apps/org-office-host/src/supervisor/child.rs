//! One `org-office-server` process: spawning it with its pipes, handing it its secrets on its key
//! socket (`handoff`), reading its control lines, copying its stderr to the office's log, and
//! stopping it.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use org_office_launch::{
    read_server_event, write_host_message, CreateOffice, DiscardOffice, HostMessage, KeySocketId,
    ServerEvent, StartOffice, CREATE_FLAG, DISCARD_FLAG, OFFICE_FLAG,
};
use org_proto::location::OFFICE_HOME_ENV;
use org_proto::{AppToken, DbKey, OfficeRole, APP_TOKEN_BYTES, DB_KEY_BYTES};

use crate::cloexec::SPAWN_LOCK;
use crate::handoff::{HandOff, KeySocket, Secrets};
use crate::host_log;
use crate::logs::RotatingLog;
use crate::server_check::ServerCheck;
use crate::spawn::{spawn_suspended, Process, Spawned};
use crate::sync::lock;

/// How often a stop polls whether the child has exited.
const EXIT_POLL: Duration = Duration::from_millis(10);

/// What an office's supervision thread hears: its child's control lines, the end of its
/// stdout, or a stop request. Child messages carry the child's generation, so a late line from
/// a child that was already replaced is ignored.
#[derive(Debug)]
pub(crate) enum Signal {
    Event {
        generation: u64,
        event: ServerEvent,
    },
    /// The child closed its stdout (it exited) or wrote something unreadable.
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
/// never touch a key store, they get their key on the key hand-off). SQLCipher needs only the
/// user's identity and home; `TMPDIR` is the user's temporary folder.
const PASSED_ENV: [&str; 6] = ["HOME", "USER", "LOGNAME", "PATH", "TMPDIR", "LANG"];

/// How to start a server: the program and the office home it is given.
#[derive(Debug, Clone)]
pub(crate) struct Spawner {
    pub(crate) program: PathBuf,
    pub(crate) home: PathBuf,
    /// Who may get a first line and a key: every child is checked before it's resumed and its
    /// line written, and the connection its key goes on at accept (`server_check`).
    pub(crate) check: Arc<ServerCheck>,
    /// How long a server has, from its first line, to take its secrets (`handoff`).
    pub(crate) hand_off_timeout: Duration,
}

/// Why a server wasn't started.
#[derive(Debug)]
pub(crate) enum SpawnError {
    /// It couldn't be spawned (or its pipes' threads couldn't start).
    Io(io::Error),
    /// It was spawned, but isn't the signed office server (or no server may be started): it
    /// was killed and reaped, and got nothing (`server_check`).
    Untrusted(String),
    /// It passed the check, but its key hand-off didn't happen (no connection in time, a peer
    /// that isn't the admitted server, or an error): it was killed and reaped, and got nothing
    /// (`handoff`).
    HandOff(String),
}

impl std::fmt::Display for SpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpawnError::Io(e) => write!(f, "{e}"),
            SpawnError::Untrusted(why) => write!(f, "refused to start it: {why}"),
            SpawnError::HandOff(why) => write!(f, "its key hand-off failed: {why}"),
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
            SpawnError::Untrusted(_) | SpawnError::HandOff(_) => io::Error::other(e.to_string()),
        }
    }
}

/// Which server to start, with what its first line and its hand-off carry. The first line holds
/// no secret: the spawn's key socket id and, for a create, the new office's details. The secrets
/// go only on the hand-off (`handoff`), which borrows them (never copies them) and zeroes its
/// encoded line.
#[derive(Clone, Copy)]
pub(crate) enum Mode<'a> {
    /// Create an office: its first line is this create's message; its hand-off, the new office's
    /// app token and database key.
    Create(&'a CreateRequest),
    /// Serve the office with this id: its first line is a start; its hand-off, the office's app
    /// token for this host run and its database key from the key store.
    Office(&'a str, &'a AppToken, &'a DbKey),
    /// Remove an unfinished office, then exit: its first line is a discard; its hand-off, an app
    /// token (a discard serves no client, so a throwaway one) and the office's database key, or
    /// `null` when the host has none.
    Discard(&'a str, &'a AppToken, Option<&'a DbKey>),
}

impl<'a> Mode<'a> {
    fn args(&self) -> Vec<&'a OsStr> {
        match *self {
            Mode::Create(_) => vec![OsStr::new(CREATE_FLAG)],
            Mode::Office(id, ..) => vec![OsStr::new(OFFICE_FLAG), OsStr::new(id)],
            Mode::Discard(id, ..) => vec![OsStr::new(DISCARD_FLAG), OsStr::new(id)],
        }
    }

    /// The first line, naming the spawn's key socket.
    fn first_line(&self, key_socket_id: &KeySocketId) -> HostMessage {
        let key_socket_id = key_socket_id.clone();
        match self {
            Mode::Create(create) => HostMessage::Create(create.message(key_socket_id)),
            Mode::Office(..) => HostMessage::Start(StartOffice { key_socket_id }),
            Mode::Discard(..) => HostMessage::Discard(DiscardOffice { key_socket_id }),
        }
    }

    /// What the hand-off carries, borrowed.
    fn secrets(&self) -> Secrets<'a> {
        match *self {
            Mode::Create(create) => Secrets {
                app_token: &create.0.app_token,
                db_key: Some(&create.0.db_key),
            },
            Mode::Office(_, app_token, db_key) => Secrets {
                app_token,
                db_key: Some(db_key),
            },
            Mode::Discard(_, app_token, db_key) => Secrets { app_token, db_key },
        }
    }
}

/// What a new office is: everything its create message carries but the key socket, which is the
/// spawn's own. None of it is secret.
#[derive(Debug, Clone)]
pub(crate) struct NewOffice {
    pub(crate) org_id: String,
    pub(crate) org_name: String,
    pub(crate) office_id: String,
    pub(crate) office_name: String,
    pub(crate) role: OfficeRole,
}

/// A `--create` run's request: the new office, and its app token for this host run and its
/// database key, which its hand-off carries.
///
/// It is built once and owns the secrets, because neither [`AppToken`] nor [`DbKey`] is `Clone`:
/// handing them off never copies them, and dropping it zeroes them. It lives on the heap (a
/// `Box`), so moving it moves a pointer, not the secrets. Building it still leaves the usual stack
/// copies behind (see `AppToken`'s docs).
pub(crate) struct CreateRequest(Box<CreateParts>);

struct CreateParts {
    office: NewOffice,
    app_token: AppToken,
    db_key: DbKey,
}

impl CreateRequest {
    pub(crate) fn new(office: NewOffice, app_token: AppToken, db_key: DbKey) -> Self {
        Self(Box::new(CreateParts {
            office,
            app_token,
            db_key,
        }))
    }

    /// The id of the office it creates.
    pub(crate) fn office_id(&self) -> &str {
        &self.0.office.office_id
    }

    /// The new office's database key, for the key store. Never copy it.
    pub(crate) fn db_key(&self) -> &DbKey {
        &self.0.db_key
    }

    /// The create message for the spawn whose key socket is `key_socket_id`.
    fn message(&self, key_socket_id: KeySocketId) -> CreateOffice {
        let office = &self.0.office;
        CreateOffice {
            org_id: office.org_id.clone(),
            org_name: office.org_name.clone(),
            office_id: office.office_id.clone(),
            office_name: office.office_name.clone(),
            role: office.role,
            key_socket_id,
        }
    }

    /// Zeroes the database key in place, once the hand-off is done (or can't be): from then on
    /// only the key store holds it.
    pub(crate) fn drop_key(&mut self) {
        // Replaced in place, leaving zeros; the old key is zeroed as it drops.
        drop(std::mem::replace(
            &mut self.0.db_key,
            DbKey::from_bytes([0; DB_KEY_BYTES]),
        ));
    }

    /// The created office's app token, so its restarts get the token it was created with.
    pub(crate) fn into_token(mut self) -> Arc<AppToken> {
        self.drop_key();
        // Taken out in place, leaving zeros: the box's memory never keeps a copy.
        Arc::new(std::mem::replace(
            &mut self.0.app_token,
            AppToken::from_bytes([0; APP_TOKEN_BYTES]),
        ))
    }
}

/// A running server process. Dropping it kills and reaps the process if it's still running,
/// so a server is never left behind unsupervised.
pub(crate) struct RunningChild {
    process: Process,
    stdin: Option<File>,
    pub(crate) generation: u64,
    pub(crate) started: Instant,
}

impl Spawner {
    /// Starts a server. Its control lines go to `signals` tagged with `generation`; its stderr
    /// goes to `log`. Once it passed the check, its first line (per [`Mode`]) is written, and
    /// its secrets are handed over on its key socket (`handoff`) before this returns.
    ///
    /// It is started suspended (`spawn::spawn_suspended`), so not one instruction of its image
    /// runs before the check (`server_check`) unless another process resumes it: a child that
    /// fails is killed still suspended, having got nothing. One that passes is resumed, and its
    /// token checked again. Only then is its key socket bound (`handoff`), right before its first
    /// line, so no connection can predate the admission; it is unlinked before this returns,
    /// whatever happens. The one connection to it must come from the admitted image, or nothing
    /// is sent and the child is killed ([`SpawnError::HandOff`]). A child that exits before it
    /// connects got nothing; it is returned, and its exit reported as usual.
    pub(crate) fn spawn(
        &self,
        mode: Mode<'_>,
        generation: u64,
        signals: Sender<Signal>,
        log: Arc<Mutex<RotatingLog>>,
    ) -> Result<RunningChild, SpawnError> {
        self.check.may_spawn().map_err(|why| {
            host_log::log(&format!("didn't start an office server: {why}"));
            SpawnError::Untrusted(why)
        })?;
        let args = mode.args();
        let mut env: Vec<(OsString, OsString)> = PASSED_ENV
            .iter()
            .filter_map(|name| std::env::var_os(name).map(|value| ((*name).into(), value)))
            .collect();
        env.push((OFFICE_HOME_ENV.into(), self.home.clone().into_os_string()));
        let spawned = {
            // See `cloexec`: no fd of ours may be half-made while a child is spawned.
            let _one_at_a_time = lock(&SPAWN_LOCK);
            spawn_suspended(&self.program, &args, &env)?
        };
        let Spawned {
            mut process,
            stdin,
            stdout,
            stderr,
        } = spawned;
        let pid = process.id();
        // Suspended: check the image, before any of it has run, and who may take its key.
        let admitted = self
            .check
            .admit(pid)
            .and_then(|admitted| Ok((admitted, self.check.peer_rule(admitted, pid)?)));
        let (admitted, rule) = match admitted {
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
            stdin: Some(stdin),
            generation,
            started: Instant::now(),
        };
        running.process.resume()?;
        thread::Builder::new()
            .name(format!("office-{pid}-out"))
            .spawn(move || read_events(stdout, generation, &signals))?;
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
        // Only now, with the image admitted and unchanged, does its key socket exist: nothing
        // could connect to it before (a pre-exec image that was resumed early and kept a
        // connection would otherwise look like the genuine server once it exec'd it). Unlinked
        // when dropped, on every path out of here. The server connects only after reading its
        // first line, so binding first can't deadlock.
        let key_socket = match KeySocket::bind(&self.home) {
            Ok(key_socket) => key_socket,
            Err(e) => {
                let why = format!("couldn't make its key socket: {e}");
                host_log::log(&format!(
                    "office server {pid}: {why}; it was stopped and got nothing"
                ));
                running.kill();
                return Err(SpawnError::HandOff(why));
            }
        };
        let key_socket_id = key_socket.id().clone();
        running.write_first_line(mode, &key_socket_id);
        // The server connects once it has read its line.
        let handed = key_socket.hand_off(
            &rule,
            self.hand_off_timeout,
            || running.has_exited(),
            mode.secrets(),
        );
        match handed {
            Ok(HandOff::Sent) => Ok(running),
            Ok(HandOff::Exited) => {
                host_log::log(&format!(
                    "office server {pid} exited before its hand-off on key socket \
                     {key_socket_id} completed; it got nothing"
                ));
                Ok(running)
            }
            Err(why) => {
                host_log::log(&format!(
                    "office server {pid}, key socket {key_socket_id}: {why}; it was stopped and \
                     got nothing"
                ));
                running.kill();
                Err(SpawnError::HandOff(why))
            }
        }
    }
}

impl RunningChild {
    /// The first line on a new server's control pipe, the one place that writes it: `--create`
    /// gets the create message, `--office` the start message, `--discard` the discard message,
    /// each naming the spawn's key socket. It holds no secret.
    fn write_first_line(&mut self, mode: Mode<'_>, key_socket_id: &KeySocketId) {
        let pid = self.process.id();
        let Some(stdin) = self.stdin.as_mut() else {
            return;
        };
        let written = write_host_message(stdin, &mode.first_line(key_socket_id));
        // A child that dies at once makes this fail with a broken pipe; the hand-off then sees
        // its exit, and its stdout closing is reported as a crash, so the error itself adds
        // nothing.
        if let Err(e) = written {
            host_log::log(&format!("couldn't write to office server {pid}: {e}"));
        }
    }

    /// Closes the control pipe: the server shuts down cleanly.
    pub(crate) fn close_stdin(&mut self) {
        self.stdin = None;
    }

    /// Asks the server to stop (closes its stdin), waits for it until `deadline`, then
    /// SIGKILLs it. Always reaps it.
    pub(crate) fn stop(&mut self, deadline: Instant) {
        self.close_stdin();
        if !self.wait_until(deadline) {
            self.kill();
        }
    }

    #[cfg(test)]
    pub(crate) fn pid(&self) -> u32 {
        self.process.id()
    }

    /// Whether the process has exited (it is reaped if so). Doesn't wait.
    pub(crate) fn has_exited(&mut self) -> bool {
        self.process.try_wait().unwrap_or(false)
    }

    /// SIGKILLs and reaps the process.
    pub(crate) fn kill(&mut self) {
        self.close_stdin();
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

/// The stdout reader: forwards control lines until EOF or an error that leaves the pipe out of
/// step. Lines it can't use (bad JSON, another version) are logged and skipped.
fn read_events(stdout: impl Read, generation: u64, signals: &Sender<Signal>) {
    let mut reader = BufReader::new(stdout);
    loop {
        match read_server_event(&mut reader) {
            Ok(Some(event)) => {
                if signals.send(Signal::Event { generation, event }).is_err() {
                    return;
                }
            }
            Ok(None) => break,
            Err(e) if e.closes_stream() => {
                host_log::log(&format!("an office server's control pipe failed: {e}"));
                break;
            }
            Err(e) => host_log::log(&format!("ignored a line from an office server: {e}")),
        }
    }
    let _ = signals.send(Signal::Closed { generation });
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
