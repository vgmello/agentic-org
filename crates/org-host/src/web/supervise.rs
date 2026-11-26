//! Supervising the web process: its starts, bounded restarts with backoff and the state the
//! host publishes.

use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use org_host_control::WebState;

use crate::sync::lock;

use super::control::POLL;

/// Consecutive failed starts before the supervisor gives up.
pub(super) const MAX_STARTS: usize = 5;

/// How long a started web process may take to report `web.serving` (GW-01 r4).
pub(super) const SERVING_DEADLINE: Duration = Duration::from_secs(10);

/// A web process that stayed `Running` this long was a healthy start: its later
/// exit begins a fresh run of consecutive failures instead of adding to one.
const STABLE_RUNNING: Duration = Duration::from_secs(60);

/// First restart delay; each further consecutive failure doubles it.
const RESTART_BACKOFF: Duration = Duration::from_secs(1);

/// Supervisor timing. Production uses [`Timing::PRODUCTION`]; tests shorten it.
#[derive(Clone, Copy)]
pub(super) struct Timing {
    pub(super) serving: Duration,
    pub(super) stable: Duration,
    pub(super) backoff: Duration,
}

impl Timing {
    pub(super) const PRODUCTION: Self = Self {
        serving: SERVING_DEADLINE,
        stable: STABLE_RUNNING,
        backoff: RESTART_BACKOFF,
    };
}

/// A started web process the supervisor watches.
pub(super) trait Serving {
    /// Whether the control connection is open and the child has not exited.
    fn serving(&mut self) -> bool;
    /// Whether the admitted web process reported, in order, that its HTTP server serves.
    fn reported(&mut self) -> bool;
    /// Why it stopped serving: fixed, secret-free text for the host log.
    fn ended(&mut self) -> String;
    /// What it is confined by, for the one line the host logs of each start.
    fn confined(&mut self) -> String;
}

/// Starts the web process once `eligible`. It stays `Starting` until the admitted
/// web process reports `web.serving` within `deadline`, then publishes `Running`
/// only while that web process serves. A missing report, an exit or control EOF
/// publishes `Failed` and stops/reaps the child, before any bounded restart.
/// Only consecutive failed starts count toward [`MAX_STARTS`]: a web process that
/// stayed `Running` for `timing.stable` resets the count. No backoff follows
/// the final failed start. Every start is passed to `log` once with what the web
/// process is confined by. Every failed start and every exit is passed to `log`
/// with its cause, escaped, together with the restart it leads to. When `eligible`
/// ends while the web process runs, it is stopped with a logged reason and starts
/// again only once eligible, without counting a failure or waiting a backoff.
pub(super) fn supervise<S: Serving>(
    stopping: &AtomicBool,
    state: &Mutex<WebState>,
    eligible: &dyn Fn() -> bool,
    timing: Timing,
    log: &dyn Fn(&str),
    mut start: impl FnMut() -> io::Result<S>,
) {
    let mut failures = 0usize;
    loop {
        // First protected bootstrap starts the host before HQ exists. Wait without
        // spending restart attempts (and without binding, unless setup handed this host
        // its listener: that one stays held, and browsers wait in its backlog); the same
        // controller observes actual completed provisioning.
        while !stopping.load(Ordering::SeqCst) && !eligible() {
            *lock(state) = WebState::Starting;
            thread::park_timeout(POLL);
        }
        if stopping.load(Ordering::SeqCst) {
            break;
        }
        *lock(state) = WebState::Starting;
        let mut stable = false;
        let reason = match start() {
            Ok(mut child) => {
                // Once per start: the layers the host verified, and why one is
                // missing where the web process said so.
                log(&format!(
                    "authenticated browser web process started, {}",
                    crate::host_log::untrusted(&child.confined())
                ));
                // HTTP initialization is still pending until the child reports it.
                let until = Instant::now() + timing.serving;
                while !stopping.load(Ordering::SeqCst)
                    && child.serving()
                    && !child.reported()
                    && Instant::now() < until
                {
                    thread::park_timeout(POLL);
                }
                let reason = if stopping.load(Ordering::SeqCst) {
                    String::new()
                } else if child.serving() && child.reported() {
                    *lock(state) = WebState::Running;
                    let running = Instant::now();
                    while !stopping.load(Ordering::SeqCst) && child.serving() && eligible() {
                        thread::park_timeout(POLL);
                    }
                    if !stopping.load(Ordering::SeqCst) && child.serving() {
                        // Eligibility ended (HQ left Active or authentication went
                        // away): stop serving now and wait to become eligible again.
                        // This is not a failed start and spends no restart attempt.
                        if running.elapsed() >= timing.stable {
                            failures = 0; // a stable run resets the count, as documented
                        }
                        *lock(state) = WebState::Starting;
                        drop(child); // kill/reap, then end control and its sessions
                        log("authenticated browser stopped because the host is no longer eligible; it starts again once eligible");
                        continue;
                    }
                    stable = running.elapsed() >= timing.stable;
                    format!("the running web process stopped: {}", child.ended())
                } else if child.serving() {
                    "it did not report web.serving before the deadline".to_owned()
                } else {
                    format!("it stopped before serving: {}", child.ended())
                };
                *lock(state) = WebState::Failed;
                drop(child); // kill/reap, then end control, before restart
                reason
            }
            Err(error) => error.to_string(),
        };
        *lock(state) = WebState::Failed;
        if stopping.load(Ordering::SeqCst) {
            break;
        }
        let reason = crate::host_log::untrusted(&reason);
        failures = if stable { 0 } else { failures + 1 };
        if failures >= MAX_STARTS {
            log(&format!(
                "authenticated browser restart limit reached after {failures} failed starts; last: {reason}"
            ));
            break;
        }
        // 1x after a stable run or the first failure, then 2x, 4x, 8x.
        let delay = timing.backoff * (1 << failures.saturating_sub(1));
        log(&format!(
            "authenticated browser restarting in {delay:?} because {reason}"
        ));
        let retry = Instant::now() + delay;
        while !stopping.load(Ordering::SeqCst) && Instant::now() < retry {
            thread::park_timeout(POLL);
        }
    }
    *lock(state) = WebState::Failed;
}
