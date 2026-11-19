//! Owned stopping and cleanup; every child and filesystem root is disposable.
#[path = "support/core_limit.rs"]
mod core_limit;
use std::fs::File;
use std::io::{self, BufRead, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use org_supervise::{
    ChildStdin, Environment, OwnedChild, PreparedSpawn, SearchPath, SessionMark, SignalTarget,
    SocketCleanup, SpawnSpec, Spawned, StopAction, StopMode, StopPolicy, StopSignal, StopStep,
};

fn private_tempdir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}

fn environment(root: &Path) -> Environment {
    Environment::new(
        SearchPath::new(
            Path::new("/usr/bin"),
            "/usr/bin:/bin".as_ref(),
            &[root.to_owned()],
        )
        .unwrap(),
        &[("HOME".into(), root.into()), ("TMPDIR".into(), root.into())],
    )
    .unwrap()
}

fn mark() -> SessionMark {
    let mut bytes = [0; 16];
    File::open("/dev/urandom")
        .unwrap()
        .read_exact(&mut bytes)
        .unwrap();
    SessionMark::new(format!("lifecycle-{:032x}", u128::from_ne_bytes(bytes))).unwrap()
}

fn shell(root: &Path, script: &str) -> Spawned {
    PreparedSpawn::new(
        SpawnSpec {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            cwd: root.to_owned(),
            environment: environment(root),
        },
        mark(),
    )
    .unwrap()
    .record_mark(|_| Ok(()))
    .unwrap()
    .spawn()
    .unwrap()
    .record_identity(|_| Ok(()))
    .unwrap()
}

// Readiness and output drains have independent deadlines; the owner kills/reaps on panic.
fn pump(spawned: Spawned) -> (OwnedChild, ChildStdin, mpsc::Receiver<io::Result<String>>) {
    let Spawned {
        child,
        stdin,
        stdout,
        stderr,
    } = spawned;
    let (ready_sender, ready) = mpsc::channel();
    let (sender, output) = mpsc::channel();
    let errors = sender.clone();
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = errors.send(
            std::io::BufReader::new(stderr)
                .read_to_string(&mut text)
                .map(|_| text),
        );
    });
    std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(stdout);
        let mut line = String::new();
        let mut text = String::new();
        let result = loop {
            match reader.read_line(&mut line) {
                Ok(0) => break Ok(text),
                Ok(_) => {
                    if line.contains("READY") {
                        let _ = ready_sender.send(());
                    }
                    text.push_str(&line);
                    line.clear();
                }
                Err(error) => break Err(error),
            }
        };
        let _ = sender.send(result);
    });
    ready
        .recv_timeout(Duration::from_secs(10))
        .expect("fixture readiness deadline");
    (child, stdin, output)
}

fn drained(output: mpsc::Receiver<io::Result<String>>) -> String {
    let mut all = String::new();
    for _ in 0..2 {
        all.push_str(
            &output
                .recv_timeout(Duration::from_secs(10))
                .expect("pipe drain deadline")
                .unwrap(),
        );
    }
    all
}

#[test]
fn eof_grace_keeps_the_pump_draining_full_pipes_and_preserves_final_output() {
    let root = private_tempdir();
    let real = root.path().canonicalize().unwrap();
    // More output than either pipe can hold, before the child reads stdin.
    let spawned = PreparedSpawn::new(
        SpawnSpec {
            program: std::env::current_exe().unwrap(),
            args: vec!["--exact".into(), "flood_child".into(), "--nocapture".into()],
            cwd: real.clone(),
            environment: environment(&real)
                .with_context(&[("ORG_TEST_CASE".into(), "flood".into())])
                .unwrap(),
        },
        mark(),
    )
    .unwrap()
    .record_mark(|_| Ok(()))
    .unwrap()
    .spawn()
    .unwrap()
    .record_identity(|_| Ok(()))
    .unwrap();
    let (mut child, mut stdin, output) = pump(spawned);
    let mut clone = stdin.clone();
    let report = child
        .stop(
            &stdin,
            &StopPolicy::new(vec![StopStep::CloseStdin {
                grace: Duration::from_secs(10),
            }])
            .unwrap(),
            StopMode::Graceful,
        )
        .unwrap();
    assert!(report.status.success());
    assert_eq!(
        report.actions,
        [StopAction::StdinClosed, StopAction::GroupKillAttempted]
    );
    assert_eq!(
        stdin.write(b"late").unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(
        clone.write(b"late").unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(child.try_exit().unwrap(), Some(report.status));
    assert!(child.cleanup_and_reap(&[]).unwrap().status.success());
    assert!(drained(output).contains("FINAL-AFTER-EOF"));
}

#[test]
fn flood_child() {
    if std::env::var("ORG_TEST_CASE").as_deref() != Ok("flood") {
        return;
    }
    println!("READY");
    let bytes = vec![b'x'; 1024 * 1024];
    io::stdout().write_all(&bytes).unwrap();
    io::stdout().write_all(b"\n").unwrap();
    io::stderr().write_all(&bytes).unwrap();
    io::stdin().read_to_end(&mut Vec::new()).unwrap();
    println!("FINAL-AFTER-EOF");
}

#[test]
fn requested_interrupt_follows_leader_term_and_its_full_grace() {
    let root = private_tempdir();
    let (mut child, stdin, output) = pump(shell(
        &root.path().canonicalize().unwrap(),
        "trap 'echo TERM' TERM; trap 'echo INT; exit 0' INT; echo READY; while :; do read line; done",
    ));
    let grace = Duration::from_secs(5);
    let policy = org_supervise::runtime::stop_policy(&org_runtime::StopPlan {
        steps: vec![org_runtime::StopStep::Signal {
            signal: org_runtime::StopSignal::Interrupt,
            target: org_runtime::SignalTarget::Group,
            grace_ms: 5000,
        }],
    })
    .unwrap();
    let start = Instant::now();
    let report = child.stop(&stdin, &policy, StopMode::Graceful).unwrap();
    assert!(start.elapsed() >= grace);
    assert!(report.status.success());
    assert_eq!(
        report.actions,
        [
            StopAction::SignalAttempted {
                signal: StopSignal::Terminate,
                target: SignalTarget::Leader
            },
            StopAction::SignalAttempted {
                signal: StopSignal::Interrupt,
                target: SignalTarget::Group
            },
            StopAction::GroupKillAttempted
        ]
    );
    child.cleanup_and_reap(&[]).unwrap();
    let text = drained(output);
    assert!(text.contains("TERM") && text.contains("INT"), "{text}");
}

#[test]
fn fast_stop_skips_stdin_waits_and_kills_after_five_seconds_of_leader_term() {
    let root = private_tempdir();
    let (mut child, stdin, output) = pump(shell(
        &root.path().canonicalize().unwrap(),
        "trap '' TERM; echo READY; while :; do read line; done",
    ));
    let policy = StopPolicy::new(vec![StopStep::CloseStdin {
        grace: Duration::from_secs(60),
    }])
    .unwrap();
    let start = Instant::now();
    let report = child.stop(&stdin, &policy, StopMode::Fast).unwrap();
    assert!(start.elapsed() >= Duration::from_secs(5));
    assert_eq!(report.status.signal(), Some(libc::SIGKILL));
    assert_eq!(
        report.actions,
        [
            StopAction::SignalAttempted {
                signal: StopSignal::Terminate,
                target: SignalTarget::Leader
            },
            StopAction::GroupKillAttempted
        ]
    );
    child.cleanup_and_reap(&[]).unwrap();
    drained(output);
}

#[test]
fn socket_failure_keeps_the_zombie_and_reports_partial_cleanup_until_retry() {
    let root = private_tempdir();
    let real = root.path().canonicalize().unwrap();
    let mut spawned = shell(&real, "exit 17");
    let pid = spawned.child.identity().pid;
    let good = real.join(format!("{pid}.sock"));
    let invalid = real.join(format!("{pid}.keep"));
    let _listener = UnixListener::bind(&good).unwrap();
    std::fs::write(&invalid, b"never unlink").unwrap();
    let port_root = org_runtime::SpecPath::try_from(real.to_str().unwrap()).unwrap();
    let paths = org_supervise::runtime::socket_cleanup(&[
        org_runtime::CleanupPath::new(
            port_root.clone(),
            &format!("{}/{{pid}}.sock", real.display()),
        )
        .unwrap(),
        org_runtime::CleanupPath::new(port_root, &format!("{}/{{pid}}.keep", real.display()))
            .unwrap(),
    ])
    .unwrap();
    let status = spawned
        .child
        .wait_until(Instant::now() + Duration::from_secs(10))
        .unwrap()
        .unwrap();
    let error = spawned.child.cleanup_and_reap(&paths).unwrap_err();
    assert!(error.owner_retained);
    assert_eq!(error.removed, std::slice::from_ref(&good));
    assert_eq!(error.unresolved, std::slice::from_ref(&invalid));
    assert_eq!(std::fs::read(&invalid).unwrap(), b"never unlink");
    assert_eq!(spawned.child.try_exit().unwrap(), Some(status));
    std::fs::remove_file(&invalid).unwrap();
    let result = spawned.child.cleanup_and_reap(&paths).unwrap();
    assert_eq!(result.status.code(), Some(17));
    assert_eq!(result.absent, [good, invalid]);
    assert_eq!(
        spawned.child.try_exit().unwrap_err().raw_os_error(),
        Some(libc::ECHILD)
    );
}

#[test]
fn premature_cleanup_retains_a_live_child_and_external_reaping_disarms_signals() {
    let root = private_tempdir();
    let (mut child, stdin, output) = pump(shell(
        &root.path().canonicalize().unwrap(),
        "echo READY; cat >/dev/null; exit 0",
    ));
    let error = child.cleanup_and_reap(&[]).unwrap_err();
    assert!(error.owner_retained);
    assert_eq!(error.error.kind(), io::ErrorKind::WouldBlock);
    stdin.close().unwrap();
    child
        .wait_until(Instant::now() + Duration::from_secs(10))
        .unwrap()
        .unwrap();
    // SAFETY: deliberate contract violation confined to this test's exited child.
    assert_eq!(
        unsafe { libc::waitpid(child.identity().pid as i32, std::ptr::null_mut(), 0) },
        child.identity().pid as i32
    );
    let error = child
        .stop(&stdin, &StopPolicy::default(), StopMode::Fast)
        .unwrap_err();
    assert!(!error.owner_retained);
    assert!(error.actions.is_empty());
    assert_eq!(error.error.raw_os_error(), Some(libc::ECHILD));
    assert_eq!(
        child.terminate().unwrap_err().raw_os_error(),
        Some(libc::ECHILD)
    );
    assert!(!child.cleanup_and_reap(&[]).unwrap_err().owner_retained);
    drained(output);
}

#[test]
fn a_nonprivate_socket_parent_retains_the_owner_without_chmod_or_unlink() {
    let root = private_tempdir();
    let real = root.path().canonicalize().unwrap();
    let mut spawned = shell(&real, "exit 0");
    let path = real.join(format!("{}.sock", spawned.child.identity().pid));
    let _listener = UnixListener::bind(&path).unwrap();
    spawned
        .child
        .wait_until(Instant::now() + Duration::from_secs(10))
        .unwrap()
        .unwrap();
    let template = SocketCleanup::new(real.clone(), "{pid}.sock".into()).unwrap();
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o770)).unwrap();
    let error = spawned
        .child
        .cleanup_and_reap(std::slice::from_ref(&template))
        .unwrap_err();
    assert!(error.owner_retained);
    assert!(path.exists());
    assert_eq!(
        std::fs::metadata(&real).unwrap().permissions().mode() & 0o7777,
        0o770
    );
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o700)).unwrap();
    spawned.child.cleanup_and_reap(&[template]).unwrap();
}

fn probe(
    root: &Path,
    mode: org_supervise::runtime::ProbeMode,
    env: Environment,
) -> io::Result<org_supervise::runtime::RuntimeProbe> {
    let program = std::env::current_exe().unwrap();
    org_supervise::runtime::prepare_probe(
        org_runtime::ProbePlan {
            program: program.clone(),
            args: vec!["--exact".into(), "probe_child".into(), "--nocapture".into()],
            env_add: vec![],
            stdin_lines: vec![org_runtime::encode_line("hello").unwrap()],
            timeout_ms: 10000,
        },
        &org_runtime::ProbeContext::new(program),
        env,
        mark(),
        root,
        mode,
    )
}

#[test]
fn probe_child() {
    if std::env::var("TERM").as_deref() != Ok("org-supervise-probe-fixture") {
        return;
    }
    let cwd = std::env::current_dir().unwrap();
    assert!(cwd
        .file_name()
        .unwrap()
        .as_encoded_bytes()
        .starts_with(b"probe-"));
    assert!(std::fs::read_dir(&cwd).unwrap().next().is_none());
    assert!(std::env::var_os("ORG_AGENT").is_none());
    assert!(std::env::var_os("ORG_SESSION_TOKEN").is_none());
    assert!(std::env::var_os("ORG_SESSION_MARK").is_some());
    std::fs::write(".probe-dotfile", b"private").unwrap();
    println!("READY");
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).unwrap();
    assert_eq!(input, "\"hello\"\n");
    println!("PROBE-DONE");
}

#[test]
fn probe_uses_fresh_private_cwd_and_caller_io_then_removes_its_files_before_reap() {
    let root = private_tempdir();
    let real = root.path().canonicalize().unwrap();
    let env = Environment::new(
        SearchPath::new(Path::new("/usr/bin"), "/usr/bin:/bin".as_ref(), &[]).unwrap(),
        &[
            ("HOME".into(), real.clone().into()),
            ("TMPDIR".into(), real.clone().into()),
            ("TERM".into(), "org-supervise-probe-fixture".into()),
        ],
    )
    .unwrap();
    let probe = probe(&real, org_supervise::runtime::ProbeMode::OneShot, env).unwrap();
    assert_eq!(probe.mode, org_supervise::runtime::ProbeMode::OneShot);
    assert_eq!(probe.timeout, Duration::from_secs(10));
    let cwd = probe.cwd.clone();
    assert!(cwd.starts_with(&real));
    assert_eq!(
        std::fs::metadata(&cwd).unwrap().permissions().mode() & 0o7777,
        0o700
    );
    let (mut child, mut stdin, output) = pump(
        probe
            .prepared
            .record_mark(|_| Ok(()))
            .unwrap()
            .spawn()
            .unwrap()
            .record_identity(|_| Ok(()))
            .unwrap(),
    );
    for line in probe.stdin_lines {
        stdin.write_all(line.as_bytes()).unwrap();
    }
    stdin.close().unwrap();
    child
        .wait_until(Instant::now() + Duration::from_secs(10))
        .unwrap()
        .unwrap();
    assert!(cwd.join(".probe-dotfile").exists());
    std::fs::set_permissions(&cwd, std::fs::Permissions::from_mode(0o750)).unwrap();
    let error = child.cleanup_and_reap(&[]).unwrap_err();
    assert!(error.owner_retained);
    assert_eq!(error.unresolved, std::slice::from_ref(&cwd));
    assert!(cwd.exists());
    assert!(child.try_exit().unwrap().is_some());
    std::fs::set_permissions(&cwd, std::fs::Permissions::from_mode(0o700)).unwrap();
    let report = child.cleanup_and_reap(&[]).unwrap();
    assert_eq!(report.removed, std::slice::from_ref(&cwd));
    assert!(!cwd.exists());
    assert!(drained(output).contains("PROBE-DONE"));
    assert!(std::fs::read_dir(&real).unwrap().next().is_none());
}

#[test]
fn failed_probe_identity_recording_removes_the_owned_cwd_and_reaps_the_child() {
    let root = private_tempdir();
    let real = root.path().canonicalize().unwrap();
    let probe = probe(
        &real,
        org_supervise::runtime::ProbeMode::OneShot,
        environment(&real),
    )
    .unwrap();
    let cwd = probe.cwd.clone();
    let pending = probe
        .prepared
        .record_mark(|_| Ok(()))
        .unwrap()
        .spawn()
        .unwrap();
    let pid = pending.identity().pid as i32;
    assert!(pending
        .record_identity(|_| Err(io::Error::other("identity persistence refused")))
        .is_err());
    assert!(!cwd.exists());
    // SAFETY: only the child whose failed persistence guard must have reaped it.
    assert_eq!(
        unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) },
        -1
    );
    assert_eq!(
        io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}

#[test]
fn probe_context_is_refused_and_abandoned_preparation_cleans_only_its_fresh_directory() {
    let root = private_tempdir();
    let real = root.path().canonicalize().unwrap();
    std::fs::write(real.join("keep"), b"keep").unwrap();
    let mode = org_supervise::runtime::ProbeMode::Conversation;
    // Sharing the namespace classifier must not broaden the caller's precise
    // Git allowlist or expose the recorded-spawn path's internal mark.
    for name in [
        "GIT_CONFIG_SYSTEM",
        "GIT_CONFIG_KEY_x",
        "GIT_CONFIG_VALUE_",
        "GH_CONFIG_OTHER",
        "ORG_SESSION_MARK",
    ] {
        assert!(
            environment(&real)
                .with_context(&[(name.into(), "refused".into())])
                .is_err(),
            "{name}"
        );
    }
    for name in [
        "ORG_AGENT",
        "ORG_SESSION_TOKEN",
        "ORG_OFFICE",
        "ORG_OTHER",
        "GH_CONFIG_DIR",
        "GIT_CONFIG_NOSYSTEM",
        "GIT_CONFIG_GLOBAL",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_KEY_0",
        "GIT_CONFIG_VALUE_0",
    ] {
        let env = environment(&real)
            .with_context(&[(name.into(), "must-not-reach-probe".into())])
            .unwrap();
        assert!(probe(&real, mode, env).is_err());
        assert_eq!(std::fs::read_dir(&real).unwrap().count(), 1);
    }
    let prepared = probe(&real, mode, environment(&real)).unwrap();
    assert_eq!(prepared.mode, mode);
    let cwd = prepared.cwd.clone();
    std::fs::write(cwd.join(".owned"), b"owned").unwrap();
    let outside = private_tempdir();
    std::fs::write(outside.path().join("keep"), b"untouched").unwrap();
    std::os::unix::fs::symlink(outside.path(), cwd.join("link")).unwrap();
    let error = prepared
        .prepared
        .record_mark(|_| Err(io::Error::other("persistence refused")));
    assert!(error.is_err());
    assert!(!cwd.exists());
    assert_eq!(
        std::fs::read(outside.path().join("keep")).unwrap(),
        b"untouched"
    );
    assert_eq!(std::fs::read(real.join("keep")).unwrap(), b"keep");
}

#[cfg(target_os = "linux")]
#[test]
fn linux_owned_zombie_group_keeps_success_or_esrch_signalling_behavior() {
    let root = private_tempdir();
    let mut spawned = shell(&root.path().canonicalize().unwrap(), "exit 17");
    spawned
        .child
        .wait_until(Instant::now() + Duration::from_secs(10))
        .unwrap()
        .unwrap();
    // SAFETY: still-unreaped owned leader reserves this private group ID.
    let result = unsafe { libc::killpg(spawned.child.identity().pgid as i32, libc::SIGKILL) };
    assert!(result == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH));
    assert_eq!(
        spawned.child.cleanup_and_reap(&[]).unwrap().status.code(),
        Some(17)
    );
}
