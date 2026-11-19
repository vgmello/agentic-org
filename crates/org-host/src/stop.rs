//! The stop of a service process, taken as soon as it is asked for, and the bound on it.
//!
//! [`Stop::watch`] starts a thread that waits for the stop (the composition passes
//! [`ShutdownSignals::wait`](crate::signals::ShutdownSignals::wait), from the moment the
//! signals are blocked), so a stop that comes while the instance is set up or the host
//! starts is taken at once rather than left pending until the start ends (X5 review, third
//! round, P1). Once taken, the stop is bounded whatever the process is doing: while the
//! start has not settled ([`Stop::settle`]) it must settle within `bound` of the stop, and
//! after that the whole stop must end ([`Stop::finish`]) within `limit` of it. Past either,
//! the thread calls the composition's `overrun`, which ends the process.

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::sync::lock;

/// A stop that a thread of its own waits for. See the module docs.
pub struct Stop {
    shared: Arc<(Mutex<Stopping>, Condvar)>,
}

#[derive(Default)]
struct Stopping {
    /// When the stop was asked for, or why waiting for it failed.
    asked: Option<Result<Instant, String>>,
    /// The start has ended: from here on the stop is bounded by `limit`.
    settled: bool,
    /// The stop has ended: nothing bounds it any more.
    finished: bool,
}

impl Stop {
    /// Starts the thread that waits for the stop with `wait`, which returns the signal's
    /// number. Once the stop is taken, the thread calls `overrun` with that number and
    /// whether the start was still running (`true`) when the start has not settled `bound`
    /// after the stop, or when the stop has not finished `limit` after it. It holds the
    /// stop's lock while it does, so the process does not settle or finish meanwhile.
    pub fn watch(
        wait: impl FnOnce() -> std::io::Result<i32> + Send + 'static,
        bound: Duration,
        limit: Duration,
        overrun: impl FnOnce(i32, bool) + Send + 'static,
    ) -> Result<Self, String> {
        let shared = Arc::new((Mutex::new(Stopping::default()), Condvar::new()));
        let watching = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("stop".into())
            .spawn(move || {
                let (state, changed) = &*watching;
                let taken = wait();
                let asked = Instant::now();
                let mut stopping = lock(state);
                stopping.asked = Some(match &taken {
                    Ok(_) => Ok(asked),
                    Err(e) => Err(format!("waiting for a signal: {e}")),
                });
                changed.notify_all();
                let Ok(signal) = taken else {
                    return;
                };
                while !stopping.finished {
                    let starting = !stopping.settled;
                    let until = asked + if starting { bound } else { limit };
                    let now = Instant::now();
                    if now >= until {
                        overrun(signal, starting);
                        return;
                    }
                    stopping = changed
                        .wait_timeout(stopping, until - now)
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .0;
                }
            })
            .map_err(|e| format!("starting the thread that waits for a stop: {e}"))?;
        Ok(Self { shared })
    }

    /// Whether a stop has been asked for, or why waiting for one failed.
    pub fn asked(&self) -> Result<bool, String> {
        match &lock(&self.shared.0).asked {
            None => Ok(false),
            Some(asked) => asked.as_ref().map(|_| true).map_err(Clone::clone),
        }
    }

    /// The start has ended (the host serves, or the process ends without serving).
    pub fn settle(&self) {
        lock(&self.shared.0).settled = true;
        self.shared.1.notify_all();
    }

    /// The stop has ended: nothing is bounded any more.
    pub fn finish(&self) {
        lock(&self.shared.0).finished = true;
        self.shared.1.notify_all();
    }

    /// Waits for the stop, and says when it was asked for.
    pub fn wait(&self) -> Result<Instant, String> {
        let (state, changed) = &*self.shared;
        let mut stopping = lock(state);
        loop {
            if let Some(asked) = &stopping.asked {
                return asked.clone();
            }
            stopping = changed
                .wait(stopping)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    type Overruns = Arc<Mutex<Vec<(i32, bool, Instant)>>>;

    /// A stop taken from a signal the test sends, and the overruns it recorded.
    fn watched(bound: Duration, limit: Duration) -> (Stop, mpsc::Sender<i32>, Overruns) {
        let (send, signal) = mpsc::channel::<i32>();
        let overruns = Overruns::default();
        let recording = Arc::clone(&overruns);
        let stop = Stop::watch(
            move || signal.recv().map_err(std::io::Error::other),
            bound,
            limit,
            move |signal, starting| {
                lock(&recording).push((signal, starting, Instant::now()));
            },
        )
        .unwrap();
        (stop, send, overruns)
    }

    fn taken(stop: &Stop, send: &mpsc::Sender<i32>) -> Instant {
        let before = Instant::now();
        send.send(libc::SIGTERM).unwrap();
        stop.wait().unwrap();
        before
    }

    /// A start that has not settled `bound` after the stop overruns then, as a start.
    #[test]
    fn a_start_that_does_not_settle_overruns_at_the_bound() {
        let (stop, send, overruns) = watched(Duration::from_millis(200), Duration::from_secs(10));
        let asked = taken(&stop, &send);
        std::thread::sleep(Duration::from_millis(600));
        let overruns = lock(&overruns).clone();
        let [(signal, starting, at)] = overruns[..] else {
            panic!("one overrun, not {overruns:?}");
        };
        assert_eq!((signal, starting), (libc::SIGTERM, true));
        let after = at.duration_since(asked);
        assert!(after >= Duration::from_millis(200), "{after:?}");
        assert!(after < Duration::from_millis(500), "{after:?}");
    }

    /// X5 review, third round (review of eadd7835, P2): a start that settled in time does
    /// not end the bound. A stop whose last steps wait past `limit` (a slow volume, say)
    /// overruns then, as a stop; one that finishes in time never does.
    #[test]
    fn a_settled_stop_that_does_not_finish_overruns_at_the_limit() {
        let (stop, send, overruns) =
            watched(Duration::from_millis(100), Duration::from_millis(400));
        let asked = taken(&stop, &send);
        stop.settle();
        std::thread::sleep(Duration::from_millis(800));
        let overruns = lock(&overruns).clone();
        let [(signal, starting, at)] = overruns[..] else {
            panic!("one overrun, not {overruns:?}");
        };
        assert_eq!((signal, starting), (libc::SIGTERM, false));
        let after = at.duration_since(asked);
        assert!(after >= Duration::from_millis(400), "{after:?}");
        assert!(after < Duration::from_millis(700), "{after:?}");

        let (stop, send, overruns) =
            watched(Duration::from_millis(100), Duration::from_millis(400));
        taken(&stop, &send);
        stop.settle();
        stop.finish();
        std::thread::sleep(Duration::from_millis(600));
        assert!(lock(&overruns).is_empty());
    }

    /// Settled before the stop: the stop is bounded by `limit`, not `bound`.
    #[test]
    fn a_stop_after_the_start_is_bounded_by_the_limit_alone() {
        let (stop, send, overruns) = watched(Duration::from_millis(50), Duration::from_millis(400));
        stop.settle();
        taken(&stop, &send);
        std::thread::sleep(Duration::from_millis(200));
        assert!(lock(&overruns).is_empty(), "overran at the start's bound");
        stop.finish();
        std::thread::sleep(Duration::from_millis(400));
        assert!(lock(&overruns).is_empty());
    }
}
