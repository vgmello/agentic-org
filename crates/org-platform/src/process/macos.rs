//! The macOS process adapter: macOS 13 or later.

use std::ffi::{CStr, CString};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::time::Duration;

use super::{check, invalid, unsupported, ExitFields, Uids};

mod scan;

pub(super) use scan::RecoveryScan;

pub(super) fn uids() -> io::Result<Uids> {
    // SAFETY: querying process credentials, without changing them.
    unsafe {
        let real = libc::getuid();
        let effective = libc::geteuid();
        Ok(Uids {
            real,
            effective,
            saved: effective,
            tainted: libc::issetugid() != 0,
        })
    }
}

fn sysctl(name: &str) -> io::Result<String> {
    let name = CString::new(name).unwrap();
    let mut data = [0u8; 256];
    let mut size = data.len();
    // SAFETY: valid output buffer and its byte length; no new value is supplied.
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            data.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    CStr::from_bytes_until_nul(&data[..size])
        .map_err(|_| invalid("invalid system identity"))?
        .to_str()
        .map(str::to_owned)
        .map_err(|_| invalid("invalid system identity"))
}

/// Disables core dumps. macOS has no attribute that closes a process to its own user:
/// a signed release image is kept closed by its code signature.
pub(super) fn harden_self() -> io::Result<()> {
    super::no_core_dumps()
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn hardened() -> io::Result<bool> {
    super::shared::core_dumps_disabled()
}

/// macOS has no subreaper: an orphan goes to `launchd`.
#[cfg(any(test, feature = "test-support"))]
pub(super) fn become_subreaper() -> io::Result<bool> {
    Ok(false)
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn is_stopped(pid: libc::pid_t) -> io::Result<bool> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: a correctly sized output record for PROC_PIDTBSDINFO.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if written != size {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: proc_pidinfo wrote the complete record.
    Ok(unsafe { info.assume_init() }.pbi_status == libc::SSTOP)
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn argv_zero() -> Option<*mut libc::c_char> {
    unsafe extern "C" {
        fn _NSGetArgv() -> *mut *mut *mut libc::c_char;
    }
    // SAFETY: _NSGetArgv returns this process's live argv vector.
    Some(unsafe { **_NSGetArgv() })
}

/// # Safety
///
/// See `testing::exit_this_thread`.
#[cfg(any(test, feature = "test-support"))]
pub(super) unsafe fn exit_this_thread() -> ! {
    // SAFETY: the caller's contract; ends only the calling thread.
    unsafe { libc::pthread_exit(std::ptr::null_mut()) }
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn posix_spawn_setsid() -> io::Result<libc::c_short> {
    Ok(POSIX_SPAWN_SETSID)
}

#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __org_platform_before_main_macos {
    (unsafe $name:ident = $function:expr) => {
        #[used]
        #[unsafe(link_section = "__DATA,__mod_init_func")]
        static $name: unsafe extern "C" fn() = $function;
    };
}
#[cfg(any(test, feature = "test-support"))]
pub use crate::__org_platform_before_main_macos as run_before_main;

/// XNU answers a signal to a group of only exited members with `EPERM`.
pub(super) const EXITED_GROUP_SIGNAL_IS_DENIED: bool = true;
/// [`session_flags`] start the child suspended.
pub(super) const SPAWNS_SUSPENDED: bool = true;
/// Darwin spawn.h defines `POSIX_SPAWN_SETSID` as 0x0400; libc doesn't export it.
const POSIX_SPAWN_SETSID: libc::c_short = 0x0400;
pub(super) const SCAN_SOURCE: super::ScanSource = super::ScanSource::Sysctl;

/// The names `lsof` lists for this process's open files.
#[cfg(any(test, feature = "test-support"))]
pub(super) fn open_paths() -> io::Result<Vec<std::path::PathBuf>> {
    let pid = std::process::id().to_string();
    let out = std::process::Command::new("/usr/sbin/lsof")
        .args(["-n", "-P", "-Fn", "-p", &pid])
        .output()?;
    let listing = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() || !listing.lines().any(|line| line == format!("p{pid}")) {
        return Err(io::Error::other("lsof did not list this process"));
    }
    Ok(listing
        .lines()
        .filter_map(|line| line.strip_prefix('n'))
        .filter(|name| name.starts_with('/'))
        .map(std::path::PathBuf::from)
        .collect())
}

pub(super) fn check_version() -> io::Result<()> {
    if sysctl("kern.osrelease")?
        .split('.')
        .next()
        .and_then(|x| x.parse::<u32>().ok())
        .is_none_or(|x| x < 22)
    {
        return Err(unsupported("org-supervise requires macOS 13 or later"));
    }
    Ok(())
}

pub(super) struct Watch(OwnedFd);

impl Watch {
    pub(super) fn open(pid: libc::pid_t) -> io::Result<Self> {
        // SAFETY: kqueue returns a new owned descriptor; event pointers are valid.
        let fd = unsafe { libc::kqueue() };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: ownership of the newly created descriptor.
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        // SAFETY: valid descriptor, flags do not transfer ownership.
        if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            return Err(io::Error::last_os_error());
        }
        let event = libc::kevent {
            ident: pid as usize,
            filter: libc::EVFILT_PROC,
            flags: libc::EV_ADD | libc::EV_ENABLE,
            fflags: libc::NOTE_EXIT,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        // SAFETY: valid initialized registration and owned queue. No events requested yet.
        if unsafe {
            libc::kevent(
                fd.as_raw_fd(),
                &event,
                1,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
            )
        } < 0
        {
            let e = io::Error::last_os_error();
            // A fast child may already be a zombie: waitid is authoritative before waiting.
            if e.raw_os_error() != Some(libc::ESRCH) {
                return Err(e);
            }
        }
        Ok(Self(fd))
    }

    /// One wait of at most `remaining`. Negative with `errno` set on failure.
    pub(super) fn poll(&self, remaining: Duration) -> libc::c_int {
        let time = libc::timespec {
            tv_sec: remaining.as_secs().min(i64::MAX as u64) as _,
            tv_nsec: remaining.subsec_nanos() as _,
        };
        let mut event = std::mem::MaybeUninit::<libc::kevent>::uninit();
        // SAFETY: valid queue, output slot and finite timeout.
        unsafe {
            libc::kevent(
                self.0.as_raw_fd(),
                std::ptr::null(),
                0,
                event.as_mut_ptr(),
                1,
                &time,
            )
        }
    }
}

pub(super) fn identity(pid: libc::pid_t) -> io::Result<(u32, String, String)> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let size = std::mem::size_of::<libc::proc_bsdinfo>();
    // SAFETY: proc_pidinfo writes at most size bytes into the correctly sized slot.
    let rc = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size as i32,
        )
    };
    if rc != size as i32 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a complete proc_bsdinfo was returned.
    let info = unsafe { info.assume_init() };
    Ok((
        info.pbi_pgid,
        format!("{}:{}", info.pbi_start_tvsec, info.pbi_start_tvusec),
        sysctl("kern.bootsessionuuid")?,
    ))
}

pub(super) fn exit_fields() -> io::Result<ExitFields> {
    Ok(sigchld_fields)
}

/// # Safety
///
/// `info` was written by a successful `waitid`.
unsafe fn sigchld_fields(info: &libc::siginfo_t) -> (libc::pid_t, libc::c_int) {
    (info.si_pid, info.si_status)
}

/// The child was started suspended (`POSIX_SPAWN_START_SUSPENDED`): continue it.
pub(super) fn release_spawned(
    pid: libc::pid_t,
    exited: impl FnOnce() -> io::Result<bool>,
) -> io::Result<()> {
    if !exited()? {
        // SAFETY: the leader is our unreaped child; no identity can be reused here.
        if unsafe { libc::kill(pid, libc::SIGCONT) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// XNU kern_sig.c:1612-1621 filters SZOMB during group iteration, then returns EPERM when
/// no live member was signalled.
/// https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_sig.c#L1612-L1621
pub(super) fn is_exited_group_denial(error: &io::Error) -> bool {
    error.raw_os_error() == Some(libc::EPERM)
}

/// Called only for an exited WNOWAIT-owned leader after group signalling returned EPERM.
/// XNU kern_sig.c:1612-1621 filters SZOMB during group iteration, then returns EPERM when
/// no live member was signalled. Never infer an empty group from that error alone.
/// https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_sig.c#L1612-L1621
pub(super) fn group_contains_only_leader(pid: libc::pid_t) -> io::Result<bool> {
    group_contains_only_leader_with(pid, |members| {
        let (buffer, bytes) = match members {
            None => (std::ptr::null_mut(), 0),
            Some(members) => (
                members.as_mut_ptr().cast(),
                std::mem::size_of_val(members) as i32,
            ),
        };
        // proc_listpgrppids returns a COUNT, whereas proc_listpids returns BYTES. The
        // null-buffer estimate may cover all system processes; it is only a size hint.
        // SAFETY: null sizing query or initialized pid buffer; query confined to held pgid.
        let count = unsafe { libc::proc_listpgrppids(pid, buffer, bytes) };
        if count <= 0 {
            return Err(io::Error::other(
                "cannot enumerate the retained leader's group",
            ));
        }
        Ok(count as usize)
    })
}

fn group_contains_only_leader_with(
    pid: libc::pid_t,
    mut list: impl FnMut(Option<&mut [libc::pid_t]>) -> io::Result<usize>,
) -> io::Result<bool> {
    let estimate = list(None)?;
    if estimate == 0 {
        return Err(io::Error::other("empty group size estimate"));
    }
    let capacity = estimate
        .checked_add(16)
        .filter(|size| *size <= i32::MAX as usize / std::mem::size_of::<libc::pid_t>())
        .ok_or_else(|| invalid("process group snapshot exceeds native buffer size"))?;
    let mut members = Vec::new();
    members
        .try_reserve_exact(capacity)
        .map_err(io::Error::other)?;
    members.resize(capacity, 0);
    let count = list(Some(&mut members))?;
    if count == 0 || count >= capacity {
        return Err(io::Error::other(
            "empty or truncated process group snapshot",
        ));
    }
    Ok(count == 1 && members[0] == pid)
}

/// Darwin has no `pipe2`: the new descriptors are not close-on-exec yet.
pub(super) const PIPE_IS_CLOEXEC: bool = false;

pub(super) fn pipe() -> io::Result<[OwnedFd; 2]> {
    let mut pair = [-1; 2];
    // SAFETY: valid out buffer. Darwin has no pipe2: other embedder spawn paths must
    // also close descriptors by default, or serialize their spawn with this entire call.
    let rc = unsafe { libc::pipe(pair.as_mut_ptr()) };
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

/// The attribute flags of a piped session leader: a new session, started suspended,
/// with every descriptor closed that the file actions do not name.
///
/// # Safety
///
/// `actions` is an initialized file-actions object. This adapter adds nothing to it.
pub(super) unsafe fn session_flags(
    _actions: *mut libc::posix_spawn_file_actions_t,
) -> io::Result<libc::c_int> {
    let flags = libc::POSIX_SPAWN_CLOEXEC_DEFAULT
        | libc::POSIX_SPAWN_START_SUSPENDED
        | libc::c_int::from(POSIX_SPAWN_SETSID)
        | libc::POSIX_SPAWN_SETSIGMASK
        | libc::POSIX_SPAWN_SETSIGDEF;
    Ok(flags)
}

pub(super) fn entropy(bytes: &mut [u8; 32]) -> io::Result<()> {
    loop {
        // SAFETY: getentropy accepts this writable buffer, which is below its 256-byte limit.
        if unsafe { libc::getentropy(bytes.as_mut_ptr().cast(), bytes.len()) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

#[cfg(test)]
mod zombie_group_tests {
    use super::*;
    #[test]
    fn only_the_complete_single_owned_leader_snapshot_is_accepted() {
        for (snapshot, expected) in [(vec![19], true), (vec![19, 20], false), (vec![20], false)] {
            assert_eq!(
                group_contains_only_leader_with(19, |members| {
                    let Some(members) = members else {
                        return Ok(2);
                    };
                    members[..snapshot.len()].copy_from_slice(&snapshot);
                    Ok(snapshot.len())
                })
                .unwrap(),
                expected
            );
        }
        assert!(
            group_contains_only_leader_with(19, |_| Err(io::Error::other("sizing denied")))
                .is_err()
        );
        assert!(
            group_contains_only_leader_with(19, |members| if members.is_none() {
                Ok(2)
            } else {
                Err(io::Error::other("enumeration denied"))
            })
            .is_err()
        );
        assert!(
            group_contains_only_leader_with(19, |members| Ok(members.map_or(2, |p| p.len())))
                .is_err()
        );
        assert!(
            group_contains_only_leader_with(19, |members| Ok(if members.is_none() {
                2
            } else {
                0
            }))
            .is_err()
        );
        assert!(group_contains_only_leader_with(19, |_| Ok(0)).is_err());
        assert!(group_contains_only_leader_with(19, |_| Ok(usize::MAX)).is_err());
    }
}
