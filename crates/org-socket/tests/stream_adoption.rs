//! Exercise the public helper with the production libc imports (no unit fault seam).
use org_socket::rights::connected_unix_stream;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::net::{UnixDatagram, UnixStream};
use std::time::Duration;

#[test]
fn public_adoption_uses_real_syscalls_and_preserves_unique_ownership() {
    let (source, mut peer) = UnixStream::pair().unwrap();
    let raw = source.as_raw_fd();
    let flags = unsafe { libc::fcntl(raw, libc::F_GETFD) };
    assert_ne!(flags & libc::FD_CLOEXEC, 0);
    let mut adopted = connected_unix_stream(OwnedFd::from(source)).unwrap();
    assert_eq!(adopted.as_raw_fd(), raw);
    assert_eq!(unsafe { libc::fcntl(raw, libc::F_GETFD) }, flags);
    adopted.write_all(b"request").unwrap();
    let mut request = [0; 7];
    peer.read_exact(&mut request).unwrap();
    assert_eq!(&request, b"request");
    drop(adopted);
    assert!(org_socket::poll_readable(peer.as_raw_fd(), Duration::from_secs(1)).unwrap());
    assert_eq!(peer.read(&mut [0]).unwrap(), 0);

    let (datagram, _other) = UnixDatagram::pair().unwrap();
    assert_eq!(
        connected_unix_stream(datagram.into()).unwrap_err().kind(),
        std::io::ErrorKind::InvalidData
    );
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let stream = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (_peer, _) = listener.accept().unwrap();
    assert_eq!(
        connected_unix_stream(stream.into()).unwrap_err().kind(),
        std::io::ErrorKind::InvalidData
    );
}
