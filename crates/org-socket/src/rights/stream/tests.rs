use super::*;
use std::io::{Read, Write};
use std::os::fd::FromRawFd;
use std::os::unix::net::{UnixDatagram, UnixListener};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn connected_pair_is_owned_usable_and_keeps_cloexec() {
    let (stream, mut peer) = UnixStream::pair().unwrap();
    let descriptor = OwnedFd::from(stream);
    let original = descriptor.as_raw_fd();
    let before = unsafe { libc::fcntl(original, libc::F_GETFD) };
    assert_ne!(before & libc::FD_CLOEXEC, 0);
    let mut stream = connected_unix_stream(descriptor).unwrap();
    assert_eq!(stream.as_raw_fd(), original, "adoption must not duplicate");
    assert_eq!(unsafe { libc::fcntl(original, libc::F_GETFD) }, before);
    stream.write_all(b"request").unwrap();
    let mut body = [0; 7];
    peer.read_exact(&mut body).unwrap();
    assert_eq!(&body, b"request");
    peer.write_all(b"reply").unwrap();
    stream.read_exact(&mut body[..5]).unwrap();
    assert_eq!(&body[..5], b"reply");
    drop(stream);
    assert!(crate::poll_readable(peer.as_raw_fd(), Duration::from_secs(1)).unwrap());
    assert_eq!(peer.read(&mut [0]).unwrap(), 0);
}

#[test]
fn syscall_status_accepts_zero_and_positive_but_refuses_every_negative() {
    for result in [-2, -1, 0, 1, libc::FD_CLOEXEC] {
        // SAFETY: an invalid fd only sets errno; it owns no resource.
        assert_eq!(unsafe { libc::close(-1) }, -1);
        match succeeded(result) {
            Ok(()) => assert!(result >= 0),
            Err(error) => {
                assert!(result < 0);
                assert_eq!(error.raw_os_error(), Some(libc::EBADF));
            }
        }
    }
}

#[test]
fn wrong_descriptor_classes_are_refused_and_owned_fds_close() {
    for case in [
        "pipe",
        "file",
        "datagram",
        "listener",
        "inet",
        "disconnected",
        "no-cloexec",
        "fault-family",
        "fault-flags",
    ] {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "rights::stream::tests::adoption_child",
                "--nocapture",
            ])
            .env("ORG_SOCKET_ADOPTION_CASE", case)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                child.kill().unwrap();
                let output = child.wait_with_output().unwrap();
                panic!("adoption {case} hung: {output:?}");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "adoption {case}: {output:?}");
        assert!(String::from_utf8_lossy(&output.stdout).contains("refused and closed"));
    }
}

// Single-test children make EBADF a valid ownership observation: no concurrent
// test can allocate the just-closed descriptor number.
#[test]
fn adoption_child() {
    let Ok(case) = std::env::var("ORG_SOCKET_ADOPTION_CASE") else {
        return;
    };
    let dir = tempfile_dir();
    let mut unix_peer = None;
    let descriptor: OwnedFd = match case.as_str() {
        "pipe" => {
            let mut fds = [-1; 2];
            // SAFETY: pipe writes exactly two new descriptors; each gets one owner.
            assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
            let read = unsafe { OwnedFd::from_raw_fd(fds[0]) };
            drop(unsafe { OwnedFd::from_raw_fd(fds[1]) });
            read
        }
        "file" => std::fs::File::create(dir.join("file")).unwrap().into(),
        "datagram" => {
            let (socket, peer) = UnixDatagram::pair().unwrap();
            drop(peer);
            socket.into()
        }
        "listener" => UnixListener::bind(dir.join("listener")).unwrap().into(),
        "inet" => {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let stream = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
            let (peer, _) = listener.accept().unwrap();
            drop(peer);
            stream.into()
        }
        "disconnected" => {
            // SAFETY: socket creates one fd, wrapped exactly once after checking success.
            let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
            assert!(fd >= 0);
            let descriptor = unsafe { OwnedFd::from_raw_fd(fd) };
            assert_eq!(
                unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) },
                0
            );
            descriptor
        }
        "no-cloexec" => {
            let (stream, peer) = UnixStream::pair().unwrap();
            assert_eq!(
                unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_SETFD, 0) },
                0
            );
            unix_peer = Some(peer);
            stream.into()
        }
        "fault-family" | "fault-flags" => {
            let (stream, peer) = UnixStream::pair().unwrap();
            unix_peer = Some(peer);
            FAULT.with(|fault| {
                fault.set(Some(match case.as_str() {
                    "fault-family" => Fault::Family,
                    _ => Fault::Flags,
                }))
            });
            stream.into()
        }
        _ => panic!("unknown adoption case"),
    };
    let raw = descriptor.as_raw_fd();
    let error = connected_unix_stream(descriptor).expect_err("invalid descriptor adopted");
    FAULT.with(|fault| fault.set(None));
    assert_eq!(unsafe { libc::fcntl(raw, libc::F_GETFD) }, -1);
    assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EBADF));
    if case.starts_with("fault-") {
        assert_eq!(error.raw_os_error(), Some(libc::EBADF));
    } else if matches!(case.as_str(), "pipe" | "file") {
        assert_eq!(error.raw_os_error(), Some(libc::ENOTSOCK));
    } else if matches!(case.as_str(), "listener" | "disconnected") {
        assert_eq!(error.kind(), io::ErrorKind::NotConnected);
    } else {
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
    if let Some(mut peer) = unix_peer {
        assert!(crate::poll_readable(peer.as_raw_fd(), Duration::from_secs(1)).unwrap());
        assert_eq!(peer.read(&mut [0]).unwrap(), 0);
    }
    drop(dir);
    println!("refused and closed");
}

struct TestDirectory(std::path::PathBuf);

impl std::ops::Deref for TestDirectory {
    type Target = std::path::Path;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tempfile_dir() -> TestDirectory {
    // This crate has no tempfile dependency. Each isolated child creates one
    // private directory under the OS temporary directory and removes it afterwards.
    let path = std::env::temp_dir().join(format!("org-adopt-{}", std::process::id()));
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&path)
        .unwrap();
    TestDirectory(path)
}

#[test]
fn unix_family_policy_is_tested_before_the_std_peer_guard_can_mask_it() {
    for family in [0, libc::AF_UNIX, libc::AF_INET, libc::AF_INET6] {
        match require_unix(family) {
            Ok(()) => assert_eq!(family, libc::AF_UNIX),
            Err(error) => {
                assert_ne!(family, libc::AF_UNIX);
                assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            }
        }
    }
}

#[test]
fn received_descriptor_passes_validation_without_a_second_flags_update() {
    let (sender, socket) = UnixDatagram::pair().unwrap();
    let (source, mut peer) = UnixStream::pair().unwrap();
    super::super::tests::send_rights(sender.as_raw_fd(), b"!", &[source.as_raw_fd()]);
    let mut receiver = crate::rights::DatagramReceiver::new(socket);
    let mut received = receiver
        .receive_up_to(&mut [0], 1, &std::sync::Mutex::new(()))
        .unwrap();
    assert_eq!(received.descriptors.len(), 1);
    drop(source);
    let descriptor = received.descriptors.pop().unwrap();
    let number = descriptor.as_raw_fd();
    let before = unsafe { libc::fcntl(number, libc::F_GETFD) };
    assert_ne!(before & libc::FD_CLOEXEC, 0);
    let mut stream = connected_unix_stream(descriptor).unwrap();
    assert_eq!(stream.as_raw_fd(), number);
    assert_eq!(unsafe { libc::fcntl(number, libc::F_GETFD) }, before);
    stream.write_all(b"x").unwrap();
    let mut byte = [0];
    peer.read_exact(&mut byte).unwrap();
    assert_eq!(byte, [b'x']);
    drop(stream);
    assert!(crate::poll_readable(peer.as_raw_fd(), Duration::from_secs(1)).unwrap());
    assert_eq!(peer.read(&mut byte).unwrap(), 0);
}

// Test-only syscall interposition leaves the live fd owned, so each real helper
// call site's error propagation and descriptor drop can be observed separately.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fault {
    Family,
    Flags,
}
thread_local! { static FAULT: std::cell::Cell<Option<Fault>> = const { std::cell::Cell::new(None) }; }
fn injected(fault: Fault) -> bool {
    let fail = FAULT.with(|value| value.get() == Some(fault));
    if fail {
        // SAFETY: no descriptor is closed; this sets a genuine thread-local EBADF.
        assert_eq!(unsafe { libc::close(-1) }, -1);
    }
    fail
}
pub(super) unsafe fn getsockname(
    fd: libc::c_int,
    address: *mut libc::sockaddr,
    length: *mut libc::socklen_t,
) -> libc::c_int {
    if injected(Fault::Family) {
        return -1;
    }
    // SAFETY: the helper supplies a live descriptor and valid kernel output buffers.
    unsafe { libc::getsockname(fd, address, length) }
}
pub(super) unsafe fn fcntl(fd: libc::c_int, command: libc::c_int) -> libc::c_int {
    if injected(Fault::Flags) {
        return -1;
    }
    // SAFETY: the helper uses F_GETFD, which requires no third argument.
    unsafe { libc::fcntl(fd, command) }
}
