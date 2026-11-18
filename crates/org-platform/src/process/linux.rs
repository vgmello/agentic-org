//! The GNU/Linux process adapter: Linux 5.10 and glibc 2.34 or later.

use std::ffi::CStr;
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
    let flags = i32::from(libc::POSIX_SPAWN_SETSID)
        | libc::POSIX_SPAWN_SETSIGMASK
        | libc::POSIX_SPAWN_SETSIGDEF;
    Ok(flags)
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
