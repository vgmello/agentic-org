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
