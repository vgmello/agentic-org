//! What consumers' tests do with processes and terminals that a product never does: a
//! forked child that only holds or probes what it inherited, a descriptor a previous
//! image left at a number, and a pseudo-terminal (`test-support`).

use std::io;
use std::os::fd::{FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::Command;

use super::check;

/// What a child of [`fork_child`] does. Each is only async-signal-safe calls ending in
/// `_exit`, so the child is sound whatever other threads the parent has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForkedChild {
    /// Close `close` (the parent's end of a release channel), then wait for one byte on
    /// `wait`: exit 0 when it is `1`, else 42. Until then the child holds every
    /// descriptor it inherited (a lock file's, a database's).
    HoldUntilByte {
        /// The descriptor the child closes first.
        close: RawFd,
        /// The descriptor the child reads its release byte from.
        wait: RawFd,
    },
    /// Ask whether a write lock (`fcntl(F_GETLK)`) on the inherited `fd`'s file over
    /// `start..start + length` conflicts with a lock another process holds: exit 0 when one
    /// does, 41 when none does, 42 when the query fails. A lock is invisible to its own
    /// process, so this asks from another one.
    ProbeWriteLock {
        /// The inherited descriptor of the file.
        fd: RawFd,
        /// The first byte of the range.
        start: i64,
        /// The range's length.
        length: i64,
    },
    /// Sleep `seconds` and exit 0: a process that exists for a while.
    SleepThenExit {
        /// How long the child lives.
        seconds: u32,
    },
}

/// Fork a child that does only `child` and exits; this process gets its ID and reaps it
/// ([`super::reap`]).
pub fn fork_child(child: ForkedChild) -> io::Result<libc::pid_t> {
    // SAFETY: the child runs only `run_child`, which makes async-signal-safe calls on plain
    // integers copied before the fork and ends in `_exit`, so no lock or allocation of
    // another thread is touched in it.
    let pid = check(unsafe { libc::fork() })?;
    if pid == 0 {
        run_child(child);
    }
    Ok(pid)
}

fn run_child(child: ForkedChild) -> ! {
    // SAFETY: each arm calls only close, read, fcntl, sleep and _exit, which are
    // async-signal-safe, on inherited descriptor numbers and stack values of this child.
    unsafe {
        match child {
            ForkedChild::HoldUntilByte { close, wait } => {
                libc::close(close);
                let mut byte = 0u8;
                let read = libc::read(wait, (&mut byte as *mut u8).cast(), 1);
                libc::_exit(if read == 1 && byte == 1 { 0 } else { 42 })
            }
            ForkedChild::ProbeWriteLock { fd, start, length } => {
                let mut lock: libc::flock = std::mem::zeroed();
                lock.l_type = libc::F_WRLCK as _;
                lock.l_whence = libc::SEEK_SET as _;
                lock.l_start = start as _;
                lock.l_len = length as _;
                let result = libc::fcntl(fd, libc::F_GETLK, &mut lock);
                libc::_exit(if result < 0 {
                    42
                } else if lock.l_type == libc::F_UNLCK as libc::c_short {
                    41
                } else {
                    0
                })
            }
            ForkedChild::SleepThenExit { seconds } => {
                libc::sleep(seconds);
                libc::_exit(0)
            }
        }
    }
}

/// A new owned, close-on-exec copy of the descriptor a previous image of this process left
/// open at `fd` for it (its number passed in an argument). The number itself stays open and
/// is never closed by this process, so nothing that may own it loses it.
pub fn adopt_inherited(fd: RawFd) -> io::Result<OwnedFd> {
    super::descriptor::duplicate_raw_above(fd, 3)
}

/// A new pseudo-terminal (`openpty`): its controlling side and its terminal side, both
/// close-on-exec, so only a child given one of them explicitly inherits it.
pub fn open_pseudo_terminal() -> io::Result<(OwnedFd, OwnedFd)> {
    let (mut controller, mut terminal) = (-1, -1);
    // SAFETY: openpty writes the two live, exclusively borrowed ints; no name, settings or
    // window size is asked for or given.
    let opened = unsafe {
        libc::openpty(
            &mut controller,
            &mut terminal,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if opened != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a successful openpty returns two new descriptors that nothing else owns.
    let (controller, terminal) = unsafe {
        (
            OwnedFd::from_raw_fd(controller),
            OwnedFd::from_raw_fd(terminal),
        )
    };
    for side in [&controller, &terminal] {
        super::set_close_on_exec(side, true)?;
    }
    Ok((controller, terminal))
}

/// Start `command` in a new session whose controlling terminal is the terminal on its
/// standard input (`setsid`, then `TIOCSCTTY`), as a person's shell would. Give it a
/// terminal side of [`open_pseudo_terminal`] as its standard input.
pub fn with_controlling_terminal(command: &mut Command) -> &mut Command {
    // SAFETY: the closure runs in the child between fork and exec and calls only setsid
    // and ioctl, which are async-signal-safe, and reads errno; it touches no state of the
    // parent.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        })
    }
}

/// Hand `fd` to the program `command` starts at the descriptor number `target`, not
/// close-on-exec there (`dup2` between fork and exec; where `fd` already is `target`, only
/// its close-on-exec flag is cleared, in the child). Keep `fd` open until the spawn.
pub fn inherit_at(
    command: &mut Command,
    fd: impl std::os::fd::AsFd,
    target: RawFd,
) -> &mut Command {
    let fd = std::os::fd::AsRawFd::as_raw_fd(&fd.as_fd());
    // SAFETY: the closure runs in the child between fork and exec and calls only fcntl and
    // dup2, which are async-signal-safe, on plain numbers copied before the fork; it
    // touches no state of the parent.
    unsafe {
        command.pre_exec(move || {
            let rc = if fd == target {
                libc::fcntl(target, libc::F_SETFD, 0)
            } else {
                libc::dup2(fd, target)
            };
            if rc < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        })
    }
}

/// Start the program `command` starts with each of `ignored` ignored (`SIG_IGN`) and each
/// of `defaulted` at its default disposition (`SIG_DFL`), as a launcher that changed them
/// would: set between fork and exec, so this process keeps its own.
pub fn with_dispositions<'a>(
    command: &'a mut Command,
    ignored: &[libc::c_int],
    defaulted: &[libc::c_int],
) -> &'a mut Command {
    let changes: Vec<(libc::c_int, libc::sighandler_t)> = ignored
        .iter()
        .map(|&signal| (signal, libc::SIG_IGN))
        .chain(defaulted.iter().map(|&signal| (signal, libc::SIG_DFL)))
        .collect();
    // SAFETY: the closure runs in the child between fork and exec, reads the list made
    // before the fork without allocating, and calls only signal, which is async-signal-safe.
    unsafe {
        command.pre_exec(move || {
            for &(signal, disposition) in &changes {
                if libc::signal(signal, disposition) == libc::SIG_ERR {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        })
    }
}
