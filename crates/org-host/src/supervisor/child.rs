//! One `agentic-org-office` process: spawning it with its pipes, reading its control lines,
//! copying its stderr to the office's log, and stopping it.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use org_office_launch::{CreateOffice, ServerEvent, CREATE_FLAG, DISCARD_FLAG, OFFICE_FLAG};
use org_proto::location::{self, OFFICE_HOME_ENV, OFFICE_RUNTIME_DIR_ENV};
use org_proto::DbKey;

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
    /// The instance's launch rendezvous: every create, start and discard goes through it.
    pub(super) broker: Arc<super::channels::Broker>,
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

/// Which server to start, with what its launch handoff carries. Every launch goes through the
/// instance's rendezvous; the handoff borrows the secrets (never copies them).
#[derive(Clone, Copy)]
pub(crate) enum Mode<'a> {
    /// Create the HQ office: its handoff carries the new office's database key and recovery code.
    Create(&'a CreateLine),
    /// Serve the office with this id: its handoff carries its database key from the key store.
    Office(&'a str, &'a DbKey),
    /// Remove an unfinished office, then exit. The discard handoff never carries a key.
    Discard(&'a str),
}

/// The HQ create: the new office's details and database key, carried in the v2 `CreateOffice`
/// message (its `app_token` is a zero placeholder the v3 handoff never sends; A1.5 removes the
/// carrier), plus the v3 first line and recovery code the rendezvous handoff delivers.
///
/// It is kept as one value, built once, because the message owns its key and [`DbKey`] isn't
/// `Clone`: the handoff borrows it, and dropping it zeroes it. It lives on the heap (a `Box`), so
/// moving it moves a pointer, not the secrets. Building it still leaves the usual stack copies
/// behind (see `DbKey`'s docs).
pub(crate) struct CreateLine(Box<CreateOffice>, Option<HqCreate>);
pub(crate) struct HqCreate {
    pub first: org_office_launch::v3::CreateLine,
    pub recovery: org_proto::RecoveryCode,
}

impl CreateLine {
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
        // The only creation is HQ provisioning: a create line always carries its HQ context.
        if let Mode::Create(CreateLine(_, None)) = mode {
            return Err(io::Error::other("a create needs its HQ creation context").into());
        }
        let rendezvous = self.broker.bind()?;
        let args: Vec<&OsStr> = match mode {
            Mode::Create(_) => vec![OsStr::new(CREATE_FLAG)],
            Mode::Office(id, ..) => vec![OsStr::new(OFFICE_FLAG), OsStr::new(id)],
            Mode::Discard(id) => vec![OsStr::new(DISCARD_FLAG), OsStr::new(id)],
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
        // Events come over the rendezvous; stdout carries nothing the host reads.
        drop(stdout);
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
        self.attach_control(&mut running, mode, rendezvous, admitted, signals)?;
        Ok(running)
    }
    fn attach_control(
        &self,
        running: &mut RunningChild,
        mode: Mode<'_>,
        rendezvous: super::channels::Rendezvous,
        admitted: Option<crate::peer::AuditToken>,
        v3_signals: Sender<Signal>,
    ) -> Result<(), SpawnError> {
        let broker = &self.broker;
        match mode {
            Mode::Office(id, db_key) => {
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
                    &mut || running.process.try_wait().unwrap_or(false),
                )?);
            }
            Mode::Create(line) => {
                let context = rendezvous;
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
                    &mut || running.process.try_wait().unwrap_or(false),
                )?);
            }
            // The discard handoff carries no secret: the key store's key (if any) stays here.
            Mode::Discard(id) => {
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
                    &mut || running.process.try_wait().unwrap_or(false),
                )?;
            }
        }
        Ok(())
    }
}

impl RunningChild {
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
