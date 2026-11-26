//! Process file descriptors: opening one, sending an individual signal through it,
//! waiting for its exit, and the descriptor errors recovery reports.

use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::time::{Duration, Instant};

pub(super) fn vanished(err: &io::Error) -> bool {
    matches!(err.raw_os_error(), Some(libc::ENOENT | libc::ESRCH))
}

pub(super) fn descriptor_error(err: io::Error) -> io::Error {
    if matches!(err.raw_os_error(), Some(libc::EMFILE | libc::ENFILE)) {
        io::Error::new(err.kind(), DescriptorExhaustion(err))
    } else {
        err
    }
}

#[derive(Debug)]
pub(super) struct DescriptorExhaustion(io::Error);

impl std::fmt::Display for DescriptorExhaustion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("doctor: Linux recovery exhausted file descriptors; increase RLIMIT_NOFILE or reduce the process count")
    }
}

impl std::error::Error for DescriptorExhaustion {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

pub(super) fn pidfd_startup_error(err: io::Error) -> io::Error {
    match err.raw_os_error() {
        Some(libc::ENOSYS | libc::EPERM) => io::Error::new(
            io::ErrorKind::Unsupported,
            "doctor: Linux recovery requires pidfd_open and pidfd_send_signal permissions",
        ),
        _ => err,
    }
}

pub(super) fn pidfd(pid: u32) -> io::Result<OwnedFd> {
    // SAFETY: validated PID in the caller's namespace, no flags or pointers.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0u32) };
    if fd < 0 {
        return Err(descriptor_error(io::Error::last_os_error()));
    }
    // SAFETY: successful pidfd_open returned a new close-on-exec descriptor.
    Ok(unsafe { OwnedFd::from_raw_fd(fd as libc::c_int) })
}

pub(super) fn send(fd: &OwnedFd, signal: libc::c_int) -> io::Result<()> {
    // SAFETY: a live owned pidfd, individual signal, null siginfo and zero flags.
    if unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            fd.as_raw_fd(),
            signal,
            std::ptr::null::<libc::siginfo_t>(),
            0u32,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(super) fn exited(fd: &OwnedFd, deadline: Option<Instant>) -> io::Result<bool> {
    let mut slot = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    poll_until(deadline, Instant::now, |remaining| {
        let timeout = libc::timespec {
            tv_sec: remaining.as_secs().min(libc::time_t::MAX as u64) as libc::time_t,
            tv_nsec: remaining.subsec_nanos() as _,
        };
        // SAFETY: one valid slot for a held pidfd, a finite timeout and no signal mask.
        match unsafe { libc::ppoll(&mut slot, 1, &timeout, std::ptr::null()) } {
            result if result < 0 => Err(io::Error::last_os_error()),
            0 => Ok(false),
            _ if slot.revents & (libc::POLLIN | libc::POLLHUP) != 0 => Ok(true),
            _ => Err(io::Error::other("unexpected pidfd poll event")),
        }
    })
}

pub(super) fn poll_until(
    deadline: Option<Instant>,
    mut now: impl FnMut() -> Instant,
    mut poll: impl FnMut(Duration) -> io::Result<bool>,
) -> io::Result<bool> {
    loop {
        let current = now();
        let remaining = match deadline {
            Some(deadline) if current >= deadline => return Ok(false),
            Some(deadline) => deadline.duration_since(current),
            None => Duration::ZERO,
        };
        let ready = poll(remaining);
        // ppoll can observe readiness on a rescan after its timer expires.
        if deadline.is_some_and(|deadline| now() >= deadline) {
            return Ok(false);
        }
        match ready {
            Ok(false) if deadline.is_some() => continue,
            Err(err) if deadline.is_some() && err.kind() == io::ErrorKind::Interrupted => continue,
            result => return result,
        }
    }
}
