//! How the live feeds (`status_feed`, `org_feed`) pace themselves: a quiet connection looks at its stop flag often,
//! and a connection that cannot be made is retried less and less often.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const FIRST_PAUSE: Duration = Duration::from_secs(1);
pub const LONGEST_PAUSE: Duration = Duration::from_secs(30);
/// How often a quiet connection looks at `stop`.
pub const LOOK: Duration = Duration::from_millis(250);

/// A connection that stayed up this long counts as a good one: the pause starts over after it. One that the
/// office closes at once (a log it cannot read) does not, or the feed would knock every second, forever.
pub const STABLE: Duration = Duration::from_secs(10);

/// How long a feed waits between connections: short after a connection that held, longer and longer while
/// connections fail or are closed at once.
#[derive(Debug)]
pub struct Pacer {
    pause: Duration,
}

impl Pacer {
    pub fn new() -> Self {
        Self { pause: FIRST_PAUSE }
    }

    /// The pause before the next connection. `up_for` is how long the last one stayed up once it was working
    /// (`None`: it never got going: no host, not ready, another version).
    pub fn next(&mut self, up_for: Option<Duration>) -> Duration {
        if up_for.is_some_and(|d| d >= STABLE) {
            self.pause = FIRST_PAUSE;
        }
        let now = self.pause;
        self.pause = (self.pause * 2).min(LONGEST_PAUSE);
        now
    }
}

pub fn wait(pause: Duration, stop: &AtomicBool) {
    let until = Instant::now() + pause;
    while Instant::now() < until && !stop.load(Ordering::SeqCst) {
        std::thread::sleep(LOOK.min(until.saturating_duration_since(Instant::now())));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELD: Option<Duration> = Some(STABLE);
    const SECONDS: fn(&mut Pacer, Option<Duration>) -> u64 = |p, up| p.next(up).as_secs();

    #[test]
    fn the_pause_grows_while_connections_fail() {
        let mut pacer = Pacer::new();
        let seen: Vec<_> = (0..7).map(|_| SECONDS(&mut pacer, None)).collect();
        assert_eq!(seen, [1, 2, 4, 8, 16, 30, 30]);
    }

    #[test]
    fn a_connection_that_held_starts_the_pause_over_before_the_wait_not_a_cycle_later() {
        let mut pacer = Pacer::new();
        for _ in 0..4 {
            pacer.next(None);
        }
        assert_eq!(
            SECONDS(&mut pacer, HELD),
            1,
            "the next reconnect is soon, not after the grown pause"
        );
        assert_eq!(SECONDS(&mut pacer, None), 2);
    }

    #[test]
    fn a_connection_closed_at_once_does_not_start_it_over() {
        let mut pacer = Pacer::new();
        let seen: Vec<_> = (0..4)
            .map(|_| SECONDS(&mut pacer, Some(Duration::from_millis(40))))
            .collect();
        assert_eq!(seen, [1, 2, 4, 8]);
    }
}
