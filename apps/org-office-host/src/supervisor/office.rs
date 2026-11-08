//! One office's supervision: keep its server running, track its status, restart it with
//! backoff after a crash, and give up (status `unavailable`) on a fatal failure.

use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use org_office_launch::{FailCode, ServerEvent};
use org_proto::OfficeStatus;

use super::child::{Mode, RunningChild, Signal, Spawner, StartLine};
use crate::host_log;
use crate::logs::RotatingLog;
use crate::sync::lock;
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
    /// The office's start line, with its app token for this host run: every start gets it.
    pub(crate) start: StartLine,
    pub(crate) spawner: Spawner,
    pub(crate) timings: Timings,
    pub(crate) status: Arc<Mutex<OfficeStatus>>,
    pub(crate) signals: Receiver<Signal>,
    pub(crate) sender: Sender<Signal>,
    pub(crate) log: Arc<Mutex<RotatingLog>>,
}

/// How one run of the server ended.
#[derive(Debug, PartialEq, Eq)]
enum Ending {
    /// The host is shutting down; the server was stopped.
    Stopped,
    /// A failure that a restart can't fix.
    Fatal,
    /// Anything else: restart with backoff.
    Crashed { ran: Duration, was_ready: bool },
}

impl OfficeRunner {
    /// Runs until the host shuts down.
    pub(crate) fn run(self, adopted: Option<Adopted>) {
        let mut backoff = Backoff::new(&self.timings);
        let mut generation = adopted.as_ref().map_or(0, |a| a.child.generation + 1);
        let mut next = adopted;
        loop {
            let ending = match next.take() {
                Some(adopted) => self.watch(adopted),
                None => {
                    self.set(OfficeStatus::Starting);
                    let mode = Mode::Office(&self.office_id, &self.start);
                    let spawned =
                        self.spawner
                            .spawn(mode, generation, self.sender.clone(), self.log.clone());
                    generation += 1;
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
                            Ending::Crashed {
                                ran: Duration::ZERO,
                                was_ready: false,
                            }
                        }
                    }
                }
            };
            match ending {
                Ending::Stopped => return,
                Ending::Fatal => {
                    self.set(OfficeStatus::Unavailable);
                    self.wait_for_stop();
                    return;
                }
                Ending::Crashed { ran, was_ready } => {
                    self.set(OfficeStatus::Offline);
                    let delay = backoff.after_crash(ran, was_ready);
                    if self.sleep_unless_stopped(delay) {
                        return;
                    }
                }
            }
        }
    }

    /// Watches one server until it exits, misses its ready deadline, or the host stops it.
    fn watch(&self, adopted: Adopted) -> Ending {
        let Adopted {
            mut child,
            backlog,
            mut ready,
            ready_from,
        } = adopted;
        if ready {
            self.set(OfficeStatus::Active);
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
                    if !ready {
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
                                return after_exit(failure, child.started.elapsed(), ready);
                            }
                            if !ready && Instant::now() >= ready_deadline {
                                child.kill();
                                // A fatal `failed` already said a restart can't help.
                                if failure.is_some_and(is_fatal) {
                                    return Ending::Fatal;
                                }
                                host_log::log(&format!(
                                    "the server of office {} wasn't ready in time; restarting it",
                                    self.office_id
                                ));
                                return Ending::Crashed {
                                    ran: child.started.elapsed(),
                                    was_ready: false,
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
                        ready = true;
                        self.set(OfficeStatus::Active);
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
                    return after_exit(failure, child.started.elapsed(), ready);
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

    /// Waits out a backoff delay. Returns true if the host stopped meanwhile.
    fn sleep_unless_stopped(&self, delay: Duration) -> bool {
        let until = Instant::now() + delay;
        loop {
            match self
                .signals
                .recv_timeout(until.saturating_duration_since(Instant::now()))
            {
                Ok(Signal::Stop { .. }) | Err(RecvTimeoutError::Disconnected) => return true,
                Ok(_) => {} // a late line from the server that just exited
                Err(RecvTimeoutError::Timeout) => return false,
            }
        }
    }

    fn wait_for_stop(&self) {
        while let Ok(signal) = self.signals.recv() {
            if matches!(signal, Signal::Stop { .. }) {
                return;
            }
        }
    }

    fn set(&self, status: OfficeStatus) {
        *lock(&self.status) = status;
    }
}

/// How a server that exited (after `ran`) ended: fatal if it reported a fatal failure, else a
/// crash to restart.
fn after_exit(failure: Option<FailCode>, ran: Duration, was_ready: bool) -> Ending {
    match failure {
        Some(code) if is_fatal(code) => Ending::Fatal,
        _ => Ending::Crashed { ran, was_ready },
    }
}

/// Failures a restart can't fix: the office stays `unavailable` until the host restarts.
/// Unknown codes (decoded as `storage_error`) and `in_use` are retried.
pub(crate) fn is_fatal(code: FailCode) -> bool {
    matches!(
        code,
        FailCode::InvalidRequest
            | FailCode::UnsupportedVersion
            | FailCode::InvalidName
            | FailCode::AlreadyExists
            | FailCode::KeyMissing
    )
}

/// Restart delays: `initial`, doubling up to `max`, back to `initial` once a server stayed up
/// (ready) for `reset_after`.
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

    /// The delay before restarting a server that ran for `ran`.
    pub(crate) fn after_crash(&mut self, ran: Duration, was_ready: bool) -> Duration {
        if was_ready && ran >= self.reset_after {
            self.next = self.initial;
        }
        let delay = self.next.min(self.max);
        self.next = (self.next * 2).min(self.max);
        delay
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_to_the_cap_and_resets_after_uptime() {
        let mut b = Backoff::new(&Timings::default());
        let s = Duration::from_secs;
        let delays: Vec<_> = (0..8).map(|_| b.after_crash(s(1), true)).collect();
        assert_eq!(delays, [s(1), s(2), s(4), s(8), s(16), s(32), s(60), s(60)]);
        // A long run that never got ready doesn't reset; a ready one does.
        assert_eq!(b.after_crash(s(120), false), s(60));
        assert_eq!(b.after_crash(s(60), true), s(1));
        assert_eq!(b.after_crash(s(1), true), s(2));
    }

    #[test]
    fn fatal_codes_are_the_agreed_ones() {
        for code in [
            FailCode::InvalidRequest,
            FailCode::UnsupportedVersion,
            FailCode::InvalidName,
            FailCode::AlreadyExists,
            FailCode::KeyMissing,
        ] {
            assert!(is_fatal(code), "{code:?}");
        }
        assert!(!is_fatal(FailCode::InUse));
        assert!(!is_fatal(FailCode::StorageError));
    }
}
