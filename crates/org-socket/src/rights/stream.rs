//! Validate a received capability before it becomes a serving stream.
use super::invalid;
use libc::getsockopt;
#[cfg(not(test))]
use libc::{fcntl, getsockname};
use std::io;
use std::mem;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
#[cfg(test)]
use tests::{fcntl, getsockname};

/// Adopt an owned, connected UNIX stream whose descriptor is already close-on-exec.
///
/// This does not repair descriptor flags: received descriptors must have become
/// CLOEXEC under the receive/spawn fence before reaching this boundary. Every
/// refusal drops the descriptor. A caller receiving an invalid channel must treat
/// the transfer as fatal and must not acknowledge it.
pub fn connected_unix_stream(descriptor: OwnedFd) -> io::Result<UnixStream> {
    let fd = descriptor.as_raw_fd();
    let mut kind: libc::c_int = 0;
    let mut length = mem::size_of_val(&kind) as libc::socklen_t;
    // SAFETY: the live owned fd and pointers cover the initialized kind and length.
    succeeded(unsafe {
        getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_TYPE,
            (&mut kind as *mut libc::c_int).cast(),
            &mut length,
        )
    })?;
    if kind != libc::SOCK_STREAM {
        return Err(invalid("channel descriptor is not a stream"));
    }
    // SAFETY: zero is a valid initial sockaddr_storage; getsockname receives its full size.
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
    // SAFETY: inspect the owned descriptor, without changing its flags outside the fence.
    let flags = unsafe { fcntl(fd, libc::F_GETFD) };
    succeeded(flags)?;
    if flags & libc::FD_CLOEXEC == 0 {
        return Err(invalid("channel descriptor is not close-on-exec"));
    }
    let stream = UnixStream::from(descriptor);
    stream.peer_addr()?; // Refuse listeners and disconnected streams before adoption/ACK.
    Ok(stream)
}
// Keep the family policy directly testable: std's getpeername wrapper may
// independently refuse a foreign family and otherwise mask a missing guard.
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
