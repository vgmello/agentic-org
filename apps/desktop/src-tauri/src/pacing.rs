//! How the live feeds (`status_feed`, `org_feed`) pace themselves: a quiet connection looks at its stop flag often,
//! and a connection that cannot be made is retried less and less often.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const FIRST_PAUSE: Duration = Duration::from_secs(1);
pub const LONGEST_PAUSE: Duration = Duration::from_secs(30);
/// How often a quiet connection looks at `stop`.
pub const LOOK: Duration = Duration::from_millis(250);

/// How long to wait before the connection after this one. It was up and then went away: soon. It never got going
/// (no host, not ready, another version): ask less and less often.
pub fn next_pause(pause: Duration, delivered: bool) -> Duration {
    if delivered {
        FIRST_PAUSE
    } else {
        (pause * 2).min(LONGEST_PAUSE)
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

    #[test]
    fn the_pause_grows_while_nothing_connects_and_starts_over_after_a_good_connection() {
        let mut pause = FIRST_PAUSE;
        let mut seen = Vec::new();
        for _ in 0..7 {
            seen.push(pause.as_secs());
            pause = next_pause(pause, false);
        }
        assert_eq!(seen, [1, 2, 4, 8, 16, 30, 30]);
        assert_eq!(next_pause(pause, true), FIRST_PAUSE);
    }
}
