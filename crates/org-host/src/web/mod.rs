//! Host-owned browser process. Its private control authority is constructed only
//! after suspended-image, actual-reader, sandbox and descriptor-receipt checks.
use crate::{service::listener::Handler, sync::lock};
use org_host_control::{listen::ListenMode, WebState};
use std::{
    io,
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Instant,
};
mod control;
mod image;
mod listener;
mod running;
mod stderr;
mod supervise;
use control::os_failed;
pub(crate) use image::{admission, Configuration, Image};
use image::{gated, https_gate, unconfinable};
pub use listener::BrowserListener;
use running::{Running, WAIT};
#[cfg(test)]
pub(crate) use stderr::captured_stderr;
use supervise::{supervise, Timing};

pub(crate) struct Supervisor {
    stop: Arc<AtomicBool>,
    state: Arc<Mutex<WebState>>,
    thread: Option<JoinHandle<()>>,
}
impl Supervisor {
    /// Supervises the web process of `config`. Its first start serves on `first` when it is
    /// given (the listener setup bound, [`BrowserListener`]); every other start binds the
    /// configured address itself.
    pub(crate) fn start(
        config: Configuration,
        first: Option<TcpListener>,
        handler: Arc<dyn Handler>,
        state: Arc<Mutex<WebState>>,
        eligible: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> io::Result<Self> {
        // No bind, child or secret handoff for unverified TLS or a system on which the
        // web process cannot be confined.
        // Each refusal names its cause for the host log.
        org_platform::confinement::web_confinement_supported().map_err(unconfinable)?;
        // Plain HTTP, in one of its two modes: on loopback, or on the published port of a
        // foreground instance, which the registered start has already judged against the
        // instance's service mode. HTTPS needs the host's pair; it is started only while
        // the stored pair verifies (`gated`).
        let eligible = match (config.listen.scheme(), &config.tls) {
            (ListenMode::Https, None) => return Err(https_gate(None, 0).unwrap_err()),
            (ListenMode::Https, Some(tls)) => gated(eligible, Arc::clone(tls)),
            _ => eligible,
        };
        config
            .check
            .may_spawn()
            .map_err(|_| io::Error::other(config.check.describe()))?;
        // One admission for every start of this host's web process, built beside the
        // host's start.
        let image = Image::new(&config.check, &config.program);
        image.begin();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let observing = Arc::clone(&state);
        let mut first = first;
        let thread = thread::Builder::new()
            .name("org-browser-supervisor".into())
            .spawn(move || {
                supervise(
                    &stopping,
                    &observing,
                    &*eligible,
                    Timing::PRODUCTION,
                    &|line| crate::host_log::log(line),
                    || {
                        let held = first.take();
                        Running::start(&config, &image, held, Arc::clone(&handler), WAIT, &stopping)
                    },
                )
            })
            .map_err(os_failed("start the supervisor thread"))?;
        Ok(Self {
            stop,
            state,
            thread: Some(thread),
        })
    }
}
impl Supervisor {
    /// Asks the supervision to stop and returns at once. A start under way ends at its next
    /// step (within [`POLL`](control::POLL) of a wait for the web process), and the web process is stopped
    /// as for any failed start; a running one ends its sessions and is killed and reaped.
    pub(crate) fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = &self.thread {
            thread.thread().unpark();
        }
    }

    /// Whether the supervision has ended: it was stopped, or it gave up after its limit of
    /// failed starts.
    #[cfg(test)]
    pub(crate) fn ended(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// [`Supervisor::stop`], then waits for the supervision to end until `until` at the
    /// latest, and says whether it did. It is left running only while a request of the web
    /// process's control connection still runs (one waiting for a write lock of the
    /// registry, say): the web process has been killed and reaped by then, and the request
    /// ends with the host's process, its answer dropped.
    pub(crate) fn join_by(mut self, until: Instant) -> bool {
        self.stop();
        self.thread
            .take()
            .is_none_or(|thread| crate::sync::join_by(thread, until))
    }
}
impl Drop for Supervisor {
    fn drop(&mut self) {
        self.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        *lock(&self.state) = WebState::Failed;
    }
}

#[cfg(test)]
mod report_tests;
#[cfg(test)]
mod tests;
