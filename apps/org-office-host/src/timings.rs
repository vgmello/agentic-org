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
    /// How long servers get to exit after their stdin closes before they're SIGKILLed.
    pub shutdown_grace: Duration,
    /// How long `--discard` of an unfinished office may take (it waits for the office lock).
    pub discard_timeout: Duration,
    /// How long `organization.create` (and `office.restart`) waits to begin: first for the
    /// startup discards, then answers `not_ready` ("host starting"); then, for a create, for a
    /// free create slot, then answers `unavailable` ("too many creates in progress"). One budget
    /// for both waits.
    pub create_wait: Duration,
    /// How long a started server has, from its first line, to connect to its key socket and take
    /// its secrets (`handoff`) before it's killed (`storage_error`). Agreed: 10 s.
    pub hand_off_timeout: Duration,
    /// How often a server's exit is checked directly, besides its control pipe closing (which a
    /// descendant holding its stdout would delay).
    pub exit_poll: Duration,
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
            shutdown_grace: Duration::from_secs(10),
            discard_timeout: Duration::from_secs(30),
            create_wait: Duration::from_secs(30),
            hand_off_timeout: Duration::from_secs(10),
            exit_poll: Duration::from_secs(1),
        }
    }
}
