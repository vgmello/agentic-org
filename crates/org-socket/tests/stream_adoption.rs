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
    // SAFETY: source owns raw; F_GETFD only queries descriptor flags.
    let flags = unsafe { libc::fcntl(raw, libc::F_GETFD) };
    assert_ne!(flags & libc::FD_CLOEXEC, 0);
    let mut adopted = connected_unix_stream(OwnedFd::from(source)).unwrap();
    assert_eq!(adopted.as_raw_fd(), raw);
    // SAFETY: adopted owns the same live fd; F_GETFD only queries flags.
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

#[test]
fn peer_closed_stream_is_adopted_with_queued_bytes_then_eof() {
    for pending in [b"".as_slice(), b"pending".as_slice()] {
        let (stream, mut peer) = UnixStream::pair().unwrap();
        let descriptor = OwnedFd::from(stream);
        let original = descriptor.as_raw_fd();
        peer.write_all(pending).unwrap();
        drop(peer);
        let mut adopted = org_socket::rights::connected_unix_stream(descriptor).unwrap();
        assert_eq!(adopted.as_raw_fd(), original);
        let mut received = vec![0; pending.len()];
        adopted.read_exact(&mut received).unwrap();
        assert_eq!(received, pending);
        assert_eq!(adopted.read(&mut [0]).unwrap(), 0);
    }
}

#[test]
fn bound_peer_closed_streams_preserve_ownership_bytes_and_eof() {
    assert_named_peer_closed("bound");
}

#[test]
fn accepted_peer_closed_streams_preserve_ownership_bytes_and_eof() {
    assert_named_peer_closed("accepted");
}

fn assert_named_peer_closed(kind: &str) {
    let directory = private_directory();
    for (index, pending) in [b"".as_slice(), b"queued".as_slice()]
        .into_iter()
        .enumerate()
    {
        let path = directory.0.join(format!("{kind}-{index}"));
        let (stream, mut peer) = if kind == "bound" {
            use std::os::unix::ffi::OsStrExt;
            let (stream, peer) = UnixStream::pair().unwrap();
            let bytes = path.as_os_str().as_bytes();
            // SAFETY: zero initializes every byte, including the UNIX path terminator.
            let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
            assert!(bytes.len() < address.sun_path.len());
            address.sun_family = libc::AF_UNIX as _;
            #[cfg(target_os = "macos")]
            {
                address.sun_len = std::mem::size_of_val(&address).try_into().unwrap();
            }
            for (slot, byte) in address.sun_path.iter_mut().zip(bytes) {
                *slot = *byte as _;
            }
            // SAFETY: stream owns fd; the initialized address is covered by the supplied length.
            assert_eq!(
                unsafe {
                    libc::bind(
                        stream.as_raw_fd(),
                        (&address as *const libc::sockaddr_un).cast(),
                        std::mem::size_of_val(&address) as _,
                    )
                },
                0
            );
            (stream, peer)
        } else {
            let listener = std::os::unix::net::UnixListener::bind(path).unwrap();
            let peer =
                UnixStream::connect(listener.local_addr().unwrap().as_pathname().unwrap()).unwrap();
            let (stream, _) = listener.accept().unwrap();
            (stream, peer)
        };
        let raw = stream.as_raw_fd();
        // SAFETY: F_GETFD only queries the test-owned stream.
        let flags = unsafe { libc::fcntl(raw, libc::F_GETFD) };
        assert_ne!(flags & libc::FD_CLOEXEC, 0);
        // macOS returned EINVAL when configuring these peer-closed sockets.
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        peer.write_all(pending).unwrap();
        drop(peer);
        let mut adopted = connected_unix_stream(stream.into()).unwrap();
        assert_eq!(
            adopted.as_raw_fd(),
            raw,
            "{kind}: adoption duplicated the fd"
        );
        // SAFETY: adopted owns the same live descriptor; F_GETFD only queries it.
        assert_eq!(unsafe { libc::fcntl(raw, libc::F_GETFD) }, flags);
        let mut received = vec![0; pending.len()];
        adopted.read_exact(&mut received).unwrap();
        assert_eq!(received, pending, "{kind}: queued bytes changed");
        assert_eq!(adopted.read(&mut [0]).unwrap(), 0, "{kind}: EOF lost");
    }
}

struct PrivateDirectory(std::path::PathBuf);
impl Drop for PrivateDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn private_directory() -> PrivateDirectory {
    use std::os::unix::fs::DirBuilderExt;
    for suffix in 0u64.. {
        let path =
            std::env::temp_dir().join(format!("org-public-adopt-{}-{suffix}", std::process::id()));
        match std::fs::DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => return PrivateDirectory(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("private socket fixture: {error}"),
        }
    }
    unreachable!("temporary directory suffix exhausted")
}
