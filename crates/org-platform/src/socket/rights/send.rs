//! Descriptor sending (X-02 §5.2's sender rule): one nonblocking `sendmsg` carrying a payload and
//! exactly the given descriptors, under one overall deadline. `EINTR` retries; `EAGAIN` waits for
//! `POLLOUT` within the time left; `ENOBUFS` (macOS: a full datagram peer while `POLLOUT` is
//! already ready) backs off 2 ms and retries; `EMSGSIZE`, any other error and an expired deadline
//! are named errors. The caller keeps its descriptors until the receiver acknowledges them.
//!
//! The caller owns `SIGPIPE`: on macOS a send to a closed stream raises it unless it is ignored
//! (Rust binaries ignore it) or the socket has `SO_NOSIGPIPE`. Linux sends with `MSG_NOSIGNAL`.

use crate::socket::send_flags;
use std::fmt;
use std::io;
use std::mem;
use std::os::fd::{AsRawFd, BorrowedFd, RawFd};
use std::time::{Duration, Instant};

use super::{control_buffer, control_space, invalid};

/// How long an `ENOBUFS` waits before the next attempt.
const NO_BUFFER_BACKOFF: Duration = Duration::from_millis(2);

/// Why a send didn't complete. It never quotes the payload.
#[derive(Debug)]
pub enum SendError {
    /// The deadline passed first: nothing was sent.
    TimedOut,
    /// The message doesn't fit the socket (`EMSGSIZE`): nothing was sent.
    TooLarge,
    /// A stream took only part of the payload: the connection is out of step.
    Short,
    /// Any other failure.
    Io(io::Error),
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TimedOut => f.write_str("descriptor send timed out"),
            Self::TooLarge => f.write_str("descriptor message too large"),
            Self::Short => f.write_str("descriptor message partly sent"),
            Self::Io(error) => write!(f, "descriptor send failed: {error}"),
        }
    }
}

impl std::error::Error for SendError {}

/// Send `payload` (at least one byte) with exactly `descriptors` in one `sendmsg` on `socket`,
/// a connected UNIX socket, by `deadline`. The socket's own blocking mode doesn't matter.
pub fn send(
    socket: BorrowedFd<'_>,
    payload: &[u8],
    descriptors: &[BorrowedFd<'_>],
    deadline: Instant,
) -> Result<(), SendError> {
    let fds: Vec<RawFd> = descriptors.iter().map(AsRawFd::as_raw_fd).collect();
    send_with(
        socket.as_raw_fd(),
        payload,
        &fds,
        deadline,
        system_send,
        wait_writable,
    )
}

// Private interposition lets tests drive each errno through the real retry loop.
fn send_with(
    socket: RawFd,
    payload: &[u8],
    fds: &[RawFd],
    deadline: Instant,
    mut call: impl FnMut(RawFd, &libc::msghdr) -> io::Result<usize>,
    mut wait: impl FnMut(RawFd, Duration) -> io::Result<()>,
) -> Result<(), SendError> {
    send_using(
        payload,
        fds,
        deadline,
        |message| call(socket, message),
        |left| wait(socket, left),
        std::thread::sleep,
        Instant::now,
    )
}

// The retry policy owns its clock and backoff boundaries, independent of syscall timing.
fn send_using(
    payload: &[u8],
    fds: &[RawFd],
    deadline: Instant,
    mut call: impl FnMut(&libc::msghdr) -> io::Result<usize>,
    mut wait: impl FnMut(Duration) -> io::Result<()>,
    mut pause: impl FnMut(Duration),
    mut now: impl FnMut() -> Instant,
) -> Result<(), SendError> {
    if payload.is_empty() {
        return Err(SendError::Io(invalid("a descriptor message needs a byte")));
    }
    let space = control_space(fds.len())
        .ok_or_else(|| SendError::Io(invalid("descriptor count is too large")))?;
    let mut control = control_buffer(space).map_err(SendError::Io)?;
    let mut data = libc::iovec {
        iov_base: payload.as_ptr().cast_mut().cast(),
        iov_len: payload.len(),
    };
    let message = rights_message(&mut data, &mut control, space, fds);
    loop {
        time_left(deadline, now())?;
        let error = match call(&message) {
            Ok(sent) if sent == payload.len() => return Ok(()),
            Ok(_) => return Err(SendError::Short),
            Err(error) => error,
        };
        match error.raw_os_error() {
            Some(libc::EINTR) => {}
            Some(libc::EAGAIN) => match wait(time_left(deadline, now())?) {
                Err(e) if e.kind() != io::ErrorKind::Interrupted => return Err(SendError::Io(e)),
                _ => {}
            },
            Some(libc::ENOBUFS) => pause(NO_BUFFER_BACKOFF.min(time_left(deadline, now())?)),
            Some(libc::EMSGSIZE) => return Err(SendError::TooLarge),
            _ => return Err(SendError::Io(error)),
        }
    }
}

/// The `msghdr` of one datagram or stream write: `data` as its one buffer and, when `fds`
/// is not empty, one `SCM_RIGHTS` header carrying them in `control`, which `space`
/// ([`control_space`] of `fds.len()`) bytes of it hold. The message points into `data` and
/// `control`: the caller keeps both live and unmoved while it is used.
fn rights_message(
    data: &mut libc::iovec,
    control: &mut [usize],
    space: usize,
    fds: &[RawFd],
) -> libc::msghdr {
    assert!(
        space <= mem::size_of_val(control),
        "control storage is too small"
    );
    assert!(
        control_space(fds.len()).is_some_and(|needed| needed <= space),
        "control space is too small for the descriptors"
    );
    // SAFETY: msghdr is plain data for which all-zero bytes (null pointers, zero lengths)
    // are a valid value.
    let mut message: libc::msghdr = unsafe { mem::zeroed() };
    message.msg_iov = data;
    message.msg_iovlen = 1;
    if !fds.is_empty() {
        message.msg_control = control.as_mut_ptr().cast();
        message.msg_controllen = space as _;
        // SAFETY: `message` describes the live `control` storage of `space` bytes, which
        // CMSG_FIRSTHDR only reads the length and pointer of.
        let header = unsafe { libc::CMSG_FIRSTHDR(&message) };
        assert!(!header.is_null(), "control space holds no header");
        // SAFETY: `control` is live, usize-aligned (which meets cmsghdr's alignment on the
        // supported systems) and holds `space` bytes, at least what `control_space` (checked
        // above) sizes for one header and `fds.len()` descriptors, so the header and data
        // writes below stay inside it. The descriptors are copied as bytes, so the data's
        // own alignment does not matter.
        unsafe {
            (*header).cmsg_level = libc::SOL_SOCKET;
            (*header).cmsg_type = libc::SCM_RIGHTS;
            (*header).cmsg_len = libc::CMSG_LEN(mem::size_of_val(fds) as u32) as _;
            std::ptr::copy_nonoverlapping(
                fds.as_ptr().cast::<u8>(),
                libc::CMSG_DATA(header),
                mem::size_of_val(fds),
            );
        }
    }
    message
}

/// One `sendmsg` of `payload` with exactly `descriptors` (none or more, in one
/// `SCM_RIGHTS` header) on `socket`, with `flags` and no retry: the bytes sent, or the
/// system's error. [`send`] is the sender rule (deadline, retries, `SIGPIPE`); this is the
/// single attempt, for a caller that drives its own loop or must send exactly what a peer
/// would (a test of a receiver). The descriptors stay the caller's.
pub fn send_once(
    socket: BorrowedFd<'_>,
    payload: &[u8],
    descriptors: &[BorrowedFd<'_>],
    flags: libc::c_int,
) -> io::Result<usize> {
    let fds: Vec<RawFd> = descriptors.iter().map(AsRawFd::as_raw_fd).collect();
    let space = control_space(fds.len()).ok_or_else(|| invalid("descriptor count is too large"))?;
    let mut control = control_buffer(space)?;
    let mut data = libc::iovec {
        iov_base: payload.as_ptr().cast_mut().cast(),
        iov_len: payload.len(),
    };
    let message = rights_message(&mut data, &mut control, space, &fds);
    // SAFETY: the message's buffers (`data`, `control`, `payload`) are live and unmoved for
    // the call, which only reads them; the socket and descriptors are borrowed.
    let sent = unsafe { libc::sendmsg(socket.as_raw_fd(), &message, flags) };
    if sent < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(sent as usize)
    }
}

/// Recompute the remaining budget after every operation that can consume deadline time.
fn time_left(deadline: Instant, now: Instant) -> Result<Duration, SendError> {
    let left = deadline.saturating_duration_since(now);
    if left.is_zero() {
        Err(SendError::TimedOut)
    } else {
        Ok(left)
    }
}

fn system_send(socket: RawFd, message: &libc::msghdr) -> io::Result<usize> {
    let flags = send_flags()?;
    // SAFETY: the message's buffers are live for the call, which only reads them.
    let sent = unsafe { libc::sendmsg(socket, message, flags) };
    if sent < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(sent as usize)
    }
}

/// Waits up to `left` for room to send. Readiness isn't reported: the loop simply tries again.
fn wait_writable(socket: RawFd, left: Duration) -> io::Result<()> {
    wait_writable_with(socket, left, |slot, millis| {
        // SAFETY: one initialized pollfd, valid for the call.
        unsafe { libc::poll(slot, 1, millis) }
    })
}

// Keep poll argument/error handling in the same owner for real calls and directed tests.
fn wait_writable_with(
    socket: RawFd,
    left: Duration,
    poll: impl FnOnce(&mut libc::pollfd, libc::c_int) -> libc::c_int,
) -> io::Result<()> {
    let mut slot = libc::pollfd {
        fd: socket,
        events: libc::POLLOUT,
        revents: 0,
    };
    let millis = left.as_millis().clamp(1, libc::c_int::MAX as u128) as libc::c_int;
    if poll(&mut slot, millis) < 0 {
        return Err(io::Error::last_os_error());
    }
    if slot.revents & libc::POLLNVAL != 0 {
        return Err(io::Error::from_raw_os_error(libc::EBADF));
    }
    Ok(())
}

/// Set a socket's send and receive buffers to `bytes` each (`SO_SNDBUF`, `SO_RCVBUF`): X-02
/// §5.2 sets both ends of a transfer socket explicitly.
pub fn set_buffer_sizes(socket: BorrowedFd<'_>, bytes: usize) -> io::Result<()> {
    let size = libc::c_int::try_from(bytes).map_err(|_| invalid("buffer size too large"))?;
    for option in [libc::SO_SNDBUF, libc::SO_RCVBUF] {
        // SAFETY: a live socket, and an in-value of the size passed.
        let rc = unsafe {
            libc::setsockopt(
                socket.as_raw_fd(),
                libc::SOL_SOCKET,
                option,
                (&size as *const libc::c_int).cast(),
                mem::size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
