//! Satellite join runs: starting a joining office's worker, waiting for its steps and
//! adopting it once it joined, or stopping it (`doc-67`, D25 re-enrollment).

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use org_office_launch::{OfficeFile, ServerEvent};
use org_proto::{OfficeStatus, OfficeStatusReason};

use crate::host_log;
use crate::keys::OfficeSecretsKeys;
use crate::logs::RotatingLog;
use crate::sync::lock;

use super::child::{JoinLine, Mode, RunningChild, Spawner};
use super::create::CreateStartError;
use super::office::Adopted;
use super::signal::Signal;
use super::state::OfficeState;
use super::{Supervisor, RUN_END_MARGIN, RUN_END_POLL};

/// An in-flight `--join` satellite enrollment run registered in `Supervisor::state.runs` (`TASK-52` H-1).
pub(crate) struct OwnedJoinRun {
    office_id: String,
    child: RunningChild,
    signals: Receiver<Signal>,
    sender: Sender<Signal>,
    log: Arc<Mutex<RotatingLog>>,
    pub(crate) run_key: u64,
}

/// Outcome of waiting for the next milestone on a `--join` worker.
pub(crate) enum JoinStep {
    Pending {
        check_code: String,
    },
    Joined {
        office: OfficeFile,
        backlog: Vec<Signal>,
        ready: bool,
    },
    Failed(String),
    Crashed,
    TimedOut,
    Stopped,
}

impl Supervisor {
    /// Stops and removes any currently supervised worker for `office_id` before D25 re-enrollment
    /// launches `--join` so the office folder lock is released cleanly (`TASK-52` H-1).
    pub(crate) fn stop_office_for_re_enroll(&self, office_id: &str) {
        stop_office_for_re_enroll_inner(self, office_id);
    }

    /// Launches `agentic-org-office --join` over the verified rendezvous for satellite enrollment
    /// or D25 re-enrollment (`TASK-52` H-1).
    pub(crate) fn begin_satellite_join(
        &self,
        first: org_office_launch::JoinLine,
        operation_id: &str,
        invitation_token: org_host_control::InvitationTokenSecret,
        actor_account: &str,
    ) -> Result<OwnedJoinRun, CreateStartError> {
        begin_satellite_join_inner(self, first, operation_id, invitation_token, actor_account)
    }

    /// Waits up to `timeout` for the next `JoinPending` or `Joined` event from a `--join` child.
    pub(crate) fn wait_join_step(&self, run: &mut OwnedJoinRun, timeout: Duration) -> JoinStep {
        wait_join_step_inner(self, run, timeout)
    }

    /// Stops and unregisters a `--join` run that failed, timed out, or was interrupted.
    pub(crate) fn finish_join_run(&self, run: OwnedJoinRun) {
        finish_join_run_inner(self, run);
    }

    /// Adopts a `--join` worker that emitted `ServerEvent::Joined` into normal office supervision.
    pub(crate) fn adopt_owned_join(
        &self,
        run: OwnedJoinRun,
        office: OfficeFile,
        backlog: Vec<Signal>,
        ready: bool,
    ) -> OfficeState {
        adopt_owned_join_inner(self, run, office, backlog, ready)
    }
}

fn stop_office_for_re_enroll_inner(sup: &Supervisor, office_id: &str) {
    let slot = lock(&sup.state).offices.remove(office_id);
    if let Some(mut slot) = slot {
        let deadline = Instant::now() + sup.config.timings.shutdown_grace;
        let _ = slot.signals.send(Signal::Stop { deadline });
        if let Some(thread) = slot.thread.take() {
            let _ = crate::sync::join_by(thread, deadline + RUN_END_MARGIN);
        }
    }
}

fn register_join_run(
    sup: &Supervisor,
    office_id: &str,
    sender: &Sender<Signal>,
) -> Result<u64, CreateStartError> {
    let mut state = lock(&sup.state);
    if state.stopping {
        return Err(CreateStartError::NotStarted(
            "the host is shutting down".into(),
        ));
    }
    if state.stopping_joins.contains(office_id)
        || state
            .pending_joins
            .get(office_id)
            .is_some_and(|k| state.runs.contains_key(k))
    {
        return Err(CreateStartError::NotStarted(
            "a join for this office is already active or being cancelled".into(),
        ));
    }
    let key = state.next_run;
    state.next_run += 1;
    state.runs.insert(key, sender.clone());
    state.pending_joins.insert(office_id.to_owned(), key);
    Ok(key)
}

/// Returns whether a live `--join` worker is currently tracked for `office_id`.
pub(crate) fn is_pending_join_active(sup: &Supervisor, office_id: &str) -> bool {
    let state = lock(&sup.state);
    state.stopping_joins.contains(office_id)
        || state
            .pending_joins
            .get(office_id)
            .is_some_and(|k| state.runs.contains_key(k) && !state.cancelled_joins.contains(k))
}

/// Cancels any active `--join` run for `office_id`, signals it to stop, and waits until the
/// worker is stopped and unregistered so it can never publish `office.json` or be adopted after
/// `office.remove_pending` succeeds (`TASK-52` #3 / P3-#6 / P4-#11). Returns `true` iff no active
/// worker remains; if the wait expires before reaping completes, keeps `stopping_joins` reserved
/// (until `finish_join_run_inner` clears it on reap) and returns `false`.
pub(crate) fn stop_pending_join(sup: &Supervisor, office_id: &str) -> bool {
    let grace = sup.config.timings.shutdown_grace;
    let deadline = Instant::now() + grace;
    let maybe = {
        let mut state = lock(&sup.state);
        if state.stopping_joins.contains(office_id) {
            return false;
        }
        state.pending_joins.remove(office_id).map(|key| {
            state.stopping_joins.insert(office_id.to_owned());
            state.cancelled_joins.insert(key);
            let tx = state.runs.get(&key).cloned();
            (key, tx)
        })
    };
    let Some((key, tx)) = maybe else {
        return true;
    };
    if let Some(sender) = tx {
        let _ = sender.send(Signal::Stop { deadline });
    }
    let wait_until = deadline + RUN_END_MARGIN;
    let mut reaped = false;
    while Instant::now() < wait_until {
        if !lock(&sup.state).runs.contains_key(&key) {
            reaped = true;
            break;
        }
        std::thread::sleep(RUN_END_POLL);
    }
    if reaped {
        lock(&sup.state).stopping_joins.remove(office_id);
    }
    reaped
}

/// Atomically verifies that `run_key` has NOT been cancelled before adopting a joined satellite.
pub(crate) fn claim_join_adoption(sup: &Supervisor, office_id: &str, run_key: u64) -> bool {
    let mut state = lock(&sup.state);
    if state.stopping || state.cancelled_joins.remove(&run_key) {
        return false;
    }
    if state.pending_joins.get(office_id) == Some(&run_key) {
        state.pending_joins.remove(office_id);
        true
    } else {
        false
    }
}

fn load_or_create_satellite_secrets_key(
    keys: &Arc<dyn OfficeSecretsKeys>,
    office_id: &str,
) -> Result<org_office_launch::OfficeSecretsKey, CreateStartError> {
    match keys.load(office_id) {
        Ok(Some(existing)) => Ok(existing),
        Ok(None) => {
            let fresh = crate::secrets::generate_office_secrets_key()
                .map_err(|e| CreateStartError::NotStarted(e.to_string()))?;
            keys.create(office_id, &fresh).map_err(|e| {
                CreateStartError::NotStarted(format!("satellite secrets key storage failed: {e}"))
            })?;
            Ok(fresh)
        }
        Err(e) => Err(CreateStartError::NotStarted(format!(
            "satellite secrets key unavailable: {e}"
        ))),
    }
}

fn prepare_satellite_join_prerequisites<'a>(
    sup: &'a Supervisor,
    first: &org_office_launch::JoinLine,
    operation_id: &str,
    actor_account: &str,
) -> Result<(&'a Spawner, org_office_launch::OfficeSecretsKey), CreateStartError> {
    sup.config
        .server_check
        .may_spawn()
        .map_err(CreateStartError::NotStarted)?;
    let spawner = sup
        .spawner()
        .map_err(|e| CreateStartError::NotStarted(e.to_string()))?;
    if let Some(writers) = &spawner.broker.writers {
        crate::writer::reserve_satellite(
            writers,
            operation_id,
            &first.org_id,
            &first.office.office_id,
            actor_account,
        )
        .map_err(|e| {
            CreateStartError::NotStarted(format!("satellite custody reservation failed: {e}"))
        })?;
    }
    let key = load_or_create_satellite_secrets_key(&sup.config.keys, &first.office.office_id)?;
    Ok((spawner, key))
}

fn begin_satellite_join_inner(
    sup: &Supervisor,
    first: org_office_launch::JoinLine,
    operation_id: &str,
    invitation_token: org_host_control::InvitationTokenSecret,
    actor_account: &str,
) -> Result<OwnedJoinRun, CreateStartError> {
    let (sender, signals) = mpsc::channel();
    let office_id = first.office.office_id.clone();
    let run_key = register_join_run(sup, &office_id, &sender)?;
    let (spawner, office_secrets_key) =
        match prepare_satellite_join_prerequisites(sup, &first, operation_id, actor_account) {
            Ok(pair) => pair,
            Err(err) => {
                let mut state = lock(&sup.state);
                state.runs.remove(&run_key);
                if state.pending_joins.get(&office_id) == Some(&run_key) {
                    state.pending_joins.remove(&office_id);
                }
                return Err(err);
            }
        };
    let line = JoinLine::satellite(first, office_secrets_key, invitation_token);
    let log = sup.log_for(line.office_id());
    let spawned = spawner.spawn(Mode::Join(&line), 0, sender.clone(), Arc::clone(&log));
    drop(line);
    let child = match spawned {
        Ok(c) => c,
        Err(e) => {
            let mut state = lock(&sup.state);
            state.runs.remove(&run_key);
            if state.pending_joins.get(&office_id) == Some(&run_key) {
                state.pending_joins.remove(&office_id);
            }
            return Err(CreateStartError::KeyStored(e.into()));
        }
    };
    Ok(OwnedJoinRun {
        office_id,
        child,
        signals,
        sender,
        log,
        run_key,
    })
}

fn wait_join_step_inner(sup: &Supervisor, run: &mut OwnedJoinRun, timeout: Duration) -> JoinStep {
    let deadline = Instant::now() + timeout;
    let grace = sup.config.timings.shutdown_grace;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let signal = match run.signals.recv_timeout(left) {
            Ok(sig) => sig,
            Err(RecvTimeoutError::Timeout) => return JoinStep::TimedOut,
            Err(RecvTimeoutError::Disconnected) => {
                run.child.kill();
                return JoinStep::Crashed;
            }
        };
        if let Some(step) = classify_join_signal(signal, run, grace) {
            return step;
        }
    }
}

fn classify_join_signal(
    signal: Signal,
    run: &mut OwnedJoinRun,
    grace: Duration,
) -> Option<JoinStep> {
    match signal {
        Signal::Stop { deadline } => {
            run.child.stop(deadline);
            Some(JoinStep::Stopped)
        }
        Signal::Event {
            event: ServerEvent::JoinPending { check_code, .. },
            ..
        } => Some(JoinStep::Pending { check_code }),
        Signal::Event {
            event: ServerEvent::Joined { office },
            ..
        } => {
            let backlog: Vec<Signal> = run.signals.try_iter().collect();
            let ready = backlog.iter().any(|s| {
                matches!(
                    s,
                    Signal::Event {
                        event: ServerEvent::Ready { .. },
                        ..
                    }
                )
            }) && !backlog.iter().any(|s| matches!(s, Signal::Closed { .. }));
            Some(JoinStep::Joined {
                office,
                backlog,
                ready,
            })
        }
        Signal::Event {
            event: ServerEvent::Failed { code, message },
            ..
        } => {
            host_log::log(&format!(
                "satellite join failed ({code:?}): {}",
                host_log::untrusted(&message)
            ));
            run.child.stop(Instant::now() + grace);
            Some(JoinStep::Failed(message))
        }
        Signal::Closed { .. } => {
            run.child.stop(Instant::now() + grace);
            Some(JoinStep::Crashed)
        }
        Signal::Restart | Signal::Reload | Signal::Event { .. } => None,
        #[cfg(all(feature = "test-support", debug_assertions))]
        Signal::KillForTest => None,
    }
}

fn finish_join_run_inner(sup: &Supervisor, mut run: OwnedJoinRun) {
    let deadline = Instant::now() + sup.config.timings.shutdown_grace;
    run.child.stop(deadline);
    let mut state = lock(&sup.state);
    state.runs.remove(&run.run_key);
    state.pending_joins.retain(|_, k| *k != run.run_key);
    state.cancelled_joins.remove(&run.run_key);
    state.stopping_joins.remove(&run.office_id);
}

fn adopt_owned_join_inner(
    sup: &Supervisor,
    run: OwnedJoinRun,
    office: OfficeFile,
    backlog: Vec<Signal>,
    ready: bool,
) -> OfficeState {
    let mut state = lock(&sup.state);
    if state.stopping {
        drop(state);
        finish_join_run_inner(sup, run);
        return OfficeState {
            status: OfficeStatus::Offline,
            reason: None,
            next_restart_at: None,
        };
    }
    state.runs.remove(&run.run_key);
    let adopted = Adopted {
        child: run.child,
        backlog,
        ready,
        ready_from: Instant::now(),
    };
    let status = if ready {
        OfficeState::active()
    } else {
        OfficeState::starting()
    };
    match sup.spawn_runner(
        &office.office_id,
        run.sender,
        run.signals,
        run.log,
        Some(adopted),
    ) {
        Ok(slot) => {
            state.offices.insert(office.office_id.clone(), slot);
            status
        }
        Err(e) => {
            host_log::log(&format!(
                "couldn't supervise joined satellite {}: {e}",
                office.office_id
            ));
            OfficeState::unavailable(OfficeStatusReason::StorageError)
        }
    }
}
