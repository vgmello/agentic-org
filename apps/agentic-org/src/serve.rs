//! agentic-org serve: the service host
//!
//! A thin binary: reads its arguments, finds the office home, starts the [`Host`] and stops it
//! on SIGTERM or SIGINT. All logic lives in the library.
//!
//! ```text
//! agentic-org serve [--version]
//! agentic-org serve --name <instance>
//! agentic-org serve --setup <file> [--name <instance>]
//! agentic-org serve [--home <absolute path>] [--server-bin <path>] [--app-team-id <team id>]   (debug builds)
//! ```
//! `--name` serves the registered instance setup reserved under that name ([`Host::start_registered`]):
//! it creates the instance's host registry at its first start and answers the administrator
//! link calls. It is accepted by both builds and excludes `--home`. A release build without a
//! pinned runtime release refuses at runtime admission, before any custody is opened.
//! `--setup` is the foreground mode, a container's command (ADR-0017 decision 10): when the
//! instance is not set up yet it first runs the non-interactive, offline setup of that file
//! (`agentic-org setup --non-interactive --config <file>`), which changes nothing on a later
//! start, and then serves the instance as `--name` does. The file's service mode must be
//! `foreground`: nothing is registered, and this process is the service. It is accepted by
//! both builds and excludes `--home`.
//! **A release build trusts only what it was built with** ([`Config::trusted`]): the app's
//! bundle id and team id (`ORG_APP_TEAM_ID` at build time; without it no signed app can use
//! `office.restart`, logged loudly), its Keychain access group
//! (`ORG_HOST_KEYCHAIN_GROUP` at build time; without it the host doesn't start), and the office
//! server next to its own executable. It refuses `--home`, `--server-bin` and `--app-team-id`.
//!
//! A debug build takes them for development and tests: `--home` overrides the office home
//! (otherwise `org_proto::location`'s rule), `--server-bin` the office server program (otherwise
//! `agentic-org-office` next to this one), and `--app-team-id` the team id it was built with.
//! It may also accept one unsigned development app for the app-only calls: the path of its
//! executable (`Contents/MacOS/<exe>`) is the build-time constant `ORG_DEV_APP_PATH` (set when
//! the host is compiled), never read from the command line, a config file or the environment at
//! run time. If it doesn't exist when the host starts, the fallback is off (logged).
//!
//! `serve` refuses to run as root, on every system and in both builds, before it opens or
//! creates anything ([`Config::refuse_root`], ADR-0017 decision 10): the service account is
//! an ordinary user. The host then closes itself to the other processes of that user before
//! it reads a key (no core dump; on Linux not dumpable).
//!
//! **The stop.** `serve` stops on SIGTERM or SIGINT, within 2.5 s of the signal (Linux host
//! design, section 3.4), from the moment it blocks them, before setup runs: a thread takes
//! the signal as soon as it arrives ([`Stop`]), so a stop that comes while the instance is
//! set up or the host starts is never left waiting for them (X5 review, third round, P1).
//! - A stop that comes before setup has ended: setup runs to its end (it waits for nothing
//!   but the disk), and `serve` exits 0 without starting the host. Setup is idempotent and
//!   what it did is kept: the next `serve --setup` with the same file continues it, and
//!   then serves.
//! - A stop that comes while the host starts: the host stops as soon as its start returns,
//!   with the stop's deadline fixed from the signal ([`Host::shutdown_from`]), and `serve`
//!   exits 0.
//! - A start (setup, or the host's) that has not returned
//!   [`stop_ends_within`](org_host::Timings::stop_ends_within) (2 s) after the signal (a
//!   slow volume, say) ends the process there, with a line that says so, inside the 2.5 s.
//!   Its status is 0 too: the stop was asked for, and a service manager must not take the
//!   exit for a failure (launchd restarts a job that does not exit 0, and systemd marks the
//!   unit failed). What setup or the start completed is kept: setup and every store the host
//!   writes recover from a process that ends at any point, as from the container runtime's
//!   own kill a moment later, and the next start continues. Office servers and the web
//!   process started meanwhile end with it (the end of their input and of their control
//!   connection).
//! - Any stop that has not ended 2.4 s after the signal (the host's last steps waiting on a
//!   slow volume, say) ends the process there, with status 0 and a line that says so.
//! - Waiting for the signal failing (it cannot, in practice) is an error, whenever it is
//!   found: `serve` stops what it started and exits non-zero.
//!
//! A debug build keeps the offices' secrets keys as files in the folder `ORG_OFFICE_DEV_KEY_DIR`
//! names, never in the Keychain, and refuses to start without it. Release builds ignore the
//! variable. The host never passes the variable on to its servers.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use org_host::signals::ShutdownSignals;
use org_host::stop::Stop;
use org_host::{BrowserListener, Config, Host};

/// The longest a stop may take, from the signal to the process's end: the design's 2.5 s
/// (Linux host design, section 3.4), less the moment the end itself takes. Past it the
/// process ends at once (`org_host::stop`).
const STOP_LIMIT: Duration = Duration::from_millis(2400);

const USAGE: &str = "usage: agentic-org serve [--version] | --name <instance> | --setup <file> \
                     [--name <instance>] | (debug builds only) [--home <absolute path>] \
                     [--server-bin <path>] [--app-team-id <team id>]";

/// Which kind of build this is: a release build refuses the arguments that change what it
/// trusts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Build {
    Debug,
    Release,
}

impl Build {
    const CURRENT: Build = if cfg!(debug_assertions) {
        Build::Debug
    } else {
        Build::Release
    };
}

/// What the command line asks for.
#[derive(Debug, Default, PartialEq, Eq)]
// A release build reads only `version`: it refuses the rest.
#[cfg_attr(not(debug_assertions), allow(dead_code))]
struct Args {
    version: bool,
    /// The registered instance to serve.
    name: Option<String>,
    /// The foreground mode: the setup configuration to apply first.
    setup: Option<PathBuf>,
    home: Option<PathBuf>,
    server_bin: Option<PathBuf>,
    app_team_id: Option<String>,
}

pub(crate) fn run(arguments: Vec<String>) -> Result<(), String> {
    run_as(arguments, Config::refuse_root)
}

/// [`run`], with the check of who may run the service given by the caller (the composition
/// passes [`Config::refuse_root`]; a test passes its own, so the order below is tested
/// without being root).
fn run_as(
    arguments: Vec<String>,
    may_run_the_service: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let args = parse_args(arguments, Build::CURRENT)?;
    if args.version {
        println!("agentic-org {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    // The service is an ordinary user's on every system. Before anything is opened or made:
    // nothing above reads more than the arguments.
    may_run_the_service()?;
    // What the service makes is its user's alone, whatever mask started it: a container
    // runtime starts it with 022, as a shell may, where the registered unit sets
    // `UMask=0077` (X5 review F10). Before any thread exists or any file is made.
    // SAFETY: umask has no preconditions and cannot fail.
    unsafe { libc::umask(0o077) };
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
                    "agentic-org: stopped by signal {signal} while starting, which had not \
                     ended {bound:?} after it; the process ends now. What setup or the start \
                     completed is kept, and the next start continues from it\n"
                )
            } else {
                format!(
                    "agentic-org: stopped by signal {signal}; the stop had not ended \
                     {STOP_LIMIT:?} after it, and the process ends now\n"
                )
            };
            // Neither std's stderr lock nor the exit handlers, which a thread blocked in a
            // write (or in anything else) could hold up: one write, best effort, then the
            // end of the process.
            // SAFETY: a write of an owned buffer to fd 2, then `_exit`, which never returns.
            unsafe {
                libc::write(2, line.as_ptr().cast(), line.len());
                libc::_exit(0)
            }
        },
    )?;
    serve_until_stopped(
        &stop,
        // The foreground mode: set the instance up from the file if it is not set up yet.
        || match &setup {
            Some(file) => crate::setup::configured_foreground(file, name.as_deref()),
            None => Ok(None),
        },
        |held| {
            config.browser_listener = held;
            if registered {
                Host::start_registered(config, name.as_deref())
            } else {
                Host::start(config)
            }
            .map_err(|e| e.to_string())
        },
        Host::shutdown_from,
    )
}

/// What `serve` does once its stop is watched: `setup` (the foreground mode's, or nothing),
/// then `start` (the host, given the browser listener setup held), and `shutdown` of what
/// started once the stop is asked for, from when it was asked for. A stop that comes before
/// `start` is called ends it there: nothing is started, and it exits 0. See the module docs.
fn serve_until_stopped<H>(
    stop: &Stop,
    setup: impl FnOnce() -> Result<Option<BrowserListener>, String>,
    start: impl FnOnce(Option<BrowserListener>) -> Result<H, String>,
    shutdown: impl FnOnce(H, Instant),
) -> Result<(), String> {
    let started = start_unless_stopped(stop, setup, start);
    // From here on the stop no longer bounds the start: it has ended, one way or another.
    stop.settle();
    let Some(host) = started.inspect_err(|_| stop.finish())? else {
        stop.finish();
        eprintln!(
            "agentic-org: stopped before serving: the stop signal came while starting; what \
             setup completed is kept, and the next start serves it"
        );
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

/// The unsigned development CLI peer the host's app policy admits. A debug build serving a
/// registered instance admits its own executable (setup and `admin` run the same binary)
/// unless the build names another one; the host logs it loudly. A release build keeps what
/// [`Config::trusted`] set (no development app), and its app policy has no development-app
/// rule at all (`AppPolicy::with_dev_app` and `Config::dev_app_path` exist only in debug
/// builds, so release code never calls this).
#[cfg_attr(not(debug_assertions), allow(dead_code))]
fn dev_cli_peer(
    build: Build,
    named: bool,
    built_in: Option<PathBuf>,
    current_exe: impl FnOnce() -> Option<PathBuf>,
) -> Option<PathBuf> {
    match (build, named, built_in) {
        (Build::Release, _, built_in) => built_in,
        (Build::Debug, true, None) => current_exe(),
        (Build::Debug, _, built_in) => built_in,
    }
}

/// A release build's config: only what it was built with.
#[cfg(not(debug_assertions))]
fn config(_args: Args) -> Result<Config, String> {
    Config::trusted()
}

/// A debug build's config: what it was built with, with the arguments' overrides and the
/// development settings.
#[cfg(debug_assertions)]
fn config(args: Args) -> Result<Config, String> {
    let home = match args.home {
        Some(home) => home,
        None => org_host::default_office_home()
            .ok_or("no office home: neither ORG_OFFICE_HOME nor HOME is an absolute path")?,
    };
    let server_bin = match args.server_bin {
        // Servers run with `/` as their working directory, so a relative path must be
        // resolved here, against ours.
        Some(path) => {
            std::path::absolute(&path).map_err(|e| format!("resolving {}: {e}", path.display()))?
        }
        None => org_host::default_server_program()
            .map_err(|e| format!("finding the office server: {e}"))?,
    };
    let mut config = Config::new(home, server_bin);
    config.set_app_team_id(
        args.app_team_id
            .or(org_host::APP_TEAM_ID.map(str::to_owned)),
    );
    // The development app, fixed when this binary was compiled.
    config.dev_app_path = option_env!("ORG_DEV_APP_PATH").map(PathBuf::from);
    // The development key folder, resolved against our working directory.
    config.dev_key_dir = match std::env::var_os("ORG_OFFICE_DEV_KEY_DIR") {
        Some(dir) if !dir.is_empty() => Some(
            std::path::absolute(&dir)
                .map_err(|e| format!("resolving ORG_OFFICE_DEV_KEY_DIR: {e}"))?,
        ),
        _ => None,
    };
    Ok(config)
}

/// Parses the arguments (without the program name) for a `build`. Pure: it reads nothing else
/// and exits nothing. A release build refuses `--home`, `--server-bin` and `--app-team-id`.
fn parse_args(args: impl IntoIterator<Item = String>, build: Build) -> Result<Args, String> {
    let mut parsed = Args::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--version" {
            parsed.version = true;
            continue;
        }
        if arg == "--name" {
            let value = args
                .next()
                .ok_or_else(|| format!("--name needs a value\n{USAGE}"))?;
            if parsed.name.replace(value).is_some() {
                return Err(format!("--name may be given once\n{USAGE}"));
            }
            continue;
        }
        if arg == "--setup" {
            let value = args
                .next()
                .ok_or_else(|| format!("--setup needs a file\n{USAGE}"))?;
            if parsed.setup.replace(PathBuf::from(value)).is_some() {
                return Err(format!("--setup may be given once\n{USAGE}"));
            }
            continue;
        }
        let known = ["--home", "--server-bin", "--app-team-id"];
        if !known.contains(&arg.as_str()) {
            return Err(format!("unexpected argument {arg:?}\n{USAGE}"));
        }
        if build == Build::Release {
            return Err(format!(
                "{arg} isn't accepted by a release build: it uses only the office home, office \
                 server and app team id it was built with"
            ));
        }
        let value = args
            .next()
            .ok_or_else(|| format!("{arg} needs a value\n{USAGE}"))?;
        match arg.as_str() {
            "--home" => parsed.home = Some(PathBuf::from(value)),
            "--server-bin" => parsed.server_bin = Some(PathBuf::from(value)),
            _ => parsed.app_team_id = Some(value),
        }
    }
    if (parsed.name.is_some() || parsed.setup.is_some()) && parsed.home.is_some() {
        return Err(format!(
            "--name and --setup can't be combined with --home: a registered instance has its \
             own home\n{USAGE}"
        ));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    /// A user who may not run the service is refused before anything is opened or made:
    /// no home, no key folder, no socket, whatever the arguments ask for. Asking for the
    /// version starts no service and is answered to anyone; an argument error is reported
    /// as that.
    #[cfg(debug_assertions)]
    #[test]
    fn a_user_who_may_not_run_the_service_is_refused_before_anything_is_made() {
        let dir = std::env::temp_dir().join(format!("org-serve-refusal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let home = dir.join("home");
        let refusal = || Err("the service does not run as this user".to_owned());
        for list in [
            vec![
                "--home",
                home.to_str().unwrap(),
                "--server-bin",
                "/nonexistent/worker",
            ],
            vec!["--name", "default"],
            vec![],
        ] {
            assert_eq!(
                run_as(args(&list), refusal),
                Err("the service does not run as this user".into()),
                "{list:?}"
            );
        }
        assert!(!dir.exists(), "something was made for a refused user");
        // Not a service start: answered whoever asks, and the check is not even made.
        let unasked = || -> Result<(), String> { panic!("asked who runs a version query") };
        assert_eq!(run_as(args(&["--version"]), unasked), Ok(()));
        let unasked = || -> Result<(), String> { panic!("asked before the arguments were read") };
        assert!(run_as(args(&["--frobnicate"]), unasked)
            .unwrap_err()
            .contains("unexpected argument"));
        // The composition's own check is the host's rule for this process's user.
        // SAFETY: no arguments and no side effects; it cannot fail.
        assert_eq!(
            Config::refuse_root().is_err(),
            unsafe { libc::geteuid() } == 0
        );
    }

    #[test]
    fn a_debug_build_takes_the_development_arguments() {
        let parsed = parse_args(
            args(&[
                "--home",
                "/tmp/h",
                "--server-bin",
                "srv",
                "--app-team-id",
                "AB12CD34EF",
            ]),
            Build::Debug,
        )
        .unwrap();
        assert_eq!(
            parsed,
            Args {
                version: false,
                name: None,
                setup: None,
                home: Some("/tmp/h".into()),
                server_bin: Some("srv".into()),
                app_team_id: Some("AB12CD34EF".into()),
            }
        );
        assert_eq!(
            parse_args(args(&[]), Build::Debug).unwrap(),
            Args::default()
        );
    }

    #[test]
    fn a_release_build_refuses_every_argument_that_changes_what_it_trusts() {
        for flag in ["--home", "--server-bin", "--app-team-id"] {
            for list in [
                vec![flag, "/tmp/x"],
                vec![flag],
                vec!["--version", flag, "x"],
            ] {
                let err = parse_args(args(&list), Build::Release).unwrap_err();
                assert!(err.contains(flag), "{list:?}: {err}");
                assert!(err.contains("release build"), "{list:?}: {err}");
            }
        }
        assert_eq!(
            parse_args(args(&[]), Build::Release).unwrap(),
            Args::default()
        );
    }

    #[test]
    fn version_works_in_both_builds_and_unknown_or_incomplete_arguments_are_refused() {
        for build in [Build::Debug, Build::Release] {
            assert!(parse_args(args(&["--version"]), build).unwrap().version);
            let err = parse_args(args(&["--frobnicate"]), build).unwrap_err();
            assert!(err.contains("unexpected argument"), "{err}");
        }
        let err = parse_args(args(&["--home"]), Build::Debug).unwrap_err();
        assert!(err.contains("needs a value"), "{err}");
    }

    #[test]
    fn both_builds_take_one_name_which_excludes_home() {
        for build in [Build::Debug, Build::Release] {
            let parsed = parse_args(args(&["--name", "office"]), build).unwrap();
            assert_eq!(parsed.name.as_deref(), Some("office"));
            let err = parse_args(args(&["--name"]), build).unwrap_err();
            assert!(err.contains("needs a value"), "{err}");
            let err = parse_args(args(&["--name", "a", "--name", "b"]), build).unwrap_err();
            assert!(err.contains("once"), "{err}");
        }
        for list in [
            ["--name", "a", "--home", "/tmp/h"],
            ["--home", "/tmp/h", "--name", "a"],
        ] {
            let err = parse_args(args(&list), Build::Debug).unwrap_err();
            assert!(err.contains("can't be combined"), "{err}");
        }
        let parsed =
            parse_args(args(&["--name", "a", "--server-bin", "srv"]), Build::Debug).unwrap();
        assert_eq!(parsed.server_bin, Some("srv".into()));
    }

    #[test]
    fn both_builds_take_one_setup_file_with_or_without_a_name_and_never_with_home() {
        for build in [Build::Debug, Build::Release] {
            let parsed = parse_args(args(&["--setup", "/etc/s.json"]), build).unwrap();
            assert_eq!(parsed.setup, Some("/etc/s.json".into()));
            assert_eq!(parsed.name, None);
            let parsed =
                parse_args(args(&["--name", "lab", "--setup", "/etc/s.json"]), build).unwrap();
            assert_eq!(
                (parsed.name.as_deref(), parsed.setup),
                (Some("lab"), Some("/etc/s.json".into()))
            );
            let err = parse_args(args(&["--setup"]), build).unwrap_err();
            assert!(err.contains("needs a file"), "{err}");
            let err = parse_args(args(&["--setup", "a", "--setup", "b"]), build).unwrap_err();
            assert!(err.contains("once"), "{err}");
        }
        let err =
            parse_args(args(&["--setup", "/s", "--home", "/tmp/h"]), Build::Debug).unwrap_err();
        assert!(err.contains("can't be combined"), "{err}");
    }

    #[test]
    fn only_a_debug_build_serving_a_named_instance_admits_its_own_executable() {
        let own = || Some(PathBuf::from("/dev/build/agentic-org"));
        let built_in = PathBuf::from("/dev/App.app/Contents/MacOS/App");
        // A release build never admits a development CLI peer, named or not.
        assert_eq!(dev_cli_peer(Build::Release, true, None, own), None);
        assert_eq!(dev_cli_peer(Build::Release, false, None, own), None);
        assert_eq!(
            dev_cli_peer(Build::Release, true, None, || -> Option<PathBuf> {
                panic!("a release build never asks for its own executable")
            }),
            None
        );
        // A debug build serving a registered instance admits itself unless the build names
        // another development app; the legacy home entry admits only the built-in one.
        assert_eq!(dev_cli_peer(Build::Debug, true, None, own), own());
        assert_eq!(
            dev_cli_peer(Build::Debug, true, Some(built_in.clone()), own),
            Some(built_in.clone())
        );
        assert_eq!(dev_cli_peer(Build::Debug, false, None, own), None);
        assert_eq!(
            dev_cli_peer(Build::Debug, false, Some(built_in.clone()), own),
            Some(built_in)
        );
    }

    /// A stop watched with a signal the test sends (`send`), and the overruns it recorded:
    /// the signal's number and when.
    #[allow(clippy::type_complexity)]
    fn watched(
        bound: Duration,
    ) -> (
        Stop,
        std::sync::mpsc::Sender<i32>,
        Arc<Mutex<Vec<(i32, Instant)>>>,
    ) {
        let (send, signal) = std::sync::mpsc::channel::<i32>();
        let overruns = Arc::new(Mutex::new(Vec::new()));
        let recording = Arc::clone(&overruns);
        let stop = Stop::watch(
            move || signal.recv().map_err(std::io::Error::other),
            bound,
            Duration::from_secs(30),
            move |signal, _| recording.lock().unwrap().push((signal, Instant::now())),
        )
        .unwrap();
        (stop, send, overruns)
    }

    /// Sends the stop and returns once the stop's thread has taken it, with a moment before.
    fn stop_now(stop: &Stop, send: &std::sync::mpsc::Sender<i32>) -> Instant {
        let before = Instant::now();
        send.send(libc::SIGTERM).unwrap();
        while !stop.asked().unwrap() {
            std::thread::sleep(Duration::from_millis(1));
        }
        before
    }

    /// X5 review, third round, P1: a stop that comes while the instance is set up is taken
    /// at once, and `serve` ends when setup returns, without starting the host: exit 0.
    #[test]
    fn a_stop_during_setup_ends_serve_without_starting_the_host() {
        let (stop, send, overruns) = watched(Duration::from_secs(10));
        let result = serve_until_stopped(
            &stop,
            || {
                stop_now(&stop, &send);
                Ok(None)
            },
            |_| -> Result<(), String> { panic!("the host started after the stop") },
            |_, _| panic!("nothing started, so nothing is shut down"),
        );
        assert_eq!(result, Ok(()));
        assert!(overruns.lock().unwrap().is_empty());
    }

    /// X5 review, third round, P1: a stop that comes while the host starts stops it as soon
    /// as its start returns, with the stop's deadline fixed from the signal, not from then.
    #[test]
    fn a_stop_during_the_start_stops_the_host_from_when_it_was_asked() {
        let (stop, send, overruns) = watched(Duration::from_secs(10));
        let shut = Mutex::new(None);
        let result = serve_until_stopped(
            &stop,
            || Ok(None),
            |_| {
                let before = stop_now(&stop, &send);
                std::thread::sleep(Duration::from_millis(200));
                Ok((before, Instant::now()))
            },
            |(before, started), asked| *shut.lock().unwrap() = Some((before, started, asked)),
        );
        assert_eq!(result, Ok(()));
        let (before, started, asked) = shut.lock().unwrap().expect("the host was shut down");
        assert!(asked >= before, "asked before the signal was sent");
        assert!(
            started.duration_since(asked) >= Duration::from_millis(200),
            "the stop was dated from the end of the start, not from the signal"
        );
        assert!(overruns.lock().unwrap().is_empty());
    }

    /// Waiting for the signal failing is an error, before the start as after it: `serve`
    /// stops what it started and says why, and never takes it for a stop that ends with 0.
    #[test]
    fn a_failed_wait_for_the_signal_is_an_error_not_a_stop() {
        // The wait fails once `fail` is sent.
        let failing = || {
            let (fail, failed) = std::sync::mpsc::channel::<()>();
            let stop = Stop::watch(
                move || {
                    let _ = failed.recv();
                    Err(std::io::Error::other("no signals here"))
                },
                Duration::from_secs(10),
                Duration::from_secs(10),
                |_, _| panic!("no signal was taken, so nothing overran"),
            )
            .unwrap();
            (stop, fail)
        };
        let (stop, fail) = failing();
        fail.send(()).unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        while stop.asked().is_ok() {
            assert!(Instant::now() < until, "the failed wait was not reported");
            std::thread::sleep(Duration::from_millis(1));
        }
        let result = serve_until_stopped(
            &stop,
            || panic!("set up after the wait failed"),
            |_| -> Result<(), String> { panic!("the host started after the wait failed") },
            |_, _| panic!("nothing started, so nothing is shut down"),
        );
        assert_eq!(
            result,
            Err("waiting for a signal: no signals here".to_owned())
        );

        // Found once the host serves: it is shut down, and the error is returned.
        let (stop, fail) = failing();
        let shut = std::cell::Cell::new(false);
        let result = serve_until_stopped(
            &stop,
            || Ok(None),
            |_| {
                fail.send(()).unwrap();
                Ok(())
            },
            |(), _| shut.set(true),
        );
        assert!(shut.get(), "the host was left running");
        assert_eq!(
            result,
            Err("waiting for a signal: no signals here".to_owned())
        );
    }

    /// X5 review, third round, P1: a start that has not ended `bound` after the stop is
    /// ended there (the composition's overrun exits the process); one that ends in time is
    /// not, and neither is a host that serves when a stop comes after its start.
    #[test]
    fn a_start_that_outlasts_the_stop_s_bound_is_ended_at_the_bound() {
        let bound = Duration::from_millis(200);
        let (stop, send, overruns) = watched(bound);
        let mut asked = None;
        let result = serve_until_stopped(
            &stop,
            || {
                asked = Some(stop_now(&stop, &send));
                // Setup that does not end (a write that waits for a slow volume, say).
                std::thread::sleep(Duration::from_millis(800));
                Ok(None)
            },
            |_| -> Result<(), String> { panic!("the host started after the stop") },
            |_, _| panic!("nothing started, so nothing is shut down"),
        );
        assert_eq!(result, Ok(()));
        let asked = asked.unwrap();
        let overruns = overruns.lock().unwrap().clone();
        let [(signal, at)] = overruns[..] else {
            panic!("one overrun, not {overruns:?}");
        };
        assert_eq!(signal, libc::SIGTERM);
        let after = at.duration_since(asked);
        assert!(after >= bound, "{after:?}");
        assert!(after < Duration::from_millis(600), "{after:?}");

        // A host that started before the stop: the stop is not a start's, and nothing ends
        // it at the bound.
        let (stop, send, overruns) = watched(Duration::from_millis(50));
        let stopping = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            send.send(libc::SIGTERM).unwrap();
        });
        let result = serve_until_stopped(
            &stop,
            || Ok(None),
            |_| Ok(()),
            |(), _| std::thread::sleep(Duration::from_millis(200)),
        );
        assert_eq!(result, Ok(()));
        stopping.join().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(overruns.lock().unwrap().is_empty());
    }
}
