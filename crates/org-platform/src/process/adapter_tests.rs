//! Tests of the contract that need a real adapter (macOS or GNU/Linux).

use std::ffi::{CStr, CString};
use std::io::{self, Read, Write};
use std::process::ExitStatus;
use std::time::{Duration, Instant};

use super::{
    check_time, identity, native_mark, release_spawned, spawn_piped, try_exit, DeadlineElapsed,
    MarkEvidence, Watch,
};

const MARK: &[u8] = b"ORG_SESSION_MARK=";

/// A real child of the test process, killed and reaped when dropped.
pub(super) struct TestChild {
    pub(super) pid: libc::pid_t,
}

impl TestChild {
    /// Start `program` as [`spawn_piped`] leaves it, with the parent's pipe ends.
    pub(super) fn spawn_piped(
        program: &CStr,
        args: &[&CStr],
    ) -> (Self, std::fs::File, std::fs::File, std::fs::File) {
        let mut argv = vec![CString::from(program)];
        argv.extend(args.iter().map(|arg| CString::from(*arg)));
        let (pid, stdin, stdout, stderr) =
            spawn_piped(program, &argv, &[c"PATH=/usr/bin:/bin".into()], c"/").unwrap();
        (Self { pid }, stdin, stdout, stderr)
    }

    /// The exit status, without reaping, once the child has exited.
    fn exit(&self) -> Option<ExitStatus> {
        loop {
            match try_exit(self.pid) {
                Ok(status) => return status,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => panic!("the test's own child cannot be observed: {error}"),
            }
        }
    }
}

impl Drop for TestChild {
    fn drop(&mut self) {
        // SAFETY: this pid is the test's own unreaped child and cannot have been
        // reused. It is killed and reaped exactly once.
        unsafe {
            libc::kill(self.pid, libc::SIGKILL);
            while libc::waitpid(self.pid, std::ptr::null_mut(), 0) < 0
                && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted
            {}
        }
    }
}

#[test]
fn a_piped_child_leads_its_session_and_is_watched_without_being_reaped() {
    let (child, mut stdin, mut stdout, _stderr) = TestChild::spawn_piped(
        c"/bin/sh",
        &[c"-c", c"read line; echo \"$line from $PWD\"; exit 7"],
    );
    let (group, started, boot) = identity(child.pid).unwrap();
    assert_eq!(group, child.pid as u32, "the child leads its own group");
    assert!(!started.is_empty() && !boot.is_empty());
    let watch = Watch::new(child.pid).unwrap();
    assert!(child.exit().is_none());
    release_spawned(child.pid, || Ok(child.exit().is_some())).unwrap();
    stdin.write_all(b"hello\n").unwrap();

    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(status) = child.exit() {
            break status;
        }
        assert!(Instant::now() < deadline, "the child did not exit");
        watch.wait(deadline).unwrap();
    };
    assert_eq!(status.code(), Some(7));
    // Observation left the zombie in place: the same exit is still there to see.
    assert_eq!(child.exit().and_then(|status| status.code()), Some(7));
    let mut output = String::new();
    stdout.read_to_string(&mut output).unwrap();
    assert_eq!(output, "hello from /\n");
}

#[test]
fn native_mark_identical_duplicates_count_once_on_each_platform() {
    for bytes in [
        b"ORG_SESSION_MARK=a\0ORG_SESSION_MARK=a\0".as_slice(),
        b"ORG_SESSION_MARK=a\0OTHER=value\0ORG_SESSION_MARK=a\0ORG_SESSION_MARK=a\0",
    ] {
        assert_eq!(native_mark(bytes, MARK), MarkEvidence::Present("a".into()));
    }
    assert_eq!(
        native_mark(
            b"ORG_SESSION_MARK=a\0ORG_SESSION_MARK=a\0ORG_SESSION_MARK=b\0",
            MARK
        ),
        MarkEvidence::Unreadable
    );
}

#[test]
fn native_guards_quarantine_conflicting_duplicate_marks() {
    assert_eq!(
        native_mark(b"ORG_SESSION_MARK=a\0ORG_SESSION_MARK=b\0", MARK),
        MarkEvidence::Unreadable
    );
    assert_eq!(
        native_mark(b"ORG_SESSION_MARK=a\0", MARK),
        MarkEvidence::Present("a".into())
    );
}

#[test]
fn a_mark_is_read_whole_and_reported_without_judging_its_value() {
    assert_eq!(native_mark(b"", MARK), MarkEvidence::Absent);
    assert_eq!(native_mark(b"OTHER=a\0", MARK), MarkEvidence::Absent);
    // The consumer decides what a valid mark is.
    assert_eq!(
        native_mark(b"ORG_SESSION_MARK=not a mark!\0", MARK),
        MarkEvidence::Present("not a mark!".into())
    );
    assert_eq!(
        native_mark(b"ORG_SESSION_MARK=\0", MARK),
        MarkEvidence::Present(String::new())
    );
    // A block that does not end at an entry boundary, or a value that is not text.
    assert_eq!(
        native_mark(b"ORG_SESSION_MARK=a", MARK),
        MarkEvidence::Unreadable
    );
    assert_eq!(
        native_mark(b"ORG_SESSION_MARK=\xff\0", MARK),
        MarkEvidence::Unreadable
    );
}

#[test]
fn native_deadline_failure_classification_preserves_io() {
    check_time(Instant::now() + Duration::from_secs(3600)).unwrap();
    let error = check_time(Instant::now() - Duration::from_nanos(1)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(error.to_string(), "native recovery deadline elapsed");
    assert!(error
        .get_ref()
        .is_some_and(|source| source.is::<DeadlineElapsed>()));
}

/// Set in the child of [`hardening_holds_in_the_process_that_asked_and_cannot_be_undone`].
const HARDEN_CHILD: &str = "ORG_PLATFORM_TEST_HARDEN";

/// [`super::harden_self`] on every system with an adapter: afterwards the process reports
/// itself hardened, its core limits are 0 and cannot be raised again, and asking twice
/// changes nothing. A child of the test does it, so the test binary keeps its limits.
#[test]
fn hardening_holds_in_the_process_that_asked_and_cannot_be_undone() {
    if std::env::var_os(HARDEN_CHILD).is_some() {
        super::harden_self().unwrap();
        assert!(super::testing::hardened().unwrap());
        let raised = libc::rlimit {
            rlim_cur: 1,
            rlim_max: 1,
        };
        // SAFETY: a valid, initialized limit. Raising a hard limit is refused to an
        // unprivileged process; root may raise it, which the parent takes into account.
        let refused = unsafe { libc::setrlimit(libc::RLIMIT_CORE, &raised) } != 0;
        // SAFETY: no arguments and no side effects; it cannot fail.
        assert!(refused || unsafe { libc::geteuid() } == 0);
        super::harden_self().unwrap();
        assert!(super::testing::hardened().unwrap());
        println!("hardened");
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "process::adapter_tests::hardening_holds_in_the_process_that_asked_and_cannot_be_undone",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(HARDEN_CHILD, "1")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && text.contains("hardened"),
        "{text}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The open-file listing sees a file while this process holds it open, as often as it is
/// open, and no longer once it is closed.
#[test]
fn the_open_file_listing_follows_what_this_process_holds_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().canonicalize().unwrap().join("held");
    let held = |path: &std::path::Path| {
        super::testing::open_paths()
            .unwrap()
            .iter()
            .filter(|open| open.as_path() == path)
            .count()
    };
    assert_eq!(held(&path), 0);
    let first = std::fs::File::create(&path).unwrap();
    assert_eq!(held(&path), 1);
    let second = std::fs::File::open(&path).unwrap();
    assert_eq!(held(&path), 2);
    drop(first);
    drop(second);
    assert_eq!(held(&path), 0);
}

// Each fact this module states about the system, checked against what the system does,
// so that a fact cannot drift from its adapter. Consumers' tests decide by these facts
// which of their cases a system has; a wrong fact would turn such a case into a test
// that checks nothing. `AN_ENDED_MAIN_THREAD_LEAVES_A_ZOMBIE_LEADER` has no probe here:
// making one needs a process whose main thread ends before `main`, which is
// org-supervise's recovery fixture (`tests/recovery_native.rs`).

use super::{
    group_contains_only_leader, is_exited_group_denial, testing, RecoveryScan,
    EXITED_GROUP_SIGNAL_IS_DENIED, SCAN_ENVIRONMENT_FOLLOWS_COUNTED_ARGUMENTS,
    SCAN_ENVIRONMENT_IS_PROC_ENVIRON, SCAN_HOLDS_A_DESCRIPTOR_PER_PROCESS,
    SCAN_WITHHOLDS_RESTRICTED_ENVIRONMENTS, SPAWNS_SUSPENDED,
};

#[test]
fn the_suspended_start_fact_is_what_spawn_piped_does() {
    let (child, _stdin, _stdout, _stderr) = TestChild::spawn_piped(c"/bin/sh", &[c"-c", c"exit 0"]);
    assert_eq!(testing::is_stopped(child.pid).unwrap(), SPAWNS_SUSPENDED);
}

#[test]
fn the_exited_group_fact_is_how_a_signal_to_such_a_group_is_answered() {
    assert_eq!(
        is_exited_group_denial(&io::Error::from_raw_os_error(libc::EPERM)),
        EXITED_GROUP_SIGNAL_IS_DENIED
    );
    let (child, _stdin, _stdout, _stderr) = TestChild::spawn_piped(c"/bin/sh", &[c"-c", c"exit 3"]);
    let watch = Watch::new(child.pid).unwrap();
    release_spawned(child.pid, || Ok(child.exit().is_some())).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while child.exit().is_none() {
        assert!(Instant::now() < deadline, "the child did not exit");
        watch.wait(deadline).unwrap();
    }
    // SAFETY: the group of the test's own unreaped child, which reserves its ID.
    let signalled = unsafe { libc::killpg(child.pid, libc::SIGKILL) };
    let error = (signalled != 0).then(io::Error::last_os_error);
    if EXITED_GROUP_SIGNAL_IS_DENIED {
        let error = error.expect("the signal is refused");
        assert!(is_exited_group_denial(&error), "{error:?}");
        assert!(group_contains_only_leader(child.pid).unwrap());
    } else if let Some(error) = error {
        assert_eq!(error.raw_os_error(), Some(libc::ESRCH), "{error:?}");
    }
}

/// A child for the scan probes: this test binary, started with an environment of
/// the probe's own, holding until its standard input closes.
#[test]
fn hold_for_a_scan() {
    if std::env::var_os("ORG_PLATFORM_HOLD_FOR_A_SCAN").is_none() {
        return;
    }
    println!("ready to be scanned");
    io::stdout().flush().unwrap();
    let _ = io::stdin().read_to_end(&mut Vec::new());
}

const PROBE_MARK: &CStr = c"ORG_SESSION_MARK=probe";

/// Starts `program` with `argv` and only the probe's mark in its environment, lets it
/// run, and when `ready` is given waits until it prints that line.
fn started(program: &CStr, argv: &[&CStr], env: &[&CStr], ready: Option<&str>) -> TestChild {
    use std::io::BufRead;
    let argv: Vec<CString> = argv.iter().map(|arg| CString::from(*arg)).collect();
    let env: Vec<CString> = env.iter().map(|entry| CString::from(*entry)).collect();
    let (pid, _stdin, stdout, _stderr) = spawn_piped(program, &argv, &env, c"/").unwrap();
    let child = TestChild { pid };
    release_spawned(pid, || Ok(child.exit().is_some())).unwrap();
    if let Some(ready) = ready {
        let found = std::io::BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
            .any(|line| line.contains(ready));
        assert!(found, "the probe child did not start");
    }
    std::mem::forget(_stdin);
    child
}

/// This test binary as a probe child, its `argv[0]` given.
fn held_child(argv_zero: &CStr) -> TestChild {
    let exe = CString::new(
        std::env::current_exe()
            .unwrap()
            .into_os_string()
            .into_encoded_bytes(),
    )
    .unwrap();
    started(
        &exe,
        &[
            argv_zero,
            c"--exact",
            c"process::adapter_tests::hold_for_a_scan",
            c"--nocapture",
            c"--test-threads=1",
        ],
        &[c"ORG_PLATFORM_HOLD_FOR_A_SCAN=1", PROBE_MARK],
        Some("ready to be scanned"),
    )
}

/// Runs the test `name` of this binary in a child process with `variable` set to
/// `value`, and returns what it printed. A scan needs core dumps disabled, which cannot
/// be undone, so it runs there and never in the process of the other tests.
fn in_a_child(name: &str, variable: &str, value: &str) -> String {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("process::adapter_tests::{name}"),
            "--nocapture",
            "--test-threads=1",
        ])
        .env(variable, value)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Prepares this child process for a scan: core dumps off, as `validate` requires.
fn ready_for_a_scan() -> RecoveryScan {
    super::no_core_dumps().unwrap();
    RecoveryScan::new("ORG_SESSION_MARK").unwrap()
}

/// In a child process: the mark a fresh scan reports for the PID it is given.
#[test]
fn scan_one_mark() {
    let Some(pid) = std::env::var_os("ORG_PLATFORM_SCAN_PID") else {
        return;
    };
    let pid: u32 = pid.to_str().unwrap().parse().unwrap();
    let mark = ready_for_a_scan()
        .snapshot(Instant::now() + Duration::from_secs(10))
        .unwrap()
        .into_iter()
        .find(|row| row.pid == pid)
        .expect("the scan observes the probe child")
        .mark;
    println!("scanned mark {mark:?}");
}

/// What a fresh scan reports as the mark of `child`.
fn scanned_mark(child: &TestChild) -> String {
    let stdout = in_a_child(
        "scan_one_mark",
        "ORG_PLATFORM_SCAN_PID",
        &child.pid.to_string(),
    );
    stdout
        .lines()
        .find_map(|line| {
            line.split_once("scanned mark ")
                .map(|(_, mark)| mark.to_owned())
        })
        .unwrap_or_else(|| panic!("no mark: {stdout}"))
}

fn present() -> String {
    format!("{:?}", MarkEvidence::Present("probe".into()))
}

fn unreadable() -> String {
    format!("{:?}", MarkEvidence::Unreadable)
}

#[test]
fn the_proc_environ_fact_is_where_the_environment_is() {
    let child = held_child(c"probe");
    let environ = std::fs::read(format!("/proc/{}/environ", child.pid));
    let listed = environ.is_ok_and(|bytes| {
        bytes
            .split(|byte| *byte == 0)
            .any(|entry| entry == PROBE_MARK.to_bytes())
    });
    assert_eq!(listed, SCAN_ENVIRONMENT_IS_PROC_ENVIRON);
    assert_eq!(scanned_mark(&child), present());
}

#[test]
fn the_counted_arguments_fact_is_what_an_empty_argv_zero_does_to_the_scan() {
    let child = held_child(c"");
    let expected = if SCAN_ENVIRONMENT_FOLLOWS_COUNTED_ARGUMENTS {
        unreadable()
    } else {
        present()
    };
    assert_eq!(scanned_mark(&child), expected);
}

#[test]
fn the_restricted_environment_fact_is_what_the_scan_reads_of_a_system_program() {
    let child = started(c"/bin/sleep", &[c"sleep", c"20"], &[PROBE_MARK], None);
    let expected = if SCAN_WITHHOLDS_RESTRICTED_ENVIRONMENTS {
        unreadable()
    } else {
        present()
    };
    assert_eq!(scanned_mark(&child), expected);
}

/// In a child process of its own, so that no other test opens or closes descriptors
/// meanwhile: whether the process holds more descriptors after a snapshot.
#[test]
fn the_descriptor_fact_is_what_a_scan_holds() {
    let open = || std::fs::read_dir("/dev/fd").unwrap().count();
    if std::env::var_os("ORG_PLATFORM_SCAN_DESCRIPTORS").is_some() {
        let mut scan = ready_for_a_scan();
        let before = open();
        let rows = scan
            .snapshot(Instant::now() + Duration::from_secs(10))
            .unwrap();
        println!("held {} for {} processes", open() - before, rows.len());
        return;
    }
    let stdout = in_a_child(
        "the_descriptor_fact_is_what_a_scan_holds",
        "ORG_PLATFORM_SCAN_DESCRIPTORS",
        "1",
    );
    let held: usize = stdout
        .split("held ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|count| count.parse().ok())
        .unwrap_or_else(|| panic!("no count: {stdout}"));
    assert_eq!(held > 0, SCAN_HOLDS_A_DESCRIPTOR_PER_PROCESS, "{stdout}");
}
