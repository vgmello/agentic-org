//! Supervision: one `org-office-server` per office, each watched by its own thread
//! (`office`), plus the short runs around them: `--create` makes a new office, `--discard`
//! removes an unfinished one.
//!
//! Each office has an app token for this host run, made when supervision of it starts (or when
//! it is created) and kept, in memory only, by its supervision: every start of its server within
//! the run gets the same token in its first line (`child::StartLine`). The next host run makes
//! new ones.
//!
//! Status per office ([`OfficeStatus`]): `starting` (spawned, no `ready` yet) → `active`
//! (`ready`, child alive) → `offline` (exited, restart pending) → `unavailable` (a fatal
//! failure; until the host restarts). Shutdown closes every child's stdin, waits up to the
//! grace period, then SIGKILLs what's left.

mod child;
mod office;

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use org_office_launch::{CreateOffice, FailCode, OfficeFile, ServerEvent};
use org_proto::OfficeStatus;

use crate::app_token;
use crate::host_log;
use crate::ids::handle_of;
use crate::logs::RotatingLog;
use crate::sync::lock;
use crate::timings::Timings;
use child::{CreateLine, Mode, RunningChild, Signal, Spawner, StartLine};
use office::{Adopted, OfficeRunner};

/// What the supervisor needs to start servers.
#[derive(Debug, Clone)]
pub(crate) struct SupervisorConfig {
    pub(crate) program: PathBuf,
    pub(crate) home: PathBuf,
    pub(crate) logs_dir: PathBuf,
    pub(crate) log_max_bytes: u64,
    pub(crate) timings: Timings,
}

pub(crate) struct Supervisor {
    config: SupervisorConfig,
    spawner: Spawner,
    state: Mutex<State>,
    /// One log per socket handle, shared by every run of that office (`--create`, `--office`,
    /// `--discard`) while any of them still writes it, so two writers never rotate the same
    /// file apart. Held weakly: a log (and its open file) goes once nothing writes it.
    logs: Mutex<HashMap<String, Weak<Mutex<RotatingLog>>>>,
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
    status: Arc<Mutex<OfficeStatus>>,
    signals: Sender<Signal>,
    thread: Option<JoinHandle<()>>,
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

/// A `--create` run in flight.
pub(crate) struct PendingCreate<'a> {
    registration: Registration<'a>,
    /// The create line the server got, kept for the token it holds.
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
    /// The office's start line, with the token it was created with.
    start: StartLine,
    child: RunningChild,
    signals: Receiver<Signal>,
    sender: Sender<Signal>,
    log: Arc<Mutex<RotatingLog>>,
    backlog: Vec<Signal>,
    ready: bool,
}

/// How a `--discard` run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiscardOutcome {
    /// The unfinished office is gone.
    Discarded,
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
        let start = StartLine::new(app_token::generate()?);
        let (sender, signals) = mpsc::channel();
        let log = self.log_for(office_id);
        let slot = self.spawn_runner(office_id, start, sender, signals, log, None)?;
        state.offices.insert(office_id.to_owned(), slot);
        Ok(())
    }

    /// The status of a supervised office, or `None` if it isn't supervised.
    pub(crate) fn status(&self, office_id: &str) -> Option<OfficeStatus> {
        let state = lock(&self.state);
        state.offices.get(office_id).map(|slot| *lock(&slot.status))
    }

    /// Starts `org-office-server --create` and sends it `create`. Its app token becomes the
    /// office's token for this host run if the office is adopted.
    pub(crate) fn begin_create(&self, create: CreateOffice) -> io::Result<PendingCreate<'_>> {
        let (sender, signals) = mpsc::channel();
        let registration = self.register(&sender)?;
        let log = self.log_for(&create.office_id);
        let line = CreateLine::new(create);
        let child = self
            .spawner
            .spawn(Mode::Create(&line), 0, sender.clone(), Arc::clone(&log))?;
        Ok(PendingCreate {
            registration,
            line,
            child,
            signals,
            sender,
            log,
        })
    }

    /// Runs `org-office-server --discard <office_id>` and waits (up to the discard timeout)
    /// for it to report. The server takes the office lock, refuses a folder that holds
    /// `office.json`, and removes the folder and any key an unfinished create left.
    pub(crate) fn discard(&self, office_id: &str) -> io::Result<DiscardOutcome> {
        let (sender, signals) = mpsc::channel();
        let _registration = self.register(&sender)?;
        let mut child =
            self.spawner
                .spawn(Mode::Discard(office_id), 0, sender, self.log_for(office_id))?;
        let timings = &self.config.timings;
        let deadline = Instant::now() + timings.discard_timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let outcome = match signals.recv_timeout(left) {
                Ok(Signal::Stop { deadline }) => {
                    child.stop(deadline);
                    return Ok(DiscardOutcome::Stopped);
                }
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
                    return Ok(DiscardOutcome::TimedOut);
                }
            };
            // It exits right after reporting.
            child.stop(Instant::now() + timings.shutdown_grace);
            return Ok(outcome);
        }
    }

    /// Hands a server that created its office over to supervision, which keeps it running as
    /// that office's server. Returns the office's status: `active` if `ready` already arrived,
    /// otherwise `starting`.
    pub(crate) fn adopt(&self, adoptable: Adoptable<'_>) -> OfficeStatus {
        let Adoptable {
            office,
            mut registration,
            start,
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
            return OfficeStatus::Offline;
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
            OfficeStatus::Active
        } else {
            OfficeStatus::Starting
        };
        match self.spawn_runner(
            &office.office_id,
            start,
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
                OfficeStatus::Unavailable
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

    /// Starts an office's supervision thread, which owns `start` (and so the office's app
    /// token) for the rest of this host run.
    fn spawn_runner(
        &self,
        office_id: &str,
        start: StartLine,
        sender: Sender<Signal>,
        signals: Receiver<Signal>,
        log: Arc<Mutex<RotatingLog>>,
        adopted: Option<Adopted>,
    ) -> io::Result<Slot> {
        let initial = match &adopted {
            Some(adopted) if adopted.ready => OfficeStatus::Active,
            _ => OfficeStatus::Starting,
        };
        let status = Arc::new(Mutex::new(initial));
        let runner = OfficeRunner {
            office_id: office_id.to_owned(),
            start,
            spawner: self.spawner.clone(),
            timings: self.config.timings.clone(),
            status: Arc::clone(&status),
            signals,
            sender: sender.clone(),
            log,
        };
        let thread = thread::Builder::new()
            .name(format!("office-{}", handle_of(office_id)))
            .spawn(move || runner.run(adopted))?;
        Ok(Slot {
            status,
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
            start: self.line.into_start_line(),
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

    use org_office_launch::write_server_event;
    use org_proto::{AppToken, OfficeRole};

    #[test]
    fn a_create_reported_after_shutdown_began_is_stopped_not_adopted() {
        let dir = tempfile::tempdir().unwrap();
        let office_id = "01AAAAAAAAAAAAAAAAAAAAAAAA";
        let office = OfficeFile::new(
            office_id,
            "HQ",
            OfficeRole::Hq,
            "01ORG",
            "Acme",
            "2026-11-08T00:00:00.000Z",
        );
        let mut created = Vec::new();
        write_server_event(&mut created, &ServerEvent::Created { office }).unwrap();
        // A server that reports `created` at once, then keeps running.
        let script = dir.path().join("server");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf '%s' '{}'\nexec /bin/sleep 30\n",
                String::from_utf8(created).unwrap()
            ),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        let timings = Timings {
            shutdown_grace: Duration::from_millis(200),
            ..Timings::default()
        };
        let supervisor = Supervisor::new(SupervisorConfig {
            program: script,
            home: dir.path().to_path_buf(),
            logs_dir: dir.path().join("logs"),
            log_max_bytes: 1024,
            timings,
        });
        let pending = supervisor
            .begin_create(CreateOffice {
                org_id: "01ORG".into(),
                org_name: "Acme".into(),
                office_id: office_id.into(),
                office_name: "HQ".into(),
                role: OfficeRole::Hq,
                app_token: AppToken::from_bytes([1; 32]),
            })
            .unwrap();
        let pid = pending.child.pid();
        let CreateOutcome::Created(adoptable) = pending.wait(Duration::from_secs(10)) else {
            panic!("the server didn't report created");
        };
        // The shutdown begins between `created` and the adoption.
        supervisor.shutdown();
        assert_eq!(supervisor.adopt(*adoptable), OfficeStatus::Offline);
        assert_eq!(supervisor.status(office_id), None, "adopted after shutdown");
        // SAFETY: signal 0 only checks the pid.
        assert!(
            unsafe { libc::kill(pid as libc::pid_t, 0) } != 0,
            "the server outlived the shutdown"
        );
    }

    #[test]
    fn no_office_is_started_once_shutdown_began() {
        let dir = tempfile::tempdir().unwrap();
        let supervisor = Supervisor::new(SupervisorConfig {
            program: dir.path().join("no-such-server"),
            home: dir.path().to_path_buf(),
            logs_dir: dir.path().join("logs"),
            log_max_bytes: 1024,
            timings: Timings::default(),
        });
        supervisor.shutdown();
        assert!(supervisor
            .start_office("01AAAAAAAAAAAAAAAAAAAAAAAA")
            .is_err());
        assert_eq!(supervisor.status("01AAAAAAAAAAAAAAAAAAAAAAAA"), None);
    }
}
