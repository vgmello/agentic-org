#[path = "support/core_limit.rs"]
mod core_limit;

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

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
        .recv_timeout(Duration::from_secs(5))
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
    #[cfg(target_os = "linux")]
    if mode.starts_with("partial-environ:") {
        hold_partial_environment();
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

#[cfg(target_os = "linux")]
fn page_size() -> usize {
    // SAFETY: _SC_PAGESIZE requires no pointer arguments.
    let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    assert!(size > 0);
    size as usize
}

#[cfg(target_os = "linux")]
fn environment_range(stat: &str) -> (usize, usize) {
    let fields = stat.rsplit_once(") ").unwrap().1;
    let mut addresses = fields.split_ascii_whitespace().skip(47);
    let start = addresses.next().unwrap().parse().unwrap();
    let end = addresses.next().unwrap().parse().unwrap();
    (start, end)
}

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
#[test]
fn native_capture_quarantines_a_nul_terminated_partial_environment() {
    use org_supervise::recovery::MarkEvidence;
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

    let mut row = Row(RecoveryRecord::new(mark, Some(identity), vec![]).unwrap());
    let error = recover_record(
        &mut row,
        &(),
        &mut native,
        Instant::now() + Duration::from_secs(5),
    )
    .expect_err("a recorded process with partial environment evidence must remain unresolved");
    assert_eq!(error.report.signals, 0);
    assert!(child.try_exit().unwrap().is_none());
}

#[cfg(target_os = "linux")]
#[used]
#[unsafe(link_section = ".init_array")]
static LEADER_EXIT_FIXTURE: extern "C" fn() = start_leader_exit_fixture;

#[cfg(target_os = "linux")]
extern "C" fn start_leader_exit_fixture() {
    // SAFETY: the loader has initialized libc and the test harness has yet to start.
    let mode = unsafe { libc::getenv(c"ORG_TEST_CASE".as_ptr()) };
    if mode.is_null()
        // SAFETY: a successful getenv returns a NUL-terminated string.
        || unsafe { std::ffi::CStr::from_ptr(mode) } != c"leader-exit"
    {
        return;
    }
    let mut worker = std::mem::MaybeUninit::<libc::pthread_t>::uninit();
    // SAFETY: a correctly sized output slot, default attributes and a C entry point.
    if unsafe {
        libc::pthread_create(
            worker.as_mut_ptr(),
            std::ptr::null(),
            hold_worker,
            std::ptr::null_mut(),
        )
    } != 0
    {
        // SAFETY: terminate this disposable test child on fixture initialization failure.
        unsafe { libc::_exit(125) };
    }
    // SAFETY: exit only the calling thread. The worker keeps this process alive.
    unsafe {
        libc::syscall(libc::SYS_exit, 0i32);
        libc::_exit(125);
    }
}

#[cfg(target_os = "linux")]
extern "C" fn hold_worker(_: *mut libc::c_void) -> *mut libc::c_void {
    let ready = b"RECOVERY_READY\n";
    // SAFETY: the fixture owns its stdout pipe. Bound its lifetime even on test failure.
    unsafe {
        libc::write(libc::STDOUT_FILENO, ready.as_ptr().cast(), ready.len());
        libc::sleep(20);
        libc::_exit(0);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn native_recovery_keeps_a_record_when_a_zombie_leader_has_live_workers() {
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

#[cfg(target_os = "macos")]
#[test]
fn native_capture_reports_an_exited_unreaped_child_as_zombie() {
    let root = tempfile::tempdir().unwrap();
    let (mut child, _) = fixture(root.path(), &unique_mark(), "hold");
    let exact = ProcessIdentity {
        pid: child.identity().pid,
        started: child.identity().started.clone(),
        boot: child.identity().boot.clone(),
    };
    let mut native = NativeRecovery::new().unwrap();
    // SAFETY: this is the test's owned unreaped child.
    assert_eq!(
        unsafe { libc::kill(exact.pid as libc::pid_t, libc::SIGTERM) },
        0
    );
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

#[cfg(target_os = "macos")]
#[test]
fn hold_mutated_argv_fixture() {
    unsafe extern "C" {
        fn _NSGetArgv() -> *mut *mut *mut libc::c_char;
    }
    // SAFETY: _NSGetArgv provides this process's live argv vector and string.
    let argv_zero = unsafe { **_NSGetArgv() };
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

#[cfg(target_os = "macos")]
fn mac_fixture_mark(
    program: &Path,
    arguments: &[&std::ffi::CStr],
    environment: &[&std::ffi::CStr],
    wait_for_ready: bool,
) -> org_supervise::recovery::MarkEvidence {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;

    let directory = tempfile::tempdir().unwrap();
    let cwd = CString::new(directory.path().as_os_str().as_bytes()).unwrap();
    let program = CString::new(program.as_os_str().as_bytes()).unwrap();
    let strings = |items: &[&std::ffi::CStr]| {
        items
            .iter()
            .map(|item| item.as_ptr().cast_mut())
            .chain(std::iter::once(std::ptr::null_mut()))
            .collect::<Vec<_>>()
    };
    let mut argv = strings(arguments);
    let mut envp = strings(environment);
    let (stdout, writer) = io::pipe().unwrap();
    let mut actions = std::ptr::null_mut();
    let mut pid = 0;
    // SAFETY: valid action pointer, open stdout fd and NUL-terminated path
    // and argument/environment arrays. The raw vectors preserve empty argv[0]
    // and duplicate environment entries for these fixtures.
    let spawn = unsafe {
        assert_eq!(libc::posix_spawn_file_actions_init(&mut actions), 0);
        assert_eq!(
            libc::posix_spawn_file_actions_addchdir_np(&mut actions, cwd.as_ptr()),
            0
        );
        assert_eq!(
            libc::posix_spawn_file_actions_adddup2(
                &mut actions,
                writer.as_raw_fd(),
                libc::STDOUT_FILENO,
            ),
            0
        );
        let result = libc::posix_spawn(
            &mut pid,
            program.as_ptr(),
            &actions,
            std::ptr::null(),
            argv.as_mut_ptr(),
            envp.as_mut_ptr(),
        );
        libc::posix_spawn_file_actions_destroy(&mut actions);
        result
    };
    assert_eq!(spawn, 0);
    drop(writer);
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
    // SAFETY: this test alone owns and reaps the fixture. Reap after capture.
    let reaped = unsafe {
        libc::kill(pid, libc::SIGKILL);
        loop {
            let reaped = libc::waitpid(pid, std::ptr::null_mut(), 0);
            if reaped == pid || io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                break reaped;
            }
        }
    };
    assert_eq!(reaped, pid);
    assert!(ready, "macOS fixture did not report readiness");
    capture
        .unwrap()
        .into_iter()
        .find(|row| row.pid == pid as u32)
        .expect("live macOS fixture")
        .mark
}

#[cfg(target_os = "macos")]
#[test]
fn native_capture_documents_the_mutated_argv_trust_boundary() {
    use org_supervise::recovery::MarkEvidence;
    let mark = unique_mark();
    let argument = std::ffi::CString::new(format!("ORG_SESSION_MARK={mark}")).unwrap();
    let observed = mac_fixture_mark(
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

#[cfg(target_os = "macos")]
#[test]
fn native_capture_quarantines_empty_argv_zero_and_restricted_processes() {
    use org_supervise::recovery::MarkEvidence;
    let mark = std::ffi::CString::new(format!("ORG_SESSION_MARK={}", unique_mark())).unwrap();
    let conflicting =
        std::ffi::CString::new(format!("ORG_SESSION_MARK={}", unique_mark())).unwrap();
    assert_eq!(
        mac_fixture_mark(
            &std::env::current_exe().unwrap(),
            &[c"", c"--exact", c"hold_fixture", c"--nocapture"],
            &[c"ORG_TEST_CASE=hold", &mark],
            true,
        ),
        MarkEvidence::Unreadable,
    );
    // XNU's restricted-process redaction can reveal just the first conflicting
    // entry when argv[0] is empty. Both forms must be quarantined.
    for argv_zero in [c"", c"sleep"] {
        assert_eq!(
            mac_fixture_mark(
                Path::new("/bin/sleep"),
                &[argv_zero, c"20"],
                &[&mark, &conflicting],
                false,
            ),
            MarkEvidence::Unreadable,
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn native_capture_accepts_identical_duplicate_environment_marks() {
    use org_supervise::recovery::MarkEvidence;
    let mark = unique_mark();
    let entry = std::ffi::CString::new(format!("ORG_SESSION_MARK={mark}")).unwrap();
    let observed = mac_fixture_mark(
        &std::env::current_exe().unwrap(),
        &[c"fixture", c"--exact", c"hold_fixture", c"--nocapture"],
        &[c"ORG_TEST_CASE=hold", &entry, &entry],
        true,
    );
    assert_eq!(observed, MarkEvidence::Present(mark));
}
