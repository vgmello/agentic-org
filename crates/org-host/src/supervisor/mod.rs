//! Supervision: one `agentic-org-office` per office, each watched by its own thread
//! (`office`), plus the short runs around them: `--create` makes a new office, `--discard`
//! removes an unfinished one.
//!
//! The host hands a server no app token: a launch handoff carries only the office's
//! secrets key (and, for the HQ create, its recovery code) over the instance's rendezvous.
//!
//! Each office's secrets key lives in the host's key store ([`OfficeSecretsKeys`], the host's own
//! abstraction: the supervisor never knows which store it is). A create stores the new key
//! before its server starts; every start and discard loads it just before the spawn, and drops
//! it once the first line is written. A start whose key is missing, or whose item isn't a key,
//! still spawns the server, whose handoff carries `null`. The supervisor
//! deletes a key **only** after the office's `--discard` reported `discarded`.
//!
//! Status per office ([`OfficeState`]): `starting` (spawned, no `ready` yet) → `active`
//! (`ready`, child alive) → `offline` (exited, a restart pending at `next_restart_at`) →
//! `unavailable` (a fatal failure, or the give-up rule; until `office.restart` or the next host
//! run), each `offline` or `unavailable` with its reason. Every change is announced to the
//! [`StatusObserver`]. Shutdown closes every child's stdin, waits up to the grace period, then
//! SIGKILLs what's left.

pub(crate) mod channels;
mod child;
mod office;
mod state;

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use org_office_launch::{FailCode, OfficeFile, ServerEvent};
use org_proto::{OfficeStatus, OfficeStatusReason};

use crate::host_log;
use crate::ids::handle_of;
use crate::keys::OfficeSecretsKeys;
use crate::logs::RotatingLog;
use crate::server_check::ServerCheck;
use crate::sync::lock;
use crate::timings::Timings;
use child::{CreateLine, Mode, RunningChild, Signal, Spawner};
use office::{Adopted, OfficeRunner};
use state::{Observer, RestartRequest, SharedState};
pub(crate) use state::{OfficeState, StatusObserver};

/// What the supervisor needs to start servers.
#[derive(Clone)]
pub(crate) struct SupervisorConfig {
    pub(crate) program: PathBuf,
    pub(crate) home: PathBuf,
    pub(crate) runtime_dir: Option<PathBuf>,
    pub(crate) logs_dir: PathBuf,
    pub(crate) log_max_bytes: u64,
    pub(crate) timings: Timings,
    /// Where every office's secrets key is kept.
    pub(crate) keys: Arc<dyn OfficeSecretsKeys>,
    /// How every server is checked before it gets its first line (`server_check`).
    pub(crate) server_check: Arc<ServerCheck>,
    /// Whether [`Supervisor::sweep_orphan_keys`] may delete keys (see `keys::sweep_orphans`
    /// for when that's safe).
    pub(crate) sweep_orphans: bool,
}

pub(crate) struct Supervisor {
    config: SupervisorConfig,
    /// `None` on a host with no registered instance: it starts and discards no office server
    /// (setup must run first). Every server is launched over the instance's rendezvous.
    spawner: Option<Spawner>,
    state: Mutex<State>,
    /// One log per socket handle, shared by every run of that office (`--create`, `--office`,
    /// `--discard`) while any of them still writes it, so two writers never rotate the same
    /// file apart. Held weakly: a log (and its open file) goes once nothing writes it.
    logs: Mutex<HashMap<String, Weak<Mutex<RotatingLog>>>>,
    observer: Observer,
    protected_offices: std::collections::BTreeSet<String>,
}

#[derive(Default)]
struct State {
    stopping: bool,
    offices: HashMap<String, Slot>,
    /// `--create` and `--discard` runs in flight, so shutdown can stop them too.
    runs: HashMap<u64, Sender<Signal>>,
    next_run: u64,
}

/// What the supervisor keeps per office for this host run (in memory only).
struct Slot {
    state: Arc<SharedState>,
    signals: Sender<Signal>,
    thread: Option<JoinHandle<()>>,
}

/// What `office.restart` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RestartOutcome {
    /// The office is being started again.
    Restarting,
    /// It isn't `offline` or `unavailable` (e.g. it's `starting` or `active`).
    Conflict,
    /// It's `unavailable` for a reason a restart can't fix.
    CantFix(OfficeStatusReason),
    /// It isn't supervised.
    NotSupervised,
}

/// How a `--create` run ended.
pub(crate) enum CreateOutcome<'a> {
    /// The office is stored; its server is still running, waiting to be adopted.
    Created(Box<Adoptable<'a>>),
    /// The server reported a failure and has exited.
    Failed,
    /// The server exited without `created` or `failed`.
    Crashed,
    /// No `created` or `failed` in time; the server was killed.
    TimedOut,
    /// The host is shutting down; the server was stopped.
    Stopped,
}

/// Why a `--create` run didn't start.
#[derive(Debug)]
pub(crate) enum CreateStartError {
    /// Nothing was stored and nothing started: the host is stopping, or the key store refused
    /// the new key (an item for that id already exists, which isn't this create's: it is left
    /// as it is).
    NotStarted(String),
    /// The new key is stored, but its server couldn't be started. The unfinished office must be
    /// discarded so the key is deleted (only after `discarded`).
    KeyStored(io::Error),
}

impl std::fmt::Display for CreateStartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CreateStartError::NotStarted(why) => f.write_str(why),
            CreateStartError::KeyStored(e) => write!(f, "{e} (its key is stored)"),
        }
    }
}

/// A `--create` run in flight.
pub(crate) struct PendingCreate<'a> {
    registration: Registration<'a>,
    child: RunningChild,
    signals: Receiver<Signal>,
    sender: Sender<Signal>,
    log: Arc<Mutex<RotatingLog>>,
}

/// A server that has created its office, to hand to [`Supervisor::adopt`]. Dropping it instead
/// kills the server.
pub(crate) struct Adoptable<'a> {
    pub(crate) office: OfficeFile,
    registration: Registration<'a>,
    child: RunningChild,
    signals: Receiver<Signal>,
    sender: Sender<Signal>,
    log: Arc<Mutex<RotatingLog>>,
    backlog: Vec<Signal>,
    ready: bool,
}

/// How a `--discard` run ended. Only [`DiscardOutcome::Discarded`] deletes the office's key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiscardOutcome {
    /// The unfinished office is gone, and so is its key.
    Discarded,
    /// The key store couldn't say whether it has a key for the office (or its item isn't a key),
    /// so no server was started: everything is kept.
    KeyUnreadable,
    /// The server refused or failed (e.g. `already_exists`: the folder holds `office.json`).
    Failed(FailCode),
    /// The server exited without `discarded` or `failed`.
    Crashed,
    /// No answer in time; the server was killed.
    TimedOut,
    /// The host is shutting down; the server was stopped.
    Stopped,
}

/// A short run's entry in [`State::runs`], removed when dropped.
struct Registration<'a> {
    supervisor: &'a Supervisor,
    key: Option<u64>,
}

impl Drop for Registration<'_> {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            lock(&self.supervisor.state).runs.remove(&key);
        }
    }
}

impl Supervisor {
    pub(crate) fn socket_path(&self, office_id: &str) -> Option<PathBuf> {
        self.spawner.as_ref()?.socket_path(office_id)
    }

    /// A supervisor for the registered instance `instance`: every create, start and discard
    /// goes through that instance's launch rendezvous.
    pub(crate) fn new(config: SupervisorConfig, instance: &str) -> Self {
        let runtime = config
            .runtime_dir
            .clone()
            .unwrap_or_else(|| config.home.join("run"));
        let spawner = Spawner {
            program: config.program.clone(),
            home: config.home.clone(),
            runtime_dir: config.runtime_dir.clone(),
            check: Arc::clone(&config.server_check),
            broker: Arc::new(channels::Broker::new(instance.into(), runtime)),
        };
        Self::with_spawner(config, Some(spawner))
    }

    /// A supervisor for a host with no registered instance: it never starts or discards an
    /// office server, and every attempt fails saying setup must run first.
    pub(crate) fn unregistered(config: SupervisorConfig) -> Self {
        Self::with_spawner(config, None)
    }

    fn with_spawner(config: SupervisorConfig, spawner: Option<Spawner>) -> Self {
        Self {
            config,
            spawner,
            state: Mutex::new(State::default()),
            logs: Mutex::new(HashMap::new()),
            observer: Arc::new(OnceLock::new()),
            protected_offices: std::collections::BTreeSet::new(),
        }
    }

    fn spawner(&self) -> io::Result<&Spawner> {
        self.spawner.as_ref().ok_or_else(|| {
            io::Error::other("no registered instance on this host: setup must run first")
        })
    }

    pub(crate) fn with_protected_offices(
        mut self,
        offices: std::collections::BTreeSet<String>,
    ) -> Self {
        self.protected_offices = offices;
        self
    }
    pub(crate) fn endpoint(&self, office: &str) -> Option<Arc<channels::Endpoint>> {
        self.spawner.as_ref()?.broker.endpoint(office)
    }
    pub(crate) fn with_writers(mut self, writers: Arc<crate::writer::Writers>) -> Self {
        if let Some(spawner) = &mut self.spawner {
            Arc::get_mut(&mut spawner.broker)
                .expect("unstarted broker")
                .writers = Some(writers);
        }
        self
    }

    /// Where every office's state changes are announced from now on (set once).
    pub(crate) fn observe(&self, observer: Weak<dyn StatusObserver>) {
        let _ = self.observer.set(observer);
    }

    /// Marks, and after 7 days deletes, the keys of offices that have nothing left (which
    /// `find_orphans` says, given the ids with a key: the caller owns the catalog), if this host
    /// may (`SupervisorConfig::sweep_orphans`); see `keys::sweep_orphans`. Run once at startup,
    /// after the startup discards and before any create.
    pub(crate) fn sweep_orphan_keys(
        &self,
        find_orphans: impl FnOnce(&[String]) -> Result<Vec<String>, String>,
    ) {
        if self.config.sweep_orphans {
            crate::keys::sweep_orphans(
                &*self.config.keys,
                &self.config.home,
                |ids| {
                    let mut orphans = find_orphans(ids)?;
                    orphans.retain(|id| !self.protected_offices.contains(id));
                    Ok(orphans)
                },
                std::time::SystemTime::now(),
            );
        }
    }

    /// Starts supervising an existing office (`--office <id>`).
    pub(crate) fn start_office(&self, office_id: &str) -> io::Result<()> {
        let mut state = lock(&self.state);
        if state.stopping {
            return Err(io::Error::other("the host is shutting down"));
        }
        if state.offices.contains_key(office_id) {
            return Ok(());
        }
        let (sender, signals) = mpsc::channel();
        let log = self.log_for(office_id);
        let slot = self.spawn_runner(office_id, sender, signals, log, None)?;
        state.offices.insert(office_id.to_owned(), slot);
        Ok(())
    }

    /// The state of a supervised office, or `None` if it isn't supervised.
    pub(crate) fn state(&self, office_id: &str) -> Option<OfficeState> {
        let state = lock(&self.state);
        state.offices.get(office_id).map(|slot| slot.state.get())
    }

    /// `office.restart`: starts an `offline` or `unavailable` office again now, with its
    /// backoff and give-up count reset. Refuses an office that's starting or active, and one
    /// `unavailable` for a reason a restart can't fix.
    pub(crate) fn restart(&self, office_id: &str) -> RestartOutcome {
        let state = lock(&self.state);
        let Some(slot) = state.offices.get(office_id) else {
            return RestartOutcome::NotSupervised;
        };
        match slot.state.request_restart(|| {
            let _ = slot.signals.send(Signal::Restart);
        }) {
            RestartRequest::Sent => RestartOutcome::Restarting,
            RestartRequest::Running => RestartOutcome::Conflict,
            RestartRequest::CantFix(reason) => RestartOutcome::CantFix(reason),
        }
    }

    /// Protected selected reservation has already installed its original key and
    /// writer custody. No create/discard cleanup is permitted on this path.
    pub(crate) fn begin_hq_create(
        &self,
        first: org_office_launch::CreateLine,
        recovery: org_proto::RecoveryCode,
    ) -> Result<PendingCreate<'_>, CreateStartError> {
        let (sender, signals) = mpsc::channel();
        let registration = self
            .register(&sender)
            .map_err(|e| CreateStartError::NotStarted(e.to_string()))?;
        self.config
            .server_check
            .may_spawn()
            .map_err(CreateStartError::NotStarted)?;
        let office_secrets_key = self
            .config
            .keys
            .load(&first.office.office_id)
            .map_err(|_| CreateStartError::NotStarted("original HQ key unavailable".into()))?
            .ok_or_else(|| CreateStartError::NotStarted("original HQ key missing".into()))?;
        let line = CreateLine::hq(first, office_secrets_key, recovery);
        let log = self.log_for(line.office_id());
        let spawned = self
            .spawner()
            .map_err(|e| CreateStartError::NotStarted(e.to_string()))?
            .spawn(Mode::Create(&line), 0, sender.clone(), Arc::clone(&log));
        // The handoff is done (or can't be): dropping the line zeroes its key copy and drops the
        // recovery code; only the key store holds the key from here on.
        drop(line);
        let child = spawned.map_err(|e| CreateStartError::KeyStored(e.into()))?;
        Ok(PendingCreate {
            registration,
            child,
            signals,
            sender,
            log,
        })
    }

    /// Runs `agentic-org-office --discard <office_id>` and waits (up to the discard timeout) for
    /// it to report. The server gets no key (its handoff carries no secret); the host reads
    /// the key only so that an unreadable key store starts no server. The server takes the office lock, refuses a folder that holds `office.json`, and removes the
    /// folder. After `discarded`, and only then, the office's key is deleted from the key store.
    /// A key store that can't be read starts no server ([`DiscardOutcome::KeyUnreadable`]).
    pub(crate) fn discard(&self, office_id: &str) -> io::Result<DiscardOutcome> {
        let (sender, signals) = mpsc::channel();
        let _registration = self.register(&sender)?;
        let key = match self.config.keys.load(office_id) {
            Ok(key) => key,
            Err(e) => {
                host_log::log(&format!(
                    "couldn't read the key of unfinished office {office_id}: {e}; it isn't \
                     discarded"
                ));
                return Ok(DiscardOutcome::KeyUnreadable);
            }
        };
        let spawned =
            self.spawner()?
                .spawn(Mode::Discard(office_id), 0, sender, self.log_for(office_id));
        // The line is written (or can't be): only the key store holds the key from here on.
        drop(key);
        let mut child = spawned.map_err(io::Error::from)?;
        let outcome = self.wait_for_discard(office_id, &mut child, &signals);
        if outcome == DiscardOutcome::Discarded {
            if let Err(e) = self.config.keys.delete(office_id) {
                host_log::log(&format!(
                    "discarded unfinished office {office_id}, but couldn't delete its key: {e}"
                ));
            }
        }
        Ok(outcome)
    }

    /// Waits (up to the discard timeout) for a `--discard` server to report, then stops it.
    fn wait_for_discard(
        &self,
        office_id: &str,
        child: &mut RunningChild,
        signals: &Receiver<Signal>,
    ) -> DiscardOutcome {
        let timings = &self.config.timings;
        let deadline = Instant::now() + timings.discard_timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let outcome = match signals.recv_timeout(left) {
                Ok(Signal::Stop { deadline }) => {
                    child.stop(deadline);
                    return DiscardOutcome::Stopped;
                }
                // Restarts are for an office's own supervision thread only.
                Ok(Signal::Restart) => continue,
                Ok(Signal::Event { event, .. }) => match event {
                    ServerEvent::Discarded => DiscardOutcome::Discarded,
                    ServerEvent::Failed { code, message } => {
                        host_log::log(&format!(
                            "discarding office {office_id} failed ({code:?}): {}",
                            host_log::untrusted(&message)
                        ));
                        DiscardOutcome::Failed(code)
                    }
                    _ => continue,
                },
                Ok(Signal::Closed { .. }) | Err(RecvTimeoutError::Disconnected) => {
                    DiscardOutcome::Crashed
                }
                Err(RecvTimeoutError::Timeout) => {
                    child.kill();
                    return DiscardOutcome::TimedOut;
                }
            };
            // It exits right after reporting.
            child.stop(Instant::now() + timings.shutdown_grace);
            return outcome;
        }
    }

    /// Hands a server that created its office over to supervision, which keeps it running as
    /// that office's server. Returns the office's state: `active` if `ready` already arrived,
    /// otherwise `starting`.
    pub(crate) fn adopt(&self, adoptable: Adoptable<'_>) -> OfficeState {
        let Adoptable {
            office,
            mut registration,
            mut child,
            signals,
            sender,
            log,
            backlog,
            ready,
        } = adoptable;
        let mut state = lock(&self.state);
        if let Some(key) = registration.key.take() {
            state.runs.remove(&key);
        }
        if state.stopping {
            drop(state);
            child.stop(Instant::now() + self.config.timings.shutdown_grace);
            return OfficeState {
                status: OfficeStatus::Offline,
                reason: None,
                next_restart_at: None,
            };
        }
        let adopted = Adopted {
            child,
            backlog,
            ready,
            // Its ready wait starts now, not at the `--create` spawn.
            ready_from: Instant::now(),
        };
        // Decided here, not read back: the new thread may already be moving it on.
        let status = if ready {
            OfficeState::active()
        } else {
            OfficeState::starting()
        };
        match self.spawn_runner(&office.office_id, sender, signals, log, Some(adopted)) {
            Ok(slot) => {
                state.offices.insert(office.office_id.clone(), slot);
                status
            }
            Err(e) => {
                host_log::log(&format!(
                    "couldn't supervise office {}: {e}",
                    office.office_id
                ));
                OfficeState {
                    status: OfficeStatus::Unavailable,
                    reason: None,
                    next_restart_at: None,
                }
            }
        }
    }

    /// Stops every server: closes their stdins, waits up to the grace period, SIGKILLs the
    /// rest, and waits for every supervision thread. Short runs (`--create`, `--discard`) in
    /// flight are stopped too.
    pub(crate) fn shutdown(&self) {
        let deadline = Instant::now() + self.config.timings.shutdown_grace;
        let threads: Vec<JoinHandle<()>> = {
            let mut state = lock(&self.state);
            state.stopping = true;
            for sender in state.runs.values() {
                let _ = sender.send(Signal::Stop { deadline });
            }
            state
                .offices
                .values_mut()
                .filter_map(|slot| {
                    let _ = slot.signals.send(Signal::Stop { deadline });
                    slot.thread.take()
                })
                .collect()
        };
        for thread in threads {
            let _ = thread.join();
        }
    }

    /// Registers a short run so shutdown can stop it; refused once shutdown has begun.
    fn register(&self, sender: &Sender<Signal>) -> io::Result<Registration<'_>> {
        let mut state = lock(&self.state);
        if state.stopping {
            return Err(io::Error::other("the host is shutting down"));
        }
        let key = state.next_run;
        state.next_run += 1;
        state.runs.insert(key, sender.clone());
        Ok(Registration {
            supervisor: self,
            key: Some(key),
        })
    }

    fn log_for(&self, office_id: &str) -> Arc<Mutex<RotatingLog>> {
        let handle = handle_of(office_id);
        let mut logs = lock(&self.logs);
        logs.retain(|_, log| log.strong_count() > 0);
        if let Some(log) = logs.get(handle).and_then(Weak::upgrade) {
            return log;
        }
        let log = Arc::new(Mutex::new(RotatingLog::new(
            &self.config.logs_dir,
            handle,
            self.config.log_max_bytes,
        )));
        logs.insert(handle.to_owned(), Arc::downgrade(&log));
        log
    }

    /// Starts an office's supervision thread.
    fn spawn_runner(
        &self,
        office_id: &str,
        sender: Sender<Signal>,
        signals: Receiver<Signal>,
        log: Arc<Mutex<RotatingLog>>,
        adopted: Option<Adopted>,
    ) -> io::Result<Slot> {
        let initial = match &adopted {
            Some(adopted) if adopted.ready => OfficeState::active(),
            _ => OfficeState::starting(),
        };
        let state = Arc::new(SharedState::new(
            office_id,
            initial,
            Arc::clone(&self.observer),
        ));
        let runner = OfficeRunner {
            office_id: office_id.to_owned(),
            keys: Arc::clone(&self.config.keys),
            spawner: self.spawner()?.clone(),
            timings: self.config.timings.clone(),
            state: Arc::clone(&state),
            signals,
            sender: sender.clone(),
            log,
        };
        let thread = thread::Builder::new()
            .name(format!("office-{}", handle_of(office_id)))
            .spawn(move || runner.run(adopted))?;
        Ok(Slot {
            state,
            signals: sender,
            thread: Some(thread),
        })
    }
}

impl Adoptable<'_> {
    /// Stops a server whose office the host refused instead of adopting it: closes its stdin,
    /// waits up to the grace period, then SIGKILLs it. It is always reaped when this returns.
    pub(crate) fn stop(mut self) {
        let grace = self.registration.supervisor.config.timings.shutdown_grace;
        self.child.stop(Instant::now() + grace);
    }
}

impl<'a> PendingCreate<'a> {
    /// Waits up to `timeout` for the server to create the office (or fail).
    pub(crate) fn wait(mut self, timeout: Duration) -> CreateOutcome<'a> {
        let grace = self.registration.supervisor.config.timings.shutdown_grace;
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let signal = match self.signals.recv_timeout(left) {
                Ok(signal) => signal,
                Err(RecvTimeoutError::Timeout) => {
                    self.child.kill();
                    return CreateOutcome::TimedOut;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    self.child.kill();
                    return CreateOutcome::Crashed;
                }
            };
            match signal {
                Signal::Stop { deadline } => {
                    self.child.stop(deadline);
                    return CreateOutcome::Stopped;
                }
                Signal::Restart => {}
                Signal::Event {
                    event: ServerEvent::Created { office },
                    ..
                } => return CreateOutcome::Created(Box::new(self.into_adoptable(office))),
                Signal::Event {
                    event: ServerEvent::Failed { code, message },
                    ..
                } => {
                    host_log::log(&format!(
                        "creating an office failed ({code:?}): {}",
                        host_log::untrusted(&message)
                    ));
                    self.child.stop(Instant::now() + grace);
                    return CreateOutcome::Failed;
                }
                Signal::Event { .. } => {}
                Signal::Closed { .. } => {
                    self.child.stop(Instant::now() + grace);
                    return CreateOutcome::Crashed;
                }
            }
        }
    }

    /// Takes the signals that already arrived after `created` along, so supervision sees them
    /// in order, and notes whether `ready` was among them.
    fn into_adoptable(self, office: OfficeFile) -> Adoptable<'a> {
        let backlog: Vec<Signal> = self.signals.try_iter().collect();
        let ready = backlog.iter().any(|s| {
            matches!(
                s,
                Signal::Event {
                    event: ServerEvent::Ready { .. },
                    ..
                }
            )
        }) && !backlog.iter().any(|s| matches!(s, Signal::Closed { .. }));
        Adoptable {
            office,
            registration: self.registration,
            child: self.child,
            signals: self.signals,
            sender: self.sender,
            log: self.log,
            backlog,
            ready,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use org_proto::OfficeSecretsKey;

    use crate::keys::memory::{Fault, MemoryKeys, Op};
    #[cfg(target_os = "macos")]
    use crate::peer::AuditToken;
    #[cfg(target_os = "macos")]
    use crate::server_check::{Scripted, Step};

    const OFFICE: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";

    fn supervisor(dir: &Path, program: PathBuf, keys: &Arc<MemoryKeys>) -> Supervisor {
        checked_supervisor(dir, program, keys, ServerCheck::Skip)
    }

    fn checked_supervisor(
        dir: &Path,
        program: PathBuf,
        keys: &Arc<MemoryKeys>,
        check: ServerCheck,
    ) -> Supervisor {
        let timings = Timings {
            shutdown_grace: Duration::from_millis(200),
            discard_timeout: Duration::from_secs(10),
            ..Timings::default()
        };
        // The launch rendezvous is bound in `<home>/run`.
        fs::create_dir_all(dir.join("run")).unwrap();
        Supervisor::new(
            SupervisorConfig {
                program,
                home: dir.to_path_buf(),
                runtime_dir: None,
                logs_dir: dir.join("logs"),
                log_max_bytes: 1024,
                timings,
                keys: Arc::clone(keys) as Arc<dyn OfficeSecretsKeys>,
                server_check: Arc::new(check),
                sweep_orphans: false,
            },
            INSTANCE,
        )
    }

    /// A temporary directory under `/tmp`, so the launch rendezvous path fits a socket address.
    fn short_tempdir() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("sv")
            .tempdir_in("/tmp")
            .unwrap()
    }

    /// An executable `sh` script in `dir`.
    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    #[test]
    fn no_office_is_started_once_shutdown_began() {
        let dir = short_tempdir();
        let keys = Arc::new(MemoryKeys::default());
        let supervisor = supervisor(dir.path(), dir.path().join("no-such-server"), &keys);
        supervisor.shutdown();
        assert!(supervisor.start_office(OFFICE).is_err());
        assert_eq!(supervisor.state(OFFICE), None);
    }

    /// Polls the office's state until `want` holds (or panics after 10 s).
    fn wait_for_state(
        supervisor: &Supervisor,
        office_id: &str,
        want: impl Fn(&OfficeState) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !supervisor.state(office_id).is_some_and(|s| want(&s)) {
            assert!(
                Instant::now() < deadline,
                "{:?}",
                supervisor.state(office_id)
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_start_without_a_usable_key_still_spawns_its_server() {
        let dir = short_tempdir();
        let ran = dir.path().join("ran");
        let program = script(
            dir.path(),
            "server",
            &format!("echo \"$@\" >> '{}'", ran.display()),
        );
        let keys = Arc::new(MemoryKeys::default());
        let supervisor = supervisor(dir.path(), program, &keys);
        let spawned = |id: &str| {
            fs::read_to_string(&ran)
                .unwrap_or_default()
                .lines()
                .any(|line| line.split_whitespace().any(|arg| arg == id))
        };
        let wait_for_spawn = |id: &str| {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !spawned(id) {
                assert!(Instant::now() < deadline, "office {id} wasn't started");
                thread::sleep(Duration::from_millis(10));
            }
        };
        // No item: the server starts anyway (its handoff would carry `null`).
        supervisor.start_office(OFFICE).unwrap();
        wait_for_spawn(OFFICE);
        // An item that isn't a key: the same.
        const B: &str = "01BBBBBBBBBBBBBBBBBBBBBBBB";
        keys.create(B, &OfficeSecretsKey::from_bytes([1; 32]))
            .unwrap();
        keys.fail(Op::Load, Fault::Corrupt);
        supervisor.start_office(B).unwrap();
        wait_for_spawn(B);
        // A store that fails otherwise: `storage_error`, retried with backoff.
        const C: &str = "01CCCCCCCCCCCCCCCCCCCCCCCC";
        keys.fail(Op::Load, Fault::Io);
        supervisor.start_office(C).unwrap();
        wait_for_state(&supervisor, C, |s| {
            s.status == OfficeStatus::Offline
                && s.reason == Some(OfficeStatusReason::StorageError)
                && s.next_restart_at.is_some()
        });
        supervisor.shutdown();
        assert!(!spawned(C), "a server started over an unreadable key store");
        // Nothing was deleted, and the item that isn't a key is still there.
        assert!(!keys.calls().iter().any(|(op, _)| *op == Op::Delete));
        assert!(keys.has(B));
    }

    /// H1: a server that isn't the signed one (this unsigned script, against a release
    /// requirement) is spawned suspended, checked, killed and reaped before it runs anything,
    /// and gets no line: no key. With no
    /// team id, nothing is even spawned. A start ends as `storage_error`, retried; a discard
    /// fails and keeps the key.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_server_that_isnt_the_signed_one_gets_nothing() {
        const B: &str = "01BBBBBBBBBBBBBBBBBBBBBBBB";
        let dir = short_tempdir();
        let got = dir.path().join("got");
        let program = script(dir.path(), "server", &format!("cat >> '{}'", got.display()));
        let keys = Arc::new(MemoryKeys::default());
        for id in [OFFICE, B] {
            keys.create(id, &OfficeSecretsKey::from_bytes([3; 32]))
                .unwrap();
        }
        let requirement = crate::server_check::server_requirement("AB12CD34EF").unwrap();
        let checked = checked_supervisor(
            dir.path(),
            program.clone(),
            &keys,
            ServerCheck::Requirement(requirement),
        );
        assert!(checked.discard(OFFICE).is_err());
        checked.start_office(B).unwrap();
        wait_for_state(&checked, B, |s| {
            s.status == OfficeStatus::Offline && s.reason == Some(OfficeStatusReason::StorageError)
        });
        checked.shutdown();
        // They got no line: nothing reached `got` (it isn't even there: they were stopped while
        // still suspended; that suspension holds is `spawn`'s own test).
        assert!(!got.exists(), "a server that isn't the signed one ran");
        assert!(keys.has(OFFICE) && keys.has(B));

        // No team id: nothing is spawned at all.
        let dir = short_tempdir();
        let ran = dir.path().join("ran");
        let program = script(
            dir.path(),
            "server",
            &format!("echo run >> '{}'", ran.display()),
        );
        let refusing = checked_supervisor(dir.path(), program, &keys, ServerCheck::Refuse);
        assert!(refusing.discard(OFFICE).is_err());
        refusing.start_office(B).unwrap();
        wait_for_state(&refusing, B, |s| {
            s.status == OfficeStatus::Offline && s.reason == Some(OfficeStatusReason::StorageError)
        });
        refusing.shutdown();
        assert!(!ran.exists(), "a server was started without a team id");
    }

    /// A check scripted by the test: `admit` and `still` answer as `script` says.
    #[cfg(target_os = "macos")]
    fn scripted(
        script: impl Fn(Step, u32) -> Result<Option<AuditToken>, String> + Send + Sync + 'static,
    ) -> ServerCheck {
        ServerCheck::Scripted(Scripted(Arc::new(script)))
    }

    /// L2: the check runs while the child is still suspended: at the admission, the child is
    /// stopped (`ps` state `T`, the kernel's `SSTOP`), so not one instruction of its image has
    /// run, and once refused it never does.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_check_runs_while_the_child_is_still_suspended() {
        let dir = short_tempdir();
        let ran = dir.path().join("ran");
        let program = script(dir.path(), "server", &format!("touch '{}'", ran.display()));
        let keys = Arc::new(MemoryKeys::default());
        let states = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = Arc::clone(&states);
        let check = scripted(move |step, pid| {
            assert_eq!(step, Step::Admit);
            let out = std::process::Command::new("/bin/ps")
                .args(["-o", "state=", "-p", &pid.to_string()])
                .output()
                .unwrap();
            lock(&seen).push(String::from_utf8_lossy(&out.stdout).trim().to_owned());
            Err("refused by the test".into())
        });
        let supervisor = checked_supervisor(dir.path(), program, &keys, check);
        assert!(supervisor.discard(OFFICE).is_err());
        let states = lock(&states).clone();
        assert_eq!(states.len(), 1, "{states:?}");
        assert!(
            states[0].starts_with('T'),
            "not stopped at the check: {states:?}"
        );
        thread::sleep(Duration::from_millis(200));
        assert!(!ran.exists(), "the child ran");
    }

    /// L3a: a child whose token changed between the admission and its first line (an exec) is
    /// refused at the re-check: it gets no line.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_child_whose_token_changed_before_its_line_gets_nothing() {
        let dir = short_tempdir();
        let got = dir.path().join("got");
        let program = script(dir.path(), "server", &format!("cat > '{}'", got.display()));
        let keys = Arc::new(MemoryKeys::default());
        keys.create(OFFICE, &OfficeSecretsKey::from_bytes([6; 32]))
            .unwrap();
        let check = scripted(|step, _| match step {
            Step::Admit => Ok(Some([1; 32])),
            Step::Still => Ok(Some([2; 32])),
        });
        let supervisor = checked_supervisor(dir.path(), program, &keys, check);
        let err = supervisor.discard(OFFICE).unwrap_err();
        assert!(err.to_string().contains("changed"), "{err}");
        thread::sleep(Duration::from_millis(200));
        assert_eq!(fs::read(&got).unwrap_or_default(), b"", "it got a line");
        assert!(keys.has(OFFICE));
    }

    /// Names the file where this test binary, run again as a launch `--discard` server by
    /// [`a_discard_hands_off_no_key_and_deletes_it_only_after_discarded`], records its
    /// handoff's op (`discard`).
    const DISCARD_SEEN: &str = "ORG_HOST_TEST_DISCARD_SEEN";
    const INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c0";

    /// Not a test: the launch `--discard` server the supervisor test runs, as the real worker
    /// speaks it (first line on stdin, the rendezvous connect, `ready_for_secrets`, the handoff
    /// header with no descriptor, then `discarded` on the stream). Without its variable it does
    /// nothing.
    #[test]
    #[ignore = "run only as the launch discard server of a supervisor test"]
    fn discard_server() {
        use org_office_launch::HostFirstLine;
        use std::os::unix::net::UnixStream;
        let Some(seen) = std::env::var_os(DISCARD_SEEN) else {
            return;
        };
        let mut stdin = std::io::BufReader::new(std::io::stdin());
        let Ok(Some(HostFirstLine::Discard(office))) =
            org_office_launch::read_first_line(&mut stdin, org_office_launch::FirstLine::Discard)
        else {
            std::process::exit(2);
        };
        if office.office_id != OFFICE || office.instance_id != INSTANCE {
            std::process::exit(3);
        }
        let mut stream = UnixStream::connect(&office.rendezvous).unwrap();
        let org_office_launch::Handoff::Discard { fds } =
            channels::tests::discard_worker_reads_handoff(&mut stream)
        else {
            std::process::exit(4);
        };
        assert!(fds.is_empty());
        let note = "discard\n";
        fs::write(seen, note).unwrap();
        org_office_launch::write_rendezvous_event(
            &mut stream,
            &org_office_launch::RendezvousEvent::Server(ServerEvent::Discarded),
        )
        .unwrap();
        std::process::exit(0);
    }

    /// The whole discard path: the spawner binds a rendezvous, writes the discard line, the
    /// broker hands off a header with no secret although the key store holds the
    /// office's key, the worker's `discarded` arrives on the stream, and only then is the key
    /// deleted.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_discard_hands_off_no_key_and_deletes_it_only_after_discarded() {
        // Under /tmp: the rendezvous path must fit a socket address.
        let dir = tempfile::Builder::new()
            .prefix("sv")
            .tempdir_in("/tmp")
            .unwrap();
        fs::create_dir(dir.path().join("run")).unwrap();
        let keys = Arc::new(MemoryKeys::default());
        let key = OfficeSecretsKey::from_bytes([8; 32]);
        keys.create(OFFICE, &key).unwrap();
        let seen = dir.path().join("seen");
        // The host closes the server's stdout over the rendezvous, so libtest's output goes nowhere.
        let program = script(
            dir.path(),
            "server",
            &format!(
                "{DISCARD_SEEN}='{}' exec '{}' --exact supervisor::tests::discard_server \
                 --ignored --test-threads=1 >/dev/null",
                seen.display(),
                std::env::current_exe().unwrap().display()
            ),
        );
        let supervisor = supervisor(dir.path(), program, &keys);
        assert_eq!(
            supervisor.discard(OFFICE).unwrap(),
            DiscardOutcome::Discarded
        );
        assert!(!keys.has(OFFICE), "the key outlived `discarded`");
        assert_eq!(
            fs::read_to_string(&seen).unwrap(),
            "discard\n",
            "the server didn't read a discard handoff"
        );
        assert!(keys
            .calls()
            .iter()
            .any(|call| *call == (Op::Delete, OFFICE.to_owned())));
    }

    /// Names the file where this test binary, run again as a launch HQ `--create` server by
    /// [`an_hq_create_reported_after_shutdown_began_is_stopped_not_adopted`], records its pid.
    const CREATE_PID: &str = "ORG_HOST_TEST_CREATE_PID";

    /// Not a test: a launch HQ `--create` server, as the real worker speaks it (first line on
    /// stdin, the rendezvous connect, `ready_for_secrets`, the handoff header, the transfer
    /// socket after its marker, the receipt), that reports `created` at once and keeps running.
    /// Without its variable it does nothing.
    #[test]
    #[ignore = "run only as the launch HQ create server of a supervisor test"]
    fn hq_create_server() {
        use org_office_launch::HostFirstLine;
        use std::os::unix::net::UnixStream;
        let Some(pid_file) = std::env::var_os(CREATE_PID) else {
            return;
        };
        fs::write(pid_file, std::process::id().to_string()).unwrap();
        let mut stdin = std::io::BufReader::new(std::io::stdin());
        let Ok(Some(HostFirstLine::Create(create))) =
            org_office_launch::read_first_line(&mut stdin, org_office_launch::FirstLine::Create)
        else {
            std::process::exit(2);
        };
        let mut stream = UnixStream::connect(&create.office.rendezvous).unwrap();
        org_proto::frame::write_message(&mut stream, &org_office_launch::ReadyForSecrets).unwrap();
        let fence = std::sync::Mutex::new(());
        let mut receiver = org_socket::rights::StreamReceiver::new(stream.try_clone().unwrap());
        let mut receive = |count: usize, marker: bool| {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let got = if marker {
                    receiver
                        .receive_marker(count, org_office_launch::FD_MARKER, &fence)
                        .map(|fds| (org_office_launch::FD_MARKER, fds))
                } else {
                    receiver.receive_byte(count, &fence)
                };
                match got {
                    Ok(value) => return value,
                    Err(org_socket::rights::ReceiveError::WouldBlock)
                        if Instant::now() < deadline =>
                    {
                        std::thread::yield_now()
                    }
                    Err(_) => std::process::exit(4),
                }
            }
        };
        let mut header = Vec::new();
        loop {
            let (b, _) = receive(0, false);
            if b == b'\n' {
                break;
            }
            header.push(b);
        }
        let Ok(org_office_launch::Handoff::Create { fds, .. }) =
            org_office_launch::decode_handoff(&header)
        else {
            std::process::exit(5);
        };
        crate::secrets::zero(&mut header);
        let (_, _transfer) = receive(fds.len(), true);
        drop(receiver);
        stream.set_nonblocking(false).unwrap();
        org_office_launch::write_rendezvous_event(
            &mut stream,
            &org_office_launch::RendezvousEvent::FdsReceived {
                of: "handoff".into(),
            },
        )
        .unwrap();
        let office = OfficeFile::new(
            &create.office.office_id,
            &create.office_name,
            create.role,
            &create.org_id,
            &create.org_name,
            "2026-11-08T00:00:00.000Z",
        );
        org_office_launch::write_rendezvous_event(
            &mut stream,
            &org_office_launch::RendezvousEvent::Server(ServerEvent::Created { office }),
        )
        .unwrap();
        std::thread::sleep(Duration::from_secs(30));
        std::process::exit(0);
    }

    /// An HQ create whose server reports `created` after shutdown began is stopped, not
    /// adopted: the supervisor never starts supervising it, its server is gone, and its key
    /// (the HQ's original key, installed before the create) stays.
    #[cfg(target_os = "macos")]
    #[test]
    fn an_hq_create_reported_after_shutdown_began_is_stopped_not_adopted() {
        let dir = short_tempdir();
        let keys = Arc::new(MemoryKeys::default());
        let key = OfficeSecretsKey::from_bytes([2; 32]);
        keys.create(OFFICE, &key).unwrap();
        let pid_file = dir.path().join("pid");
        // The host closes the server's stdout over the rendezvous, so libtest's output goes nowhere.
        let program = script(
            dir.path(),
            "server",
            &format!(
                "{CREATE_PID}='{}' exec '{}' --exact supervisor::tests::hq_create_server \
                 --ignored --test-threads=1 >/dev/null",
                pid_file.display(),
                std::env::current_exe().unwrap().display()
            ),
        );
        let supervisor = supervisor(dir.path(), program, &keys);
        let first = org_office_launch::CreateLine {
            office: org_office_launch::OfficeLine {
                id_profile: None,
                selected_writer_required: false,
                instance_id: INSTANCE.into(),
                office_id: OFFICE.into(),
                rendezvous: String::new(),
            },
            org_id: "01ORG".into(),
            org_name: "Acme".into(),
            office_name: "HQ".into(),
            role: org_proto::OfficeRole::Hq,
            owner_member_id: "01M72N8MG00000000000000M01".into(),
            hq_creation: None,
        };
        let recovery = org_proto::RecoveryCode::new("ABCD-EFGH-IJKL".into()).unwrap();
        let pending = supervisor.begin_hq_create(first, recovery).unwrap();
        let CreateOutcome::Created(adoptable) = pending.wait(Duration::from_secs(10)) else {
            panic!("the server didn't report created");
        };
        let pid: libc::pid_t = fs::read_to_string(&pid_file).unwrap().parse().unwrap();
        // The shutdown begins between `created` and the adoption.
        supervisor.shutdown();
        assert_eq!(supervisor.adopt(*adoptable).status, OfficeStatus::Offline);
        assert_eq!(supervisor.state(OFFICE), None, "adopted after shutdown");
        // The stop reaps the server: signal 0 then finds no such process.
        // SAFETY: signal 0 only checks the pid.
        assert!(
            unsafe { libc::kill(pid, 0) } != 0,
            "the server outlived the shutdown"
        );
        // The HQ's original key stays: it was installed before the create, not by it.
        assert!(keys.holds(OFFICE, &key));
    }

    #[test]
    fn a_discard_whose_key_cant_be_read_starts_nothing_and_keeps_everything() {
        let dir = short_tempdir();
        let keys = Arc::new(MemoryKeys::default());
        keys.create(OFFICE, &OfficeSecretsKey::from_bytes([7; 32]))
            .unwrap();
        let seen = dir.path().join("seen");
        let program = script(
            dir.path(),
            "server",
            &format!("echo run >> '{}'", seen.display()),
        );
        let supervisor = supervisor(dir.path(), program, &keys);
        for fault in [Fault::Corrupt, Fault::Io, Fault::Os(-25308)] {
            keys.fail(Op::Load, fault);
            assert_eq!(
                supervisor.discard(OFFICE).unwrap(),
                DiscardOutcome::KeyUnreadable,
                "{fault:?}"
            );
        }
        assert!(!seen.exists(), "a server started");
        assert!(keys.has(OFFICE));
        assert!(!keys.calls().iter().any(|(op, _)| *op == Op::Delete));
    }

    /// G1: a host with no registered instance spawns nothing: no discard, no office start.
    #[test]
    fn an_unregistered_supervisor_spawns_nothing() {
        let dir = short_tempdir();
        let ran = dir.path().join("ran");
        let program = script(
            dir.path(),
            "server",
            &format!("echo run >> '{}'", ran.display()),
        );
        let keys = Arc::new(MemoryKeys::default());
        keys.create(OFFICE, &OfficeSecretsKey::from_bytes([3; 32]))
            .unwrap();
        let supervisor = Supervisor::unregistered(SupervisorConfig {
            program,
            home: dir.path().to_path_buf(),
            runtime_dir: None,
            logs_dir: dir.path().join("logs"),
            log_max_bytes: 1024,
            timings: Timings::default(),
            keys: Arc::clone(&keys) as Arc<dyn OfficeSecretsKeys>,
            server_check: Arc::new(ServerCheck::Skip),
            sweep_orphans: false,
        });
        let err = supervisor.discard(OFFICE).unwrap_err();
        assert!(err.to_string().contains("setup must run first"), "{err}");
        let err = supervisor.start_office(OFFICE).unwrap_err();
        assert!(err.to_string().contains("setup must run first"), "{err}");
        supervisor.shutdown();
        assert!(!ran.exists(), "an unregistered host started a server");
        assert!(keys.has(OFFICE));
        assert!(!keys.calls().iter().any(|(op, _)| *op == Op::Delete));
    }
}
