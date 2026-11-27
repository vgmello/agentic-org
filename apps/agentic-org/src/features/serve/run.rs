//! Running the host until it is stopped: the bounded stop, the foreground setup and the start.

use std::io;
use std::time::{Duration, Instant};

use org_host::signals::ShutdownSignals;
use org_host::stop::Stop;
use org_host::{BrowserListener, Config, Host};

#[cfg(debug_assertions)]
use super::args::dev_cli_peer;
use super::args::{config, parse_args, Build};

/// The longest a stop may take, from the signal to the process's end: the design's 2.5 s
/// (Linux host design, section 3.4), less the moment the end itself takes. Past it the
/// process ends at once (`org_host::stop`).
pub(super) const STOP_LIMIT: Duration = Duration::from_millis(2400);

/// What a `serve` run did.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Served {
    /// `--version`: the caller prints it; nothing was started.
    Version,
    /// The host ran and stopped (or stopped before it started).
    Stopped,
}

pub(crate) fn run(arguments: Vec<String>) -> Result<Served, String> {
    run_as(arguments, Config::refuse_root)
}

/// [`run`], with the check of who may run the service given by the caller (the composition
/// passes [`Config::refuse_root`]; a test passes its own, so the order below is tested
/// without being root).
pub(super) fn run_as(
    arguments: Vec<String>,
    may_run_the_service: impl FnOnce() -> Result<(), String>,
) -> Result<Served, String> {
    let args = parse_args(arguments, Build::CURRENT)?;
    if args.version {
        return Ok(Served::Version);
    }
    // The service is an ordinary user's on every system. Before anything is opened or made:
    // nothing above reads more than the arguments.
    may_run_the_service()?;
    // What the service makes is its user's alone, whatever mask started it: a container
    // runtime starts it with 022, as a shell may, where the registered unit sets
    // `UMask=0077` (X5 review F10). Before any thread exists or any file is made.
    org_platform::process::owner_only_umask();
    // Before any thread exists, so every thread inherits the mask.
    let signals = ShutdownSignals::block().map_err(|e| format!("blocking signals: {e}"))?;
    let registered = args.name.is_some() || args.setup.is_some();
    let name = args.name.clone();
    let setup = args.setup.clone();
    let mut config = config(args)?;
    // Release builds have no `dev_app_path` field: no development peer can be configured.
    #[cfg(debug_assertions)]
    {
        config.dev_app_path = dev_cli_peer(
            Build::CURRENT,
            registered,
            config.dev_app_path.take(),
            || std::env::current_exe().ok(),
        );
    }
    let bound = config.timings.stop_ends_within();
    let stop = Stop::watch(
        move || signals.wait(),
        bound,
        STOP_LIMIT,
        move |signal, starting| {
            let line = if starting {
                format!(
                    "stopped by signal {signal} while starting, which had not ended {bound:?} \
                     after it; the process ends now. What setup or the start completed is \
                     kept, and the next start continues from it"
                )
            } else {
                format!(
                    "stopped by signal {signal}; the stop had not ended {STOP_LIMIT:?} after \
                     it, and the process ends now"
                )
            };
            // A full standard error (a pipe nobody reads) cannot hold the end up: the line
            // is written by the host log's own thread, waited for a short, bounded time,
            // and then the process ends without its exit handlers (X5 review, fourth round,
            // P1-6).
            org_host::stop::end_process(&line)
        },
    )?;
    serve_until_stopped(
        &stop,
        // The foreground mode: set the instance up from the file if it is not set up yet.
        // Its report goes to standard output; a reader that went away (`| head`) does not
        // fail the setup or the serve that follows (`ReportOut`).
        || match &setup {
            Some(file) => crate::host::configured_foreground(
                file,
                name.as_deref(),
                &mut crate::host::ReportOut::new(io::stdout()),
            ),
            None => Ok(None),
        },
        |held| {
            config.browser_listener = held;
            if registered {
                org_host_sqlite::start_registered(config, name.as_deref())
            } else {
                Host::start(config)
            }
            .map_err(|e| e.to_string())
        },
        Host::shutdown_from,
    )
    .map(|()| Served::Stopped)
}

/// What `serve` does once its stop is watched: `setup` (the foreground mode's, or nothing),
/// then `start` (the host, given the browser listener setup held), and `shutdown` of what
/// started once the stop is asked for, from when it was asked for. A stop that comes before
/// `start` is called ends it there: nothing is started, and it exits 0. See the module docs.
pub(super) fn serve_until_stopped<H>(
    stop: &Stop,
    setup: impl FnOnce() -> Result<Option<BrowserListener>, String>,
    start: impl FnOnce(Option<BrowserListener>) -> Result<H, String>,
    shutdown: impl FnOnce(H, Instant),
) -> Result<(), String> {
    let started = start_unless_stopped(stop, setup, start);
    // From here on the stop no longer bounds the start: it has ended, one way or another.
    stop.settle();
    // An error is returned with the stop still bounded: the caller writes it to standard
    // error, and a stop asked for meanwhile still ends the process within its limit if that
    // write blocks (a full pipe nobody reads; X5 final review).
    let Some(host) = started? else {
        // Through the host log, which waits for its line a short, bounded time only.
        org_host::stop::log(
            "stopped before serving: the stop signal came while starting; what setup \
             completed is kept, and the next start serves it",
        );
        stop.finish();
        return Ok(());
    };
    let stopped = match stop.wait() {
        Ok(asked) => {
            shutdown(host, asked);
            Ok(())
        }
        Err(error) => {
            shutdown(host, Instant::now());
            Err(error)
        }
    };
    stop.finish();
    stopped
}

/// `setup`, then `start`, unless the stop is asked for before either: `None` then.
fn start_unless_stopped<H>(
    stop: &Stop,
    setup: impl FnOnce() -> Result<Option<BrowserListener>, String>,
    start: impl FnOnce(Option<BrowserListener>) -> Result<H, String>,
) -> Result<Option<H>, String> {
    if stop.asked()? {
        return Ok(None);
    }
    let held = setup()?;
    // A stop that came during setup: what setup did is kept, and nothing is started.
    if stop.asked()? {
        return Ok(None);
    }
    start(held).map(Some)
}
