//! HQ office creation, adoption of a created office and discarding one.

use std::io;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use org_office_launch::{OfficeFile, ServerEvent};
use org_proto::OfficeStatus;

use crate::host_log;
use crate::logs::RotatingLog;
use crate::sync::lock;

use super::child::{CreateLine, Mode, RunningChild};
use super::office::Adopted;
use super::signal::Signal;
use super::state::OfficeState;
use super::{DiscardOutcome, Registration, Supervisor};

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
    child: RunningChild,
    signals: Receiver<Signal>,
    sender: Sender<Signal>,
    log: Arc<Mutex<RotatingLog>>,
    /// Last, so it is dropped after `child`: the run stays registered until its server is
    /// killed and reaped (see [`Supervisor::shutdown`]).
    registration: Registration<'a>,
}

/// A server that has created its office, to hand to [`Supervisor::adopt`]. Dropping it instead
/// kills the server.
pub(crate) struct Adoptable<'a> {
    pub(crate) office: OfficeFile,
    child: RunningChild,
    signals: Receiver<Signal>,
    sender: Sender<Signal>,
    log: Arc<Mutex<RotatingLog>>,
    backlog: Vec<Signal>,
    ready: bool,
    /// Last, so it is dropped after `child` (as in [`PendingCreate`]).
    registration: Registration<'a>,
}

impl Supervisor {
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
        // Before its key is read: a create needs the writer custody provisioning reserved
        // for this office, organization and owner member.
        self.spawner()
            .map_err(|e| CreateStartError::NotStarted(e.to_string()))?
            .custody(
                &first.office.office_id,
                Some((&first.org_id, &first.owner_member_id)),
            )
            .map_err(|e| CreateStartError::NotStarted(e.to_string()))?;
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
                Ok(Signal::Restart | Signal::Reload) => continue,
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
                #[cfg(test)]
                Ok(Signal::KillForTest) => continue,
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
        if state.stopping {
            drop(state);
            child.stop(Instant::now() + self.config.timings.shutdown_grace);
            // Only now: the shutdown waits for this run until its server is gone.
            drop(registration);
            return OfficeState {
                status: OfficeStatus::Offline,
                reason: None,
                next_restart_at: None,
            };
        }
        // Supervision owns the server from here on: it is no short run any more.
        if let Some(key) = registration.key.take() {
            state.runs.remove(&key);
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
                Signal::Restart | Signal::Reload => {}
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
                #[cfg(test)]
                Signal::KillForTest => {}
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
