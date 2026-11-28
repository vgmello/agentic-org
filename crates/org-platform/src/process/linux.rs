//! The GNU/Linux process adapter: Linux 5.10 and glibc 2.34 or later.

use std::ffi::CStr;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::time::Duration;

use super::{check, invalid, unsupported, ExitFields, Uids};

mod rss;
mod scan;

pub(super) use rss::{process_rss_bytes, process_tree_rss_bytes};
pub(super) use scan::RecoveryScan;

pub(super) fn uids() -> io::Result<Uids> {
    // SAFETY: querying process credentials, without changing them.
    unsafe {
        let real = libc::getuid();
        let effective = libc::geteuid();
        let (mut real_out, mut effective_out, mut saved) = (0, 0, 0);
        if libc::getresuid(&mut real_out, &mut effective_out, &mut saved) != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Uids {
            real,
            effective,
            saved,
            tainted: false,
        })
    }
}

/// Disables core dumps and makes this process non-dumpable, then reads the attribute
/// back: a kernel or a container profile that ignores the request is an error, not a
/// process that goes on believing it is closed.
pub(super) fn harden_self() -> io::Result<()> {
    super::no_core_dumps()?;
    // SAFETY: `PR_SET_DUMPABLE` takes one integer argument and changes only this
    // process's own attribute.
    if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0 {
        let error = io::Error::last_os_error();
        return Err(io::Error::new(
            error.kind(),
            format!("prctl(PR_SET_DUMPABLE): {error}"),
        ));
    }
    if dumpable()? {
        return Err(io::Error::other(
            "prctl(PR_SET_DUMPABLE): the process is still dumpable",
        ));
    }
    Ok(())
}

/// The core-dump limit of this process, soft and hard, set to 0 ([`super::disable_core_dumps`]):
/// the process stays dumpable otherwise, unlike [`harden_self`].
pub(super) fn disable_core_dumps() -> io::Result<()> {
    super::no_core_dumps()
}

pub(super) fn parent_pid(pid: libc::pid_t) -> io::Result<Option<libc::pid_t>> {
    let stat = match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => stat,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let tail = stat
        .rsplit_once(')')
        .ok_or_else(|| invalid("invalid process stat"))?
        .1;
    let ppid = tail
        .split_whitespace()
        .nth(1)
        .and_then(|field| field.parse::<libc::pid_t>().ok())
        .ok_or_else(|| invalid("invalid parent pid in process stat"))?;
    Ok(Some(ppid))
}

/// Whether this process is dumpable (`PR_GET_DUMPABLE`): 0 is not; any other value is.
fn dumpable() -> io::Result<bool> {
    // SAFETY: `PR_GET_DUMPABLE` takes no further argument and changes nothing.
    match unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) } {
        0 => Ok(false),
        value if value > 0 => Ok(true),
        _ => {
            let error = io::Error::last_os_error();
            Err(io::Error::new(
                error.kind(),
                format!("prctl(PR_GET_DUMPABLE): {error}"),
            ))
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn hardened() -> io::Result<bool> {
    Ok(super::shared::core_dumps_disabled()? && !dumpable()?)
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn become_subreaper() -> io::Result<bool> {
    // SAFETY: PR_SET_CHILD_SUBREAPER takes an integer flag and no pointer.
    if unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(true)
}

/// The state field of `/proc/<pid>/stat`: `T` (stopped) or `t` (stopped for tracing).
#[cfg(any(test, feature = "test-support"))]
pub(super) fn is_stopped(pid: libc::pid_t) -> io::Result<bool> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let state = stat
        .rsplit_once(") ")
        .and_then(|(_, fields)| fields.chars().next())
        .ok_or_else(|| invalid("unexpected /proc stat format"))?;
    Ok(matches!(state, 'T' | 't'))
}

/// The process state, for adapter tests: whether `pid` is a zombie (`Z`).
#[cfg(test)]
pub(super) fn is_zombie(pid: libc::pid_t) -> io::Result<bool> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let state = stat
        .rsplit_once(") ")
        .and_then(|(_, fields)| fields.chars().next())
        .ok_or_else(|| invalid("unexpected /proc stat format"))?;
    Ok(state == 'Z')
}

/// This process's environment array, for adapter tests.
#[cfg(test)]
pub(super) fn environment_strings() -> *const *const libc::c_char {
    unsafe extern "C" {
        // The C library changes it when the environment changes.
        static mut environ: *const *const libc::c_char;
    }
    // SAFETY: the C library's environment pointer, read once, with no writer meanwhile.
    unsafe { *std::ptr::addr_of!(environ) }
}

/// The system's read of `pid`'s environment area, for adapter tests: the bytes of
/// `/proc/<pid>/environ`, which stop before a page the process made unreadable.
#[cfg(test)]
pub(super) fn read_environment_area(pid: libc::pid_t) -> io::Result<Vec<u8>> {
    std::fs::read(format!("/proc/{pid}/environ"))
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn argv_zero() -> Option<*mut libc::c_char> {
    None
}

/// # Safety
///
/// See `testing::exit_this_thread`.
#[cfg(any(test, feature = "test-support"))]
pub(super) unsafe fn exit_this_thread() -> ! {
    // SAFETY: the caller's contract; the exit system call ends only the calling thread,
    // even the main thread.
    unsafe {
        libc::syscall(libc::SYS_exit, 0i32);
        libc::_exit(125)
    }
}

#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __org_platform_before_main_linux {
    (unsafe $name:ident = $function:expr) => {
        #[used]
        #[unsafe(link_section = ".init_array")]
        static $name: unsafe extern "C" fn() = $function;
    };
}
#[cfg(any(test, feature = "test-support"))]
pub use crate::__org_platform_before_main_linux as run_before_main;

/// Linux delivers a signal to a group of only exited members, or answers `ESRCH`.
pub(super) const EXITED_GROUP_SIGNAL_IS_DENIED: bool = false;
/// [`session_flags`] start the child running.
pub(super) const SPAWNS_SUSPENDED: bool = false;
/// The spawn flag of [`session_flags`].
const POSIX_SPAWN_SETSID: libc::c_short = libc::POSIX_SPAWN_SETSID as libc::c_short;

#[cfg(any(test, feature = "test-support"))]
pub(super) use super::raw_spawn::spawn_raw;

#[cfg(any(test, feature = "test-support"))]
pub(super) fn posix_spawn_setsid() -> io::Result<libc::c_short> {
    Ok(POSIX_SPAWN_SETSID)
}
/// `scan.rs` pins each observed process with a pidfd.
pub(super) const SCAN_HOLDS_A_DESCRIPTOR_PER_PROCESS: bool = true;
/// `scan.rs` reads `/proc/<pid>/environ`, which stops at an unreadable page.
pub(super) const SCAN_ENVIRONMENT_READ_STOPS_AT_AN_UNREADABLE_PAGE: bool = true;
/// Linux ends a thread alone, the main thread too; the process lives on.
pub(super) const AN_ENDED_MAIN_THREAD_LEAVES_A_ZOMBIE_LEADER: bool = true;
pub(super) const SCAN_ENVIRONMENT_FOLLOWS_COUNTED_ARGUMENTS: bool = false;
pub(super) const SCAN_WITHHOLDS_RESTRICTED_ENVIRONMENTS: bool = false;

/// The targets of this process's descriptor links that are absolute paths.
#[cfg(any(test, feature = "test-support"))]
pub(super) fn open_paths() -> io::Result<Vec<std::path::PathBuf>> {
    let mut paths = Vec::new();
    for entry in std::fs::read_dir("/proc/self/fd")? {
        // The descriptor that lists this directory is closed before its link is read.
        if let Ok(target) = std::fs::read_link(entry?.path()) {
            if target.is_absolute() {
                paths.push(target);
            }
        }
    }
    Ok(paths)
}

pub(super) fn check_version() -> io::Result<()> {
    let release = std::fs::read_to_string("/proc/sys/kernel/osrelease")?;
    let version = |s: &str| -> Option<(u32, u32)> {
        let mut fields = s.split('.');
        Some((fields.next()?.parse().ok()?, fields.next()?.parse().ok()?))
    };
    // SAFETY: glibc returns a static, NUL-terminated version string.
    let glibc = unsafe { CStr::from_ptr(libc::gnu_get_libc_version()) }
        .to_str()
        .map_err(|_| invalid("invalid libc version"))?;
    if version(&release).is_none_or(|v| v < (5, 10)) || version(glibc).is_none_or(|v| v < (2, 34)) {
        return Err(unsupported(
            "org-supervise requires Linux 5.10 and glibc 2.34 or later",
        ));
    }
    Ok(())
}

pub(super) struct Watch(OwnedFd);

impl Watch {
    pub(super) fn open(pid: libc::pid_t) -> io::Result<Self> {
        // SAFETY: syscall arguments are an owned unreaped child pid and zero flags.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: pidfd_open returns a new owned close-on-exec descriptor.
        Ok(Self(unsafe { OwnedFd::from_raw_fd(fd as i32) }))
    }

    /// One wait of at most `remaining`. Negative with `errno` set on failure.
    pub(super) fn poll(&self, remaining: Duration) -> libc::c_int {
        let mut pfd = libc::pollfd {
            fd: self.0.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let millis = remaining
            .as_millis()
            .saturating_add(u128::from(
                !remaining.subsec_nanos().is_multiple_of(1_000_000),
            ))
            .min(i32::MAX as u128) as i32;
        // SAFETY: valid poll slot and finite timeout; pidfd pins the child identity.
        unsafe { libc::poll(&mut pfd, 1, millis) }
    }
}

pub(super) fn identity(pid: libc::pid_t) -> io::Result<(u32, String, String)> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let tail = stat
        .rsplit_once(')')
        .ok_or_else(|| invalid("invalid process stat"))?
        .1;
    let fields: Vec<_> = tail.split_whitespace().collect();
    let pgid = fields
        .get(2)
        .and_then(|x| x.parse().ok())
        .ok_or_else(|| invalid("invalid process group"))?;
    let started = fields
        .get(19)
        .ok_or_else(|| invalid("invalid process start identity"))?
        .to_string();
    let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
    Ok((pgid, started, boot.trim().to_owned()))
}

pub(super) fn exit_fields() -> io::Result<ExitFields> {
    Ok(sigchld_fields)
}

/// # Safety
///
/// `info` was written by a successful `waitid`.
unsafe fn sigchld_fields(info: &libc::siginfo_t) -> (libc::pid_t, libc::c_int) {
    // SAFETY: waitid returns the SIGCHLD variant of this union.
    unsafe { (info.si_pid(), info.si_status()) }
}

/// The child was started running: there is nothing to release, and `exited` is not
/// asked.
pub(super) fn release_spawned(
    _pid: libc::pid_t,
    _exited: impl FnOnce() -> io::Result<bool>,
) -> io::Result<()> {
    Ok(())
}

/// Linux delivers a group signal to a group whose only members have exited, so no
/// error stands for that case.
pub(super) fn is_exited_group_denial(_error: &io::Error) -> bool {
    false
}

/// Never asked on Linux, where [`is_exited_group_denial`] is never true. The group is
/// not proven empty, so the caller's signal error stands.
pub(super) fn group_contains_only_leader(_pid: libc::pid_t) -> io::Result<bool> {
    Ok(false)
}

/// `pipe2` sets close-on-exec atomically with creation.
pub(super) const PIPE_IS_CLOEXEC: bool = true;

pub(super) fn pipe() -> io::Result<[OwnedFd; 2]> {
    let mut pair = [-1; 2];
    // SAFETY: valid two-element out buffer; CLOEXEC is set atomically with creation.
    let rc = unsafe { libc::pipe2(pair.as_mut_ptr(), libc::O_CLOEXEC) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: pipe returned two distinct owned descriptors.
    Ok(unsafe { [OwnedFd::from_raw_fd(pair[0]), OwnedFd::from_raw_fd(pair[1])] })
}

/// Make the child start in `cwd`.
///
/// # Safety
///
/// `actions` is an initialized file-actions object, and `cwd` outlives the spawn call
/// that uses it.
pub(super) unsafe fn add_chdir(
    actions: *mut libc::posix_spawn_file_actions_t,
    cwd: &CStr,
) -> io::Result<()> {
    // SAFETY: the caller's initialized actions and NUL-terminated path.
    check(unsafe { libc::posix_spawn_file_actions_addchdir_np(actions, cwd.as_ptr()) })
}

/// The attribute flags of a piped session leader: a new session. Adds the action
/// that closes every descriptor from 3 up, after the actions already added.
///
/// # Safety
///
/// `actions` is an initialized file-actions object.
pub(super) unsafe fn session_flags(
    actions: *mut libc::posix_spawn_file_actions_t,
) -> io::Result<libc::c_int> {
    // SAFETY: the caller's initialized actions.
    check(unsafe { libc::posix_spawn_file_actions_addclosefrom_np(actions, 3) })?;
    let flags =
        i32::from(POSIX_SPAWN_SETSID) | libc::POSIX_SPAWN_SETSIGMASK | libc::POSIX_SPAWN_SETSIGDEF;
    Ok(flags)
}

/// The `Threads:` line of `/proc/self/status`.
pub(super) fn thread_count() -> io::Result<usize> {
    let status = std::fs::read_to_string("/proc/self/status")?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("Threads:"))
        .and_then(|count| count.trim().parse().ok())
        .ok_or_else(|| invalid("no thread count in /proc/self/status"))
}

pub(super) fn entropy(bytes: &mut [u8; 32]) -> io::Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        // SAFETY: this is the remaining initialized writable slice. Refuse an
        // uninitialized entropy pool instead of blocking past the recovery deadline.
        let count = unsafe {
            libc::getrandom(
                bytes[offset..].as_mut_ptr().cast(),
                bytes.len() - offset,
                libc::GRND_NONBLOCK,
            )
        };
        if count < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "empty entropy read",
            ));
        }
        offset += count as usize;
    }
    Ok(())
}

#[cfg(test)]
mod harden_tests {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::fs::MetadataExt;
    use std::process::{Command, Stdio};

    /// Set in the child of [`a_hardened_process_is_closed_to_its_own_user`].
    const CHILD: &str = "ORG_PLATFORM_TEST_HARDEN_CHILD";

    /// A process that hardens itself is closed to the processes of its own user: its core
    /// limits are 0, it is not dumpable, root owns its `/proc/<pid>/mem`, and another
    /// process of the same user is refused its memory and its executable link, where
    /// before it had both. The child of this test is the process that changes, so the
    /// test binary itself stays as it was for every other test.
    #[test]
    fn a_hardened_process_is_closed_to_its_own_user() {
        if std::env::var_os(CHILD).is_some() {
            return child();
        }
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "process::linux::harden_tests::a_hardened_process_is_closed_to_its_own_user",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = child.id();
        let mut stdin = child.stdin.take().unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let mut next = |want: &str| loop {
            let line = lines
                .next()
                .unwrap_or_else(|| panic!("the child ended before `{want}`"))
                .unwrap();
            // The test harness may print its own text before the child's on a line.
            if let Some((_, rest)) = line.split_once(want) {
                return rest.to_owned();
            }
        };
        // SAFETY: no arguments and no side effects; it cannot fail.
        let root = unsafe { libc::geteuid() } == 0;
        let memory = format!("/proc/{pid}/mem");
        let link = format!("/proc/{pid}/exe");
        // Before: an ordinary process, open to its own user.
        assert_eq!(next("before "), "dumpable=true hardened=false");
        assert!(std::fs::File::open(&memory).is_ok());
        assert!(std::fs::read_link(&link).is_ok());
        stdin.write_all(b"harden\n").unwrap();
        // After: closed. Root can still open it, so the denial is checked as another user.
        assert_eq!(
            next("after "),
            "dumpable=false hardened=true core=0/0 mem_uid=0"
        );
        if root {
            println!("as root: the denial to the same user is not checked (root is not refused)");
        } else {
            for denied in [
                std::fs::File::open(&memory).map(drop),
                std::fs::read_link(&link).map(drop),
                std::fs::read_dir(format!("/proc/{pid}/fd")).map(drop),
            ] {
                assert_eq!(
                    denied.unwrap_err().kind(),
                    std::io::ErrorKind::PermissionDenied
                );
            }
        }
        stdin.write_all(b"again\n").unwrap();
        assert_eq!(next("again "), "hardened=true");
        drop(stdin);
        assert!(child.wait().unwrap().success());
    }

    fn child() {
        let state = || {
            format!(
                "dumpable={} hardened={}",
                super::dumpable().unwrap(),
                super::hardened().unwrap()
            )
        };
        let mut input = std::io::stdin().lock();
        let mut wait = || {
            let mut line = String::new();
            input.read_line(&mut line).unwrap();
        };
        println!("before {}", state());
        wait();
        super::harden_self().unwrap();
        let mut limit = libc::rlimit {
            rlim_cur: 1,
            rlim_max: 1,
        };
        // SAFETY: the kernel writes one `rlimit` into valid storage.
        assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_CORE, &mut limit) }, 0);
        println!(
            "after {} core={}/{} mem_uid={}",
            state(),
            limit.rlim_cur,
            limit.rlim_max,
            std::fs::metadata("/proc/self/mem").unwrap().uid()
        );
        wait();
        // Hardening an already hardened process succeeds and changes nothing.
        super::harden_self().unwrap();
        println!("again hardened={}", super::hardened().unwrap());
    }
}
