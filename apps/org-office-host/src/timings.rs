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
    /// A server that stayed up (ready) this long resets the delay to `backoff_initial`.
    pub backoff_reset_after: Duration,
    /// How long servers get to exit after their stdin closes before they're SIGKILLed.
    pub shutdown_grace: Duration,
    /// How long `--discard` of an unfinished office may take (it waits for the office lock).
    pub discard_timeout: Duration,
    /// How long `organization.create` waits for the startup discards before answering
    /// `unavailable` ("host starting").
    pub startup_wait: Duration,
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
            shutdown_grace: Duration::from_secs(10),
            discard_timeout: Duration::from_secs(30),
            startup_wait: Duration::from_secs(30),
            exit_poll: Duration::from_secs(1),
        }
    }
}
