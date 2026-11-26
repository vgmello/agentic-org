//! `serve`: its command line, its refusals and its bounded stop.
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use org_host::stop::Stop;
use org_host::Config;

use super::args::{dev_cli_peer, parse_args, Args, Build};
use super::run::{run_as, serve_until_stopped, Served};

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
    assert_eq!(run_as(args(&["--version"]), unasked), Ok(Served::Version));
    let unasked = || -> Result<(), String> { panic!("asked before the arguments were read") };
    assert!(run_as(args(&["--frobnicate"]), unasked)
        .unwrap_err()
        .contains("unexpected argument"));
    // The composition's own check is the host's rule for this process's user.
    assert_eq!(
        Config::refuse_root().is_err(),
        org_socket::effective_uid() == 0
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
    let parsed = parse_args(args(&["--name", "a", "--server-bin", "srv"]), Build::Debug).unwrap();
    assert_eq!(parsed.server_bin, Some("srv".into()));
}

#[test]
fn both_builds_take_one_setup_file_with_or_without_a_name_and_never_with_home() {
    for build in [Build::Debug, Build::Release] {
        let parsed = parse_args(args(&["--setup", "/etc/s.json"]), build).unwrap();
        assert_eq!(parsed.setup, Some("/etc/s.json".into()));
        assert_eq!(parsed.name, None);
        let parsed = parse_args(args(&["--name", "lab", "--setup", "/etc/s.json"]), build).unwrap();
        assert_eq!(
            (parsed.name.as_deref(), parsed.setup),
            (Some("lab"), Some("/etc/s.json".into()))
        );
        let err = parse_args(args(&["--setup"]), build).unwrap_err();
        assert!(err.contains("needs a file"), "{err}");
        let err = parse_args(args(&["--setup", "a", "--setup", "b"]), build).unwrap_err();
        assert!(err.contains("once"), "{err}");
    }
    let err = parse_args(args(&["--setup", "/s", "--home", "/tmp/h"]), Build::Debug).unwrap_err();
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
#[expect(
    clippy::type_complexity,
    reason = "test helper returns stop guard, signal sender, and recorded overrun log"
)]
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

const FULL_STDERR_CHILD: &str = "AGENTIC_ORG_TEST_SERVE_FULL_STDERR";

/// Runs this test binary's test `test` alone in a child process whose stderr is a socket
/// that is full and never read, with [`FULL_STDERR_CHILD`] set to `case`, and returns
/// its exit status and stdout, or `None` if it had not ended within 20 s (it is then
/// killed). A write to its stderr blocks for as long as it lives.
fn with_full_stderr(test: &str, case: &str) -> Option<(std::process::ExitStatus, String)> {
    let (reader, mut full) = std::os::unix::net::UnixStream::pair().unwrap();
    full.set_nonblocking(true).unwrap();
    loop {
        match full.write(&[b'x'; 4096]) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(e) => panic!("filling the socket: {e}"),
        }
    }
    // Blocking again (the child shares this open file), and full.
    full.set_nonblocking(false).unwrap();
    let full = std::os::fd::OwnedFd::from(full);
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env(FULL_STDERR_CHILD, case)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(full)
        .spawn()
        .unwrap();
    let until = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut stdout = String::new();
    let _ = std::io::Read::read_to_string(&mut child.stdout.take().unwrap(), &mut stdout);
    drop(reader);
    status.map(|status| (status, stdout))
}

/// X5 final review: a stop during setup is not held past its bound by a standard error
/// that is a full pipe nobody reads. The line that says it stopped before serving is
/// written through the host log, which waits for it a bounded time; and an error from
/// setup is returned with the stop still bounded, so that when `main` blocks writing it,
/// the stop's overrun ends the process (here at 300 ms; 2.4 s in `serve`).
#[test]
fn a_stop_during_setup_is_not_held_up_by_a_full_stderr() {
    const TEST: &str =
        "features::serve::tests::a_stop_during_setup_is_not_held_up_by_a_full_stderr";
    if let Some(case) = std::env::var_os(FULL_STDERR_CHILD) {
        let (send, signal) = std::sync::mpsc::channel::<i32>();
        let stop = Stop::watch(
            move || signal.recv().map_err(std::io::Error::other),
            Duration::from_secs(10),
            Duration::from_millis(300),
            |_, _| org_host::stop::end_process("the stop overran"),
        )
        .unwrap();
        let failed = case == "error";
        let result = serve_until_stopped(
            &stop,
            || {
                stop_now(&stop, &send);
                if failed {
                    Err("setup failed".to_owned())
                } else {
                    Ok(None)
                }
            },
            |_| -> Result<(), String> { panic!("the host started after the stop") },
            |_, _| panic!("nothing started, so nothing is shut down"),
        );
        let _ = writeln!(std::io::stdout(), "returned {result:?}");
        if let Err(message) = result {
            // What `main` does with it: a write that blocks while stderr is full.
            let _ = writeln!(std::io::stderr(), "agentic-org: {message}");
            let _ = writeln!(std::io::stdout(), "the error was written");
        }
        return;
    }
    let (status, stdout) =
        with_full_stderr(TEST, "stopped").expect("the stop before serving blocked");
    assert!(status.success(), "{status:?} {stdout}");
    assert!(stdout.contains("returned Ok(())"), "{stdout}");
    let (status, stdout) =
        with_full_stderr(TEST, "error").expect("the error after the stop blocked");
    assert!(status.success(), "{status:?} {stdout}");
    assert!(
        stdout.contains("returned Err(\"setup failed\")"),
        "{stdout}"
    );
    assert!(!stdout.contains("the error was written"), "{stdout}");
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
