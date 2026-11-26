//! One `agentic-org-office` process: spawning it with its pipes, reading its control lines,
//! copying its stderr to the office's log, and stopping it.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use org_office_launch::{CREATE_FLAG, DISCARD_FLAG, JOIN_FLAG, OFFICE_FLAG};
use org_platform_host::admission::{Admission, Child, ChildEnv, SpawnRefused, Started};
use org_proto::location::{self, OFFICE_HOME_ENV, OFFICE_RUNTIME_DIR_ENV};
use org_proto::OfficeSecretsKey;

use crate::cloexec::SPAWN_LOCK;
use crate::host_log;
use crate::logs::RotatingLog;
use crate::server_check::{NoAdmission, ServerCheck};
use crate::sync::lock;

use super::signal::Signal;

/// How often a stop polls whether the child has exited.
const EXIT_POLL: Duration = Duration::from_millis(10);

/// The environment a server gets, besides `ORG_OFFICE_HOME`: only these of the host's own, so
/// nothing else in the login session reaches it (the development key folder included: servers
/// never touch a key store, they get their key in the launch handoff). A worker needs only
/// the user's identity, home, `TMPDIR`, and optional HQ federation listener endpoints (`TASK-52` H-2).
const PASSED_ENV: [&str; 8] = [
    "HOME",
    "USER",
    "LOGNAME",
    "PATH",
    "TMPDIR",
    "LANG",
    "AGENTIC_ORG_FEDERATION_BIND",
    "AGENTIC_ORG_FEDERATION_ADVERTISE",
];

const FEDERATION_BIND_ENV: &str = "AGENTIC_ORG_FEDERATION_BIND";
const FEDERATION_ADVERTISE_ENV: &str = "AGENTIC_ORG_FEDERATION_ADVERTISE";

/// A server's whole environment: the [`PASSED_ENV`] names the host has (read through `host_var`),
/// overridden by any persisted setup `federation` config, then `ORG_OFFICE_HOME` and, if set,
/// `ORG_OFFICE_RUNTIME_DIR`. Nothing else of the host's.
fn server_env(
    home: &Path,
    runtime_dir: Option<&Path>,
    federation: Option<&crate::instance::setup::FederationConfig>,
    host_var: impl Fn(&str) -> Option<OsString>,
) -> Vec<(OsString, OsString)> {
    let mut env: Vec<(OsString, OsString)> = PASSED_ENV
        .iter()
        .filter(|name| {
            federation.is_none()
                || (**name != FEDERATION_BIND_ENV && **name != FEDERATION_ADVERTISE_ENV)
        })
        .filter_map(|name| host_var(name).map(|value| ((*name).into(), value)))
        .collect();
    if let Some(federation) = federation {
        env.push((FEDERATION_BIND_ENV.into(), federation.bind.clone().into()));
        if !federation.advertise.is_empty() {
            env.push((
                FEDERATION_ADVERTISE_ENV.into(),
                federation.advertise.join(",").into(),
            ));
        }
    }
    env.push((OFFICE_HOME_ENV.into(), home.as_os_str().to_owned()));
    if let Some(runtime) = runtime_dir {
        env.push((
            OFFICE_RUNTIME_DIR_ENV.into(),
            runtime.as_os_str().to_owned(),
        ));
    }
    env
}

/// The one admission of a host's office servers: built once, when the host builds its
/// supervisor, and shared by every create, start and discard of that host's lifetime.
///
/// A system that executes verified bytes reads, checks and seals the server image when the
/// admission is built (ADR-0017 decision 7): every server of this host then runs exactly the
/// bytes read at that moment, whatever the program's path names later. Where the loaded image
/// is checked instead, building reads nothing and this changes nothing.
///
/// Reading and hashing a whole program takes time, so the host does it beside its start, not
/// in it: the build begins at once on its own thread, `host.sock` is served meanwhile, and a
/// start that comes first waits for it.
///
/// A host whose admission cannot be built starts no server and still serves `host.sock`:
/// the reason is logged, each start fails with it as a refused server does, and the next
/// start asks again, so the host's give-up rule ends the retries. An admission, once built,
/// is never built again.
pub(crate) struct Images {
    check: Arc<ServerCheck>,
    program: PathBuf,
    built: Mutex<Option<Arc<Admission>>>,
}

impl std::fmt::Debug for Images {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Images")
            .field("program", &self.program)
            .field("built", &lock(&self.built).is_some())
            .finish_non_exhaustive()
    }
}

impl Images {
    /// Begins building the admission of `check`'s servers started from `program`, and says in
    /// the host log why there is none. A check that may start no server has already said so
    /// ([`ServerCheck::describe`]) and builds nothing.
    pub(crate) fn new(check: Arc<ServerCheck>, program: PathBuf) -> Arc<Self> {
        let images = Arc::new(Self {
            check,
            program,
            built: Mutex::new(None),
        });
        if images.check.may_spawn().is_ok() {
            let building = Arc::clone(&images);
            let begun = thread::Builder::new()
                .name("office-images".into())
                .spawn(move || building.build_at_start());
            if begun.is_err() {
                // No thread to build it on: build it here, as a start would.
                images.build_at_start();
            }
        }
        images
    }

    /// The build a host makes at its start, with the reason logged when there is none.
    fn build_at_start(&self) {
        if let Err(none) = self.admission() {
            host_log::log(&format!(
                "no office server can be started: {none}; each start asks again"
            ));
        }
    }

    /// The host's admission, built now if no earlier attempt succeeded. A caller waits for
    /// a build that is under way: there is never more than one.
    pub(crate) fn admission(&self) -> Result<Arc<Admission>, NoAdmission> {
        let mut built = lock(&self.built);
        if let Some(admission) = built.as_ref() {
            return Ok(Arc::clone(admission));
        }
        let admission = Arc::new(self.check.admission(&self.program)?);
        if let Some(line) = self.check.describe_admitted(&self.program, &admission) {
            host_log::log(&line);
        }
        *built = Some(Arc::clone(&admission));
        Ok(admission)
    }
}

/// How to start a server: the admission of its program and the office home it is given.
#[derive(Debug, Clone)]
pub(crate) struct Spawner {
    pub(crate) home: PathBuf,
    pub(crate) runtime_dir: Option<PathBuf>,
    /// Who may get a first line: every child is checked before it's written (`server_check`).
    pub(crate) check: Arc<ServerCheck>,
    /// The admission every server of this host is started and checked through.
    pub(crate) images: Arc<Images>,
    /// The instance's launch rendezvous: every create, start and discard goes through it.
    pub(super) broker: Arc<super::channels::Broker>,
    /// Optional persisted HQ federation config from setup.
    pub(crate) federation: Option<crate::instance::setup::FederationConfig>,
    /// Tests only: variables added to every server's environment after the host's own, so a
    /// test can show what the platform refuses there.
    #[cfg(test)]
    pub(crate) extra_env: Vec<(OsString, OsString)>,
}

/// Why a server wasn't started.
#[derive(Debug)]
pub(crate) enum SpawnError {
    /// It couldn't be spawned (or its pipes' threads couldn't start).
    Io(io::Error),
    /// A refusal, in the check's words (`server_check`): no server may be started, the
    /// platform will not start this image or this environment, or it was spawned and isn't
    /// the server it must be (then it was killed and reaped). Nothing got a line.
    Untrusted(String),
    /// The office has no valid writer custody (a schema-3 record and its reservation marker):
    /// nothing was started. Only protected provisioning reserves custody, so a restart alone
    /// can't fix it.
    NoCustody,
    /// The office's writer custody could not be read (the custody store or the host
    /// registry): nothing was started. It may pass, so it is retried.
    CustodyUnreadable,
}

impl std::fmt::Display for SpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpawnError::Io(e) => write!(f, "{e}"),
            SpawnError::Untrusted(why) => write!(f, "refused to start it: {why}"),
            SpawnError::NoCustody => f.write_str("it has no valid writer custody"),
            SpawnError::CustodyUnreadable => f.write_str("its writer custody can't be read"),
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
            SpawnError::Untrusted(_) | SpawnError::NoCustody | SpawnError::CustodyUnreadable => {
                io::Error::other(e.to_string())
            }
        }
    }
}

/// Which server to start, with what its launch handoff carries. Every launch goes through the
/// instance's rendezvous; the handoff borrows the secrets (never copies them).
#[derive(Clone, Copy)]
pub(crate) enum Mode<'a> {
    /// Create the HQ office: its handoff carries the new office's secrets key and recovery code.
    Create(&'a CreateLine),
    /// Join or re-enroll a satellite office: its handoff carries the office's secrets key and
    /// the one-time invitation token (`TASK-52` H-1).
    Join(&'a JoinLine),
    /// Serve the office with this id: its handoff carries its secrets key from the key store,
    /// or `null` when the store has none for it (the start proceeds).
    Office(&'a str, Option<&'a OfficeSecretsKey>),
    /// Remove an unfinished office, then exit. The discard handoff never carries a key.
    Discard(&'a str),
}

/// A satellite join or D25 re-enrollment launch: non-secret join metadata plus the office's
/// secrets key and one-time invitation token delivered over the verified rendezvous (`TASK-52` H-1).
pub(crate) struct JoinLine(Box<SatelliteJoinLaunch>);
struct SatelliteJoinLaunch {
    first: org_office_launch::JoinLine,
    office_secrets_key: OfficeSecretsKey,
    invitation_token: org_host_control::InvitationTokenSecret,
}

impl JoinLine {
    pub(crate) fn satellite(
        first: org_office_launch::JoinLine,
        office_secrets_key: OfficeSecretsKey,
        invitation_token: org_host_control::InvitationTokenSecret,
    ) -> Self {
        Self(Box::new(SatelliteJoinLaunch {
            first,
            office_secrets_key,
            invitation_token,
        }))
    }

    pub(crate) fn office_id(&self) -> &str {
        &self.0.first.office.office_id
    }

    fn handoff_secrets(&self) -> super::channels::HandoffSecrets<'_> {
        super::channels::HandoffSecrets::Join {
            office_secrets_key: &self.0.office_secrets_key,
            invitation_token: self.0.invitation_token.expose(),
        }
    }
}

/// The HQ create: the launch first line (no secret: the office, organization, names, role and HQ
/// context) plus the new office's secrets key and recovery code, which the rendezvous handoff
/// delivers. The first line is the only source of the office's details, so the line the worker
/// reads and the key the handoff sends always name the same office.
///
/// It is kept as one value, built once, because it owns its key and [`OfficeSecretsKey`] isn't `Clone`: the
/// handoff borrows it, and dropping it zeroes it. It lives on the heap (a `Box`), so moving it
/// moves a pointer, not the secrets. Building it still leaves the usual stack copies behind (see
/// `OfficeSecretsKey`'s docs).
pub(crate) struct CreateLine(Box<HqCreate>);
struct HqCreate {
    first: org_office_launch::CreateLine,
    office_secrets_key: OfficeSecretsKey,
    recovery: org_proto::RecoveryCode,
}

impl CreateLine {
    pub(crate) fn hq(
        first: org_office_launch::CreateLine,
        office_secrets_key: OfficeSecretsKey,
        recovery: org_proto::RecoveryCode,
    ) -> Self {
        Self(Box::new(HqCreate {
            first,
            office_secrets_key,
            recovery,
        }))
    }

    /// The id of the office it creates.
    pub(crate) fn office_id(&self) -> &str {
        &self.0.first.office.office_id
    }

    fn org_id(&self) -> &str {
        &self.0.first.org_id
    }

    fn owner_member_id(&self) -> &str {
        &self.0.first.owner_member_id
    }

    /// The new office's secrets key and recovery code, for the handoff. Never copy them.
    fn handoff_secrets(&self) -> super::channels::HandoffSecrets<'_> {
        super::channels::HandoffSecrets::Create {
            office_secrets_key: &self.0.office_secrets_key,
            recovery: &self.0.recovery,
        }
    }
}

/// A running server process. Dropping it kills and reaps the process if it's still running,
/// so a server is never left behind unsupervised.
pub(crate) struct RunningChild {
    process: Child,
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

    pub(crate) fn update_owner(
        &self,
        office: &str,
        org_id: &str,
        owner_account: &str,
        owner_member: &str,
    ) -> io::Result<()> {
        self.broker
            .update_owner(office, org_id, owner_account, owner_member)
    }

    /// Whether `office` may start or be created: it holds valid writer custody and, for a
    /// create, that custody is bound to the create's organization and owner member. Logged
    /// with the office, never with a path or a record.
    pub(crate) fn custody(
        &self,
        office: &str,
        create: Option<(&str, &str)>,
    ) -> Result<(), SpawnError> {
        match self.broker.custody(office) {
            Ok(bound)
                if create.is_none_or(|(organization, member)| {
                    bound.organization_id == organization && bound.owner_member == member
                }) =>
            {
                Ok(())
            }
            Err(crate::writer::AdmitError::Unreadable) => {
                host_log::log(&format!(
                    "didn't start office {office}: its writer custody can't be read now; it \
                     is tried again"
                ));
                Err(SpawnError::CustodyUnreadable)
            }
            _ => {
                host_log::log(&format!(
                    "didn't start office {office}: it has no valid writer custody (only \
                     protected provisioning reserves it; a restart alone can't)"
                ));
                Err(SpawnError::NoCustody)
            }
        }
    }

    /// Starts a server. Its control lines go to `signals` tagged with `generation`; its stderr
    /// goes to `log`. Its first line (per [`Mode`]) is written at once, once it passed the check.
    ///
    /// The host's admission starts it ([`Images`]): on macOS suspended, so
    /// not one instruction of its image
    /// runs before the check (`server_check`) unless another process resumes it (the residual
    /// risk in the admission adapter's docs): a child that fails is killed still suspended, having got
    /// nothing. One that passes is resumed, and its token checked again right before the line.
    /// Where the verified bytes are executed, the image was checked when the admission was
    /// built and the child runs those bytes from its first instruction.
    ///
    /// A check that may start no server, an image the platform refuses and a child it refuses
    /// are all [`SpawnError::Untrusted`], in the check's words: nothing got a line.
    ///
    /// Every office needs writer custody: a start or a create of an office without it is
    /// [`SpawnError::NoCustody`], before anything is made for a child. A discard claims none.
    pub(crate) fn spawn(
        &self,
        mode: Mode<'_>,
        generation: u64,
        signals: Sender<Signal>,
        log: Arc<Mutex<RotatingLog>>,
    ) -> Result<RunningChild, SpawnError> {
        match mode {
            Mode::Office(id, _) => self.custody(id, None)?,
            Mode::Create(line) => self.custody(
                line.office_id(),
                Some((line.org_id(), line.owner_member_id())),
            )?,
            Mode::Join(line) => self.custody(line.office_id(), None)?,
            Mode::Discard(_) => {}
        }
        self.check.may_spawn().map_err(|why| {
            host_log::log(&format!("didn't start an office server: {why}"));
            SpawnError::Untrusted(why)
        })?;
        // Before anything is made for the child: a host without an admission starts nothing.
        let admission = self.images.admission().map_err(|none| match none {
            NoAdmission::Refused(why) => {
                host_log::log(&format!("didn't start an office server: {why}"));
                SpawnError::Untrusted(why)
            }
            NoAdmission::Unavailable(error) => SpawnError::Io(error),
        })?;
        let rendezvous = self.broker.bind()?;
        let args: Vec<&OsStr> = match mode {
            Mode::Create(_) => vec![OsStr::new(CREATE_FLAG)],
            Mode::Join(_) => vec![OsStr::new(JOIN_FLAG)],
            Mode::Office(id, ..) => vec![OsStr::new(OFFICE_FLAG), OsStr::new(id)],
            Mode::Discard(id) => vec![OsStr::new(DISCARD_FLAG), OsStr::new(id)],
        };
        let env = self.child_env()?;
        // See `cloexec`: no fd of ours may be half-made while a child is spawned. The
        // admission holds `SPAWN_LOCK` while it makes the pipes and the process, and no
        // longer. It checks the image before any of it has run, kills a child that fails
        // (still suspended) and lets one that passes run.
        let started = admission.spawn(&args, &env, &SPAWN_LOCK).map_err(|error| {
            match SpawnRefused::of(&error) {
                Some(refused) => {
                    let why = self.check.why(&refused.refusal);
                    host_log::log(&format!(
                        "refused office server {}: {why}; it was stopped before it ran and got \
                         nothing",
                        refused.pid
                    ));
                    SpawnError::Untrusted(why)
                }
                None => SpawnError::Io(error),
            }
        })?;
        let Started {
            child: process,
            stdin,
            stdout,
            stderr,
        } = started;
        let pid = process.id();
        let mut running = RunningChild {
            process,
            stdin: Some(stdin),
            channel: None,
            generation,
            started: Instant::now(),
        };
        // Events come over the rendezvous; stdout carries nothing the host reads.
        drop(stdout);
        thread::Builder::new()
            .name(format!("office-{pid}-err"))
            .spawn(move || copy_stderr(stderr, &log))?;

        // Still the image that was checked (no exec since), right before the line.
        if let Err(refusal) = admission.recheck(&running.process) {
            let why = self.check.why(&refusal);
            host_log::log(&format!(
                "refused office server {pid}: {why}; it was stopped and got nothing"
            ));
            running.kill();
            return Err(SpawnError::Untrusted(why));
        }
        self.attach_control(&mut running, mode, rendezvous, &admission, signals)?;
        Ok(running)
    }
    /// A server's whole environment ([`server_env`]). A variable that changes how the loader
    /// runs the image is refused by the platform on every system: nothing is started then.
    fn child_env(&self) -> Result<ChildEnv, SpawnError> {
        let variables = server_env(
            &self.home,
            self.runtime_dir.as_deref(),
            self.federation.as_ref(),
            |name| std::env::var_os(name),
        );
        #[cfg(test)]
        let variables = variables.into_iter().chain(self.extra_env.iter().cloned());
        let mut env = ChildEnv::new();
        for (name, value) in variables {
            if let Err(error) = env.set(name.clone(), value) {
                // The name only: a value is never logged.
                let why = format!("{error} ({})", name.to_string_lossy());
                host_log::log(&format!("didn't start an office server: {why}"));
                return Err(SpawnError::Untrusted(why));
            }
        }
        Ok(env)
    }

    fn attach_control(
        &self,
        running: &mut RunningChild,
        mode: Mode<'_>,
        rendezvous: super::channels::Rendezvous,
        admission: &Admission,
        launch_signals: Sender<Signal>,
    ) -> Result<(), SpawnError> {
        let broker = &self.broker;
        match mode {
            Mode::Office(id, office_secrets_key) => {
                let first =
                    org_office_launch::HostFirstLine::Start(org_office_launch::OfficeLine {
                        instance_id: broker.instance.clone(),
                        office_id: id.into(),
                        rendezvous: rendezvous
                            .path()
                            .to_str()
                            .ok_or_else(|| io::Error::other("invalid rendezvous path"))?
                            .into(),
                    });
                org_office_launch::write_first_line(
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
                    admission,
                    &mut running.process,
                    super::channels::HandoffSecrets::Start(office_secrets_key),
                    running.generation,
                    launch_signals,
                )?);
            }
            Mode::Create(line) => {
                let context = rendezvous;
                let mut first = line.0.first.clone();
                if first.office.instance_id != broker.instance {
                    return Err(io::Error::other("HQ create scope mismatch").into());
                }
                first.office.rendezvous = context
                    .path()
                    .to_str()
                    .ok_or_else(|| io::Error::other("invalid rendezvous"))?
                    .into();
                org_office_launch::write_first_line(
                    running
                        .stdin
                        .as_mut()
                        .ok_or_else(|| io::Error::other("missing stdin"))?,
                    &org_office_launch::HostFirstLine::Create(first),
                )
                .map_err(|_| io::Error::other("HQ create line refused"))?;
                running.channel = Some(broker.accept(
                    context,
                    line.office_id(),
                    admission,
                    &mut running.process,
                    line.handoff_secrets(),
                    running.generation,
                    launch_signals,
                )?);
            }
            Mode::Join(line) => {
                let context = rendezvous;
                let mut first = line.0.first.clone();
                if first.office.instance_id != broker.instance {
                    return Err(io::Error::other("satellite join scope mismatch").into());
                }
                first.office.rendezvous = context
                    .path()
                    .to_str()
                    .ok_or_else(|| io::Error::other("invalid rendezvous"))?
                    .into();
                org_office_launch::write_first_line(
                    running
                        .stdin
                        .as_mut()
                        .ok_or_else(|| io::Error::other("missing stdin"))?,
                    &org_office_launch::HostFirstLine::Join(first),
                )
                .map_err(|_| io::Error::other("satellite join line refused"))?;
                running.channel = Some(broker.accept(
                    context,
                    line.office_id(),
                    admission,
                    &mut running.process,
                    line.handoff_secrets(),
                    running.generation,
                    launch_signals,
                )?);
            }
            // The discard handoff carries no secret: the key store's key (if any) stays here.
            Mode::Discard(id) => {
                let first =
                    org_office_launch::HostFirstLine::Discard(org_office_launch::OfficeLine {
                        // A discard never claims writer custody: the host keeps the
                        // custody record and the folder's `selected-writer.required` marker.
                        instance_id: broker.instance.clone(),
                        office_id: id.into(),
                        rendezvous: rendezvous
                            .path()
                            .to_str()
                            .ok_or_else(|| io::Error::other("invalid rendezvous path"))?
                            .into(),
                    });
                org_office_launch::write_first_line(
                    running
                        .stdin
                        .as_mut()
                        .ok_or_else(|| io::Error::other("missing worker stdin"))?,
                    &first,
                )
                .map_err(|_| io::Error::other("worker discard line failed"))?;
                broker.discard(
                    rendezvous,
                    admission,
                    &mut running.process,
                    running.generation,
                    launch_signals,
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

#[cfg(test)]
mod tests;
