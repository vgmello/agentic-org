//! Host-owned browser process. Its private control authority is constructed only
//! after suspended-image, actual-reader, sandbox and descriptor-receipt checks.
use crate::{
    cloexec::SPAWN_LOCK,
    home,
    server_check::{NoAdmission, ServerCheck},
    service::listener::{Handler, WebConnection},
    sync::lock,
};
use org_host_control::{
    gateway::{
        GatewayFd, GatewayFirstLine, GatewayHandoff, GatewayReadyForSecrets, GATEWAY_HANDOFF_FDS,
    },
    listen::{ListenConfig, ListenMode},
    GatewayState,
};
use org_platform_host::admission::{Admission, Child, ChildEnv, ImageRefusal, SpawnRefused};
use std::{
    ffi::OsStr,
    fs,
    io::{self, Read, Write},
    net::TcpListener,
    os::{
        fd::{AsFd, AsRawFd},
        unix::{
            fs::{DirBuilderExt, PermissionsExt},
            net::{UnixDatagram, UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// How long a started child has to connect to the rendezvous, send its ready line
/// and acknowledge the handoff. [`Supervisor::start`] always uses this; tests pass a
/// longer bound to [`Running::start`] so a loaded machine cannot fail them.
const WAIT: Duration = Duration::from_secs(5);
/// Consecutive failed starts before the supervisor gives up.
const MAX_STARTS: usize = 5;
const POLL: Duration = Duration::from_millis(50);
/// How long a started web process may take to report `web.serving` (GW-01 r4).
const SERVING_DEADLINE: Duration = Duration::from_secs(10);
/// A web process that stayed `Running` this long was a healthy start: its later
/// exit begins a fresh run of consecutive failures instead of adding to one.
const STABLE_RUNNING: Duration = Duration::from_secs(60);
/// First restart delay; each further consecutive failure doubles it.
const RESTART_BACKOFF: Duration = Duration::from_secs(1);

/// What one instance's web processes wrote to stderr, for a test.
#[cfg(test)]
#[derive(Clone, Default)]
struct Captured {
    bytes: Vec<u8>,
    /// Readers that have not reached the end of their pipe yet.
    reading: usize,
    /// Why a reader stopped before the end of its pipe.
    failed: Option<String>,
}
/// What each instance's web processes wrote to stderr. The host keeps none of it; a
/// real-process test reads it to prove what the web process never prints.
#[cfg(test)]
static CAPTURED_STDERR: Mutex<std::collections::BTreeMap<String, Captured>> =
    Mutex::new(std::collections::BTreeMap::new());
/// Everything `instance`'s web processes wrote to stderr, once every reader has read its
/// pipe to the end. A reader that failed, or one still reading after ten seconds, fails
/// the test: a partial capture would prove nothing about what was never printed.
#[cfg(test)]
pub(crate) fn captured_stderr(instance: &str) -> Option<Vec<u8>> {
    let until = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let captured = lock(&CAPTURED_STDERR).get(instance).cloned()?;
        if let Some(failed) = captured.failed {
            panic!("a reader of {instance}'s web stderr failed: {failed}");
        }
        if captured.reading == 0 {
            return Some(captured.bytes);
        }
        assert!(
            std::time::Instant::now() < until,
            "{instance}'s web stderr was still being read after ten seconds"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

/// Supervisor timing. Production uses [`Timing::PRODUCTION`]; tests shorten it.
#[derive(Clone, Copy)]
struct Timing {
    serving: Duration,
    stable: Duration,
    backoff: Duration,
}
impl Timing {
    const PRODUCTION: Self = Self {
        serving: SERVING_DEADLINE,
        stable: STABLE_RUNNING,
        backoff: RESTART_BACKOFF,
    };
}
fn unavailable() -> io::Error {
    io::Error::other("authenticated browser service unavailable")
}
fn invalid<T>(_: T) -> io::Error {
    unavailable()
}
/// The launch check refused the web process at `step`. Its cause names only the check
/// and the executable's identity, so it is kept for the supervisor's log line.
fn refused(step: &'static str) -> impl FnOnce(String) -> io::Error {
    move |why| io::Error::other(format!("web process admission refused at {step}: {why}"))
}
/// A failed start step with a fixed, secret-free cause for the supervisor's log line.
fn failed(cause: &str) -> io::Error {
    io::Error::other(format!("web process start failed: {cause}"))
}
/// A failed start step caused by an OS error. Only the error's kind is kept, never
/// its text, so the cause stays fixed and secret-free.
fn os_failed(cause: &'static str) -> impl FnOnce(io::Error) -> io::Error {
    move |error| failed(&format!("{cause} ({:?})", error.kind()))
}

/// The longest reason the host shows of a confinement refusal, in bytes.
const REASON_BYTES: usize = 240;

/// A confinement adapter's reason, or a web process's own, made fit for a log line:
/// printable ASCII only (anything else becomes `?`) and at most [`REASON_BYTES`]
/// long. The adapters' reasons are call names, error names and small numbers; this
/// is the bound that holds whatever the text is. It is shown and never parsed.
fn shown(reason: &str) -> String {
    reason
        .chars()
        .take(REASON_BYTES)
        .map(|c| if matches!(c, ' '..='~') { c } else { '?' })
        .collect()
}

/// The control stream ended where a line was expected. Its text is that of any other
/// failed step; the type lets the start tell a child that went away by itself.
#[derive(Debug)]
struct ControlEof(String);
impl std::fmt::Display for ControlEof {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ControlEof {}

/// What a web process that could not confine itself writes to its standard error
/// before it exits (`agentic-org-web`'s `process::REFUSAL_PREFIX`):
///
/// ```text
/// line   = "agentic-org-web: startup refused: confinement: " reason LF
/// reason = 1*240 ( %x20-7E )
/// ```
///
/// The host looks for this one line in the first [`STDERR_KEPT`] bytes of the
/// standard error of a child that ended before it reported ready, and only shows its
/// reason. Nothing else a child writes there is kept, and the reason decides nothing.
const REFUSAL_PREFIX: &str = "agentic-org-web: startup refused: confinement: ";
/// What a web process that confined itself without Landlock, where its adapter has
/// that layer, writes to its standard error before its ready line
/// (`agentic-org-web`'s `process::LANDLOCK_NOT_APPLIED_PREFIX`):
///
/// ```text
/// line   = "agentic-org-web: confinement: landlock not applied: " reason LF
/// reason = 1*240 ( %x20-7E )
/// ```
///
/// The ready line cannot carry a reason, so it travels here. The host looks for
/// this one line in the first [`STDERR_KEPT`] bytes of the standard error of a child
/// whose report it has verified, and only when the platform contract says the
/// report lacks a Landlock layer. It shows the reason once per start and decides
/// nothing by it: whether the child is confined is what the host observed.
const LANDLOCK_NOT_APPLIED_PREFIX: &str = "agentic-org-web: confinement: landlock not applied: ";
/// How much of a child's standard error is kept; the rest is read and dropped.
const STDERR_KEPT: usize = 1024;
/// How long a child whose control stream ended may take to exit by itself, so that
/// its last line is whole when it is read. After that it is killed as before.
const EXIT_GRACE: Duration = Duration::from_millis(250);
/// How long the end of an exited child's standard error is waited for.
const STDERR_GRACE: Duration = Duration::from_millis(250);

/// The reason of the first well-formed refusal line in `captured`, if there is one.
/// A line that is cut off, empty, too long or not printable ASCII is no refusal line.
fn refusal_reason(captured: &[u8]) -> Option<&str> {
    line_reason(captured, REFUSAL_PREFIX)
}

/// The reason of the first line in `captured` that starts with `prefix`, if that
/// line is whole and its reason is 1 to [`REASON_BYTES`] bytes of printable ASCII.
fn line_reason<'a>(captured: &'a [u8], prefix: &str) -> Option<&'a str> {
    let mut rest = captured;
    while let Some(end) = rest.iter().position(|byte| *byte == b'\n') {
        let (line, after) = rest.split_at(end);
        rest = &after[1..];
        let Some(reason) = line.strip_prefix(prefix.as_bytes()) else {
            continue;
        };
        let printable = reason.iter().all(|byte| (0x20..=0x7e).contains(byte));
        if reason.is_empty() || reason.len() > REASON_BYTES || !printable {
            return None;
        }
        return std::str::from_utf8(reason).ok();
    }
    None
}

/// The start of a child's standard error, read by its own thread until the child's
/// end closes it. `changed` is notified when bytes are kept and when it has ended.
struct Stderr {
    kept: Mutex<(Vec<u8>, bool)>,
    changed: Condvar,
}
impl Stderr {
    /// Reads `stderr` to its end on a new thread, keeping its first [`STDERR_KEPT`]
    /// bytes and dropping the rest, so that a child never blocks on a full pipe.
    /// A test build also copies all of it into [`CAPTURED_STDERR`] under `instance`.
    fn capture(mut stderr: impl Read + Send + 'static, instance: &str) -> io::Result<Arc<Self>> {
        #[cfg(test)]
        let instance = instance.to_owned();
        #[cfg(not(test))]
        let _ = instance;
        let captured = Arc::new(Self {
            kept: Mutex::new((Vec::new(), false)),
            changed: Condvar::new(),
        });
        let filling = Arc::clone(&captured);
        // Counted before the reader starts, so a test that reads the capture after the
        // process ended waits for this reader too.
        #[cfg(test)]
        {
            lock(&CAPTURED_STDERR)
                .entry(instance.clone())
                .or_default()
                .reading += 1;
        }
        #[cfg(test)]
        let ended = {
            let instance = instance.clone();
            move |failed: Option<String>| {
                if let Some(entry) = lock(&CAPTURED_STDERR).get_mut(&instance) {
                    entry.reading -= 1;
                    entry.failed = entry.failed.take().or(failed);
                }
            }
        };
        #[cfg(test)]
        let on_spawn_failure = ended.clone();
        let spawned = thread::Builder::new()
            .name("web-stderr".into())
            .spawn(move || {
                let mut bytes = [0u8; 4096];
                // Reads until the end of the pipe or an error, which also ends it.
                let stopped = loop {
                    let read = match stderr.read(&mut bytes) {
                        Ok(0) => break None,
                        Ok(read) => read,
                        Err(error) => break Some(error),
                    };
                    #[cfg(test)]
                    lock(&CAPTURED_STDERR)
                        .entry(instance.clone())
                        .or_default()
                        .bytes
                        .extend_from_slice(&bytes[..read]);
                    let mut kept = lock(&filling.kept);
                    let room = STDERR_KEPT - kept.0.len();
                    kept.0.extend_from_slice(&bytes[..read.min(room)]);
                    drop(kept);
                    filling.changed.notify_all();
                };
                lock(&filling.kept).1 = true;
                filling.changed.notify_all();
                #[cfg(test)]
                ended(stopped.map(|error| error.to_string()));
                #[cfg(not(test))]
                let _ = stopped;
            });
        #[cfg(test)]
        if let Err(error) = &spawned {
            on_spawn_failure(Some(format!("the reader did not start: {error}")));
        }
        spawned?;
        Ok(captured)
    }

    /// The reason of the child's refusal line, once its standard error has ended or
    /// `wait` has passed.
    fn refusal(&self, wait: Duration) -> Option<String> {
        let (kept, _) = self
            .changed
            .wait_timeout_while(lock(&self.kept), wait, |kept| !kept.1)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        refusal_reason(&kept.0).map(shown)
    }

    /// The reason of the child's "landlock not applied" line, as soon as the line is
    /// among the kept bytes. The child wrote it before its ready line, so it is
    /// there once this reader has caught up; without it this returns when the
    /// standard error has ended, the kept bytes are full or `wait` has passed.
    fn landlock_not_applied(&self, wait: Duration) -> Option<String> {
        let found = |kept: &[u8]| line_reason(kept, LANDLOCK_NOT_APPLIED_PREFIX).map(shown);
        let (kept, _) = self
            .changed
            .wait_timeout_while(lock(&self.kept), wait, |kept| {
                !kept.1 && kept.0.len() < STDERR_KEPT && found(&kept.0).is_none()
            })
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        found(&kept.0)
    }
}

/// What the host logs of a started web process's confinement: the layers the
/// platform contract names for its verified report, and, where a Landlock layer is
/// missing from them, the reason the web process gave (`reason` is asked only then).
fn confined_by(
    layers: &org_platform::confinement::Layers,
    reason: impl FnOnce() -> Option<String>,
) -> String {
    let by = &layers.description;
    if !layers.landlock_missing {
        return format!("confined by {by}");
    }
    match reason() {
        Some(reason) => format!("confined by {by}: Landlock was not applied: {reason}"),
        None => format!(
            "confined by {by}: Landlock was not applied, and the web process gave no reason"
        ),
    }
}

/// The cause of a start that failed before the ready line, with the reason of the
/// child's own refusal when it left one. Only a child that has ended by itself is
/// asked: one that is already reaped, or, when its control stream ended, one that
/// exits within [`EXIT_GRACE`]. Any other child is left to be killed at once.
fn with_refusal(error: io::Error, captured: &Stderr, child: &mut Child) -> io::Error {
    let went_away = error
        .get_ref()
        .is_some_and(|inner| inner.is::<ControlEof>());
    let until = Instant::now()
        + if went_away {
            EXIT_GRACE
        } else {
            Duration::ZERO
        };
    while !matches!(child.try_wait(), Ok(true)) {
        if Instant::now() >= until {
            return error;
        }
        thread::sleep(Duration::from_millis(5));
    }
    match captured.refusal(STDERR_GRACE) {
        Some(reason) => io::Error::other(format!(
            "{error}; it reported that it cannot confine itself: {reason}"
        )),
        None => error,
    }
}

/// The cause of a rendezvous peer that is not the admitted child. A child that has
/// ended is no reader either, and it may have connected first: that is said as what
/// it is, with the reason of its own refusal when it left one.
fn not_the_reader(captured: &Stderr, child: &mut Child) -> io::Error {
    if !matches!(child.try_wait(), Ok(true)) {
        return failed("the rendezvous peer is not the admitted child");
    }
    let ended = failed(&format!(
        "the child exited before its ready line ({})",
        exited(child)
    ));
    with_refusal(ended, captured, child)
}

/// The web process's whole environment: nothing. Not one variable of the host's
/// reaches it, so neither `RUST_BACKTRACE` (a backtrace opens files, which a confined
/// process cannot do) nor a loader variable can. It needs none: its release, its
/// rendezvous and its instance come on the command line and the first line.
fn web_env() -> ChildEnv {
    ChildEnv::new()
}

/// After `verify_web`: the status it read was the admitted child's only if that
/// child is still this process's unreaped child. A process id alone can be reused.
fn still_running(child: &mut Child) -> io::Result<()> {
    match child.try_wait() {
        Ok(false) => Ok(()),
        Ok(true) => Err(failed(&format!(
            "the child exited after its sandbox was verified ({})",
            exited(child)
        ))),
        Err(error) => Err(os_failed("wait for the child")(error)),
    }
}

pub(crate) struct Configuration {
    pub program: PathBuf,
    pub instance: String,
    pub runtime: PathBuf,
    pub listen: ListenConfig,
    pub check: ServerCheck,
    pub critical_paths: Vec<PathBuf>,
    pub release: Option<org_host_control::gateway::WebRelease>,
}
/// The selected release's web process image pin (`Err`: the release has none) and the
/// release identity bound into the web process's first line.
pub(crate) type SelectedRelease<E> = (
    Result<crate::server_check::Pin, E>,
    org_host_control::gateway::WebRelease,
);

/// The web process admission policy of a build (`debug`: `cfg!(debug_assertions)`) with app
/// team id `team_id` and the selected `release`, if any: the check every start uses and
/// the release binding of its first line. A malformed team id or a release without a
/// web process image hash refuses every start; a selected release pins its exact image.
pub(crate) fn admission<E>(
    debug: bool,
    team_id: Option<&str>,
    release: Option<SelectedRelease<E>>,
) -> (ServerCheck, Option<org_host_control::gateway::WebRelease>) {
    let check = ServerCheck::gateway_for_build(debug, team_id)
        .unwrap_or(ServerCheck::WebRefused("malformed app team id"));
    match release {
        None => (check, None),
        Some((Ok(pin), binding)) => (check.pinned(pin), Some(binding)),
        Some((Err(_), binding)) => (
            ServerCheck::WebRefused("the selected release has no agentic-org-web image hash"),
            Some(binding),
        ),
    }
}

/// The web process's admission, built once in a host's lifetime.
///
/// A system that executes verified bytes reads, checks and seals the web process's image
/// when the admission is built: every web process this host starts, each restart included,
/// then runs exactly the bytes read at that moment, and no restart reads or hashes the
/// program again. Where the loaded image is checked instead, building reads nothing and
/// this changes nothing.
///
/// It has the properties of the office servers' admission (`supervisor::child::Images`),
/// with its own state and its own words:
///
/// - the build begins with the supervisor, on its own thread, beside the host's start;
/// - a start waits for a build that is under way: there is never more than one;
/// - a build that fails is logged, fails that start with its reason, and is asked for again
///   by the next start, so the supervisor's limit of consecutive failed starts ends the
///   retries;
/// - an admission, once built, is never built again.
pub(crate) struct Image {
    check: ServerCheck,
    program: PathBuf,
    built: Mutex<Option<Arc<Admission>>>,
}
impl Image {
    /// The admission of `check`'s web process started from `program`, not built yet.
    fn new(check: &ServerCheck, program: &Path) -> Arc<Self> {
        Arc::new(Self {
            check: check.clone(),
            program: program.to_owned(),
            built: Mutex::new(None),
        })
    }

    /// Begins the build on its own thread. Without a thread to build it on, the first
    /// start builds it.
    fn begin(self: &Arc<Self>) {
        let building = Arc::clone(self);
        let _ = thread::Builder::new()
            .name("web-image".into())
            .spawn(move || building.build_at_start());
    }

    /// The build a host makes at its start, with the reason logged when there is none.
    fn build_at_start(&self) {
        if let Err(none) = self.admission() {
            crate::host_log::log(&format!(
                "the web process cannot be started: {}; each start asks again",
                crate::host_log::untrusted(&none.to_string())
            ));
        }
    }

    /// The host's admission of its web process, built now if no earlier attempt
    /// succeeded. A caller waits for a build that is under way.
    fn admission(&self) -> Result<Arc<Admission>, NoAdmission> {
        let mut built = lock(&self.built);
        if let Some(admission) = built.as_ref() {
            return Ok(Arc::clone(admission));
        }
        let admission = Arc::new(self.check.admission(&self.program)?);
        if let Some(line) = self.check.describe_admitted(&self.program, &admission) {
            crate::host_log::log(&line);
        }
        *built = Some(Arc::clone(&admission));
        Ok(admission)
    }
}

/// The refusal of a system on which the web process cannot be confined: fixed
/// wording, then the adapter's reason (which call failed with which error, or that
/// the system has no adapter).
fn unconfinable(reason: io::Error) -> io::Error {
    io::Error::other(format!(
        "the web process requires process confinement, which is unavailable here: {}",
        shown(&reason.to_string())
    ))
}

pub(crate) struct Supervisor {
    stop: Arc<AtomicBool>,
    state: Arc<Mutex<GatewayState>>,
    thread: Option<JoinHandle<()>>,
}
impl Supervisor {
    pub(crate) fn start(
        config: Configuration,
        handler: Arc<dyn Handler>,
        state: Arc<Mutex<GatewayState>>,
        eligible: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> io::Result<Self> {
        // No bind, child or secret handoff for unverified TLS or a system on which the
        // web process cannot be confined.
        // Each refusal names its cause for the host log.
        org_platform::confinement::web_confinement_supported().map_err(unconfinable)?;
        // Plain HTTP, in one of its two modes: on loopback, or on the published port of a
        // foreground instance, which the registered start has already judged against the
        // instance's service mode. HTTPS needs a verified pair, which no host has yet.
        if !matches!(
            config.listen.scheme(),
            ListenMode::HttpLoopback | ListenMode::HttpPublished
        ) {
            return Err(io::Error::other(
                "the web process requires a plain HTTP listener (http_loopback, or \
                 http_published in the foreground mode): HTTPS has no verified certificate yet",
            ));
        }
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
        let thread = thread::Builder::new()
            .name("org-browser-supervisor".into())
            .spawn(move || {
                supervise(
                    &stopping,
                    &observing,
                    &*eligible,
                    Timing::PRODUCTION,
                    &|line| crate::host_log::log(line),
                    || Running::start(&config, &image, Arc::clone(&handler), WAIT),
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
impl Drop for Supervisor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
        *lock(&self.state) = GatewayState::Failed;
    }
}

/// A started web process the supervisor watches.
trait Serving {
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
fn supervise<S: Serving>(
    stopping: &AtomicBool,
    state: &Mutex<GatewayState>,
    eligible: &dyn Fn() -> bool,
    timing: Timing,
    log: &dyn Fn(&str),
    mut start: impl FnMut() -> io::Result<S>,
) {
    let mut failures = 0usize;
    loop {
        // First protected bootstrap starts the host before HQ exists. Wait without
        // binding or spending restart attempts; the same controller observes actual
        // completed provisioning.
        while !stopping.load(Ordering::SeqCst) && !eligible() {
            *lock(state) = GatewayState::Starting;
            thread::park_timeout(POLL);
        }
        if stopping.load(Ordering::SeqCst) {
            break;
        }
        *lock(state) = GatewayState::Starting;
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
                    *lock(state) = GatewayState::Running;
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
                        *lock(state) = GatewayState::Starting;
                        drop(child); // end sessions/control, then kill/reap
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
                *lock(state) = GatewayState::Failed;
                drop(child); // end sessions/control, then kill/reap before restart
                reason
            }
            Err(error) => error.to_string(),
        };
        *lock(state) = GatewayState::Failed;
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
    *lock(state) = GatewayState::Failed;
}

struct Rendezvous {
    listener: UnixListener,
    path: PathBuf,
}
impl Rendezvous {
    fn bind(runtime: &Path) -> io::Result<Self> {
        let directory = runtime.join("r");
        match fs::DirBuilder::new().mode(0o700).create(&directory) {
            Ok(()) => (),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e),
        }
        home::owner_only_dir(&directory).map_err(invalid)?;
        let path = directory.join(crate::secrets::generate_unique_name()?);
        org_proto::location::check_socket_path(&path, home::socket_path_limit()?)
            .map_err(invalid)?;
        let listener = {
            let _fence = lock(&SPAWN_LOCK);
            UnixListener::bind(&path)?
        };
        let bound = Self { listener, path };
        fs::set_permissions(&bound.path, fs::Permissions::from_mode(0o600))?;
        bound.listener.set_nonblocking(true)?;
        Ok(bound)
    }
    fn accept(&self, process: &mut Child, until: Instant) -> io::Result<UnixStream> {
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    stream
                        .set_nonblocking(false)
                        .map_err(os_failed("make the control stream blocking"))?;
                    return Ok(stream);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock && Instant::now() < until => {
                    if process
                        .try_wait()
                        .map_err(os_failed("wait for the child"))?
                    {
                        return Err(failed(&format!(
                            "the child exited before connecting ({})",
                            exited(process)
                        )));
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    return Err(failed("the child did not connect before the deadline"))
                }
                Err(e) => return Err(os_failed("accept the rendezvous")(e)),
            }
        }
    }
}
/// How the reaped child ended, for a log line.
fn exited(process: &Child) -> String {
    process
        .ended()
        .unwrap_or_else(|| "status unknown".to_owned())
}
impl Drop for Rendezvous {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

struct Running {
    // Drop order: close control/revoke first, then kill/reap the child.
    control: WebConnection,
    process: Child,
    _stdin: fs::File,
    /// The layers of the report the host verified.
    layers: org_platform::confinement::Layers,
    /// The child's standard error, for the reason a layer is missing.
    captured: Arc<Stderr>,
}
impl Serving for Running {
    fn serving(&mut self) -> bool {
        self.control.live() && matches!(self.process.try_wait(), Ok(false))
    }
    fn reported(&mut self) -> bool {
        self.control.reported_serving()
    }
    fn ended(&mut self) -> String {
        let refused = self.control.serving_refused();
        let exit = matches!(self.process.try_wait(), Ok(true)).then(|| exited(&self.process));
        ended_cause(refused, exit, self.control.live())
    }
    fn confined(&mut self) -> String {
        confined_by(&self.layers, || {
            self.captured.landlock_not_applied(STDERR_GRACE)
        })
    }
}
/// Why a running web process ended, pure: a refused `web.serving` wins over the EOF it
/// causes, then a reaped child's exit status (`exit`), then the control EOF.
fn ended_cause(serving_refused: bool, exit: Option<String>, live: bool) -> String {
    if serving_refused {
        "the host refused its web.serving as a protocol error".to_owned()
    } else if let Some(status) = exit {
        format!("the child exited ({status})")
    } else if !live {
        "its control connection reached EOF".to_owned()
    } else {
        "it stopped serving".to_owned()
    }
}
impl Running {
    /// Starts the web process of `config` through `image`, the host's one admission of it.
    /// `wait` bounds the steps from spawn through the handoff ACK.
    fn start(
        config: &Configuration,
        image: &Image,
        handler: Arc<dyn Handler>,
        wait: Duration,
    ) -> io::Result<Self> {
        config
            .check
            .may_spawn()
            .map_err(refused("the pre-spawn check"))?;
        // Validated configuration is frozen before bind. No port-zero substitution.
        let listener = {
            let _fence = lock(&SPAWN_LOCK);
            TcpListener::bind((config.listen.address(), config.listen.port()))
                .map_err(os_failed("bind the browser listener"))?
        };
        let bound = listener
            .local_addr()
            .map_err(os_failed("read the browser listener address"))?;
        if bound.port() != config.listen.port() {
            return Err(failed("the browser listener is not on its configured port"));
        }
        let rendezvous =
            Rendezvous::bind(&config.runtime).map_err(os_failed("bind the rendezvous"))?;
        // The platform's admission starts the child with only its three pipes (holding
        // `SPAWN_LOCK` while it makes them and the process), checks its image before any
        // of it has run, and holds it until it is released. A system without an admission
        // adapter starts nothing. The admission is the host's one: built at its start, or
        // now if that build failed. A refused image keeps the check's words for the log.
        let checked = image.admission().map_err(|none| match none {
            NoAdmission::Refused(why) => refused("admission")(why),
            NoAdmission::Unavailable(error) => os_failed("spawn the web process suspended")(error),
        })?;
        let held = checked
            .start(&[OsStr::new("--serve")], &web_env(), &SPAWN_LOCK)
            .map_err(|error| match SpawnRefused::of(&error) {
                Some(refusal) => refused("admission")(config.check.why(&refusal.refusal)),
                None => os_failed("spawn the web process suspended")(error),
            })?;
        let pid = held.child().id();
        checked
            .recheck(held.child())
            .map_err(|refusal| refused("the pre-resume re-check")(config.check.why(&refusal)))?;
        let mut spawned = held
            .release()
            .map_err(os_failed("resume the web process"))?;
        let first = GatewayFirstLine {
            rendezvous: rendezvous
                .path
                .to_str()
                .ok_or_else(|| failed("the rendezvous path is not UTF-8"))?
                .into(),
            instance_id: config.instance.clone(),
            release: config.release.clone(),
        };
        let mut first = serde_json::to_vec(&first).map_err(|_| failed("encode the first line"))?;
        if first.len() >= 1024 {
            return Err(failed("the first line exceeds 1023 bytes"));
        }
        first.push(b'\n');
        spawned
            .stdin
            .write_all(&first)
            .map_err(os_failed("write the first line"))?;
        // Its standard output is drained and dropped: nothing of it is kept.
        let mut stdout = spawned.stdout;
        thread::Builder::new()
            .name("web-stdout".into())
            .spawn(move || {
                let _ = io::copy(&mut stdout, &mut io::sink());
            })
            .map_err(os_failed("start the stdout drain"))?;
        // Its standard error is drained too; only its start is kept, for the one line
        // of a child that could not confine itself.
        let captured = Stderr::capture(spawned.stderr, &config.instance)
            .map_err(os_failed("start the stderr drain"))?;
        let until = Instant::now() + wait;
        let mut stream = rendezvous
            .accept(&mut spawned.child, until)
            .map_err(|error| with_refusal(error, &captured, &mut spawned.child))?;
        crate::supervisor::channels::reader_is_child(&checked, &spawned.child, &stream)
            .map_err(|_| not_the_reader(&captured, &mut spawned.child))?;
        let mut receiver = org_socket::rights::StreamReceiver::new(
            stream
                .try_clone()
                .map_err(os_failed("clone the control stream"))?,
        );
        let ready = line(&mut receiver, until, "ready line")
            .map_err(|error| with_refusal(error, &captured, &mut spawned.child))?;
        let ready: GatewayReadyForSecrets = serde_json::from_slice(&ready)
            .map_err(|_| failed("the ready line does not match ready_for_secrets"))?;
        let enforcement = reported(&ready).map_err(failed)?;
        org_platform::confinement::verify_ready_report(&enforcement)
            .map_err(|error| failed(&error.to_string()))?;
        crate::supervisor::channels::reader_is_child(&checked, &spawned.child, &stream)
            .map_err(|_| failed("the ready-line peer is not the admitted child"))?;
        checked
            .recheck(&spawned.child)
            .map_err(|refusal| refused("the post-ready re-check")(config.check.why(&refusal)))?;
        // Admitted once more from the start: it must be the identity admitted before.
        checked
            .readmit(&spawned.child)
            .map_err(|refusal| match refusal {
                ImageRefusal::Readmitted => io::Error::other(
                    "web process admission refused at re-admission: the identity changed",
                ),
                refusal => refused("re-admission")(config.check.why(&refusal)),
            })?;
        let paths: Vec<_> = config.critical_paths.iter().map(PathBuf::as_path).collect();
        org_platform::confinement::verify_web(pid, &enforcement, &paths).map_err(|error| {
            failed(&format!(
                "verify the web process sandbox: {}",
                shown(&error.to_string())
            ))
        })?;
        // The child is held unreaped across that check; it must not have exited since.
        still_running(&mut spawned.child)?;
        // What the verified report says the child is confined by, for the log.
        let layers = org_platform::confinement::layers(&enforcement).map_err(|error| {
            failed(&format!(
                "name the web process sandbox: {}",
                shown(&error.to_string())
            ))
        })?;
        let (transfer, child) = transfer_pair().map_err(os_failed("create the transfer pair"))?;
        send_handoff(&mut stream, &child, &listener, until)
            .map_err(os_failed("send the descriptor handoff"))?;
        check_ack(&line(&mut receiver, until, "handoff ACK")?)?;
        drop(receiver);
        drop(child);
        drop(listener);
        drop(rendezvous);
        stream
            .set_write_timeout(None)
            .map_err(os_failed("clear the control write timeout"))?;
        let control = WebConnection::start(stream, transfer, config.listen.clone(), handler)
            .map_err(os_failed("start the control connection"))?;
        Ok(Self {
            control,
            process: spawned.child,
            _stdin: spawned.stdin,
            layers,
            captured,
        })
    }
}

/// Accepts only the web process's `{"v":1,"event":"fds.received","of":"handoff"}` line.
/// Each refusal has a fixed, secret-free cause for the supervisor's log line.
fn check_ack(bytes: &[u8]) -> io::Result<()> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Ack {
        v: u32,
        event: String,
        of: String,
    }
    let ack: Ack =
        serde_json::from_slice(bytes).map_err(|_| failed("the handoff ACK is malformed"))?;
    if ack.v != org_host_control::API_VERSION || ack.event != "fds.received" || ack.of != "handoff"
    {
        return Err(failed("the handoff ACK does not acknowledge the handoff"));
    }
    Ok(())
}

/// The host's transfer end (send-only) and the web process's end (receive-only).
/// The web process's write side is shut down before the end is handed off, so the
/// web process cannot send on its transfer endpoint.
fn transfer_pair() -> io::Result<(UnixDatagram, UnixDatagram)> {
    let (transfer, child) = {
        let _fence = lock(&SPAWN_LOCK);
        UnixDatagram::pair()?
    };
    org_socket::rights::set_buffer_sizes(
        transfer.as_fd(),
        org_office_launch::TRANSFER_SOCKET_BUFFER_BYTES,
    )?;
    org_socket::rights::set_buffer_sizes(
        child.as_fd(),
        org_office_launch::TRANSFER_SOCKET_BUFFER_BYTES,
    )?;
    child.shutdown(std::net::Shutdown::Write)?;
    transfer.shutdown(std::net::Shutdown::Read)?;
    Ok((transfer, child))
}

/// One [`GatewayHandoff`] line, then one `sendmsg` with the host.sock marker and
/// exactly [`GATEWAY_HANDOFF_FDS`] in order: the transfer end, then the listener.
fn send_handoff(
    stream: &mut UnixStream,
    child: &UnixDatagram,
    listener: &TcpListener,
    until: Instant,
) -> io::Result<()> {
    let fds = GATEWAY_HANDOFF_FDS.map(|fd| match fd {
        GatewayFd::Transfer => child.as_fd(),
        GatewayFd::Listener => listener.as_fd(),
    });
    stream.set_write_timeout(Some(WAIT))?;
    org_proto::frame::write_message(stream, &GatewayHandoff).map_err(invalid)?;
    org_socket::rights::send(
        stream.as_fd(),
        &[org_host_control::ops::HOST_SOCK_MARKER],
        &fds,
        until,
    )
    .map_err(invalid)
}

// recvmsg reads exactly through LF, never consuming a pipelined hello or silently
// discarding early descriptors. A fatal ancillary frame terminates the host as
// required by X-02; none of its possibly unreported descriptors may survive.
fn line(
    receiver: &mut org_socket::rights::StreamReceiver,
    until: Instant,
    what: &str,
) -> io::Result<Vec<u8>> {
    let timed_out = || failed(&format!("the {what} did not arrive before the deadline"));
    let mut bytes = Vec::new();
    loop {
        match receiver.receive_byte(0, &SPAWN_LOCK) {
            Ok((b'\n', _)) => return Ok(bytes),
            Ok((b, _)) if bytes.len() < 1023 => bytes.push(b),
            Ok(_) => return Err(failed(&format!("the {what} exceeds 1023 bytes"))),
            Err(org_socket::rights::ReceiveError::WouldBlock) => {
                let left = until
                    .checked_duration_since(Instant::now())
                    .ok_or_else(timed_out)?;
                let fd = receiver
                    .fd()
                    .ok_or_else(|| failed(&format!("the {what} stream is closed")))?;
                if !org_socket::poll_readable(fd.as_raw_fd(), left)
                    .map_err(os_failed("poll the control stream"))?
                {
                    return Err(timed_out());
                }
            }
            // The child's end of the control stream is gone. Linux reports that as a reset
            // (`ECONNRESET`), once, when the child closed or died with bytes of the host's
            // it had not read, and as the end of the stream otherwise; macOS reports the
            // end of the stream in both cases. A reset comes only after everything the
            // child had sent was read, and carries no bytes and no descriptors: it is the
            // same event, and this start fails as for any child that went away.
            Err(org_socket::rights::ReceiveError::Fatal(e))
                if matches!(
                    e.kind(),
                    io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset
                ) =>
            {
                let cause = failed(&format!("control EOF before the {what}"));
                return Err(io::Error::other(ControlEof(cause.to_string())));
            }
            Err(org_socket::rights::ReceiveError::Fatal(e)) => {
                crate::host_log::log(&fatal_receive(what, e.kind()));
                std::process::exit(1)
            }
        }
    }
}

/// The host's last log line before a fatal ancillary frame terminates it: the
/// step and the error's kind only, never its text, escaped for one log line.
fn fatal_receive(what: &str, kind: io::ErrorKind) -> String {
    crate::host_log::untrusted(&format!(
        "web process start failed fatally while receiving the {what} ({kind:?}); the host exits"
    ))
}

/// What the web process's ready line reports about its confinement, as the platform
/// contract takes it. An [`Enforcement`](org_platform::confinement::Enforcement) says a
/// Landlock domain is in force in full, so a line that reports one enforced only in
/// part, or not at all, never becomes one: it is refused here. The line has no
/// seccomp field; where the system has seccomp the host observes it on the process
/// itself (Linux host design, section 3.3).
fn reported(
    ready: &GatewayReadyForSecrets,
) -> Result<org_platform::confinement::Enforcement, &'static str> {
    use org_host_control::gateway::LandlockEnforcement::FullyEnforced;
    match ready.landlock {
        Some(status) if status.enforcement != FullyEnforced => {
            Err("the ready line reports landlock not fully enforced")
        }
        landlock => Ok(org_platform::confinement::Enforcement {
            landlock_abi: landlock.map(|status| status.abi),
            seccomp: false,
        }),
    }
}

#[cfg(test)]
mod report_tests;
#[cfg(test)]
mod tests;
