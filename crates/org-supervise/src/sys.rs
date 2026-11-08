//! The crate's only `unsafe` code: thin, checked wrappers over the macOS system calls the
//! supervisor needs.
//!
//! Every wrapper takes and returns safe types (`OwnedFd`, `BorrowedFd`, `CStr`, plain values),
//! turns a failed call into an `io::Error`, and says in a `SAFETY` comment why its `unsafe`
//! block is sound. The rest of the crate is safe Rust (`#![deny(unsafe_code)]` in `lib.rs`).
//!
//! Constants that `libc` 0.2.191 doesn't define for Apple targets are written out here, with
//! the header they come from (checked against the macOS 26 SDK).

#![allow(unsafe_code)]

use std::ffi::{CStr, CString};
use std::io;
use std::mem::{self, MaybeUninit};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::ptr;

use libc::{c_char, c_int, c_short, c_void, pid_t};

/// `POSIX_SPAWN_SETSID` (`<sys/spawn.h>`): the child starts a new session.
const POSIX_SPAWN_SETSID: c_int = 0x0400;
/// `TIOCSWINSZ` (`<sys/ttycom.h>`): `_IOW('t', 103, struct winsize)`.
const TIOCSWINSZ: libc::c_ulong = 0x8008_7467;
/// `TIOCGWINSZ` (`<sys/ttycom.h>`): `_IOR('t', 104, struct winsize)`.
const TIOCGWINSZ: libc::c_ulong = 0x4008_7468;
/// `TIOCPTYGNAME` (`<sys/ttycom.h>`): `_IOC(IOC_OUT, 't', 83, 128)`, the thread-safe form
/// of `ptsname(3)`.
const TIOCPTYGNAME: libc::c_ulong = 0x4080_7453;
/// The buffer size `TIOCPTYGNAME` writes into.
const PTY_NAME_LEN: usize = 128;
/// `proc_listpids` filters (`<sys/proc_info.h>`).
const PROC_PGRP_ONLY: u32 = 2;
const PROC_UID_ONLY: u32 = 4;
const PROC_PPID_ONLY: u32 = 6;
/// `SZOMB` (`<sys/proc.h>`): a process awaiting collection by its parent.
const SZOMB: u32 = 5;

fn cvt(r: c_int) -> io::Result<c_int> {
    if r == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(r)
    }
}

/// Retries a call that failed with `EINTR`.
fn cvt_r(mut f: impl FnMut() -> c_int) -> io::Result<c_int> {
    loop {
        match cvt(f()) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            other => return other,
        }
    }
}

/// A posix_spawn-style call returns the error number itself instead of setting `errno`.
fn cvt_spawn(r: c_int) -> io::Result<()> {
    if r == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(r))
    }
}

/// Refuses a name that isn't exactly one path component, so an `*at` call can never be
/// steered outside the directory it's given.
fn check_component(name: &CStr) -> io::Result<()> {
    let bytes = name.to_bytes();
    if bytes.is_empty() || bytes == b"." || bytes == b".." || bytes.contains(&b'/') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a single path component",
        ));
    }
    Ok(())
}

/// Takes ownership of a descriptor a successful call just returned.
fn owned(fd: c_int) -> OwnedFd {
    debug_assert!(fd >= 0);
    // SAFETY: `fd` was just returned by a successful open-like call, so it's an open
    // descriptor that nothing else owns.
    unsafe { OwnedFd::from_raw_fd(fd) }
}

// ---------------------------------------------------------------------------------------------
// Spawning

/// How the child's fds 0, 1 and 2 are set up.
pub(crate) enum ChildStdio<'a> {
    /// Each descriptor is `dup2`'d onto 0, 1 and 2. They must all be 3 or above (see
    /// [`above_stdio`]), so no `dup2` can overwrite another's source.
    Fds {
        stdin: BorrowedFd<'a>,
        stdout: BorrowedFd<'a>,
        stderr: BorrowedFd<'a>,
    },
    /// A terminal: the child starts a new session, opens this slave path as fd 0 (without
    /// `O_NOCTTY`, so it becomes the session's controlling terminal), and dups it to 1 and 2.
    Terminal { slave: &'a CStr },
}

/// What [`spawn`] runs.
pub(crate) struct SpawnRequest<'a> {
    /// The absolute path of the binary (never looked up on `PATH`).
    pub program: &'a CStr,
    /// `argv`, `argv[0]` included.
    pub argv: &'a [CString],
    /// The whole environment, `NAME=value` each.
    pub envp: &'a [CString],
    /// The working directory.
    pub cwd: &'a CStr,
    pub stdio: ChildStdio<'a>,
}

/// An initialized `posix_spawn_file_actions_t`, destroyed on drop.
struct FileActions(libc::posix_spawn_file_actions_t);

impl FileActions {
    fn new() -> io::Result<Self> {
        let mut raw: libc::posix_spawn_file_actions_t = ptr::null_mut();
        // SAFETY: `raw` is a valid place for the call to initialize.
        cvt_spawn(unsafe { libc::posix_spawn_file_actions_init(&mut raw) })?;
        Ok(FileActions(raw))
    }

    fn dup2(&mut self, from: c_int, to: c_int) -> io::Result<()> {
        // SAFETY: `self.0` is initialized; the call only records the action.
        cvt_spawn(unsafe { libc::posix_spawn_file_actions_adddup2(&mut self.0, from, to) })
    }

    fn open(&mut self, to: c_int, path: &CStr, flags: c_int) -> io::Result<()> {
        // SAFETY: `self.0` is initialized and `path` is a valid C string (the call copies it).
        cvt_spawn(unsafe {
            libc::posix_spawn_file_actions_addopen(&mut self.0, to, path.as_ptr(), flags, 0)
        })
    }

    fn chdir(&mut self, path: &CStr) -> io::Result<()> {
        // SAFETY: `self.0` is initialized and `path` is a valid C string (the call copies it).
        cvt_spawn(unsafe { libc::posix_spawn_file_actions_addchdir_np(&mut self.0, path.as_ptr()) })
    }
}

impl Drop for FileActions {
    fn drop(&mut self) {
        // SAFETY: `self.0` was initialized by `new` and is destroyed exactly once.
        unsafe { libc::posix_spawn_file_actions_destroy(&mut self.0) };
    }
}

/// An initialized `posix_spawnattr_t`, destroyed on drop.
struct SpawnAttr(libc::posix_spawnattr_t);

impl SpawnAttr {
    fn new() -> io::Result<Self> {
        let mut raw: libc::posix_spawnattr_t = ptr::null_mut();
        // SAFETY: `raw` is a valid place for the call to initialize.
        cvt_spawn(unsafe { libc::posix_spawnattr_init(&mut raw) })?;
        Ok(SpawnAttr(raw))
    }
}

impl Drop for SpawnAttr {
    fn drop(&mut self) {
        // SAFETY: `self.0` was initialized by `new` and is destroyed exactly once.
        unsafe { libc::posix_spawnattr_destroy(&mut self.0) };
    }
}

fn empty_sigset() -> libc::sigset_t {
    let mut set = MaybeUninit::<libc::sigset_t>::uninit();
    // SAFETY: `sigemptyset` initializes the whole set; it can't fail for a valid pointer.
    unsafe {
        libc::sigemptyset(set.as_mut_ptr());
        set.assume_init()
    }
}

/// Every signal except `SIGKILL` and `SIGSTOP` (which can't be caught, so needn't be reset).
fn resettable_sigset() -> libc::sigset_t {
    let mut set = MaybeUninit::<libc::sigset_t>::uninit();
    // SAFETY: `sigfillset` initializes the whole set; `sigdelset` with a valid signal number
    // only clears a bit in it.
    unsafe {
        libc::sigfillset(set.as_mut_ptr());
        let mut set = set.assume_init();
        libc::sigdelset(&mut set, libc::SIGKILL);
        libc::sigdelset(&mut set, libc::SIGSTOP);
        set
    }
}

/// Spawns `req` and returns the child's pid.
///
/// - `POSIX_SPAWN_CLOEXEC_DEFAULT`: the child gets **only** fds 0, 1 and 2 from the file
///   actions; every other descriptor of this process is closed in the child, whatever its
///   close-on-exec flag and whichever thread opened it.
/// - `POSIX_SPAWN_SETSIGMASK` (empty mask) and `POSIX_SPAWN_SETSIGDEF` (every catchable
///   signal): the child starts with no blocked signals and default dispositions, so an
///   ignored `SIGPIPE` (Rust's default) or a blocked signal of the parent's doesn't leak in.
/// - Pipes: `POSIX_SPAWN_SETPGROUP` with pgroup 0, so the child leads a new process group
///   (pgid = pid) in the caller's session. A terminal: `POSIX_SPAWN_SETSID`, so it leads a new
///   session (sid = pgid = pid) and acquires the slave as its controlling terminal when the
///   open file action runs.
/// - `posix_spawn_file_actions_addchdir_np` sets the cwd in the child before the exec.
///
/// `posix_spawn` is one system call on macOS: on failure (no such binary, no permission) no
/// child exists, so nothing needs reaping.
pub(crate) fn spawn(req: &SpawnRequest<'_>) -> io::Result<pid_t> {
    let mut argv: Vec<*mut c_char> = req.argv.iter().map(|a| a.as_ptr().cast_mut()).collect();
    argv.push(ptr::null_mut());
    let mut envp: Vec<*mut c_char> = req.envp.iter().map(|e| e.as_ptr().cast_mut()).collect();
    envp.push(ptr::null_mut());

    let mut actions = FileActions::new()?;
    let session = match req.stdio {
        ChildStdio::Fds {
            stdin,
            stdout,
            stderr,
        } => {
            for (fd, target) in [(stdin, 0), (stdout, 1), (stderr, 2)] {
                let raw = fd.as_raw_fd();
                if raw < 3 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "a child stdio source must be fd 3 or above",
                    ));
                }
                actions.dup2(raw, target)?;
            }
            false
        }
        ChildStdio::Terminal { slave } => {
            actions.open(0, slave, libc::O_RDWR)?;
            actions.dup2(0, 1)?;
            actions.dup2(0, 2)?;
            true
        }
    };
    actions.chdir(req.cwd)?;

    let mut attr = SpawnAttr::new()?;
    let mut flags = libc::POSIX_SPAWN_CLOEXEC_DEFAULT
        | libc::POSIX_SPAWN_SETSIGMASK
        | libc::POSIX_SPAWN_SETSIGDEF;
    if session {
        flags |= POSIX_SPAWN_SETSID;
    } else {
        flags |= libc::POSIX_SPAWN_SETPGROUP;
        // SAFETY: `attr.0` is initialized; pgroup 0 means "a new group led by the child".
        cvt_spawn(unsafe { libc::posix_spawnattr_setpgroup(&mut attr.0, 0) })?;
    }
    let mask = empty_sigset();
    let defaults = resettable_sigset();
    let flags = c_short::try_from(flags)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "spawn flags overflow"))?;
    // SAFETY: `attr.0` is initialized; the sets are valid, initialized `sigset_t`s that the
    // calls copy.
    unsafe {
        cvt_spawn(libc::posix_spawnattr_setsigmask(&mut attr.0, &mask))?;
        cvt_spawn(libc::posix_spawnattr_setsigdefault(&mut attr.0, &defaults))?;
        cvt_spawn(libc::posix_spawnattr_setflags(&mut attr.0, flags))?;
    }

    let mut pid: pid_t = 0;
    // SAFETY: `program` is a valid C string; `argv` and `envp` are NULL-terminated arrays of
    // pointers into `CString`s that outlive the call (`req` borrows them); `actions` and
    // `attr` are initialized. `posix_spawn` doesn't keep any of the pointers.
    cvt_spawn(unsafe {
        libc::posix_spawn(
            &mut pid,
            req.program.as_ptr(),
            &actions.0,
            &attr.0,
            argv.as_ptr(),
            envp.as_ptr(),
        )
    })?;
    Ok(pid)
}

// ---------------------------------------------------------------------------------------------
// Waiting and signals

/// How a reaped child ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WaitStatus {
    pub code: Option<i32>,
    pub signal: Option<i32>,
}

/// Whether the child `pid` has exited, **without reaping it** (`waitid(P_PID, WEXITED |
/// WNOWAIT)`): it stays a zombie, so its pid (and so its process group id) can't be reused
/// until [`reap`]. With `block`, waits for the exit.
pub(crate) fn has_exited(pid: pid_t, block: bool) -> io::Result<bool> {
    let id = libc::id_t::try_from(pid)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "bad pid"))?;
    let mut options = libc::WEXITED | libc::WNOWAIT;
    if !block {
        options |= libc::WNOHANG;
    }
    loop {
        // SAFETY: `siginfo_t` is plain data, so all-zero is a valid value, and `waitid` writes
        // into it only.
        let mut info: libc::siginfo_t = unsafe { mem::zeroed() };
        // SAFETY: `info` is a valid, writable `siginfo_t`.
        match cvt(unsafe { libc::waitid(libc::P_PID, id, &mut info, options) }) {
            Ok(_) => return Ok(info.si_pid == pid),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
}

/// Reaps the child `pid` (blocking until it has exited).
pub(crate) fn reap(pid: pid_t) -> io::Result<WaitStatus> {
    let mut status: c_int = 0;
    // SAFETY: `status` is a valid, writable int.
    cvt_r(|| unsafe { libc::waitpid(pid, &mut status, 0) })?;
    Ok(WaitStatus {
        code: libc::WIFEXITED(status).then(|| libc::WEXITSTATUS(status)),
        signal: libc::WIFSIGNALED(status).then(|| libc::WTERMSIG(status)),
    })
}

/// Sends `signal` to one process. Refuses pids 0 and below, which `kill(2)` would read as a
/// process group or "every process".
pub(crate) fn kill(pid: pid_t, signal: c_int) -> io::Result<()> {
    if pid <= 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "bad pid"));
    }
    // SAFETY: `kill` has no memory effects; `pid` is a single positive pid.
    cvt(unsafe { libc::kill(pid, signal) }).map(drop)
}

/// Sends `signal` to a process group. Refuses groups 1 and below (`killpg(0)` is the
/// caller's own group; 1 is launchd's).
pub(crate) fn killpg(pgid: pid_t, signal: c_int) -> io::Result<()> {
    if pgid <= 1 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "bad pgid"));
    }
    // SAFETY: `killpg` has no memory effects; `pgid` is a positive group id other than 1.
    cvt(unsafe { libc::killpg(pgid, signal) }).map(drop)
}

/// The session id of `pid`.
pub(crate) fn getsid(pid: pid_t) -> io::Result<pid_t> {
    if pid <= 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "bad pid"));
    }
    // SAFETY: `getsid` has no memory effects.
    cvt(unsafe { libc::getsid(pid) })
}

/// This process's effective uid.
pub(crate) fn geteuid() -> u32 {
    // SAFETY: `geteuid` can't fail and has no memory effects.
    unsafe { libc::geteuid() }
}

// ---------------------------------------------------------------------------------------------
// Descriptors

/// Moves `fd` to 3 or above if it's 0, 1 or 2 (a parent that closed its own stdio could get
/// one), keeping close-on-exec, so a child's `dup2` file actions never collide.
pub(crate) fn above_stdio(fd: OwnedFd) -> io::Result<OwnedFd> {
    if fd.as_raw_fd() >= 3 {
        return Ok(fd);
    }
    // SAFETY: `fd` is open; `F_DUPFD_CLOEXEC` returns a new descriptor (>= 3) or -1.
    let new = cvt(unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) })?;
    Ok(owned(new))
}

fn set_cloexec(fd: c_int) -> io::Result<()> {
    // SAFETY: `fd` is an open descriptor; `F_SETFD` only changes its flags.
    cvt(unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) }).map(drop)
}

/// A pipe, `(read end, write end)`, both close-on-exec and 3 or above.
///
/// macOS has no `pipe2`, so the flag is set right after `pipe`. Children this crate spawns
/// never inherit either end anyway (`POSIX_SPAWN_CLOEXEC_DEFAULT`); the flag protects against
/// other spawners in the process.
pub(crate) fn pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0 as c_int; 2];
    // SAFETY: `fds` has room for the two descriptors `pipe` writes.
    cvt(unsafe { libc::pipe(fds.as_mut_ptr()) })?;
    let (read, write) = (owned(fds[0]), owned(fds[1]));
    set_cloexec(read.as_raw_fd())?;
    set_cloexec(write.as_raw_fd())?;
    Ok((above_stdio(read)?, above_stdio(write)?))
}

/// Sets `O_NONBLOCK` on `fd`.
pub(crate) fn set_nonblocking(fd: BorrowedFd<'_>) -> io::Result<()> {
    // SAFETY: `fd` is open; `F_GETFL` / `F_SETFL` only read and change its status flags.
    unsafe {
        let flags = cvt(libc::fcntl(fd.as_raw_fd(), libc::F_GETFL))?;
        cvt(libc::fcntl(
            fd.as_raw_fd(),
            libc::F_SETFL,
            flags | libc::O_NONBLOCK,
        ))?;
    }
    Ok(())
}

/// Makes a write to `fd` after its reader is gone fail with `EPIPE` instead of raising
/// `SIGPIPE` (`F_SETNOSIGPIPE`), so a dead child can't kill a host that doesn't ignore it.
pub(crate) fn set_nosigpipe(fd: BorrowedFd<'_>) -> io::Result<()> {
    // SAFETY: `fd` is open; `F_SETNOSIGPIPE` only changes a flag on it.
    cvt(unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETNOSIGPIPE, 1) }).map(drop)
}

/// `poll(2)`, retried on `EINTR`. Returns how many entries have events.
pub(crate) fn poll(fds: &mut [libc::pollfd], timeout_ms: c_int) -> io::Result<usize> {
    let n = libc::nfds_t::try_from(fds.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "too many fds"))?;
    // SAFETY: `fds` is a valid, writable slice of `n` `pollfd`s.
    let ready = cvt_r(|| unsafe { libc::poll(fds.as_mut_ptr(), n, timeout_ms) })?;
    Ok(usize::try_from(ready).unwrap_or(0))
}

/// 8 random bytes from the kernel's generator (`arc4random_buf`), for temporary file names.
pub(crate) fn random_u64() -> u64 {
    let mut value = 0u64;
    // SAFETY: the buffer is `value`, 8 writable bytes.
    unsafe { libc::arc4random_buf((&mut value as *mut u64).cast::<c_void>(), 8) };
    value
}

// ---------------------------------------------------------------------------------------------
// Files, never following a symlink

const DIR_FLAGS: c_int = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;

/// Opens the directory at `path` with `O_DIRECTORY | O_NOFOLLOW`: fails if the last component
/// is a symlink or not a directory.
pub(crate) fn open_dir_nofollow(path: &CStr) -> io::Result<OwnedFd> {
    // SAFETY: `path` is a valid C string.
    cvt_r(|| unsafe { libc::open(path.as_ptr(), DIR_FLAGS) }).map(owned)
}

/// Opens the subdirectory `name` of `dir`, refusing a symlink or a non-directory.
pub(crate) fn openat_dir_nofollow(dir: BorrowedFd<'_>, name: &CStr) -> io::Result<OwnedFd> {
    check_component(name)?;
    // SAFETY: `dir` is open and `name` is a valid C string.
    cvt_r(|| unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), DIR_FLAGS) }).map(owned)
}

/// Opens `name` in `dir` for reading, refusing a symlink. `O_NONBLOCK` so a FIFO can't block
/// the open; the caller checks the file's type.
pub(crate) fn openat_read_nofollow(dir: BorrowedFd<'_>, name: &CStr) -> io::Result<OwnedFd> {
    check_component(name)?;
    let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
    // SAFETY: `dir` is open and `name` is a valid C string.
    cvt_r(|| unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), flags) }).map(owned)
}

/// Creates the new file `name` in `dir` (`O_CREAT | O_EXCL | O_NOFOLLOW`): fails if anything,
/// a symlink included, is already there.
pub(crate) fn openat_create_excl(
    dir: BorrowedFd<'_>,
    name: &CStr,
    mode: libc::mode_t,
) -> io::Result<OwnedFd> {
    check_component(name)?;
    let flags = libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    let mode = libc::c_uint::from(mode);
    // SAFETY: `dir` is open, `name` is a valid C string, and `O_CREAT` takes the mode as its
    // variadic argument (promoted to `unsigned int`).
    cvt_r(|| unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), flags, mode) }).map(owned)
}

/// Creates the directory `name` in `dir`.
pub(crate) fn mkdirat(dir: BorrowedFd<'_>, name: &CStr, mode: libc::mode_t) -> io::Result<()> {
    check_component(name)?;
    // SAFETY: `dir` is open and `name` is a valid C string.
    cvt(unsafe { libc::mkdirat(dir.as_raw_fd(), name.as_ptr(), mode) }).map(drop)
}

/// Renames `from` to `to`, both in `dir`. `rename` never follows a symlink at `to`: it
/// replaces the link itself.
pub(crate) fn renameat(dir: BorrowedFd<'_>, from: &CStr, to: &CStr) -> io::Result<()> {
    check_component(from)?;
    check_component(to)?;
    let fd = dir.as_raw_fd();
    // SAFETY: `dir` is open and both names are valid C strings.
    cvt(unsafe { libc::renameat(fd, from.as_ptr(), fd, to.as_ptr()) }).map(drop)
}

/// Unlinks the non-directory `name` in `dir` (no `AT_REMOVEDIR`, so a directory is refused
/// by the kernel too).
pub(crate) fn unlinkat(dir: BorrowedFd<'_>, name: &CStr) -> io::Result<()> {
    check_component(name)?;
    // SAFETY: `dir` is open and `name` is a valid C string.
    cvt(unsafe { libc::unlinkat(dir.as_raw_fd(), name.as_ptr(), 0) }).map(drop)
}

/// What `fstatat(AT_SYMLINK_NOFOLLOW)` says about an entry (a symlink is described, not
/// followed).
#[derive(Debug, Clone, Copy)]
pub(crate) struct EntryStat {
    pub is_socket: bool,
    pub uid: u32,
}

/// Describes `name` in `dir` without following a symlink.
pub(crate) fn fstatat_nofollow(dir: BorrowedFd<'_>, name: &CStr) -> io::Result<EntryStat> {
    check_component(name)?;
    let mut st = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `dir` is open, `name` is a valid C string and `st` is writable.
    cvt(unsafe {
        libc::fstatat(
            dir.as_raw_fd(),
            name.as_ptr(),
            st.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    })?;
    // SAFETY: `fstatat` succeeded, so it filled in `st`.
    let st = unsafe { st.assume_init() };
    Ok(EntryStat {
        is_socket: st.st_mode & libc::S_IFMT == libc::S_IFSOCK,
        uid: st.st_uid,
    })
}

// ---------------------------------------------------------------------------------------------
// Terminals

/// A new pseudo-terminal: `(master, slave path, slave)`.
///
/// Both sides are opened with `O_NOCTTY` (and are close-on-exec), so the terminal never
/// becomes this process's controlling terminal, even if this process leads a session without
/// one (a launchd service does). The window size is set through the slave: macOS refuses
/// `TIOCSWINSZ` on a master whose slave was never opened (`ENOTTY`). The caller keeps the
/// slave only until the child, which opens it again by path, is spawned.
pub(crate) fn open_pty(cols: u16, rows: u16) -> io::Result<(OwnedFd, CString, OwnedFd)> {
    // SAFETY: `posix_openpt` has no memory effects.
    let master = owned(cvt(unsafe {
        libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY)
    })?);
    set_cloexec(master.as_raw_fd())?;
    let master = above_stdio(master)?;
    // SAFETY: `master` is an open pty master; these calls only change the slave's owner and
    // lock state.
    unsafe {
        cvt(libc::grantpt(master.as_raw_fd()))?;
        cvt(libc::unlockpt(master.as_raw_fd()))?;
    }
    let mut name = [0 as c_char; PTY_NAME_LEN];
    // SAFETY: `TIOCPTYGNAME` writes a NUL-terminated name of at most `PTY_NAME_LEN` bytes
    // into the buffer.
    cvt(unsafe { libc::ioctl(master.as_raw_fd(), TIOCPTYGNAME, name.as_mut_ptr()) })?;
    // SAFETY: the buffer holds a NUL-terminated string (the ioctl succeeded, and the buffer
    // is the size it writes).
    let path = unsafe { CStr::from_ptr(name.as_ptr()) }.to_owned();
    let flags = libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC;
    // SAFETY: `path` is a valid C string.
    let slave = above_stdio(owned(cvt_r(|| unsafe {
        libc::open(path.as_ptr(), flags)
    })?))?;
    set_winsize(slave.as_fd(), cols, rows)?;
    Ok((master, path, slave))
}

/// Sets a terminal's window size (`TIOCSWINSZ`); the foreground process group gets
/// `SIGWINCH`.
pub(crate) fn set_winsize(fd: BorrowedFd<'_>, cols: u16, rows: u16) -> io::Result<()> {
    let ws = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: `TIOCSWINSZ` reads one `winsize` from the pointer.
    cvt(unsafe { libc::ioctl(fd.as_raw_fd(), TIOCSWINSZ, &ws) }).map(drop)
}

/// A terminal's window size, `(cols, rows)`.
pub(crate) fn get_winsize(fd: BorrowedFd<'_>) -> io::Result<(u16, u16)> {
    let mut ws = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: `TIOCGWINSZ` writes one `winsize` through the pointer.
    cvt(unsafe { libc::ioctl(fd.as_raw_fd(), TIOCGWINSZ, &mut ws) })?;
    Ok((ws.ws_col, ws.ws_row))
}

// ---------------------------------------------------------------------------------------------
// The process table

/// What the process table says about one process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProcInfo {
    pub pid: pid_t,
    pub ppid: pid_t,
    /// The effective uid.
    pub uid: u32,
    pub pgid: pid_t,
    /// When it started, in microseconds since the epoch: with the pid, it names one process
    /// (a reused pid has a later start time).
    pub start_us: u64,
    pub zombie: bool,
}

/// The process `pid`, or `None` if there's none (or it's gone too far to describe).
pub(crate) fn proc_info(pid: pid_t) -> io::Result<Option<ProcInfo>> {
    if pid <= 0 {
        return Ok(None);
    }
    let mut info = MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let size = c_int::try_from(mem::size_of::<libc::proc_bsdinfo>())
        .map_err(|_| io::Error::other("proc_bsdinfo too large"))?;
    // SAFETY: `info` is a writable buffer of exactly `size` bytes.
    let n = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast::<c_void>(),
            size,
        )
    };
    if n <= 0 {
        let err = io::Error::last_os_error();
        return match err.raw_os_error() {
            Some(libc::ESRCH) | Some(0) | None => Ok(None),
            _ => Err(err),
        };
    }
    if n != size {
        return Ok(None);
    }
    // SAFETY: the call filled all `size` bytes.
    let info = unsafe { info.assume_init() };
    let to_pid = |v: u32| pid_t::try_from(v).unwrap_or(-1);
    Ok(Some(ProcInfo {
        pid: to_pid(info.pbi_pid),
        ppid: to_pid(info.pbi_ppid),
        uid: info.pbi_uid,
        pgid: to_pid(info.pbi_pgid),
        start_us: info
            .pbi_start_tvsec
            .saturating_mul(1_000_000)
            .saturating_add(info.pbi_start_tvusec),
        zombie: info.pbi_status == SZOMB,
    }))
}

/// Which processes [`list_pids`] lists.
#[derive(Debug, Clone, Copy)]
pub(crate) enum PidFilter {
    /// Those with this effective uid.
    Uid(u32),
    /// The members of this process group.
    Pgrp(pid_t),
    /// The children of this process.
    Ppid(pid_t),
}

/// The pids the process table lists for `filter` (a snapshot).
pub(crate) fn list_pids(filter: PidFilter) -> io::Result<Vec<pid_t>> {
    let (kind, arg) = match filter {
        PidFilter::Uid(uid) => (PROC_UID_ONLY, uid),
        PidFilter::Pgrp(pgid) => (PROC_PGRP_ONLY, u32::try_from(pgid).unwrap_or(0)),
        PidFilter::Ppid(pid) => (PROC_PPID_ONLY, u32::try_from(pid).unwrap_or(0)),
    };
    let pid_size = mem::size_of::<pid_t>();
    for _ in 0..4 {
        // SAFETY: a NULL buffer asks only for the size needed.
        let needed = cvt(unsafe { libc::proc_listpids(kind, arg, ptr::null_mut(), 0) })?;
        let count = usize::try_from(needed).unwrap_or(0) / pid_size + 64;
        let mut pids = vec![0 as pid_t; count];
        let bytes = c_int::try_from(count * pid_size)
            .map_err(|_| io::Error::other("process table too large"))?;
        // SAFETY: `pids` is a writable buffer of `bytes` bytes.
        let got = cvt(unsafe {
            libc::proc_listpids(kind, arg, pids.as_mut_ptr().cast::<c_void>(), bytes)
        })?;
        let got = usize::try_from(got).unwrap_or(0) / pid_size;
        if got < count {
            pids.truncate(got);
            pids.retain(|&p| p > 0);
            return Ok(pids);
        }
        // The table grew past the slack between the calls: ask again.
    }
    Err(io::Error::other("the process table kept growing"))
}

/// The kernel's `KERN_ARGMAX`: the largest argument area a process can have.
fn arg_max() -> io::Result<usize> {
    let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
    let mut value: c_int = 0;
    let mut len = mem::size_of::<c_int>();
    // SAFETY: `mib` names a 2-level int sysctl; `value` and `len` are writable and `len` holds
    // its size.
    cvt(unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            2,
            (&mut value as *mut c_int).cast::<c_void>(),
            &mut len,
            ptr::null_mut(),
            0,
        )
    })?;
    usize::try_from(value).map_err(|_| io::Error::other("bad KERN_ARGMAX"))
}

/// The raw `KERN_PROCARGS2` area of `pid`: its argc, exec path, argv and environment as they
/// were at exec. The kernel gives it only for processes this user may inspect.
pub(crate) fn procargs(pid: pid_t) -> io::Result<Vec<u8>> {
    let mut buf = vec![0u8; arg_max()?];
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let mut len = buf.len();
    // SAFETY: `mib` names a 3-level sysctl; `buf` is writable and `len` holds its size.
    cvt(unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            buf.as_mut_ptr().cast::<c_void>(),
            &mut len,
            ptr::null_mut(),
            0,
        )
    })?;
    buf.truncate(len);
    Ok(buf)
}

// ---------------------------------------------------------------------------------------------
// Test support

/// Duplicates `fd` **without** close-on-exec, so a careless spawner would leak it.
#[cfg(test)]
pub(crate) fn dup_inheritable(fd: BorrowedFd<'_>) -> io::Result<OwnedFd> {
    // SAFETY: `fd` is open; `F_DUPFD` returns a new descriptor (>= 3, no close-on-exec).
    cvt(unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD, 3) }).map(owned)
}

/// Blocks (`true`) or unblocks `signal` in the calling thread.
#[cfg(test)]
pub(crate) fn block_signal(signal: c_int, block: bool) -> io::Result<()> {
    let mut set = empty_sigset();
    // SAFETY: `set` is an initialized `sigset_t`; `sigaddset` only sets a bit in it, and
    // `pthread_sigmask` reads it (the old mask isn't asked for).
    let r = unsafe {
        libc::sigaddset(&mut set, signal);
        let how = if block {
            libc::SIG_BLOCK
        } else {
            libc::SIG_UNBLOCK
        };
        libc::pthread_sigmask(how, &set, ptr::null_mut())
    };
    cvt_spawn(r)
}

/// A Unix socket **without** close-on-exec.
#[cfg(test)]
pub(crate) fn socket_inheritable() -> io::Result<OwnedFd> {
    // SAFETY: `socket` has no memory effects.
    cvt(unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) }).map(owned)
}
