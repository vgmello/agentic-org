use std::io;

/// See the module documentation. The refusing adapter refuses.
pub fn hardened() -> io::Result<bool> {
    super::native::hardened()
}

/// The paths of the files this process holds open now, one entry per open descriptor
/// that the system names by a path (a file opened twice is listed twice; sockets and
/// pipes are left out or carry a name that is no path). For a consumer's test that
/// its code leaves no file open. GNU/Linux reads `/proc/self/fd`; macOS asks
/// `lsof` about this process. The refusing adapter refuses.
pub fn open_paths() -> io::Result<Vec<std::path::PathBuf>> {
    super::native::open_paths()
}

/// Makes this process the reaper of its orphaned descendants, where the system has
/// such an attribute (GNU/Linux: `PR_SET_CHILD_SUBREAPER`): `true` when it is now
/// one, `false` where the system has none (macOS: an orphan goes to `launchd`), so a
/// test reaps a grandchild only where it was given it. Irreversible for the process:
/// for a test's own child process only. The refusing adapter refuses.
pub fn become_subreaper() -> io::Result<bool> {
    super::native::become_subreaper()
}

/// Whether the process `pid` is stopped (suspended by a signal or started
/// suspended), as the system reports its state: `proc_pidinfo` on macOS, the state
/// field of `/proc/<pid>/stat` on GNU/Linux. The refusing adapter refuses.
pub fn is_stopped(pid: libc::pid_t) -> io::Result<bool> {
    super::native::is_stopped(pid)
}

/// The storage of this process's own `argv[0]`, which the process may overwrite,
/// where the system exposes it (macOS: `_NSGetArgv`), else `None`. For a test of
/// what a recovery scan reads after a process changed its arguments.
pub fn argv_zero() -> Option<*mut libc::c_char> {
    super::native::argv_zero()
}

/// Ends the calling thread alone; the process goes on while another thread runs.
/// GNU/Linux: the `exit` system call (not `exit_group`), which also ends a main
/// thread alone. Elsewhere `pthread_exit`.
///
/// # Safety
///
/// No destructor of the calling thread runs and its stack is given up. Call it only
/// where no value on that stack is borrowed elsewhere, pinned or owed a drop: for
/// example before `main` (from [`crate::process::run_before_main`]), where nothing
/// of the Rust runtime is in use.
pub unsafe fn exit_this_thread() -> ! {
    // SAFETY: the caller's contract above.
    unsafe { super::native::exit_this_thread() }
}

/// The `posix_spawn` attribute flag that starts the child in a new session, as
/// `setsid` does. Its value differs by system, and the macOS C library does not
/// name it. The refusing adapter refuses: no flag stands in for it.
pub fn posix_spawn_setsid() -> io::Result<libc::c_short> {
    super::native::posix_spawn_setsid()
}

/// `disable_core_dumps_before_main!()`: has the loader set this process's soft and hard
/// core-dump limits to zero before `main`, so before a test harness starts any thread or
/// any test starts a child (which inherits them). The limit is the whole process's and
/// lowering the hard one cannot be undone: for a test binary of its own. A process whose
/// limit cannot be set writes why on standard error and exits 125 before the harness runs,
/// rather than running weaker tests. The consumer writes no `unsafe`: the function the
/// loader calls is this crate's ([`disable_core_dumps_or_exit`]).
#[macro_export]
macro_rules! disable_core_dumps_before_main {
    () => {
        $crate::process::run_before_main!(
            unsafe __ORG_PLATFORM_DISABLE_CORE_DUMPS =
                $crate::process::testing::disable_core_dumps_or_exit
        );
    };
}
pub use crate::disable_core_dumps_before_main;

/// What [`disable_core_dumps_before_main`] has the loader call before `main`: only raw C
/// library calls, which need nothing of the Rust runtime, and it never unwinds.
#[doc(hidden)]
pub extern "C" fn disable_core_dumps_or_exit() {
    let limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: a valid limit record that setrlimit only reads.
    if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) } != 0 {
        let message = b"cannot disable core dumps before the test harness starts\n";
        // SAFETY: static bytes written to the inherited diagnostic descriptor, then an
        // exit that runs nothing of the Rust runtime and does not unwind.
        unsafe {
            libc::write(2, message.as_ptr().cast(), message.len());
            libc::_exit(125);
        }
    }
}

/// Start `program` in `directory` with exactly `arguments` (argv, which may begin with an
/// empty `argv[0]`) and `environment` (envp, repeated names kept) and its standard output
/// on a new pipe; with `new_session`, in a new session (`posix_spawn`, with the flag of
/// [`posix_spawn_setsid`]). For a test of what a scan sees of a process the supervisor did
/// not start. Returns its ID, which the caller reaps, and the pipe's reading end. The
/// refusing adapter refuses.
pub fn spawn_raw(
    directory: &std::path::Path,
    program: &std::path::Path,
    arguments: &[&std::ffi::CStr],
    environment: &[&std::ffi::CStr],
    new_session: bool,
) -> io::Result<(libc::pid_t, io::PipeReader)> {
    super::native::spawn_raw(directory, program, arguments, environment, new_session)
}

/// `leader_exit_fixture_before_main!()`: in a process started with
/// `ORG_TEST_CASE=leader-exit`, before `main` (so before the test harness exists), starts a
/// worker thread that writes `RECOVERY_READY` on standard output and ends the process after
/// 20 seconds, then ends the main thread alone ([`exit_this_thread`]). On systems where an
/// ended main thread leaves a zombie leader, that is what a recovery scan then meets. Any
/// other process returns at once.
#[macro_export]
macro_rules! leader_exit_fixture_before_main {
    () => {
        $crate::process::run_before_main!(
            unsafe __ORG_PLATFORM_LEADER_EXIT_FIXTURE =
                $crate::process::testing::leader_exit_fixture
        );
    };
}
pub use crate::leader_exit_fixture_before_main;

/// What [`leader_exit_fixture_before_main`] has the loader call.
///
/// # Safety
///
/// Only before `main`: it may end the calling thread, whose stack must hold nothing
/// borrowed, pinned or owed a drop.
#[doc(hidden)]
pub unsafe extern "C" fn leader_exit_fixture() {
    // SAFETY: the loader has initialized the C library and no other thread exists yet.
    let mode = unsafe { libc::getenv(c"ORG_TEST_CASE".as_ptr()) };
    // SAFETY: a non-null getenv result is a NUL-terminated string.
    if mode.is_null() || unsafe { std::ffi::CStr::from_ptr(mode) } != c"leader-exit" {
        return;
    }
    let mut worker = std::mem::MaybeUninit::<libc::pthread_t>::uninit();
    // SAFETY: a correctly sized output slot, default attributes and a C entry point that
    // takes no argument.
    let started = unsafe {
        libc::pthread_create(
            worker.as_mut_ptr(),
            std::ptr::null(),
            hold_then_exit,
            std::ptr::null_mut(),
        )
    };
    if started != 0 {
        // SAFETY: end this disposable test process before any of its runtime starts.
        unsafe { libc::_exit(125) };
    }
    // SAFETY: the caller's contract: before main, nothing on this thread's stack is
    // borrowed, pinned or owed a drop. The worker keeps the process alive.
    unsafe { exit_this_thread() }
}

extern "C" fn hold_then_exit(_: *mut libc::c_void) -> *mut libc::c_void {
    let ready = b"RECOVERY_READY\n";
    // SAFETY: static bytes to the inherited standard output, a sleep and an exit, all raw C
    // library calls that need nothing of the Rust runtime.
    unsafe {
        libc::write(libc::STDOUT_FILENO, ready.as_ptr().cast(), ready.len());
        libc::sleep(20);
        libc::_exit(0)
    }
}
