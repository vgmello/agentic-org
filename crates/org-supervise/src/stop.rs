//! Owned process stopping. The caller keeps draining its readers on another thread.
use std::fmt;
use std::io;
use std::process::ExitStatus;
use std::time::{Duration, Instant};

use crate::{invalid, ChildStdin, OwnedChild};

const TERM_GRACE: Duration = Duration::from_secs(5);
const KILL_EXIT_GRACE: Duration = Duration::from_secs(5);

/// A catchable stop signal. KILL is always the supervisor's final action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopSignal {
    /// SIGINT.
    Interrupt,
    /// SIGTERM.
    Terminate,
}

impl StopSignal {
    pub(crate) fn number(self) -> libc::c_int {
        match self {
            Self::Interrupt => libc::SIGINT,
            Self::Terminate => libc::SIGTERM,
        }
    }
}

/// An owned leader or its process group, whose ID remains reserved until reaping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalTarget {
    /// Only the runtime leader, allowing it to stop detached tools itself.
    Leader,
    /// Its process group.
    Group,
}

/// A graceful stop instruction and the grace that follows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopStep {
    /// Close stdin idempotently, then wait for exit.
    CloseStdin {
        /// Maximum wait, measured from this step's start.
        grace: Duration,
    },
    /// Send a signal, then wait for exit.
    Signal {
        /// Catchable signal.
        signal: StopSignal,
        /// Owned leader or group.
        target: SignalTarget,
        /// Maximum wait, measured from this step's start.
        grace: Duration,
    },
}

/// Validated graceful steps. Stdin closes must precede all signal steps.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StopPolicy(Vec<StopStep>);

impl StopPolicy {
    /// Validate the sequence without performing process or pipe operations.
    pub fn new(steps: Vec<StopStep>) -> io::Result<Self> {
        let mut signalled = false;
        for step in &steps {
            let grace = match step {
                StopStep::CloseStdin { grace } if !signalled => *grace,
                StopStep::CloseStdin { .. } => {
                    return Err(invalid("stdin close must precede signals"));
                }
                StopStep::Signal { grace, .. } => {
                    signalled = true;
                    *grace
                }
            };
            deadline(grace)?;
        }
        Ok(Self(steps))
    }

    fn sequence(&self, mode: StopMode) -> Vec<StopStep> {
        let term = |grace| StopStep::Signal {
            signal: StopSignal::Terminate,
            target: SignalTarget::Leader,
            grace,
        };
        if mode == StopMode::Fast {
            return vec![term(TERM_GRACE)];
        }
        let mut result = Vec::new();
        let mut first_signal = true;
        for step in &self.0 {
            if let StopStep::Signal { grace, .. } = step {
                if first_signal {
                    result.push(term(*grace));
                    first_signal = false;
                    if *step == term(*grace) {
                        continue;
                    }
                }
            }
            result.push(*step);
        }
        if first_signal {
            result.push(term(TERM_GRACE));
        }
        result
    }
}

/// Whether the pump finished the protocol-close exchange or requests immediate stopping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopMode {
    /// Run after the pump's CloseStdin response or its protocol-close deadline.
    Graceful,
    /// Skip protocol and graceful steps; TERM leader, five seconds, group KILL.
    Fast,
}

/// A completed process-control action. A signal attempt does not prove delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopAction {
    /// The shared stdin was closed.
    StdinClosed,
    /// A catchable signal was attempted while child ownership was verified.
    SignalAttempted {
        /// Requested signal.
        signal: StopSignal,
        /// Its target.
        target: SignalTarget,
    },
    /// Residual processes in the owned group were sent SIGKILL (ESRCH is harmless).
    GroupKillAttempted,
}

/// Exit observation and process-control evidence. The leader is still unreaped.
#[derive(Debug)]
pub struct StopReport {
    /// Observed terminal status.
    pub status: ExitStatus,
    /// Actions in execution order.
    pub actions: Vec<StopAction>,
}

/// An incomplete stop, retaining the actions already attempted and the process owner.
#[derive(Debug)]
pub struct StopFailure {
    /// Underlying process, clock or pipe error.
    pub error: io::Error,
    /// Actions completed before the error.
    pub actions: Vec<StopAction>,
    /// False only when an external reaper violated ownership; no further signals are sent.
    pub owner_retained: bool,
}

impl fmt::Display for StopFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "process stop unresolved: {}", self.error)
    }
}

impl std::error::Error for StopFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

fn deadline(grace: Duration) -> io::Result<Instant> {
    Instant::now()
        .checked_add(grace)
        .ok_or_else(|| invalid("stop grace exceeds the monotonic clock range"))
}

impl OwnedChild {
    /// Stop synchronously without reading any pipe or reaping. The pump must drain its
    /// stdout/stderr concurrently. Call only after its protocol-close response/deadline.
    /// Every step fixes its monotonic deadline before taking its action. The first signal
    /// is TERM to the leader, followed by the adapter's remaining steps and group KILL.
    /// Even a natural exit gets residual group KILL. Detached tools need slice-3 discovery.
    ///
    /// Errors retain the owner for recovery; ECHILD disarms all subsequent signals. A
    /// leader still unobservable five seconds after KILL returns TimedOut. Cleanup and
    /// reaping remain explicit: callers must not release their lock or admit replacement.
    pub fn stop(
        &mut self,
        stdin: &ChildStdin,
        policy: &StopPolicy,
        mode: StopMode,
    ) -> Result<StopReport, StopFailure> {
        let mut actions = Vec::new();
        let result = (|| {
            let sequence = policy.sequence(mode);
            for step in &sequence {
                deadline(match step {
                    StopStep::CloseStdin { grace } | StopStep::Signal { grace, .. } => *grace,
                })?;
            }
            let mut status = self.try_exit()?;
            for step in sequence {
                if status.is_some() {
                    break;
                }
                let (action, until) = match step {
                    StopStep::CloseStdin { grace } => {
                        let until = deadline(grace)?;
                        stdin.close()?;
                        (StopAction::StdinClosed, until)
                    }
                    StopStep::Signal {
                        signal,
                        target,
                        grace,
                    } => {
                        let until = deadline(grace)?;
                        self.signal_owned(signal.number(), target)?;
                        (StopAction::SignalAttempted { signal, target }, until)
                    }
                };
                actions.push(action);
                status = self.wait_until(until)?;
            }
            self.kill_owned_group()?;
            actions.push(StopAction::GroupKillAttempted);
            if status.is_none() {
                status = self.wait_until(deadline(KILL_EXIT_GRACE)?)?;
            }
            let status = status.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "owned leader has not exited after KILL",
                )
            })?;
            Ok(status)
        })();
        match result {
            Ok(status) => Ok(StopReport { status, actions }),
            Err(error) => Err(StopFailure {
                error,
                actions,
                owner_retained: self.is_owned(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn term_precedes_the_first_signal_and_preserves_both_graces() {
        let close = StopStep::CloseStdin {
            grace: Duration::from_secs(30),
        };
        let interrupt = StopStep::Signal {
            signal: StopSignal::Interrupt,
            target: SignalTarget::Group,
            grace: Duration::from_millis(17),
        };
        let policy = StopPolicy::new(vec![close, interrupt]).unwrap();
        assert_eq!(
            policy.sequence(StopMode::Graceful),
            vec![
                close,
                StopStep::Signal {
                    signal: StopSignal::Terminate,
                    target: SignalTarget::Leader,
                    grace: Duration::from_millis(17)
                },
                interrupt
            ]
        );
    }

    #[test]
    fn explicit_leader_term_is_not_sent_twice_and_fast_ignores_the_plan() {
        let term = StopStep::Signal {
            signal: StopSignal::Terminate,
            target: SignalTarget::Leader,
            grace: Duration::from_secs(2),
        };
        let policy = StopPolicy::new(vec![term]).unwrap();
        assert_eq!(policy.sequence(StopMode::Graceful), vec![term]);
        let default = StopStep::Signal {
            signal: StopSignal::Terminate,
            target: SignalTarget::Leader,
            grace: TERM_GRACE,
        };
        assert_eq!(policy.sequence(StopMode::Fast), vec![default]);
        assert_eq!(
            StopPolicy::default().sequence(StopMode::Graceful),
            vec![default]
        );
    }

    #[test]
    fn invalid_order_and_unrepresentable_grace_fail_before_actions() {
        assert!(StopPolicy::new(vec![
            StopStep::Signal {
                signal: StopSignal::Interrupt,
                target: SignalTarget::Group,
                grace: Duration::ZERO
            },
            StopStep::CloseStdin {
                grace: Duration::ZERO
            }
        ])
        .is_err());
        assert!(StopPolicy::new(vec![StopStep::CloseStdin {
            grace: Duration::MAX
        }])
        .is_err());
    }
}
