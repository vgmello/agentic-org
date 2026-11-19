//! Every wait the host makes, in one place.

use std::time::Duration;

/// Every wait the host makes. The defaults are the agreed ones; tests shorten them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timings {
    /// How long a started server has to report `ready` before it's killed and restarted.
    pub ready_timeout: Duration,
    /// How long `organization.create` waits for `created` or `failed`.
    pub create_timeout: Duration,
    /// The first restart delay; it doubles after each crash.
    pub backoff_initial: Duration,
    /// The longest restart delay.
    pub backoff_max: Duration,
    /// A server that stayed up this long after `ready` resets the delay to `backoff_initial`,
    /// and counts as a fresh start for the give-up rule, not a failed restart.
    pub backoff_reset_after: Duration,
    /// The give-up rule: this many failed restarts within `give_up_window` and the office is
    /// `unavailable` (`restarts_exhausted`) until `office.restart` or the next host run.
    pub give_up_after: u32,
    pub give_up_window: Duration,
    /// How long servers get to exit after their stdin closes before they're SIGKILLed. It
    /// bounds the host's own stop: every server is asked at once and waited for until one
    /// shared deadline, fixed when the host is asked to stop, and every other wait of the
    /// stop ends 0.5 s after it at the latest, so the host exits within this grace plus
    /// 0.5 s and the moments its last steps take. The design's bound is 2.5 s from
    /// `SIGTERM` to exit 0 (Linux host design, section 3.4), inside the shortest default
    /// grace of a container runtime: Docker Desktop's 3 s (`default-stop-timeout`; Docker Engine's is 10 s). A worker
    /// asked to stop exits well within it; one killed at the deadline loses nothing, since
    /// the office store recovers from a kill at any point.
    pub shutdown_grace: Duration,
    /// How long `--discard` of an unfinished office may take (it waits for the office lock).
    pub discard_timeout: Duration,
    /// How long `organization.create` (and `office.restart`) waits to begin: first for the
    /// startup discards, then answers `not_ready` ("host starting"); then, for a create, for a
    /// free create slot, then answers `unavailable` ("too many creates in progress"). One budget
    /// for both waits.
    pub create_wait: Duration,
    /// How often a server's exit is checked directly, besides its control pipe closing (which a
    /// descendant holding its stdout would delay).
    pub exit_poll: Duration,
}

impl Timings {
    /// How long after a stop is asked for every wait of it has ended: the servers' grace,
    /// then the margin the host gives its own threads after it. A host's last steps (removing
    /// `host.sock`, releasing `host.lock`) follow at once.
    pub fn stop_ends_within(&self) -> Duration {
        self.shutdown_grace + crate::THREAD_END_MARGIN
    }
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            ready_timeout: Duration::from_secs(30),
            create_timeout: Duration::from_secs(30),
            backoff_initial: Duration::from_secs(1),
            backoff_max: Duration::from_secs(60),
            backoff_reset_after: Duration::from_secs(60),
            give_up_after: 10,
            give_up_window: Duration::from_secs(30 * 60),
            shutdown_grace: Duration::from_millis(1500),
            discard_timeout: Duration::from_secs(30),
            create_wait: Duration::from_secs(30),
            exit_poll: Duration::from_secs(1),
        }
    }
}
