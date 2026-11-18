//! Public adoption uses the production syscall paths, without unit interposition.
use org_socket::rights::{tcp_listener, unix_datagram};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixDatagram, UnixStream};
use std::time::Duration;

/// Whether this machine has an IPv6 loopback to bind. A machine without one (a sandbox
/// or a container with IPv6 switched off) answers "address family not supported" or
/// "address not available": the IPv6 case is then skipped, with the reason printed, and
/// the IPv4 case still runs. Any other failure to bind is a failure of the test.
fn ipv6_loopback() -> bool {
    match TcpListener::bind("[::1]:0") {
        Ok(_) => true,
        Err(error)
            if error.kind() == std::io::ErrorKind::AddrNotAvailable
                || error.raw_os_error() == Some(libc::EAFNOSUPPORT) =>
        {
            eprintln!("skipped the IPv6 case: this machine has no IPv6 loopback ({error})");
            false
        }
        Err(error) => panic!("binding the IPv6 loopback: {error}"),
    }
}

#[test]
fn public_unix_datagram_adoption_preserves_descriptor_and_traffic() {
    let (source, peer) = UnixDatagram::pair().unwrap();
    let raw = source.as_raw_fd();
    let adopted = unix_datagram(source.into()).unwrap();
    assert_eq!(adopted.as_raw_fd(), raw);
    adopted.send(b"request").unwrap();
    assert!(org_socket::poll_readable(peer.as_raw_fd(), Duration::from_secs(1)).unwrap());
    let mut body = [0; 7];
    assert_eq!(peer.recv(&mut body).unwrap(), 7);
    assert_eq!(&body, b"request");
    peer.send(b"reply").unwrap();
    assert!(org_socket::poll_readable(raw, Duration::from_secs(1)).unwrap());
    assert_eq!(adopted.recv(&mut body).unwrap(), 5);
    assert_eq!(&body[..5], b"reply");
}

#[test]
fn public_tcp_listener_adoption_preserves_both_families_and_accepts_traffic() {
    for address in ["127.0.0.1:0", "[::1]:0"] {
        if address.starts_with('[') && !ipv6_loopback() {
            continue;
        }
        let source = TcpListener::bind(address).unwrap();
        let raw = source.as_raw_fd();
        let adopted = tcp_listener(source.into()).unwrap();
        assert_eq!(adopted.as_raw_fd(), raw);
        let mut client = TcpStream::connect(adopted.local_addr().unwrap()).unwrap();
        assert!(org_socket::poll_readable(raw, Duration::from_secs(1)).unwrap());
        let (mut accepted, _) = adopted.accept().unwrap();
        client.write_all(b"request").unwrap();
        assert!(org_socket::poll_readable(accepted.as_raw_fd(), Duration::from_secs(1)).unwrap());
        let mut body = [0; 7];
        accepted.read_exact(&mut body).unwrap();
        assert_eq!(&body, b"request");
    }
}

#[test]
fn public_adoption_refuses_wrong_kind_family_and_non_socket_descriptors() {
    let (stream, _peer) = UnixStream::pair().unwrap();
    assert_eq!(
        unix_datagram(stream.into()).unwrap_err().kind(),
        std::io::ErrorKind::InvalidData
    );
    assert_eq!(
        unix_datagram(UdpSocket::bind("127.0.0.1:0").unwrap().into())
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::InvalidData
    );
    assert_eq!(
        tcp_listener(UdpSocket::bind("127.0.0.1:0").unwrap().into())
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::InvalidData
    );
    assert_eq!(
        tcp_listener(std::fs::File::open("/dev/null").unwrap().into())
            .unwrap_err()
            .raw_os_error(),
        Some(libc::ENOTSOCK)
    );
    assert_eq!(
        unix_datagram(std::fs::File::open("/dev/null").unwrap().into())
            .unwrap_err()
            .raw_os_error(),
        Some(libc::ENOTSOCK)
    );
}
