//! The renewal of the instance's own certificate while the host runs: a start renews one with
//! fewer than 30 days left, and so does this thread, for a host that runs longer than that.
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::WebTls;
use crate::host_log::log;
use crate::sync::lock;

/// How often a running host looks at its own certificate.
pub(crate) const EVERY: Duration = Duration::from_secs(6 * 60 * 60);

/// The thread that renews the pair; it ends when the host stops.
pub(crate) struct Renewal {
    stop: Arc<(Mutex<bool>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl Renewal {
    /// Looks every `every` (never at once) and renews the instance's own certificate when it
    /// is expiring; `renewed` is then called, to send `tls.renew` to the web process. A
    /// failed renewal is logged and tried again at the next look.
    pub(crate) fn start(
        tls: Arc<WebTls>,
        every: Duration,
        renewed: Box<dyn Fn() + Send>,
    ) -> std::io::Result<Self> {
        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let stopping = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("org-tls-renewal".into())
            .spawn(move || {
                let (flag, wake) = &*stopping;
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
                    match super::now().map(|now| tls.renew_if_expiring(now)) {
                        Some(Ok(true)) => renewed(),
                        Some(Ok(false)) => {}
                        Some(Err(why)) => log(&format!(
                            "the TLS certificate could not be renewed: {why}; tried again later"
                        )),
                        None => {
                            log("the TLS certificate was not checked: the clock is unavailable")
                        }
                    }
                    stopped = lock(flag);
                }
            })?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }

    /// Asks the thread to stop and waits for it until `until`; says whether it ended.
    pub(crate) fn join_by(mut self, until: Instant) -> bool {
        self.ask();
        self.thread
            .take()
            .is_none_or(|thread| crate::sync::join_by(thread, until))
    }

    fn ask(&self) {
        *lock(&self.stop.0) = true;
        self.stop.1.notify_all();
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
