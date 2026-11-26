//! Descriptor flags, duplication, readiness and the bytes a socket holds.
//!
//! A function that changes a descriptor (its flags, what it refers to) takes it through
//! [`AsFd`], so only its owner can change it. A function that only asks about a number
//! takes a [`RawFd`]: a number that is not an open descriptor is the kernel's `EBADF`.

use std::io;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::time::Duration;

use super::{check, check_size};

/// The descriptor flags of `fd` (`fcntl(F_GETFD)`). Only asks: a number that is not an
/// open descriptor is an error.
pub fn descriptor_flags(fd: RawFd) -> io::Result<libc::c_int> {
    // SAFETY: F_GETFD takes no third argument and only reads the descriptor table entry
    // for `fd`; any number, open or not, is a valid argument.
    check(unsafe { libc::fcntl(fd, libc::F_GETFD) })
}

/// Whether `fd` is an open descriptor of this process.
pub fn is_open(fd: RawFd) -> bool {
    descriptor_flags(fd).is_ok()
}

/// Whether `fd` is close-on-exec (`FD_CLOEXEC`).
pub fn is_close_on_exec(fd: RawFd) -> io::Result<bool> {
    Ok(descriptor_flags(fd)? & libc::FD_CLOEXEC != 0)
}

/// Make `fd` close-on-exec (`on`), or inherited by a program this process starts.
/// Other descriptor flags are kept.
pub fn set_close_on_exec(fd: impl AsFd, on: bool) -> io::Result<()> {
    let fd = fd.as_fd().as_raw_fd();
    let flags = descriptor_flags(fd)?;
    let flags = if on {
        flags | libc::FD_CLOEXEC
    } else {
        flags & !libc::FD_CLOEXEC
    };
    // SAFETY: `fd` is borrowed from its owner for this call; F_SETFD takes an int and
    // changes only that descriptor's flags.
    check(unsafe { libc::fcntl(fd, libc::F_SETFD, flags) }).map(drop)
}

/// The file status flags of `fd` (`fcntl(F_GETFL)`: `O_NONBLOCK`, `O_APPEND`, the access
/// mode). Only asks.
pub fn status_flags(fd: RawFd) -> io::Result<libc::c_int> {
    // SAFETY: F_GETFL takes no third argument and only reads the open file description.
    check(unsafe { libc::fcntl(fd, libc::F_GETFL) })
}

/// Set the file status flags of `fd` (`fcntl(F_SETFL)`). They belong to the open file
/// description, so every copy of the descriptor sees them.
pub fn set_status_flags(fd: impl AsFd, flags: libc::c_int) -> io::Result<()> {
    let fd = fd.as_fd().as_raw_fd();
    // SAFETY: `fd` is borrowed from its owner for this call; F_SETFL takes an int and
    // changes only the status flags of its open file description.
    check(unsafe { libc::fcntl(fd, libc::F_SETFL, flags) }).map(drop)
}

/// Set or clear `O_NONBLOCK` on `fd`, keeping its other status flags.
pub fn set_nonblocking(fd: impl AsFd, on: bool) -> io::Result<()> {
    let fd = fd.as_fd();
    let flags = status_flags(fd.as_raw_fd())?;
    let flags = if on {
        flags | libc::O_NONBLOCK
    } else {
        flags & !libc::O_NONBLOCK
    };
    set_status_flags(fd, flags)
}

/// A new close-on-exec copy of `fd` at the lowest free number not below `minimum`
/// (`fcntl(F_DUPFD_CLOEXEC)`), owned by the caller. `fd` itself is unchanged.
pub fn duplicate_above(fd: impl AsFd, minimum: RawFd) -> io::Result<OwnedFd> {
    duplicate_raw_above(fd.as_fd().as_raw_fd(), minimum)
}

/// [`duplicate_above`] for a descriptor number: the copy is new and owned by the caller,
/// and the number is neither closed nor changed.
pub(crate) fn duplicate_raw_above(fd: RawFd, minimum: RawFd) -> io::Result<OwnedFd> {
    // SAFETY: F_DUPFD_CLOEXEC takes an int and only adds a descriptor to the table.
    let copy = check(unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, minimum) })?;
    // SAFETY: a successful F_DUPFD_CLOEXEC returns a new descriptor that nothing else in
    // this process owns; ownership passes to the returned value exactly once.
    Ok(unsafe { OwnedFd::from_raw_fd(copy) })
}

/// Which of the three standard descriptors [`reopen_standard_on_null`] points at the null
/// device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standard {
    /// Descriptor 0.
    Input,
    /// Descriptor 1.
    Output,
    /// Descriptor 2.
    Error,
}

impl Standard {
    fn number(self) -> RawFd {
        match self {
            Self::Input => libc::STDIN_FILENO,
            Self::Output => libc::STDOUT_FILENO,
            Self::Error => libc::STDERR_FILENO,
        }
    }
}

/// Make `stream` refer to what `fd` refers to (`dup2`), so this process reads or writes it
/// there and a program it starts inherits it. Whatever the standard descriptor referred to
/// before is closed for it; take a copy first ([`duplicate_above`]) to keep it. `fd` itself
/// stays open and the caller's.
///
/// The standard descriptors belong to the process, not to a value: the standard library
/// never closes them. Call it before any thread reads or writes the standard streams.
pub fn replace_standard(stream: Standard, fd: impl AsFd) -> io::Result<()> {
    // SAFETY: `fd` is borrowed from its owner and the target is one of the three standard
    // numbers, which no Rust value owns; `dup2` atomically replaces what it refers to.
    check(unsafe { libc::dup2(fd.as_fd().as_raw_fd(), stream.number()) }).map(drop)
}

/// Points each of `streams` at `/dev/null` (read and write), so a program this process
/// starts inherits the null device there ([`replace_standard`]).
pub fn reopen_standard_on_null(streams: &[Standard]) -> io::Result<()> {
    let null = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/null")?;
    let null = OwnedFd::from(null);
    for &stream in streams {
        replace_standard(stream, &null)?;
    }
    if null.as_raw_fd() <= libc::STDERR_FILENO {
        // A standard descriptor was closed when this process started, so the null device
        // took its number: keep that number (inherited) and give up ownership of it.
        let number = std::os::fd::IntoRawFd::into_raw_fd(null);
        // SAFETY: `number` is the open descriptor just released from `null`; F_SETFD
        // with 0 only clears its close-on-exec flag.
        check(unsafe { libc::fcntl(number, libc::F_SETFD, 0) })?;
    }
    Ok(())
}

/// Waits up to `timeout` (`None`: no limit; zero: only checks) for an event on any of
/// `slots` (`poll`), and returns how many have one. Each slot's `revents` is filled in. A
/// slot whose number is not an open descriptor reports `POLLNVAL` and is not an error
/// here. An interrupted wait is [`io::ErrorKind::Interrupted`].
pub fn poll(slots: &mut [libc::pollfd], timeout: Option<Duration>) -> io::Result<usize> {
    let count = libc::nfds_t::try_from(slots.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "too many poll slots"))?;
    let millis = match timeout {
        None => -1,
        Some(timeout) if timeout.is_zero() => 0,
        Some(timeout) => timeout.as_millis().clamp(1, libc::c_int::MAX as u128) as libc::c_int,
    };
    // SAFETY: `slots` is a live, initialized, exclusively borrowed array of exactly
    // `count` pollfd records, which poll reads and whose `revents` it writes.
    let ready = check(unsafe { libc::poll(slots.as_mut_ptr(), count, millis) })?;
    Ok(ready as usize)
}

/// One `poll` slot that waits for `events` on `fd`.
pub fn poll_slot(fd: RawFd, events: libc::c_short) -> libc::pollfd {
    libc::pollfd {
        fd,
        events,
        revents: 0,
    }
}

/// How many bytes `fd` (a socket, pipe or terminal) holds ready to read (`FIONREAD`).
pub fn bytes_waiting(fd: impl AsFd) -> io::Result<usize> {
    let mut waiting: libc::c_int = 0;
    // SAFETY: `fd` is borrowed from its owner; FIONREAD writes one int to the valid,
    // exclusively borrowed `waiting`.
    check(unsafe { libc::ioctl(fd.as_fd().as_raw_fd(), libc::FIONREAD, &mut waiting) })?;
    Ok(usize::try_from(waiting).unwrap_or(0))
}

/// Receive into `buffer` from the socket `fd` (`recv`) with `flags` (for example
/// `MSG_PEEK | MSG_DONTWAIT`), and return the byte count (zero at end of stream).
pub fn receive(fd: impl AsFd, buffer: &mut [u8], flags: libc::c_int) -> io::Result<usize> {
    // SAFETY: `buffer` is live and exclusively borrowed for its whole length, which recv
    // writes at most; `fd` is borrowed from its owner.
    check_size(unsafe {
        libc::recv(
            fd.as_fd().as_raw_fd(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            flags,
        )
    })
}

/// Set one integer socket option of `fd` (`setsockopt` at `level`).
pub fn set_socket_option(
    fd: impl AsFd,
    level: libc::c_int,
    option: libc::c_int,
    value: libc::c_int,
) -> io::Result<()> {
    // SAFETY: `fd` is borrowed from its owner; the value pointer and length describe one
    // live int, which setsockopt only reads.
    check(unsafe {
        libc::setsockopt(
            fd.as_fd().as_raw_fd(),
            level,
            option,
            (&value as *const libc::c_int).cast(),
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        )
    })
    .map(drop)
}

/// One integer socket option of `fd` (`getsockopt` at `level`). A reply of another size
/// is an error.
pub fn socket_option(
    fd: RawFd,
    level: libc::c_int,
    option: libc::c_int,
) -> io::Result<libc::c_int> {
    let mut value: libc::c_int = 0;
    let mut length = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    // SAFETY: the value pointer and length describe one live, exclusively borrowed int,
    // and `length` is a live in/out socklen_t; getsockopt writes at most `length` bytes.
    check(unsafe {
        libc::getsockopt(
            fd,
            level,
            option,
            (&mut value as *mut libc::c_int).cast(),
            &mut length,
        )
    })?;
    if length as usize != std::mem::size_of::<libc::c_int>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected socket option size",
        ));
    }
    Ok(value)
}

/// The address family of the socket `fd` (`getsockname`'s `ss_family`: `AF_UNIX`,
/// `AF_INET`, `AF_INET6`).
pub fn socket_family(fd: RawFd) -> io::Result<libc::c_int> {
    // SAFETY: sockaddr_storage is plain data for which all-zero bytes are a valid value.
    let mut address: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of_val(&address) as libc::socklen_t;
    // SAFETY: the address pointer and `length` describe the live, exclusively borrowed
    // storage, which getsockname writes at most `length` bytes of.
    check(unsafe {
        libc::getsockname(
            fd,
            (&mut address as *mut libc::sockaddr_storage).cast(),
            &mut length,
        )
    })?;
    Ok(libc::c_int::from(address.ss_family))
}
