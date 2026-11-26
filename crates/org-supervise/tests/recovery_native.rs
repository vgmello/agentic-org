#[path = "support/core_limit.rs"]
mod core_limit;

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;
use std::sync::{mpsc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{Duration, Instant};

use org_platform::posix;
use org_platform::process::{
    AN_ENDED_MAIN_THREAD_LEAVES_A_ZOMBIE_LEADER, SCAN_ENVIRONMENT_FOLLOWS_COUNTED_ARGUMENTS,
    SCAN_ENVIRONMENT_READ_STOPS_AT_AN_UNREADABLE_PAGE, SCAN_HOLDS_A_DESCRIPTOR_PER_PROCESS,
    SCAN_WITHHOLDS_RESTRICTED_ENVIRONMENTS,
};
use org_supervise::recovery::{
    recover_record, CheckpointTarget, NativeRecovery, ProcessIdentity, RecoveryCheckpoint,
    RecoveryProcesses, RecoveryRecord, RecoveryRecords,
};
use org_supervise::{
    ChildIdentity, Environment, PreparedSpawn, SearchPath, SessionMark, SpawnSpec, Spawned,
};

struct Row(RecoveryRecord);
impl RecoveryRecords for Row {
    type Key = ();
    fn pending(&mut self) -> io::Result<Vec<((), RecoveryRecord)>> {
        unreachable!("live recovery loads only its keyed row")
    }
    fn load(&mut self, _: &(), _: Instant) -> io::Result<Option<RecoveryRecord>> {
        Ok(Some(self.0.clone()))
    }
}
impl RecoveryCheckpoint for Row {
    fn checkpoint(
        &mut self,
        _: &(),
        mark: &str,
        inventory: &[CheckpointTarget],
        _: Instant,
    ) -> io::Result<()> {
        assert_eq!(self.0.mark(), mark);
        assert!(self
            .0
            .inventory()
            .iter()
            .all(|saved| inventory.contains(saved)));
        self.0 = self.0.clone().with_inventory(inventory.to_vec())?;
        Ok(())
    }
}

/// Native scans take one descriptor per visible process. The fd-limit fixtures fork
/// rlim_cur*3/5 children (614 under the 1024 cap), a storm that pushes a parallel sibling's scan
/// past its own NOFILE: those fixtures run alone, and every other scanning test shares the lock.
static NATIVE_SCANS: RwLock<()> = RwLock::new(());

fn shared_scans() -> RwLockReadGuard<'static, ()> {
    NATIVE_SCANS
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn exclusive_scans() -> RwLockWriteGuard<'static, ()> {
    NATIVE_SCANS
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn unique_mark() -> String {
    let mut bytes = [0; 16];
    File::open("/dev/urandom")
        .unwrap()
        .read_exact(&mut bytes)
        .unwrap();
    format!("native-recovery-{:032x}", u128::from_ne_bytes(bytes))
}

fn fixture(
    root: &Path,
    mark: &str,
    mode: &str,
) -> (org_supervise::OwnedChild, Option<ProcessIdentity>) {
    let search = SearchPath::new(
        Path::new("/usr/bin"),
        "/usr/bin:/bin".as_ref(),
        &[root.to_owned()],
    )
    .unwrap();
    let Spawned { child, stdout, .. } = PreparedSpawn::new(
        SpawnSpec {
            program: std::env::current_exe().unwrap(),
            args: vec![
                "--exact".into(),
                "hold_fixture".into(),
                "--nocapture".into(),
            ],
            cwd: root.to_owned(),
            environment: Environment::new(search, &[])
                .unwrap()
                .with_context(&[("ORG_TEST_CASE".into(), mode.into())])
                .unwrap(),
        },
        SessionMark::new(mark.to_owned()).unwrap(),
    )
    .unwrap()
    .record_mark(|_| Ok(()))
    .unwrap()
    .spawn()
    .unwrap()
    .record_identity(|_| Ok(()))
    .unwrap();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let result = loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break Err(io::Error::other("fixture exited before readiness")),
                Ok(_) if line.split_ascii_whitespace().next() == Some("RECOVERY_READY") => {
                    break Ok(line)
                }
                Ok(_) => {}
                Err(error) => break Err(error),
            }
        };
        let _ = sender.send(result);
        let _ = io::copy(&mut reader, &mut io::sink());
    });
    let ready = receiver
        .recv_timeout(Duration::from_secs(if mode.starts_with("fd-") {
            15
        } else {
            5
        }))
        .expect("fixture readiness deadline")
        .unwrap();
    let mut fields = ready.split_ascii_whitespace().skip(1);
    let ready = fields.next().map(|pid| ProcessIdentity {
        pid: pid.parse().unwrap(),
        started: fields.next().unwrap().into(),
        boot: fields.next().unwrap().into(),
    });
    (child, ready)
}

#[test]
fn hold_fixture() {
    let Ok(mode) = std::env::var("ORG_TEST_CASE") else {
        return;
    };
    // Each of these modes is started only by a test of the scan source it needs.
    if mode == "fd-limit" || mode == "fd-exhausted" {
        capture_under_low_fd_limit(mode == "fd-exhausted");
    }
    if mode.starts_with("partial-environ:") {
        hold_partial_environment();
    }
    if mode == "restricted-job" || mode == "restricted-escape" {
        hold_restricted_job(mode == "restricted-escape");
        return;
    }
    let _nested = if mode == "delegate" {
        let mark = std::env::var("ORG_SESSION_MARK").unwrap();
        let root = std::env::current_dir().unwrap();
        let (nested, _) = fixture(&root, &mark, "hold");
        let id = nested.identity();
        println!("RECOVERY_READY {} {} {}", id.pid, id.started, id.boot);
        Some(nested)
    } else {
        println!("RECOVERY_READY");
        None
    };
    io::stdout().flush().unwrap();
    // Bound the fixture's life even if a test fails after its parent was killed.
    std::thread::sleep(Duration::from_secs(20));
}

#[test]
fn native_recovery_signals_the_recorded_child_without_reaping_it() {
    let _scans = shared_scans();
    let root = tempfile::tempdir().unwrap();
    let (mut marked, _) = fixture(root.path(), &unique_mark(), "hold");
    let (mut unrelated, _) = fixture(root.path(), &unique_mark(), "hold");
    let identity = marked.identity().clone();
    let mut row =
        Row(RecoveryRecord::new(identity.mark.clone(), Some(identity.clone()), vec![]).unwrap());
    let mut native = NativeRecovery::new().unwrap();
    let gone = recover_record(
        &mut row,
        &(),
        &mut native,
        Instant::now() + Duration::from_secs(10),
    )
    .unwrap();
    assert_eq!(gone.mark(), identity.mark);
    assert!(row
        .0
        .inventory()
        .iter()
        .any(|target| target.identity.pid == identity.pid));
    assert!(
        marked.try_exit().unwrap().is_some(),
        "the owned process must still be waitable"
    );
    assert!(unrelated.try_exit().unwrap().is_none());
}

#[test]
fn native_recovery_refuses_a_reused_non_child_identity_and_scans_detached_tools() {
    let _scans = shared_scans();
    let root = tempfile::tempdir().unwrap();
    let mark = unique_mark();
    let (mut owner, tool) = fixture(root.path(), &mark, "delegate");
    let tool = tool.expect("delegate fixture must report its child identity");
    let stale = ChildIdentity {
        pid: tool.pid,
        pgid: tool.pid,
        started: format!("{}-stale", tool.started),
        boot: tool.boot.clone(),
        mark: format!("{mark}-stale"),
    };
    let mut native = NativeRecovery::new().unwrap();
    let rows = native
        .snapshot(Instant::now() + Duration::from_secs(5))
        .unwrap();
    let nested = rows
        .iter()
        .find(|row| row.identity.as_ref() == Some(&tool))
        .expect("live detached tool");
    assert_eq!(
        nested.parent.as_ref(),
        Some(&ProcessIdentity {
            pid: owner.identity().pid,
            started: owner.identity().started.clone(),
            boot: owner.identity().boot.clone(),
        }),
        "native snapshot must verify the exact parent identity"
    );
    let mut stale_row = Row(RecoveryRecord::new(stale.mark.clone(), Some(stale), vec![]).unwrap());
    let gone = recover_record(
        &mut stale_row,
        &(),
        &mut native,
        Instant::now() + Duration::from_secs(10),
    )
    .unwrap();
    assert_eq!(gone.report().signals, 0);
    assert!(!native.wait_exited(&tool, Instant::now()).unwrap());
    assert!(owner.try_exit().unwrap().is_none());

    let mut row = Row(RecoveryRecord::new(mark.clone(), None, vec![]).unwrap());
    let gone = recover_record(
        &mut row,
        &(),
        &mut native,
        Instant::now() + Duration::from_secs(10),
    )
    .unwrap();
    assert_eq!(gone.mark(), mark);
    assert!(row.0.inventory().iter().any(|entry| entry.identity == tool));
    assert!(native.wait_exited(&tool, Instant::now()).unwrap());
    assert!(owner.try_exit().unwrap().is_some());
}

/// Where a scan holds a descriptor per process ([`SCAN_HOLDS_A_DESCRIPTOR_PER_PROCESS`]).
fn capture_under_low_fd_limit(exhaust: bool) {
    struct Children(Vec<libc::pid_t>);
    impl Drop for Children {
        fn drop(&mut self) {
            for pid in &self.0 {
                // The fixture owns these unreaped children.
                let _ = posix::kill(*pid, libc::SIGKILL);
                let _ = posix::reap(*pid);
            }
        }
    }
    let mut children = Children(Vec::new());
    let mut limit = posix::resource_limit(posix::Resource::OpenFiles).unwrap();
    limit.rlim_cur = limit.rlim_cur.min(if exhaust { 64 } else { 1024 });
    // Reduce only this disposable fixture's soft limit.
    posix::set_resource_limit(posix::Resource::OpenFiles, limit).unwrap();
    let count = limit.rlim_cur as usize * if exhaust { 6 } else { 3 } / 5;
    for _ in 0..count {
        // A synthetic child that only sleeps, its lifetime bounded.
        let pid =
            posix::testing::fork_child(posix::testing::ForkedChild::SleepThenExit { seconds: 20 })
                .unwrap();
        children.0.push(pid);
    }
    let mark = unique_mark();
    let mut row = Row(RecoveryRecord::new(mark, None, vec![]).unwrap());
    let mut native = NativeRecovery::new().unwrap();
    if exhaust {
        let error = native
            .snapshot(Instant::now() + Duration::from_secs(5))
            .unwrap_err();
        assert!(error.to_string().contains("doctor:"), "{error}");
        assert!(error.to_string().contains("RLIMIT_NOFILE"), "{error}");
        return;
    }
    // Drop the previous capture's pidfds before opening the next capture's pins.
    // Otherwise these consecutive snapshots exceed NOFILE even with one fd per pin.
    for _ in 0..2 {
        let captured = native
            .snapshot(Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert!(
            captured.len() >= count,
            "the capture must include the synthetic children"
        );
    }
    let gone = recover_record(
        &mut row,
        &(),
        &mut native,
        Instant::now() + Duration::from_secs(10),
    )
    .expect("recovery must fit one retained descriptor per process");
    assert_eq!(gone.report().signals, 0);
}

#[test]
fn native_recovery_fits_one_descriptor_per_process() {
    if !SCAN_HOLDS_A_DESCRIPTOR_PER_PROCESS {
        return;
    }
    let _scans = exclusive_scans();
    let root = tempfile::tempdir().unwrap();
    let (_child, _) = fixture(root.path(), &unique_mark(), "fd-limit");
}

#[test]
fn native_recovery_explains_descriptor_exhaustion() {
    if !SCAN_HOLDS_A_DESCRIPTOR_PER_PROCESS {
        return;
    }
    let _scans = exclusive_scans();
    let root = tempfile::tempdir().unwrap();
    let (_child, _) = fixture(root.path(), &unique_mark(), "fd-exhausted");
}

fn page_size() -> usize {
    posix::page_size().unwrap()
}

/// The environment's address range, from fields 48 and 49 of `/proc/<pid>/stat`.
fn environment_range(stat: &str) -> (usize, usize) {
    let fields = stat.rsplit_once(") ").unwrap().1;
    let mut addresses = fields.split_ascii_whitespace().skip(47);
    let start = addresses.next().unwrap().parse().unwrap();
    let end = addresses.next().unwrap().parse().unwrap();
    (start, end)
}

fn hold_partial_environment() -> ! {
    let mark = std::env::var("ORG_SESSION_MARK").unwrap();
    let entry = format!("ORG_SESSION_MARK={mark}\0");
    let conflict = b"ORG_SESSION_MARK=hidden-conflict\0";
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap();
    let (start, end) = environment_range(&stat);
    let page = page_size();
    let hidden = (start + page).next_multiple_of(page);
    assert!(hidden + page <= end);
    assert!(entry.len() < hidden - start);

    let ready = b"RECOVERY_READY\n";
    // SAFETY: the fixture owns its writable initial environment and has
    // copied the launch values it needs. The protected page lies wholly
    // within that environment. Afterward only raw libc calls are used.
    unsafe {
        std::ptr::write_bytes(start as *mut u8, 0, hidden - start);
        std::ptr::copy_nonoverlapping(entry.as_ptr(), start as *mut u8, entry.len());
        std::ptr::copy_nonoverlapping(conflict.as_ptr(), hidden as *mut u8, conflict.len());
        if libc::mprotect(hidden as *mut libc::c_void, page, libc::PROT_NONE) != 0 {
            libc::_exit(125);
        }
        libc::write(libc::STDOUT_FILENO, ready.as_ptr().cast(), ready.len());
        libc::sleep(20);
        libc::_exit(0);
    }
}

#[test]
fn native_capture_quarantines_a_nul_terminated_partial_environment() {
    if !SCAN_ENVIRONMENT_READ_STOPS_AT_AN_UNREADABLE_PAGE {
        return;
    }
    let _scans = shared_scans();
    use org_supervise::recovery::MarkEvidence;
    for recorded_job in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mark = unique_mark();
        let mode = format!("partial-environ:{}", "x".repeat(4 * page_size()));
        let (mut child, _) = fixture(root.path(), &mark, &mode);
        let identity = child.identity().clone();
        let proc = format!("/proc/{}", identity.pid);
        let stat = std::fs::read_to_string(format!("{proc}/stat")).unwrap();
        let (start, end) = environment_range(&stat);
        let prefix = std::fs::read(format!("{proc}/environ")).unwrap();
        assert!(prefix.starts_with(format!("ORG_SESSION_MARK={mark}\0").as_bytes()));
        assert!(
            prefix.len() < end - start,
            "fixture must truncate the procfs read"
        );
        let mut native = NativeRecovery::new().unwrap();
        let rows = native
            .snapshot(Instant::now() + Duration::from_secs(5))
            .unwrap();
        let row = rows.iter().find(|row| row.pid == identity.pid).unwrap();
        assert_eq!(row.mark, MarkEvidence::Unreadable);

        // An untrusted environment prefix and numeric PID give no authority.
        let mut row = Row(RecoveryRecord::new(mark.clone(), None, vec![]).unwrap());
        let gone = recover_record(
            &mut row,
            &(),
            &mut native,
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(gone.report().signals, 0);
        assert!(gone.report().unreadable_environments > 0);
        assert!(child.try_exit().unwrap().is_none());
        // A trusted exact checkpoint can recover the same S0 process independently.
        if recorded_job {
            row.0 = RecoveryRecord::new(identity.mark.clone(), Some(identity), vec![]).unwrap();
        } else {
            row.0 = row
                .0
                .with_inventory(vec![CheckpointTarget {
                    identity: ProcessIdentity {
                        pid: identity.pid,
                        started: identity.started,
                        boot: identity.boot,
                    },
                    ancestry: vec![],
                }])
                .unwrap();
        }
        let gone = recover_record(
            &mut row,
            &(),
            &mut native,
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(gone.report().signals, 1);
        assert!(child.try_exit().unwrap().is_some());
    }
}

// In a child started with `ORG_TEST_CASE=leader-exit`, the main thread ends before the
// harness starts while a worker keeps the process alive (the platform's fixture).
org_platform::process::testing::leader_exit_fixture_before_main!();

/// Where a process whose main thread ended while a worker runs is a zombie leader
/// ([`AN_ENDED_MAIN_THREAD_LEAVES_A_ZOMBIE_LEADER`]), recovery must not take it for gone.
#[test]
fn native_recovery_keeps_a_record_when_a_zombie_leader_has_live_workers() {
    if !AN_ENDED_MAIN_THREAD_LEAVES_A_ZOMBIE_LEADER {
        return;
    }
    let _scans = shared_scans();
    let root = tempfile::tempdir().unwrap();
    let (mut child, _) = fixture(root.path(), &unique_mark(), "leader-exit");
    let identity = child.identity().clone();
    let stat_path = format!("/proc/{}/stat", identity.pid);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let stat = std::fs::read_to_string(&stat_path).unwrap();
        if stat.rsplit_once(") ").unwrap().1.starts_with("Z ") {
            break;
        }
        assert!(Instant::now() < deadline, "main thread did not exit");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(child.try_exit().unwrap().is_none(), "worker is still live");

    let mut row =
        Row(RecoveryRecord::new(identity.mark.clone(), Some(identity.clone()), vec![]).unwrap());
    let mut native = NativeRecovery::new().unwrap();
    let exact = ProcessIdentity {
        pid: identity.pid,
        started: identity.started,
        boot: identity.boot,
    };
    let exited_before_capture = native.wait_exited(&exact, Instant::now()).unwrap();
    let result = recover_record(
        &mut row,
        &(),
        &mut native,
        Instant::now() + Duration::from_secs(5),
    );
    assert!(result.is_err(), "recovery issued Gone for a live worker");
    assert!(!exited_before_capture, "unpinned wait used thread exit");
    assert!(!native.wait_exited(&exact, Instant::now()).unwrap());
    let submillisecond = Instant::now() + Duration::from_micros(750);
    assert!(!native.wait_exited(&exact, submillisecond).unwrap());
    assert!(Instant::now() >= submillisecond);
}

#[test]
fn native_capture_reports_an_exited_unreaped_child_as_zombie() {
    let _scans = shared_scans();
    let root = tempfile::tempdir().unwrap();
    let (mut child, _) = fixture(root.path(), &unique_mark(), "hold");
    let exact = ProcessIdentity {
        pid: child.identity().pid,
        started: child.identity().started.clone(),
        boot: child.identity().boot.clone(),
    };
    let mut native = NativeRecovery::new().unwrap();
    // This is the test's owned unreaped child.
    posix::kill(exact.pid as libc::pid_t, libc::SIGTERM).unwrap();
    assert!(native
        .wait_exited(&exact, Instant::now() + Duration::from_secs(5))
        .unwrap());
    let rows = native
        .snapshot(Instant::now() + Duration::from_secs(5))
        .unwrap();
    assert!(rows
        .iter()
        .any(|row| row.identity.as_ref() == Some(&exact) && row.zombie));
    assert!(child.try_exit().unwrap().is_some());
}

#[test]
fn hold_mutated_argv_fixture() {
    // Only where the process's own argv[0] storage is exposed (macOS).
    let Some(argv_zero) = org_platform::process::testing::argv_zero() else {
        return;
    };
    // SAFETY: argv_zero is this process's live, NUL-terminated argv[0].
    if unsafe { std::ffi::CStr::from_ptr(argv_zero) } != c"ABC" {
        return;
    }
    // SAFETY: the fixture launched with the writable three-byte argv[0] "ABC".
    // The test harness has already parsed its arguments.
    unsafe { argv_zero.add(1).write(0) };
    println!("RECOVERY_READY");
    io::stdout().flush().unwrap();
    std::thread::sleep(Duration::from_secs(20));
}

fn native_spawn(
    directory: &Path,
    program: &Path,
    arguments: &[&std::ffi::CStr],
    environment: &[&std::ffi::CStr],
    new_session: bool,
) -> (libc::pid_t, io::PipeReader) {
    // The raw argv and envp keep an empty argv[0] and repeated environment entries.
    org_platform::process::testing::spawn_raw(
        directory,
        program,
        arguments,
        environment,
        new_session,
    )
    .unwrap()
}

fn native_fixture_mark(
    program: &Path,
    arguments: &[&std::ffi::CStr],
    environment: &[&std::ffi::CStr],
    wait_for_ready: bool,
) -> org_supervise::recovery::MarkEvidence {
    let directory = tempfile::tempdir().unwrap();
    let (pid, stdout) = native_spawn(directory.path(), program, arguments, environment, false);
    let ready = if wait_for_ready {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let found = BufReader::new(stdout)
                .lines()
                .map_while(Result::ok)
                .any(|line| line == "RECOVERY_READY");
            let _ = sender.send(found);
        });
        receiver.recv_timeout(Duration::from_secs(5)) == Ok(true)
    } else {
        true
    };
    let capture = NativeRecovery::new()
        .and_then(|mut native| native.snapshot(Instant::now() + Duration::from_secs(5)));
    // This test alone owns and reaps the fixture. Reap after capture.
    let _ = posix::kill(pid, libc::SIGKILL);
    posix::reap(pid).unwrap();
    assert!(ready, "native fixture did not report readiness");
    capture
        .unwrap()
        .into_iter()
        .find(|row| row.pid == pid as u32)
        .expect("live native fixture")
        .mark
}

/// Where the scan reads a process's environment after the arguments counted at its
/// start ([`SCAN_ENVIRONMENT_FOLLOWS_COUNTED_ARGUMENTS`]), a process that moved a mark
/// from its arguments is believed.
#[test]
fn native_capture_documents_the_mutated_argv_trust_boundary() {
    if !SCAN_ENVIRONMENT_FOLLOWS_COUNTED_ARGUMENTS {
        return;
    }
    let _scans = shared_scans();
    use org_supervise::recovery::MarkEvidence;
    let mark = unique_mark();
    let argument = std::ffi::CString::new(format!("ORG_SESSION_MARK={mark}")).unwrap();
    let observed = native_fixture_mark(
        &std::env::current_exe().unwrap(),
        &[
            c"ABC",
            c"--exact",
            c"hold_mutated_argv_fixture",
            c"--nocapture",
            c"--skip",
            &argument,
        ],
        &[],
        true,
    );
    // The child deliberately moved a known exact mark from argv into the region
    // inferred as environment from the saved argc. The adapter cannot tell.
    assert_eq!(observed, MarkEvidence::Present(mark));
}

/// Where the scan's environment follows the counted arguments, an empty `argv[0]`
/// makes it ambiguous, and where the system withholds a restricted process's
/// environment, the scan quarantines it. Each part runs where its behaviour exists.
#[test]
fn native_capture_quarantines_empty_argv_zero_and_restricted_processes() {
    if !SCAN_ENVIRONMENT_FOLLOWS_COUNTED_ARGUMENTS && !SCAN_WITHHOLDS_RESTRICTED_ENVIRONMENTS {
        return;
    }
    let _scans = shared_scans();
    use org_supervise::recovery::MarkEvidence;
    let mark = std::ffi::CString::new(format!("ORG_SESSION_MARK={}", unique_mark())).unwrap();
    let conflicting =
        std::ffi::CString::new(format!("ORG_SESSION_MARK={}", unique_mark())).unwrap();
    if SCAN_ENVIRONMENT_FOLLOWS_COUNTED_ARGUMENTS {
        assert_eq!(
            native_fixture_mark(
                &std::env::current_exe().unwrap(),
                &[c"", c"--exact", c"hold_fixture", c"--nocapture"],
                &[c"ORG_TEST_CASE=hold", &mark],
                true,
            ),
            MarkEvidence::Unreadable,
        );
    }
    if !SCAN_WITHHOLDS_RESTRICTED_ENVIRONMENTS {
        return;
    }
    // XNU's restricted-process redaction can reveal just the first conflicting
    // entry when argv[0] is empty. Both forms must be quarantined.
    for argv_zero in [c"", c"sleep"] {
        assert_eq!(
            native_fixture_mark(
                Path::new("/bin/sleep"),
                &[argv_zero, c"20"],
                &[&mark, &conflicting],
                false,
            ),
            MarkEvidence::Unreadable,
        );
    }
}

#[test]
fn native_capture_accepts_identical_duplicate_environment_marks() {
    let _scans = shared_scans();
    use org_supervise::recovery::MarkEvidence;
    let mark = unique_mark();
    let entry = std::ffi::CString::new(format!("ORG_SESSION_MARK={mark}")).unwrap();
    let observed = native_fixture_mark(
        &std::env::current_exe().unwrap(),
        &[c"fixture", c"--exact", c"hold_fixture", c"--nocapture"],
        &[c"ORG_TEST_CASE=hold", &entry, &entry],
        true,
    );
    assert_eq!(observed, MarkEvidence::Present(mark));
}

fn hold_restricted_job(escape: bool) {
    let mark = std::ffi::CString::new(format!(
        "ORG_SESSION_MARK={}",
        std::env::var("ORG_SESSION_MARK").unwrap()
    ))
    .unwrap();
    let (pid, stdout) = if escape {
        native_spawn(
            &std::env::current_dir().unwrap(),
            Path::new("/bin/sleep"),
            &[c"sleep", c"20"],
            &[&mark],
            true,
        )
    } else {
        native_spawn(
            &std::env::current_dir().unwrap(),
            Path::new("/bin/zsh"),
            &[
                c"zsh",
                c"-c",
                c"nohup /bin/sleep 20 >/dev/null 2>&1 & print -r -- $!; wait",
            ],
            &[&mark, c"PATH=/usr/bin:/bin"],
            false,
        )
    };
    let target_pid = if escape {
        pid as u32
    } else {
        let mut line = String::new();
        BufReader::new(stdout).read_line(&mut line).unwrap();
        line.trim().parse().unwrap()
    };
    let observation = NativeRecovery::new()
        .unwrap()
        .snapshot(Instant::now() + Duration::from_secs(5))
        .unwrap()
        .into_iter()
        .find(|p| p.pid == target_pid)
        .unwrap();
    assert_eq!(
        observation.mark,
        org_supervise::recovery::MarkEvidence::Unreadable
    );
    let id = observation.identity.unwrap();
    println!("RECOVERY_READY {} {} {}", id.pid, id.started, id.boot);
    io::stdout().flush().unwrap();
    std::thread::sleep(Duration::from_secs(20));
    // This fixture owns this unreaped child and its PID until it is reaped.
    let _ = posix::kill(pid, libc::SIGKILL);
    let _ = posix::reap(pid);
}

#[test]
fn native_recovery_checkpoints_restricted_nohup_job_before_stopping_its_leader() {
    if !SCAN_WITHHOLDS_RESTRICTED_ENVIRONMENTS {
        return;
    }
    let _scans = shared_scans();
    let root = tempfile::tempdir().unwrap();
    let (mut owner, child) = fixture(root.path(), &unique_mark(), "restricted-job");
    let child = child.unwrap();
    let leader = owner.identity().clone();
    let mut row =
        Row(RecoveryRecord::new(leader.mark.clone(), Some(leader.clone()), vec![]).unwrap());
    let mut native = NativeRecovery::new().unwrap();
    let snapshot = native
        .snapshot(Instant::now() + Duration::from_secs(5))
        .unwrap();
    let observed = snapshot
        .iter()
        .find(|p| p.identity.as_ref() == Some(&child))
        .unwrap();
    assert!(observed.group == leader.pid || observed.session == leader.pid);
    assert_eq!(
        observed.mark,
        org_supervise::recovery::MarkEvidence::Unreadable
    );
    let gone = recover_record(
        &mut row,
        &(),
        &mut native,
        Instant::now() + Duration::from_secs(10),
    )
    .unwrap();
    assert!(gone.report().unreadable_environments > 0);
    assert!(row.0.inventory().iter().any(|t| t.identity == child));
    assert!(native
        .wait_exited(&child, Instant::now() + Duration::from_secs(2))
        .unwrap());
    assert!(owner.try_exit().unwrap().is_some());
}

#[test]
fn native_recovery_warns_and_skips_unknown_restricted_setsid_escape() {
    if !SCAN_WITHHOLDS_RESTRICTED_ENVIRONMENTS {
        return;
    }
    let _scans = shared_scans();
    let root = tempfile::tempdir().unwrap();
    let (mut owner, child) = fixture(root.path(), &unique_mark(), "restricted-escape");
    let child = child.unwrap();
    let leader = owner.identity().clone();
    let mut row = Row(RecoveryRecord::new(leader.mark.clone(), Some(leader), vec![]).unwrap());
    let mut native = NativeRecovery::new().unwrap();
    let gone = recover_record(
        &mut row,
        &(),
        &mut native,
        Instant::now() + Duration::from_secs(10),
    )
    .unwrap();
    assert!(gone.report().unreadable_environments > 0);
    assert!(!row.0.inventory().iter().any(|t| t.identity == child));
    assert!(!native.wait_exited(&child, Instant::now()).unwrap());
    assert!(owner.try_exit().unwrap().is_some());
    // Clean only the exact child created by this fixture, via trusted inventory.
    row.0 = row
        .0
        .with_inventory(vec![CheckpointTarget {
            identity: child.clone(),
            ancestry: vec![],
        }])
        .unwrap();
    recover_record(
        &mut row,
        &(),
        &mut native,
        Instant::now() + Duration::from_secs(5),
    )
    .unwrap();
    assert!(native
        .wait_exited(&child, Instant::now() + Duration::from_secs(2))
        .unwrap());
}
