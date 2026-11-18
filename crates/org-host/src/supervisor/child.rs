//! One `agentic-org-office` process: spawning it with its pipes, reading its control lines,
//! copying its stderr to the office's log, and stopping it.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use org_office_launch::{
    read_server_event, write_host_line, CreateOffice, HostLine, ServerEvent, CREATE_FLAG,
    DISCARD_FLAG, OFFICE_FLAG,
};
use org_proto::location::{self, OFFICE_HOME_ENV, OFFICE_RUNTIME_DIR_ENV};
use org_proto::{AppToken, DbKey, APP_TOKEN_BYTES, DB_KEY_BYTES};

use crate::cloexec::SPAWN_LOCK;
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
/// never touch a key store, they get their key on the first line). SQLCipher needs only the
/// user's identity and home; `TMPDIR` is the user's temporary folder.
const PASSED_ENV: [&str; 6] = ["HOME", "USER", "LOGNAME", "PATH", "TMPDIR", "LANG"];

/// How to start a server: the program and the office home it is given.
#[derive(Debug, Clone)]
pub(crate) struct Spawner {
    pub(crate) program: PathBuf,
    pub(crate) home: PathBuf,
    pub(crate) runtime_dir: Option<PathBuf>,
    /// Who may get a first line: every child is checked before it's written (`server_check`).
    pub(crate) check: Arc<ServerCheck>,
    pub(super) broker: Option<Arc<super::channels::Broker>>,
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

/// Which server to start, with what its first line carries. Every first line is written with
/// `write_host_line`, which borrows the secrets (never copies them) and zeroes its encoded line.
#[derive(Clone, Copy)]
pub(crate) enum Mode<'a> {
    /// Create an office: its first line is this create message (the new office's app token and
    /// database key).
    Create(&'a CreateLine),
    /// Serve the office with this id: its first line is a start with the office's app token for
    /// this host run and its database key from the key store.
    Office(&'a str, &'a AppToken, &'a DbKey),
    /// Remove an unfinished office, then exit. A legacy v2 first line is a discard with the
    /// office's database key, or `null` when the host has none; the v3 discard handoff never
    /// carries the key.
    Discard(&'a str, Option<&'a DbKey>),
}

/// The `--create` first line: the new office's create message, holding its app token for this
/// host run and its database key.
///
/// It is kept as the message itself, built once, because the message owns its secrets and
/// neither [`AppToken`] nor [`DbKey`] is `Clone`: writing it never copies them, and dropping it
/// zeroes them. It lives on the heap (a `Box`), so moving it moves a pointer, not the secrets.
/// Building it still leaves the usual stack copies behind (see `AppToken`'s docs).
pub(crate) struct CreateLine(Box<CreateOffice>, Option<HqCreate>);
pub(crate) struct HqCreate {
    pub first: org_office_launch::v3::CreateLine,
    pub recovery: org_proto::RecoveryCode,
}

impl CreateLine {
    pub(crate) fn new(create: CreateOffice) -> Self {
        Self(Box::new(create), None)
    }

    pub(crate) fn hq(create: CreateOffice, context: HqCreate) -> Self {
        Self(Box::new(create), Some(context))
    }

    /// The id of the office it creates.
    pub(crate) fn office_id(&self) -> &str {
        &self.0.office_id
    }

    /// The new office's database key, for the key store. Never copy it.
    pub(crate) fn db_key(&self) -> &DbKey {
        &self.0.db_key
    }

    /// Zeroes the database key in place, once the first line is written (or can't be): from then
    /// on only the key store holds it.
    pub(crate) fn drop_key(&mut self) {
        self.1.take(); // the delivered recovery-code copy is no longer needed
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
    channel: Option<Arc<super::channels::Endpoint>>,
    pub(crate) generation: u64,
    pub(crate) started: Instant,
}

impl Spawner {
    pub(crate) fn socket_path(&self, office_id: &str) -> Option<PathBuf> {
        if org_office_launch::is_selected_entity_id(office_id) {
            let runtime = self
                .runtime_dir
                .clone()
                .unwrap_or_else(|| self.home.join("run"));
            return location::selected_office_socket_in(&runtime, office_id);
        }
        match &self.runtime_dir {
            Some(runtime) => location::office_socket_in(runtime, office_id),
            None => location::office_socket_path(&self.home, office_id),
        }
    }

    /// Starts a server. Its control lines go to `signals` tagged with `generation`; its stderr
    /// goes to `log`. Its first line (per [`Mode`]) is written at once, once it passed the check.
    ///
    /// It is started suspended (`spawn::spawn_suspended`), so not one instruction of its image
    /// runs before the check (`server_check`) unless another process resumes it (the residual
    /// risk in `spawn`'s docs): a child that fails is killed still suspended, having got
    /// nothing. One that passes is resumed, and its token checked again right before the line.
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
        let rendezvous = match (&self.broker, mode) {
            (Some(broker), Mode::Office(..) | Mode::Discard(..)) => Some(broker.bind()?),
            (Some(broker), Mode::Create(line)) if line.1.is_some() => Some(broker.bind()?),
            (_, Mode::Create(line)) if line.1.is_some() => {
                return Err(io::Error::other("registered HQ creation requires launch v3").into())
            }
            _ => None,
        };
        let args: Vec<&OsStr> = match mode {
            Mode::Create(_) => vec![OsStr::new(CREATE_FLAG)],
            Mode::Office(id, ..) => vec![OsStr::new(OFFICE_FLAG), OsStr::new(id)],
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
            stdin,
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
            stdin: Some(stdin),
            channel: None,
            generation,
            started: Instant::now(),
        };
        running.process.resume()?;
        let v3_signals = signals.clone();
        if rendezvous.is_none() {
            thread::Builder::new()
                .name(format!("office-{pid}-out"))
                .spawn(move || read_events(stdout, generation, &signals))?;
        } else {
            drop(stdout);
        }
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
        self.attach_control(&mut running, mode, rendezvous, admitted, v3_signals)?;
        Ok(running)
    }
    fn attach_control(
        &self,
        running: &mut RunningChild,
        mode: Mode<'_>,
        rendezvous: Option<super::channels::Rendezvous>,
        admitted: Option<crate::peer::AuditToken>,
        v3_signals: Sender<Signal>,
    ) -> Result<(), SpawnError> {
        match (&self.broker, rendezvous, mode) {
            (Some(broker), Some(rendezvous), Mode::Office(id, _, db_key)) => {
                let first = org_office_launch::v3::HostFirstLine::Start(
                    org_office_launch::v3::OfficeLine {
                        id_profile: org_office_launch::is_selected_entity_id(id)
                            .then_some(org_office_launch::v3::IdProfile::SelectedV1),
                        selected_writer_required: broker.writer_required(id)?,
                        instance_id: broker.instance.clone(),
                        office_id: id.into(),
                        rendezvous: rendezvous
                            .path()
                            .to_str()
                            .ok_or_else(|| io::Error::other("invalid rendezvous path"))?
                            .into(),
                    },
                );
                org_office_launch::v3::write_first_line(
                    running
                        .stdin
                        .as_mut()
                        .ok_or_else(|| io::Error::other("missing worker stdin"))?,
                    &first,
                )
                .map_err(|_| io::Error::other("worker start line failed"))?;
                running.channel = Some(broker.accept(
                    rendezvous,
                    id,
                    running.process.id(),
                    admitted,
                    &self.check,
                    db_key,
                    None,
                    running.generation,
                    v3_signals,
                )?);
            }
            (Some(broker), Some(context), Mode::Create(line)) => {
                let hq = line
                    .1
                    .as_ref()
                    .ok_or_else(|| io::Error::other("missing HQ creation context"))?;
                let mut first = hq.first.clone();
                if first.office.instance_id != broker.instance
                    || first.office.office_id != line.0.office_id
                    || first.org_id != line.0.org_id
                    || first.org_name != line.0.org_name
                    || first.office_name != line.0.office_name
                    || first.role != line.0.role
                {
                    return Err(io::Error::other("HQ create scope mismatch").into());
                }
                first.office.rendezvous = context
                    .path()
                    .to_str()
                    .ok_or_else(|| io::Error::other("invalid rendezvous"))?
                    .into();
                org_office_launch::v3::write_first_line(
                    running
                        .stdin
                        .as_mut()
                        .ok_or_else(|| io::Error::other("missing stdin"))?,
                    &org_office_launch::v3::HostFirstLine::Create(first),
                )
                .map_err(|_| io::Error::other("HQ create line refused"))?;
                running.channel = Some(broker.accept(
                    context,
                    line.office_id(),
                    running.process.id(),
                    admitted,
                    &self.check,
                    line.db_key(),
                    Some(&hq.recovery),
                    running.generation,
                    v3_signals,
                )?);
            }
            // The discard handoff carries no secret: the key store's key (if any) stays here.
            (Some(broker), Some(rendezvous), Mode::Discard(id, _)) => {
                let first = org_office_launch::v3::HostFirstLine::Discard(
                    org_office_launch::v3::OfficeLine {
                        id_profile: org_office_launch::is_selected_entity_id(id)
                            .then_some(org_office_launch::v3::IdProfile::SelectedV1),
                        // As over v2: a discard never claims writer custody, so the worker
                        // keeps refusing a folder marked `selected-writer.required`.
                        selected_writer_required: false,
                        instance_id: broker.instance.clone(),
                        office_id: id.into(),
                        rendezvous: rendezvous
                            .path()
                            .to_str()
                            .ok_or_else(|| io::Error::other("invalid rendezvous path"))?
                            .into(),
                    },
                );
                org_office_launch::v3::write_first_line(
                    running
                        .stdin
                        .as_mut()
                        .ok_or_else(|| io::Error::other("missing worker stdin"))?,
                    &first,
                )
                .map_err(|_| io::Error::other("worker discard line failed"))?;
                super::channels::Broker::discard(
                    rendezvous,
                    running.process.id(),
                    admitted,
                    &self.check,
                    running.generation,
                    v3_signals,
                )?;
            }
            _ => running.write_first_line(mode),
        }
        Ok(())
    }
}

impl RunningChild {
    /// The first line on a new server's control pipe, the one place that writes it: `--create`
    /// gets the create message, `--office` the start message (the app token and the database
    /// key), `--discard` the discard message (the database key or `null`). `write_host_line`
    /// zeroes its encoded copy of the line.
    fn write_first_line(&mut self, mode: Mode<'_>) {
        let pid = self.process.id();
        let Some(stdin) = self.stdin.as_mut() else {
            return;
        };
        let line = match mode {
            Mode::Create(CreateLine(create, None)) => HostLine::Create(create),
            Mode::Create(CreateLine(_, Some(_))) => return,
            Mode::Office(_, app_token, db_key) => HostLine::Start { app_token, db_key },
            Mode::Discard(_, db_key) => HostLine::Discard { db_key },
        };
        let written = write_host_line(stdin, &line);
        // A child that dies at once makes this fail with a broken pipe; its stdout then closes
        // and that is reported as a crash, so the error itself adds nothing. The error never
        // holds a secret: it is an I/O error or a fixed message.
        if let Err(e) = written {
            host_log::log(&format!("couldn't write to office server {pid}: {e}"));
        }
    }

    /// Closes the control pipe: the server shuts down cleanly.
    pub(crate) fn close_stdin(&mut self) {
        if let Some(channel) = self.channel.take() {
            channel.fail();
        }
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
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
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
