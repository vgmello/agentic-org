//! Supervision: one `agentic-org-office` per office, each watched by its own thread
//! (`office`), plus the short runs around them: `--create` makes a new office, `--discard`
//! removes an unfinished one.
//!
//! A launch handoff carries only the office's
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
mod signal;
mod state;

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use org_office_launch::FailCode;
use org_proto::OfficeStatusReason;

use crate::host_log;
use crate::ids::handle_of;
use crate::keys::OfficeSecretsKeys;
use crate::logs::RotatingLog;
use crate::server_check::ServerCheck;
use crate::sync::lock;
use crate::timings::Timings;
use child::Spawner;
use office::{Adopted, OfficeRunner};
use signal::Signal;
use state::{Observer, RestartRequest, SharedState};
pub(crate) use state::{OfficeState, StatusObserver};
mod create;
mod join;
pub(crate) use create::{CreateOutcome, CreateStartError};
pub(crate) use join::{
    claim_join_adoption, is_pending_join_active, stop_pending_join, JoinStep, OwnedJoinRun,
};

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
    /// Optional persisted HQ mTLS federation listener configuration from setup.
    pub(crate) federation: Option<crate::instance::setup::FederationConfig>,
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
    /// `--create`, `--join`, and `--discard` runs in flight, so shutdown can stop them too.
    runs: HashMap<u64, Sender<Signal>>,
    /// Active satellite `--join` run key indexed by `office_id` so removal cancels and reaps it
    /// before clearing pending state, and replay knows whether a join is still live (`TASK-52` #3/#4).
    pending_joins: HashMap<String, u64>,
    /// Offices whose `--join` worker is currently being stopped and reaped, preventing a second
    /// concurrent join worker from spawning before reaping completes (`TASK-52` P3-#6).
    stopping_joins: std::collections::HashSet<String>,
    /// Join run keys cancelled by `office.remove_pending` so their watcher can never adopt them.
    cancelled_joins: std::collections::HashSet<u64>,
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

/// How long past the grace period a shutdown waits for its short runs and the offices'
/// supervision threads to end: the time a run's owner (or an office's thread) needs to
/// SIGKILL and reap its server at the deadline and drop its entry. With the host's other
/// waits it ends inside the host's stop bound of 2.5 s (`Timings`).
const RUN_END_MARGIN: Duration = Duration::from_millis(500);
const RUN_END_POLL: Duration = Duration::from_millis(10);

/// A short run's entry in [`State::runs`], removed when dropped: after its server was
/// stopped (or handed to supervision by [`Supervisor::adopt`]).
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
            home: config.home.clone(),
            runtime_dir: config.runtime_dir.clone(),
            check: Arc::clone(&config.server_check),
            // Built from here on, once for this host's lifetime: every server starts from it.
            images: child::Images::new(Arc::clone(&config.server_check), config.program.clone()),
            broker: Arc::new(channels::Broker::new(instance.into(), runtime)),
            federation: config.federation.clone(),
            #[cfg(test)]
            extra_env: Vec::new(),
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

    /// Updates an office's persisted writer owner binding.
    pub(crate) fn update_owner(
        &self,
        office_id: &str,
        org_id: &str,
        owner_account: &str,
        owner_member: &str,
    ) -> io::Result<()> {
        if let Some(spawner) = &self.spawner {
            spawner.update_owner(office_id, org_id, owner_account, owner_member)?;
        }
        Ok(())
    }

    /// Gracefully reloads a supervised office worker without tripping crash backoff.
    pub(crate) fn reload_office(&self, office_id: &str) {
        let state = lock(&self.state);
        if let Some(slot) = state.offices.get(office_id) {
            let _ = slot.signals.send(Signal::Reload);
        }
    }

    /// Runs `agentic-org-office --inspect-import` read-only to clone and validate an existing
    /// organization's Git state branch (`TASK-39` / R1). Carries no keys, no writer custody,
    /// clears the host environment to the minimal allowlist, and enforces a strict wall-clock timeout.
    pub(crate) fn inspect_import(
        &self,
        repository_remote_url: &str,
        state_branch: &str,
    ) -> Result<org_office_launch::InspectImportResponse, String> {
        use std::io::{Read, Write};
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};

        const INSPECT_TIMEOUT: Duration = Duration::from_secs(60);
        const PASSED_ENV: [&str; 6] = ["HOME", "USER", "LOGNAME", "PATH", "TMPDIR", "LANG"];

        self.config.server_check.may_spawn()?;
        let request = org_office_launch::InspectImportRequest {
            repository_remote_url: repository_remote_url.to_owned(),
            state_branch: state_branch.to_owned(),
        };
        let line = serde_json::to_string(&request).map_err(|e| e.to_string())?;
        let mut cmd = Command::new(&self.config.program);
        cmd.env_clear();
        for name in PASSED_ENV {
            if let Some(value) = std::env::var_os(name) {
                cmd.env(name, value);
            }
        }
        let mut child = cmd
            .arg(org_office_launch::INSPECT_IMPORT_FLAG)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("could not run office inspector: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = writeln!(stdin, "{line}");
        }
        let stdout = child.stdout.take();
        let reader = std::thread::spawn(move || {
            let mut out = String::new();
            if let Some(s) = stdout {
                let _ = s.take(1024 * 1024).read_to_string(&mut out);
            }
            out
        });
        let deadline = Instant::now() + INSPECT_TIMEOUT;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = reader.join();
                    return Err("office inspector timed out".to_owned());
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = reader.join();
                    return Err("office inspector wait failed".to_owned());
                }
            }
        }
        let out = reader.join().unwrap_or_default();
        serde_json::from_str(out.trim())
            .map_err(|_| "office inspector returned an unreadable response".to_owned())
    }

    #[cfg(test)]
    pub(crate) fn kill_office_for_test(&self, office_id: &str) {
        let state = lock(&self.state);
        if let Some(slot) = state.offices.get(office_id) {
            let _ = slot.signals.send(Signal::KillForTest);
        }
    }

    /// Stops every server: closes their stdins, waits up to the grace period, SIGKILLs the
    /// rest, and waits for every supervision thread. Short runs (`--create`, `--discard`) in
    /// flight are stopped too, and this waits for them: a run leaves [`State::runs`] only
    /// after its server was stopped or killed and reaped, so no server of a run is left when
    /// this returns. These waits end [`RUN_END_MARGIN`] after the grace period at the latest
    /// (a run whose owner never takes its stop signal, or an office thread still in its
    /// server's launch handshake, is logged, and kills its server when it takes the stop or
    /// drops it).
    pub(crate) fn shutdown(&self) {
        self.shutdown_by(Instant::now() + self.config.timings.shutdown_grace);
    }

    /// [`Supervisor::shutdown`] with the grace period ending at `deadline`, which a stopping
    /// host fixes when it is asked to stop: every server is SIGKILLed then, and the wait for
    /// the short runs ends [`RUN_END_MARGIN`] after it.
    pub(crate) fn shutdown_by(&self, deadline: Instant) {
        let threads: Vec<JoinHandle<()>> = {
            let mut state = lock(&self.state);
            state.stopping = true;
            // A server still in its launch handshake is killed by its owner at once.
            if let Some(spawner) = &self.spawner {
                spawner.broker.stop();
            }
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
        let until = deadline + RUN_END_MARGIN;
        // A thread whose server is still in its launch handshake takes its stop only when
        // the handshake ends (up to its own deadline): it is not waited for past the bound,
        // and stops that server as soon as it sees the stop.
        let mut left = false;
        for thread in threads {
            left |= !crate::sync::join_by(thread, until);
        }
        if left {
            host_log::log("an office's supervision did not end with the host");
        }
        while !lock(&self.state).runs.is_empty() {
            if Instant::now() >= until {
                host_log::log("a create or discard run did not end with the host");
                return;
            }
            thread::sleep(RUN_END_POLL);
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

#[cfg(test)]
pub(crate) mod tests;
