//! Supervision: one `org-office-server` per office, each watched by its own thread
//! (`office`), plus the short runs around them: `--create` makes a new office, `--discard`
//! removes an unfinished one.
//!
//! Each office has an app token for this host run, made when supervision of it starts (or when
//! it is created) and kept, in memory only, by its supervision: every start of its server within
//! the run gets the same token in its first line. The next host run makes new ones.
//!
//! Each office's database key lives in the host's key store ([`KeyStore`], the host's own
//! abstraction: the supervisor never knows which store it is). A create stores the new key
//! before its server starts; every start and discard loads it just before the spawn, and drops
//! it once the first line is written. A start whose key is missing, or whose item isn't a key,
//! spawns nothing: the office is `unavailable`/`key_missing` until `office.restart`. The supervisor
//! deletes a key **only** after the office's `--discard` reported `discarded`.
//!
//! Status per office ([`OfficeState`]): `starting` (spawned, no `ready` yet) → `active`
//! (`ready`, child alive) → `offline` (exited, a restart pending at `next_restart_at`) →
//! `unavailable` (a fatal failure, or the give-up rule; until `office.restart` or the next host
//! run), each `offline` or `unavailable` with its reason. Every change is announced to the
//! [`StatusObserver`]. Shutdown closes every child's stdin, waits up to the grace period, then
//! SIGKILLs what's left.

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

use org_office_launch::{CreateOffice, FailCode, OfficeFile, ServerEvent};
use org_proto::{AppToken, OfficeStatus, OfficeStatusReason};

use crate::host_log;
use crate::ids::handle_of;
use crate::keys::KeyStore;
use crate::logs::RotatingLog;
use crate::secrets;
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
    pub(crate) logs_dir: PathBuf,
    pub(crate) log_max_bytes: u64,
    pub(crate) timings: Timings,
    /// Where every office's database key is kept.
    pub(crate) keys: Arc<dyn KeyStore>,
    /// Whether [`Supervisor::sweep_orphan_keys`] may delete keys (see `keys::sweep_orphans`
    /// for when that's safe).
    pub(crate) sweep_orphans: bool,
}

pub(crate) struct Supervisor {
    config: SupervisorConfig,
    spawner: Spawner,
    state: Mutex<State>,
    /// One log per socket handle, shared by every run of that office (`--create`, `--office`,
    /// `--discard`) while any of them still writes it, so two writers never rotate the same
    /// file apart. Held weakly: a log (and its open file) goes once nothing writes it.
    logs: Mutex<HashMap<String, Weak<Mutex<RotatingLog>>>>,
    observer: Observer,
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
    /// The office's app token for this host run: shared with its supervision thread.
    token: Arc<AppToken>,
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
    Failed(FailCode),
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
    /// The create line the server got, kept for the token it holds (its key is already zeroed).
    line: CreateLine,
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
    /// The office's app token, the one it was created with.
    token: Arc<AppToken>,
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
    pub(crate) fn new(config: SupervisorConfig) -> Self {
        let spawner = Spawner {
            program: config.program.clone(),
            home: config.home.clone(),
        };
        Self {
            config,
            spawner,
            state: Mutex::new(State::default()),
            logs: Mutex::new(HashMap::new()),
            observer: Arc::new(OnceLock::new()),
        }
    }

    /// Where every office's state changes are announced from now on (set once).
    pub(crate) fn observe(&self, observer: Weak<dyn StatusObserver>) {
        let _ = self.observer.set(observer);
    }

    /// Deletes the keys of offices with no folder (see `keys::sweep_orphans`), if this host may
    /// (`SupervisorConfig::sweep_orphans`). Run once at startup, after the startup discards and
    /// before any create.
    pub(crate) fn sweep_orphan_keys(&self) {
        if self.config.sweep_orphans {
            crate::keys::sweep_orphans(&*self.config.keys, &self.config.home);
        }
    }

    /// Starts supervising an existing office (`--office <id>`), with a fresh app token.
    pub(crate) fn start_office(&self, office_id: &str) -> io::Result<()> {
        let mut state = lock(&self.state);
        if state.stopping {
            return Err(io::Error::other("the host is shutting down"));
        }
        if state.offices.contains_key(office_id) {
            return Ok(());
        }
        let token = Arc::new(secrets::generate_app_token()?);
        let (sender, signals) = mpsc::channel();
        let log = self.log_for(office_id);
        let slot = self.spawn_runner(office_id, token, sender, signals, log, None)?;
        state.offices.insert(office_id.to_owned(), slot);
        Ok(())
    }

    /// The state of a supervised office, or `None` if it isn't supervised.
    pub(crate) fn state(&self, office_id: &str) -> Option<OfficeState> {
        let state = lock(&self.state);
        state.offices.get(office_id).map(|slot| slot.state.get())
    }

    /// Runs `f` with a supervised office's app token, without copying it. `None` if the office
    /// isn't supervised.
    pub(crate) fn with_app_token<R>(
        &self,
        office_id: &str,
        f: impl FnOnce(&AppToken) -> R,
    ) -> Option<R> {
        let token = lock(&self.state)
            .offices
            .get(office_id)
            .map(|slot| Arc::clone(&slot.token))?;
        Some(f(&token))
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

    /// Stores `create`'s database key, then starts `org-office-server --create` and sends it
    /// `create`; the key is zeroed in memory once the line is written. Its app token becomes the
    /// office's token for this host run if the office is adopted.
    pub(crate) fn begin_create(
        &self,
        create: CreateOffice,
    ) -> Result<PendingCreate<'_>, CreateStartError> {
        let (sender, signals) = mpsc::channel();
        let registration = self
            .register(&sender)
            .map_err(|e| CreateStartError::NotStarted(e.to_string()))?;
        let mut line = CreateLine::new(create);
        // Before the spawn: no server ever holds a key the host hasn't stored.
        if let Err(e) = self.config.keys.create(line.office_id(), line.db_key()) {
            return Err(CreateStartError::NotStarted(format!(
                "couldn't store the new office's key: {e}"
            )));
        }
        let log = self.log_for(line.office_id());
        let spawned = self
            .spawner
            .spawn(Mode::Create(&line), 0, sender.clone(), Arc::clone(&log));
        line.drop_key();
        let child = spawned.map_err(CreateStartError::KeyStored)?;
        Ok(PendingCreate {
            registration,
            line,
            child,
            signals,
            sender,
            log,
        })
    }

    /// Runs `org-office-server --discard <office_id>` with the office's key (or `null` when
    /// the key store has none) and waits (up to the discard timeout) for it to report. The
    /// server takes the office lock, refuses a folder that holds `office.json`, and removes the
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
        let spawned = self.spawner.spawn(
            Mode::Discard(office_id, key.as_deref()),
            0,
            sender,
            self.log_for(office_id),
        );
        // The line is written (or can't be): only the key store holds the key from here on.
        drop(key);
        let mut child = spawned?;
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
            token,
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
        match self.spawn_runner(
            &office.office_id,
            token,
            sender,
            signals,
            log,
            Some(adopted),
        ) {
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

    /// Starts an office's supervision thread, which shares the office's app token for the rest
    /// of this host run.
    fn spawn_runner(
        &self,
        office_id: &str,
        token: Arc<AppToken>,
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
            token: Arc::clone(&token),
            keys: Arc::clone(&self.config.keys),
            spawner: self.spawner.clone(),
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
            token,
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
                    return CreateOutcome::Failed(code);
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
            token: self.line.into_token(),
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

    use org_office_launch::write_server_event;
    use org_proto::{AppToken, DbKey, OfficeRole};

    use crate::keys::memory::{Fault, MemoryKeys, Op};

    const OFFICE: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";

    fn supervisor(dir: &Path, program: PathBuf, keys: &Arc<MemoryKeys>) -> Supervisor {
        let timings = Timings {
            shutdown_grace: Duration::from_millis(200),
            discard_timeout: Duration::from_secs(10),
            ..Timings::default()
        };
        Supervisor::new(SupervisorConfig {
            program,
            home: dir.to_path_buf(),
            logs_dir: dir.join("logs"),
            log_max_bytes: 1024,
            timings,
            keys: Arc::clone(keys) as Arc<dyn KeyStore>,
            sweep_orphans: false,
        })
    }

    /// An executable `sh` script in `dir`.
    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    fn create_message(key: DbKey) -> CreateOffice {
        CreateOffice {
            org_id: "01ORG".into(),
            org_name: "Acme".into(),
            office_id: OFFICE.into(),
            office_name: "HQ".into(),
            role: OfficeRole::Hq,
            app_token: AppToken::from_bytes([1; 32]),
            db_key: key,
        }
    }

    #[test]
    fn a_create_reported_after_shutdown_began_is_stopped_not_adopted() {
        let dir = tempfile::tempdir().unwrap();
        let office = OfficeFile::new(
            OFFICE,
            "HQ",
            OfficeRole::Hq,
            "01ORG",
            "Acme",
            "2026-11-08T00:00:00.000Z",
        );
        let mut created = Vec::new();
        write_server_event(&mut created, &ServerEvent::Created { office }).unwrap();
        // A server that reports `created` at once, then keeps running.
        let program = script(
            dir.path(),
            "server",
            &format!(
                "printf '%s' '{}'\nexec /bin/sleep 30",
                String::from_utf8(created).unwrap()
            ),
        );
        let keys = Arc::new(MemoryKeys::default());
        let supervisor = supervisor(dir.path(), program, &keys);
        let pending = supervisor
            .begin_create(create_message(DbKey::from_bytes([2; 32])))
            .unwrap();
        let pid = pending.child.pid();
        let CreateOutcome::Created(adoptable) = pending.wait(Duration::from_secs(10)) else {
            panic!("the server didn't report created");
        };
        // The shutdown begins between `created` and the adoption.
        supervisor.shutdown();
        assert_eq!(supervisor.adopt(*adoptable).status, OfficeStatus::Offline);
        assert_eq!(supervisor.state(OFFICE), None, "adopted after shutdown");
        // SAFETY: signal 0 only checks the pid.
        assert!(
            unsafe { libc::kill(pid as libc::pid_t, 0) } != 0,
            "the server outlived the shutdown"
        );
        // The office was created: its key stays.
        assert!(keys.holds(OFFICE, &DbKey::from_bytes([2; 32])));
    }

    #[test]
    fn no_office_is_started_once_shutdown_began() {
        let dir = tempfile::tempdir().unwrap();
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
    fn a_start_without_a_usable_key_spawns_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let ran = dir.path().join("ran");
        let program = script(
            dir.path(),
            "server",
            &format!("echo \"$@\" >> '{}'", ran.display()),
        );
        let keys = Arc::new(MemoryKeys::default());
        let supervisor = supervisor(dir.path(), program, &keys);
        let unavailable = |reason| move |s: &OfficeState| *s == OfficeState::unavailable(reason);
        // No item: `key_missing`, fatal.
        supervisor.start_office(OFFICE).unwrap();
        wait_for_state(
            &supervisor,
            OFFICE,
            unavailable(OfficeStatusReason::KeyMissing),
        );
        // `office.restart` loads it again (and still finds none).
        let loads = || {
            keys.calls()
                .iter()
                .filter(|call| **call == (Op::Load, OFFICE.to_owned()))
                .count()
        };
        assert_eq!(loads(), 1);
        assert_eq!(supervisor.restart(OFFICE), RestartOutcome::Restarting);
        let deadline = Instant::now() + Duration::from_secs(10);
        while loads() < 2 {
            assert!(
                Instant::now() < deadline,
                "the restart didn't reload the key"
            );
            thread::sleep(Duration::from_millis(10));
        }
        wait_for_state(
            &supervisor,
            OFFICE,
            unavailable(OfficeStatusReason::KeyMissing),
        );
        // An item that isn't a key: the same.
        const B: &str = "01BBBBBBBBBBBBBBBBBBBBBBBB";
        keys.create(B, &DbKey::from_bytes([1; 32])).unwrap();
        keys.fail(Op::Load, Fault::Corrupt);
        supervisor.start_office(B).unwrap();
        wait_for_state(&supervisor, B, unavailable(OfficeStatusReason::KeyMissing));
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
        assert!(!ran.exists(), "a server started without its key");
        // Nothing was deleted, and the item that isn't a key is still there.
        assert!(!keys.calls().iter().any(|(op, _)| *op == Op::Delete));
        assert!(keys.has(B));
    }

    #[test]
    fn a_create_stores_its_key_before_its_server_starts() {
        let dir = tempfile::tempdir().unwrap();
        // A server that records its run and exits at once.
        let ran = dir.path().join("ran");
        let program = script(
            dir.path(),
            "server",
            &format!("echo \"$@\" >> '{}'", ran.display()),
        );
        let keys = Arc::new(MemoryKeys::default());
        let supervisor = supervisor(dir.path(), program, &keys);
        let pending = supervisor
            .begin_create(create_message(DbKey::from_bytes([3; 32])))
            .unwrap();
        // Stored first, then spawned (the spawn returned, so the key was stored before it).
        assert_eq!(keys.calls(), [(Op::Create, OFFICE.to_owned())]);
        assert!(keys.holds(OFFICE, &DbKey::from_bytes([3; 32])));
        // The line is written, so the create line's copy of the key is zeroed.
        assert!(*pending.line.db_key() == DbKey::from_bytes([0; 32]));
        // Ended without reporting: the key stays (only a discard deletes it).
        assert!(matches!(
            pending.wait(Duration::from_secs(10)),
            CreateOutcome::Crashed
        ));
        assert!(keys.has(OFFICE));
        assert_eq!(fs::read_to_string(&ran).unwrap(), "--create\n");
    }

    #[test]
    fn a_create_whose_key_cant_be_stored_starts_nothing_and_touches_no_key() {
        let dir = tempfile::tempdir().unwrap();
        let ran = dir.path().join("ran");
        let program = script(
            dir.path(),
            "server",
            &format!("echo \"$@\" >> '{}'", ran.display()),
        );
        let keys = Arc::new(MemoryKeys::default());
        let supervisor = supervisor(dir.path(), program, &keys);
        // An item already there isn't this create's: it is left as it is.
        keys.create(OFFICE, &DbKey::from_bytes([9; 32])).unwrap();
        let err = supervisor
            .begin_create(create_message(DbKey::from_bytes([4; 32])))
            .err()
            .expect("an existing key refuses the create");
        assert!(matches!(err, CreateStartError::NotStarted(_)), "{err}");
        assert!(keys.holds(OFFICE, &DbKey::from_bytes([9; 32])));
        // A store that fails: nothing stored, nothing started.
        keys.delete(OFFICE).unwrap();
        keys.fail(Op::Create, Fault::Os(-34018));
        let err = supervisor
            .begin_create(create_message(DbKey::from_bytes([4; 32])))
            .err()
            .expect("a failing store refuses the create");
        assert!(matches!(err, CreateStartError::NotStarted(_)), "{err}");
        assert!(!keys.has(OFFICE));
        assert!(!ran.exists(), "a server started");
    }

    #[test]
    fn a_create_whose_server_cant_start_keeps_its_key_for_the_discard() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Arc::new(MemoryKeys::default());
        let supervisor = supervisor(dir.path(), dir.path().join("no-such-server"), &keys);
        let err = supervisor
            .begin_create(create_message(DbKey::from_bytes([5; 32])))
            .err()
            .expect("no server to start");
        assert!(matches!(err, CreateStartError::KeyStored(_)), "{err}");
        assert!(keys.holds(OFFICE, &DbKey::from_bytes([5; 32])));
        assert!(!keys.calls().iter().any(|(op, _)| *op == Op::Delete));
    }

    /// A `--discard` server that records its first line's `db_key` (`null` or `key`) and reports
    /// what `report` says.
    fn discard_server(dir: &Path, report: &str) -> PathBuf {
        let seen = dir.join("seen");
        let mut line = Vec::new();
        match report {
            "discarded" => write_server_event(&mut line, &ServerEvent::Discarded).unwrap(),
            _ => write_server_event(
                &mut line,
                &ServerEvent::Failed {
                    code: FailCode::StorageError,
                    message: "refused".into(),
                },
            )
            .unwrap(),
        }
        script(
            dir,
            &format!("server-{report}"),
            &format!(
                "read -r first\ncase \"$first\" in *'\"db_key\":null'*) echo null >> '{seen}';; \
                 *) echo key >> '{seen}';; esac\nprintf '%s' '{}'",
                String::from_utf8(line).unwrap(),
                seen = seen.display()
            ),
        )
    }

    #[test]
    fn a_discard_deletes_the_key_only_after_discarded() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Arc::new(MemoryKeys::default());
        let key = DbKey::from_bytes([6; 32]);
        keys.create(OFFICE, &key).unwrap();

        let refusing = supervisor(dir.path(), discard_server(dir.path(), "failed"), &keys);
        assert_eq!(
            refusing.discard(OFFICE).unwrap(),
            DiscardOutcome::Failed(FailCode::StorageError)
        );
        assert!(
            keys.holds(OFFICE, &key),
            "a refused discard deleted the key"
        );

        let discarding = supervisor(dir.path(), discard_server(dir.path(), "discarded"), &keys);
        assert_eq!(
            discarding.discard(OFFICE).unwrap(),
            DiscardOutcome::Discarded
        );
        assert!(!keys.has(OFFICE));
        // Without a key: a `null` line, and a delete that finds nothing.
        assert_eq!(
            discarding.discard(OFFICE).unwrap(),
            DiscardOutcome::Discarded
        );
        let seen = fs::read_to_string(dir.path().join("seen")).unwrap();
        assert_eq!(seen, "key\nkey\nnull\n");
    }

    #[test]
    fn a_discard_whose_key_cant_be_read_starts_nothing_and_keeps_everything() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Arc::new(MemoryKeys::default());
        keys.create(OFFICE, &DbKey::from_bytes([7; 32])).unwrap();
        let supervisor = supervisor(dir.path(), discard_server(dir.path(), "discarded"), &keys);
        for fault in [Fault::Corrupt, Fault::Io, Fault::Os(-25308)] {
            keys.fail(Op::Load, fault);
            assert_eq!(
                supervisor.discard(OFFICE).unwrap(),
                DiscardOutcome::KeyUnreadable,
                "{fault:?}"
            );
        }
        assert!(!dir.path().join("seen").exists(), "a server started");
        assert!(keys.has(OFFICE));
        assert!(!keys.calls().iter().any(|(op, _)| *op == Op::Delete));
    }
}
