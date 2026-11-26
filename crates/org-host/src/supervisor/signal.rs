//! What an office's supervision thread hears ([`Signal`]).

use std::time::Instant;

use org_office_launch::ServerEvent;

/// What an office's supervision thread hears: its child's control lines, the end of its
/// stdout, or a stop request. Child messages carry the child's generation, so a late line from
/// a child that was already replaced is ignored.
#[derive(Debug)]
pub(crate) enum Signal {
    Event {
        generation: u64,
        event: ServerEvent,
    },
    /// The child closed its stdout (it exited) or wrote something unreadable.
    Closed {
        generation: u64,
    },
    /// The host is shutting down: stop the child by this deadline.
    Stop {
        deadline: Instant,
    },
    /// `office.restart`: start the office again now, if it isn't running.
    Restart,
    /// Gracefully stop and immediately restart an active office (e.g. after ownership handover)
    /// without tripping crash backoff.
    Reload,
    /// Tests only: SIGKILL the live office worker so supervision observes a real crash.
    #[cfg(test)]
    KillForTest,
}
