//! Validate a received capability before it becomes a serving stream.
use super::invalid;
#[cfg(not(test))]
use libc::{fcntl, getsockname, getsockopt};
use std::io;
use std::mem;
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::os::unix::net::{SocketAddr, UnixStream};
#[cfg(test)]
use tests::{fcntl, getsockname, getsockopt, peer_address};

/// Adopt an owned UNIX stream that was connected and is already close-on-exec.
/// A peer that has since closed is adopted normally; the reader observes EOF.
///
/// This does not repair descriptor flags: received descriptors must have become
/// CLOEXEC under the receive/spawn fence before reaching this boundary. Every
/// refusal drops the descriptor. A caller receiving an invalid channel must treat
/// the transfer as fatal and must not acknowledge it.
pub fn connected_unix_stream(descriptor: OwnedFd) -> io::Result<UnixStream> {
    let fd = descriptor.as_raw_fd();
    let kind = socket_option(fd, libc::SO_TYPE)?;
    if kind != libc::SOCK_STREAM {
        return Err(invalid("channel descriptor is not a stream"));
    }
    // SAFETY: zeroed storage prevents uninitialized reads; kernel output is trusted.
    let mut address: libc::sockaddr_storage = unsafe { mem::zeroed() };
    let mut length = mem::size_of_val(&address) as libc::socklen_t;
    // SAFETY: the owned fd and live address/length buffers cover the kernel's output.
    succeeded(unsafe {
        getsockname(
            fd,
            (&mut address as *mut libc::sockaddr_storage).cast(),
            &mut length,
        )
    })?;
    require_unix(libc::c_int::from(address.ss_family))?;
    #[cfg(target_os = "linux")]
    if socket_option(fd, libc::SO_ACCEPTCONN)? != 0 {
        return Err(invalid("channel descriptor is a listener"));
    }
    // SAFETY: inspect the owned descriptor, without changing its flags outside the fence.
    let flags = unsafe { fcntl(fd, libc::F_GETFD) };
    succeeded(flags)?;
    if flags & libc::FD_CLOEXEC == 0 {
        return Err(invalid("channel descriptor is not close-on-exec"));
    }
    let stream = UnixStream::from(descriptor);
    if let Err(error) = peer_address(&stream) {
        if !peer_closed(&error, cfg!(target_os = "macos")) {
            return Err(error);
        }
    }
    Ok(stream)
}
fn socket_option(fd: RawFd, option: libc::c_int) -> io::Result<libc::c_int> {
    let mut value: libc::c_int = 0;
    let mut length = mem::size_of_val(&value) as libc::socklen_t;
    // SAFETY: the live fd and initialized output/length cover this integer socket option.
    succeeded(unsafe {
        getsockopt(
            fd,
            libc::SOL_SOCKET,
            option,
            (&mut value as *mut libc::c_int).cast(),
            &mut length,
        )
    })?;
    Ok(value)
}
#[cfg(not(test))]
fn peer_address(stream: &UnixStream) -> io::Result<SocketAddr> {
    stream.peer_addr()
}
// Measured macOS peer query: peer-gone EINVAL; never-connected and listeners
// (empty or queued) ENOTCONN. SO_ACCEPTCONN is unsupported there (ENOPROTOOPT).
// Linux explicitly rejects listeners above and never accepts peer-query errors.
fn peer_closed(error: &io::Error, macos: bool) -> bool {
    macos && error.raw_os_error() == Some(libc::EINVAL)
}
// Directly test family policy before std's peer guard can mask a missing check.
fn require_unix(family: libc::c_int) -> io::Result<()> {
    if family != libc::AF_UNIX {
        Err(invalid("channel descriptor is not UNIX"))
    } else {
        Ok(())
    }
}
fn succeeded(result: libc::c_int) -> io::Result<()> {
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
#[cfg(test)]
mod tests;
