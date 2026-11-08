//! One office's supervision: keep its server running, track its status (with why it's
//! `offline` or `unavailable`, and when it restarts next), restart it with backoff, and stop
//! restarting on a fatal failure or when it keeps failing (the give-up rule), until
//! `office.restart` or the next host run.
//!
//! Every start loads the office's database key from the key store first. A missing item, or one
//! that isn't a key, starts no server: the office is `unavailable`/`key_missing`, the same as a
//! server reporting `key_missing`, and `office.restart` loads it again. Any other key store error
//! is a `storage_error`, retried with backoff. The key is dropped once its line is written.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use org_office_launch::{FailCode, ServerEvent};
use org_proto::{AppToken, OfficeStatusReason};

use super::child::{Mode, RunningChild, Signal, Spawner};
use super::state::{OfficeState, SharedState};
use crate::host_log;
use crate::keys::{KeyStore, KeyStoreError};
use crate::logs::RotatingLog;
use crate::timings::Timings;

/// A server that's already running when supervision starts (the one that just created the
/// office), with the signals that arrived before supervision took over.
pub(crate) struct Adopted {
    pub(crate) child: RunningChild,
    pub(crate) backlog: Vec<Signal>,
    pub(crate) ready: bool,
    /// When its ready wait started: its spawn, or its adoption after a create.
    pub(crate) ready_from: Instant,
}

/// How long, after a server's exit is seen directly, its last lines are waited for.
const EXIT_LINES_WAIT: Duration = Duration::from_millis(200);

/// Everything one office's supervision thread owns.
pub(crate) struct OfficeRunner {
    pub(crate) office_id: String,
    /// The office's app token for this host run: every start gets it. Shared with the
    /// supervisor, which hands the token to the app (`office.app_token`).
    pub(crate) token: Arc<AppToken>,
    /// Where the office's database key is kept; loaded before every start.
    pub(crate) keys: Arc<dyn KeyStore>,
    pub(crate) spawner: Spawner,
    pub(crate) timings: Timings,
    pub(crate) state: Arc<SharedState>,
    pub(crate) signals: Receiver<Signal>,
    pub(crate) sender: Sender<Signal>,
    pub(crate) log: Arc<Mutex<RotatingLog>>,
}

/// How one run of the server ended.
#[derive(Debug, PartialEq, Eq)]
enum Ending {
    /// The host is shutting down; the server was stopped.
    Stopped,
    /// It ended for `reason`. `fatal`: a restart can't fix it, so none is attempted on its own.
    /// `uptime`: how long it ran after `ready` (`None` if it never was).
    Ended {
        reason: OfficeStatusReason,
        fatal: bool,
        uptime: Option<Duration>,
    },
}

/// What [`OfficeRunner::take_restart_requests`] found.
enum Taken {
    Nothing,
    Restart,
    Stop,
}

/// What ended a wait between runs.
enum Wake {
    /// Time to start again.
    Due,
    /// `office.restart`: start again now, with the backoff and the give-up count reset.
    Restart,
    /// The host is shutting down.
    Stop,
}

impl OfficeRunner {
    /// Runs until the host shuts down.
    pub(crate) fn run(self, adopted: Option<Adopted>) {
        let mut backoff = Backoff::new(&self.timings);
        let mut give_up = GiveUp::new(&self.timings);
        let mut generation = adopted.as_ref().map_or(0, |a| a.child.generation + 1);
        let mut next = adopted;
        loop {
            let ending = match next.take() {
                Some(adopted) => self.watch(adopted),
                None => {
                    // `starting` first, then the restart requests sent while it was down: see
                    // `SharedState::request_restart`.
                    self.state.set(OfficeState::starting());
                    match self.take_restart_requests() {
                        Taken::Stop => return,
                        Taken::Restart => {
                            backoff.reset();
                            give_up.reset();
                        }
                        Taken::Nothing => {}
                    }
                    self.start(&mut generation)
                }
            };
            let wake = match ending {
                Ending::Stopped => return,
                Ending::Ended {
                    reason,
                    fatal: true,
                    ..
                } => {
                    self.state.set(OfficeState::unavailable(reason));
                    self.wait_for_restart()
                }
                Ending::Ended { reason, uptime, .. } => {
                    if give_up.after_exit(uptime) {
                        host_log::log(&format!(
                            "office {} kept failing; it won't be restarted on its own",
                            self.office_id
                        ));
                        self.state.set(OfficeState::unavailable(
                            OfficeStatusReason::RestartsExhausted,
                        ));
                        self.wait_for_restart()
                    } else {
                        let delay = backoff.after_exit(uptime);
                        self.state
                            .set(OfficeState::offline(reason, SystemTime::now() + delay));
                        self.wait_out(delay)
                    }
                }
            };
            match wake {
                Wake::Stop => return,
                Wake::Restart => {
                    backoff.reset();
                    give_up.reset();
                }
                Wake::Due => {}
            }
        }
    }

    /// Takes the signals queued while the office was down: restart requests (an `office.restart`
    /// that came as a backoff ended still resets the counts), and a stop.
    fn take_restart_requests(&self) -> Taken {
        let mut taken = Taken::Nothing;
        while let Ok(signal) = self.signals.try_recv() {
            match signal {
                Signal::Stop { .. } => return Taken::Stop,
                Signal::Restart => taken = Taken::Restart,
                // A late line from a server that already exited.
                _ => {}
            }
        }
        taken
    }

    /// Loads the office's key, then starts its server (already marked `starting`) and watches
    /// it. Without its key, nothing is started.
    fn start(&self, generation: &mut u64) -> Ending {
        let key = match self.keys.load(&self.office_id) {
            Ok(Some(key)) => key,
            Ok(None) | Err(KeyStoreError::Corrupt) => {
                host_log::log(&format!(
                    "office {} has no usable key in the host's key store; it isn't started \
                     (office.restart tries again)",
                    self.office_id
                ));
                return Ending::Ended {
                    reason: OfficeStatusReason::KeyMissing,
                    fatal: true,
                    uptime: None,
                };
            }
            Err(e) => {
                host_log::log(&format!(
                    "couldn't load the key of office {}: {e}",
                    self.office_id
                ));
                return Ending::Ended {
                    reason: OfficeStatusReason::StorageError,
                    fatal: false,
                    uptime: None,
                };
            }
        };
        let mode = Mode::Office(&self.office_id, &self.token, &key);
        let spawned = self
            .spawner
            .spawn(mode, *generation, self.sender.clone(), self.log.clone());
        // The line is written (or can't be): only the key store holds the key from here on.
        drop(key);
        *generation += 1;
        match spawned {
            Ok(child) => self.watch(Adopted {
                ready_from: child.started,
                child,
                backlog: Vec::new(),
                ready: false,
            }),
            Err(e) => {
                host_log::log(&format!(
                    "couldn't start the server of office {}: {e}",
                    self.office_id
                ));
                Ending::Ended {
                    reason: OfficeStatusReason::Crashed,
                    fatal: false,
                    uptime: None,
                }
            }
        }
    }

    /// Watches one server until it exits, misses its ready deadline, or the host stops it.
    fn watch(&self, adopted: Adopted) -> Ending {
        let Adopted {
            mut child,
            backlog,
            ready,
            ready_from,
        } = adopted;
        // When it became ready: its uptime counts from then, not from its spawn.
        let mut ready_at = ready.then(Instant::now);
        if ready {
            self.state.set(OfficeState::active());
        }
        let ready_deadline = ready_from + self.timings.ready_timeout;
        let mut failure: Option<FailCode> = None;
        let mut backlog = backlog.into_iter();
        loop {
            let signal = match backlog.next() {
                Some(signal) => signal,
                None => {
                    // Wake at least every `exit_poll`: a server whose stdout a descendant still
                    // holds exits without its pipe closing, so its exit is watched for directly.
                    let mut wait = self.timings.exit_poll;
                    if ready_at.is_none() {
                        wait = wait.min(ready_deadline.saturating_duration_since(Instant::now()));
                    }
                    match self.signals.recv_timeout(wait) {
                        Ok(signal) => signal,
                        Err(RecvTimeoutError::Disconnected) => {
                            child.kill();
                            return Ending::Stopped;
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            if child.has_exited() {
                                let Some(failure) =
                                    self.lines_after_exit(child.generation, failure)
                                else {
                                    return Ending::Stopped;
                                };
                                return after_exit(failure, ready_at);
                            }
                            if ready_at.is_none() && Instant::now() >= ready_deadline {
                                child.kill();
                                // A `failed` line already said why: its reason (and whether a
                                // restart can help) stands.
                                if let Some((reason, fatal)) = failure.map(reason_for) {
                                    return Ending::Ended {
                                        reason,
                                        fatal,
                                        uptime: None,
                                    };
                                }
                                host_log::log(&format!(
                                    "the server of office {} wasn't ready in time; restarting it",
                                    self.office_id
                                ));
                                return Ending::Ended {
                                    reason: OfficeStatusReason::StartTimeout,
                                    fatal: false,
                                    uptime: None,
                                };
                            }
                            continue;
                        }
                    }
                }
            };
            match signal {
                Signal::Stop { deadline } => {
                    child.stop(deadline);
                    return Ending::Stopped;
                }
                // It's already running: nothing to restart.
                Signal::Restart => {}
                Signal::Event { generation, .. } | Signal::Closed { generation }
                    if generation != child.generation => {}
                Signal::Event { event, .. } => match event {
                    ServerEvent::Ready { socket, .. } => {
                        if Path::new(&socket) != self.expected_socket() {
                            host_log::log(&format!(
                                "office {} reported a socket other than its own; clients use its own",
                                self.office_id
                            ));
                        }
                        ready_at.get_or_insert_with(Instant::now);
                        self.state.set(OfficeState::active());
                    }
                    ServerEvent::Failed { code, message } => {
                        host_log::log(&format!(
                            "the server of office {} failed ({code:?}): {}",
                            self.office_id,
                            host_log::untrusted(&message)
                        ));
                        failure = Some(code);
                    }
                    _ => {}
                },
                Signal::Closed { .. } => {
                    child.stop(Instant::now() + self.timings.shutdown_grace);
                    return after_exit(failure, ready_at);
                }
            }
        }
    }

    /// After a server's exit was seen directly: takes the lines it sent that are still on their
    /// way from its stdout reader (until its pipe closes, at most [`EXIT_LINES_WAIT`]), so a
    /// fatal `failed` isn't missed. Returns the failure it reported, or `None` if the host is
    /// stopping.
    fn lines_after_exit(
        &self,
        generation: u64,
        mut failure: Option<FailCode>,
    ) -> Option<Option<FailCode>> {
        let until = Instant::now() + EXIT_LINES_WAIT;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            match self.signals.recv_timeout(left) {
                Ok(Signal::Stop { .. }) | Err(RecvTimeoutError::Disconnected) => return None,
                Ok(Signal::Event {
                    generation: g,
                    event: ServerEvent::Failed { code, message },
                }) if g == generation => {
                    host_log::log(&format!(
                        "the server of office {} failed ({code:?}): {}",
                        self.office_id,
                        host_log::untrusted(&message)
                    ));
                    failure = Some(code);
                }
                Ok(Signal::Closed { generation: g }) if g == generation => return Some(failure),
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => {
                    host_log::log(&format!(
                        "the server of office {} exited, though its control pipe is still open",
                        self.office_id
                    ));
                    return Some(failure);
                }
            }
        }
    }

    fn expected_socket(&self) -> std::path::PathBuf {
        org_proto::location::office_socket_path(&self.spawner.home, &self.office_id)
            .unwrap_or_default()
    }

    /// Waits out a backoff delay.
    fn wait_out(&self, delay: Duration) -> Wake {
        let until = Instant::now() + delay;
        loop {
            match self
                .signals
                .recv_timeout(until.saturating_duration_since(Instant::now()))
            {
                Ok(Signal::Stop { .. }) | Err(RecvTimeoutError::Disconnected) => return Wake::Stop,
                Ok(Signal::Restart) => return Wake::Restart,
                Ok(_) => {} // a late line from the server that just exited
                Err(RecvTimeoutError::Timeout) => return Wake::Due,
            }
        }
    }

    /// Waits, `unavailable`, for `office.restart` (or the host stopping).
    fn wait_for_restart(&self) -> Wake {
        loop {
            match self.signals.recv() {
                Ok(Signal::Stop { .. }) | Err(_) => return Wake::Stop,
                Ok(Signal::Restart) => return Wake::Restart,
                Ok(_) => {}
            }
        }
    }
}

/// How a server that exited ended: the reason its `failed` line gave (fatal or not), else a
/// crash; `ready_at` gives its uptime.
fn after_exit(failure: Option<FailCode>, ready_at: Option<Instant>) -> Ending {
    let (reason, fatal) = failure.map_or((OfficeStatusReason::Crashed, false), reason_for);
    Ending::Ended {
        reason,
        fatal,
        uptime: ready_at.map(|at| at.elapsed()),
    }
}

/// The status reason for a `failed` code, and whether it's fatal (no restart on its own).
/// `invalid_name` and `already_exists` end only a create; from an `--office` run they mean the
/// server misunderstood the host, like `invalid_request`. Unknown codes (decoded as
/// `storage_error`) are retried.
pub(crate) fn reason_for(code: FailCode) -> (OfficeStatusReason, bool) {
    match code {
        FailCode::InUse => (OfficeStatusReason::InUse, false),
        FailCode::KeyMissing => (OfficeStatusReason::KeyMissing, true),
        FailCode::UnsupportedVersion => (OfficeStatusReason::UnsupportedVersion, true),
        FailCode::InvalidRequest | FailCode::InvalidName | FailCode::AlreadyExists => {
            (OfficeStatusReason::InvalidRequest, true)
        }
        _ => (OfficeStatusReason::StorageError, false),
    }
}

/// Restart delays: `initial`, doubling up to `max`, back to `initial` once a server stayed up
/// for `reset_after` after getting ready.
#[derive(Debug)]
pub(crate) struct Backoff {
    initial: Duration,
    max: Duration,
    reset_after: Duration,
    next: Duration,
}

impl Backoff {
    pub(crate) fn new(timings: &Timings) -> Self {
        Self {
            initial: timings.backoff_initial,
            max: timings.backoff_max,
            reset_after: timings.backoff_reset_after,
            next: timings.backoff_initial,
        }
    }

    /// The delay before restarting a server that was up for `uptime` after `ready` (`None`:
    /// it never got ready).
    pub(crate) fn after_exit(&mut self, uptime: Option<Duration>) -> Duration {
        if uptime.is_some_and(|up| up >= self.reset_after) {
            self.next = self.initial;
        }
        let delay = self.next.min(self.max);
        self.next = (self.next * 2).min(self.max);
        delay
    }

    pub(crate) fn reset(&mut self) {
        self.next = self.initial;
    }
}

/// The give-up rule: `limit` failed restarts within `window` and the host stops restarting. A
/// failed restart is an exit before `fresh_after` of uptime after `ready` (or with no `ready`);
/// a longer run counts as a fresh start and clears the count.
#[derive(Debug)]
pub(crate) struct GiveUp {
    limit: usize,
    window: Duration,
    fresh_after: Duration,
    failures: VecDeque<Instant>,
}

impl GiveUp {
    pub(crate) fn new(timings: &Timings) -> Self {
        Self {
            limit: timings.give_up_after as usize,
            window: timings.give_up_window,
            fresh_after: timings.backoff_reset_after,
            failures: VecDeque::new(),
        }
    }

    /// Counts an exit after `uptime`. Returns whether the office has now failed too often.
    pub(crate) fn after_exit(&mut self, uptime: Option<Duration>) -> bool {
        if uptime.is_some_and(|up| up >= self.fresh_after) {
            self.failures.clear();
            return false;
        }
        let now = Instant::now();
        self.failures.push_back(now);
        while self
            .failures
            .front()
            .is_some_and(|&at| now.duration_since(at) > self.window)
        {
            self.failures.pop_front();
        }
        self.failures.len() >= self.limit
    }

    pub(crate) fn reset(&mut self) {
        self.failures.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_to_the_cap_and_resets_after_uptime_from_ready() {
        let mut b = Backoff::new(&Timings::default());
        let s = Duration::from_secs;
        let delays: Vec<_> = (0..8).map(|_| b.after_exit(Some(s(1)))).collect();
        assert_eq!(delays, [s(1), s(2), s(4), s(8), s(16), s(32), s(60), s(60)]);
        // A server that never got ready doesn't reset it; a long one after ready does.
        assert_eq!(b.after_exit(None), s(60));
        assert_eq!(b.after_exit(Some(s(60))), s(1));
        assert_eq!(b.after_exit(Some(s(1))), s(2));
        b.reset();
        assert_eq!(b.after_exit(None), s(1));
    }

    #[test]
    fn reasons_follow_the_failed_codes() {
        use OfficeStatusReason as R;
        for (code, reason, fatal) in [
            (FailCode::InUse, R::InUse, false),
            (FailCode::StorageError, R::StorageError, false),
            (FailCode::KeyMissing, R::KeyMissing, true),
            (FailCode::UnsupportedVersion, R::UnsupportedVersion, true),
            (FailCode::InvalidRequest, R::InvalidRequest, true),
            (FailCode::InvalidName, R::InvalidRequest, true),
            (FailCode::AlreadyExists, R::InvalidRequest, true),
        ] {
            assert_eq!(reason_for(code), (reason, fatal), "{code:?}");
        }
    }

    #[test]
    fn the_give_up_rule_counts_short_runs_and_clears_on_a_long_one() {
        let timings = Timings {
            give_up_after: 3,
            ..Timings::default()
        };
        let mut g = GiveUp::new(&timings);
        let s = Duration::from_secs;
        assert!(!g.after_exit(None));
        assert!(!g.after_exit(Some(s(5))));
        // A run of 60 s after ready is a fresh start.
        assert!(!g.after_exit(Some(s(60))));
        assert!(!g.after_exit(None));
        assert!(!g.after_exit(None));
        assert!(g.after_exit(None));
        g.reset();
        assert!(!g.after_exit(None));
    }
}
