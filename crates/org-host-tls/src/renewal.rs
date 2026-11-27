//! Background renewal thread for the instance's own certificate while the host runs.

use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How often a running host looks at its own certificate.
pub const RENEW_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The thread that renews the pair; it ends when the host stops.
pub struct Renewal {
    stop: Arc<(Mutex<bool>, Condvar)>,
    done: Arc<(Mutex<bool>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

struct DoneGuard(Arc<(Mutex<bool>, Condvar)>);

impl Drop for DoneGuard {
    fn drop(&mut self) {
        *lock(&self.0 .0) = true;
        self.0 .1.notify_all();
    }
}

impl Renewal {
    /// Looks every `every` (never at once) and invokes `renew_if_expiring(now)`; when it returns
    /// `Ok(true)`, calls `on_renewed`. Failures and missing clocks are reported via `on_log`.
    pub fn start(
        every: Duration,
        renew_if_expiring: Box<dyn Fn(i64) -> Result<bool, String> + Send>,
        on_renewed: Box<dyn Fn() + Send>,
        on_log: Box<dyn Fn(&str) + Send>,
    ) -> std::io::Result<Self> {
        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let done = Arc::new((Mutex::new(false), Condvar::new()));
        let stopping = Arc::clone(&stop);
        let finishing = Arc::clone(&done);
        let thread = std::thread::Builder::new()
            .name("org-tls-renewal".into())
            .spawn(move || {
                let _done = DoneGuard(finishing);
                run_renewal_loop(
                    &stopping,
                    every,
                    &*renew_if_expiring,
                    &*on_renewed,
                    &*on_log,
                );
            })?;
        Ok(Self {
            stop,
            done,
            thread: Some(thread),
        })
    }

    /// Asks the thread to stop and waits for it until `until`; says whether it ended.
    pub fn join_by(mut self, until: Instant) -> bool {
        self.ask();
        let Some(thread) = self.thread.take() else {
            return true;
        };
        let (done_lock, done_cv) = &*self.done;
        let mut finished = lock(done_lock);
        while !*finished {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            finished = done_cv
                .wait_timeout(finished, left)
                .map(|(guard, _)| guard)
                .unwrap_or_else(|poisoned| poisoned.into_inner().0);
        }
        drop(finished);
        let _ = thread.join();
        true
    }

    fn ask(&self) {
        *lock(&self.stop.0) = true;
        self.stop.1.notify_all();
    }
}

fn run_renewal_loop(
    stopping: &(Mutex<bool>, Condvar),
    every: Duration,
    renew_if_expiring: &dyn Fn(i64) -> Result<bool, String>,
    on_renewed: &dyn Fn(),
    on_log: &dyn Fn(&str),
) {
    let (flag, wake) = stopping;
    let mut stopped = lock(flag);
    loop {
        let until = Instant::now() + every;
        while !*stopped {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            stopped = wake
                .wait_timeout(stopped, left)
                .map(|(guard, _)| guard)
                .unwrap_or_else(|poisoned| poisoned.into_inner().0);
        }
        if *stopped {
            return;
        }
        drop(stopped);
        match super::now().map(renew_if_expiring) {
            Some(Ok(true)) => on_renewed(),
            Some(Ok(false)) => {}
            Some(Err(why)) => on_log(&format!(
                "the TLS certificate could not be renewed: {why}; tried again later"
            )),
            None => on_log("the TLS certificate was not checked: the clock is unavailable"),
        }
        stopped = lock(flag);
    }
}

impl Drop for Renewal {
    fn drop(&mut self) {
        self.ask();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
