//! Validate owned socket shape and flags before typed adoption.
use super::invalid;
#[cfg(not(test))]
use libc::{fcntl, getsockname, getsockopt};
#[cfg(not(test))]
use org_platform::socket::listening;
use org_platform::socket::peer_closed;
use std::io;
use std::mem;
use std::net::TcpListener;
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::os::unix::net::{SocketAddr, UnixDatagram, UnixStream};
#[cfg(test)]
use tests::{fcntl, getsockname, getsockopt, listening, peer_address};

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
    require_unix(socket_family(fd)?)?;
    if listening(fd)? {
        return Err(invalid("channel descriptor is a listener"));
    }
    require_cloexec(fd)?;
    let stream = UnixStream::from(descriptor);
    // Listeners are rejected above, including macOS sockets that were connected
    // before listening. Only then may the platform's peer-gone error be adopted.
    if let Err(error) = peer_address(&stream) {
        if !peer_closed(&error) {
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

/// Adopt an owned UNIX datagram socket that is already close-on-exec.
///
/// This validates socket shape only, not connected/receive-only host authority.
/// It never repairs flags; every refusal drops the descriptor. Receivers must
/// establish CLOEXEC inside their receive/spawn fence before calling this helper.
pub fn unix_datagram(descriptor: OwnedFd) -> io::Result<UnixDatagram> {
    let fd = descriptor.as_raw_fd();
    require_kind(fd, libc::SOCK_DGRAM)?;
    require_unix(socket_family(fd)?)?;
    require_cloexec(fd)?;
    Ok(UnixDatagram::from(descriptor))
}

/// Adopt an owned IPv4 or IPv6 TCP listener that is already close-on-exec.
///
/// This validates socket shape/listening state, not host provenance or authority
/// to serve a configured origin. It never repairs flags; every refusal drops the
/// descriptor. Adoption must complete before acknowledging a descriptor transfer.
pub fn tcp_listener(descriptor: OwnedFd) -> io::Result<TcpListener> {
    let fd = descriptor.as_raw_fd();
    require_kind(fd, libc::SOCK_STREAM)?;
    require_inet(socket_family(fd)?)?;
    if !listening(fd)? {
        return Err(invalid("descriptor is not a listener"));
    }
    require_cloexec(fd)?;
    Ok(TcpListener::from(descriptor))
}

fn require_kind(fd: RawFd, expected: libc::c_int) -> io::Result<()> {
    if socket_option(fd, libc::SO_TYPE)? != expected {
        return Err(invalid("descriptor has the wrong socket type"));
    }
    Ok(())
}

fn socket_family(fd: RawFd) -> io::Result<libc::c_int> {
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
    Ok(libc::c_int::from(address.ss_family))
}

fn require_cloexec(fd: RawFd) -> io::Result<()> {
    // SAFETY: inspect the owned descriptor without changing flags outside the fence.
    let flags = unsafe { fcntl(fd, libc::F_GETFD) };
    succeeded(flags)?;
    if flags & libc::FD_CLOEXEC == 0 {
        return Err(invalid("channel descriptor is not close-on-exec"));
    }
    Ok(())
}

fn require_inet(family: libc::c_int) -> io::Result<()> {
    if family != libc::AF_INET && family != libc::AF_INET6 {
        Err(invalid("listener descriptor is not IPv4 or IPv6"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
