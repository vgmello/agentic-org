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
    // SAFETY: F_GETFD only queries the live descriptor owned above.
    let before = unsafe { libc::fcntl(original, libc::F_GETFD) };
    assert_ne!(before & libc::FD_CLOEXEC, 0);
    let mut stream = connected_unix_stream(descriptor).unwrap();
    assert_eq!(stream.as_raw_fd(), original, "adoption must not duplicate");
    // SAFETY: the adopted stream still owns original; F_GETFD only queries flags.
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
        #[cfg(target_os = "linux")]
        "seqpacket",
        "listener",
        "listener-queued",
        #[cfg(target_os = "macos")]
        "listener-crafted",
        #[cfg(target_os = "macos")]
        "listener-crafted-queued",
        "inet",
        "never-connected",
        "no-cloexec",
        "fault-peer",
        #[cfg(target_os = "linux")]
        "fault-peer-einval",
        #[cfg(target_os = "linux")]
        "fault-listener",
        #[cfg(target_os = "macos")]
        "fault-proc-error",
        #[cfg(target_os = "macos")]
        "fault-proc-negative",
        #[cfg(target_os = "macos")]
        "fault-proc-short",
        #[cfg(target_os = "macos")]
        "fault-proc-long",
        "fault-family",
        "fault-flags",
    ] {
        run_case(case);
    }
}

#[test]
fn native_platform_state_table_checks_real_peer_errno_and_adoption_outcome() {
    for case in [
        "connected",
        "peer-closed",
        "never-connected",
        "listener",
        "listener-queued",
        #[cfg(target_os = "macos")]
        "listener-crafted",
        #[cfg(target_os = "macos")]
        "listener-crafted-queued",
        "datagram",
        "pipe",
        #[cfg(target_os = "linux")]
        "seqpacket",
    ] {
        run_case(case);
    }
    // SAFETY: querying -1 owns no resource and reports the real EBADF result.
    assert_eq!(unsafe { libc::fcntl(-1, libc::F_GETFD) }, -1);
    assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EBADF));
}

fn run_case(case: &str) {
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
    assert!(String::from_utf8_lossy(&output.stdout).contains("verified and closed"));
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
        "connected" | "peer-closed" => {
            let (stream, peer) = UnixStream::pair().unwrap();
            if case == "connected" {
                unix_peer = Some(peer);
            }
            stream.into()
        }
        "pipe" => {
            let mut fds = [-1; 2];
            // SAFETY: pipe writes exactly two new descriptors; each gets one owner.
            assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
            // SAFETY: successful pipe created these two fresh fds; wrap each once.
            let read = unsafe { OwnedFd::from_raw_fd(fds[0]) };
            // SAFETY: the second fresh pipe fd has not yet been wrapped or closed.
            drop(unsafe { OwnedFd::from_raw_fd(fds[1]) });
            read
        }
        "file" => std::fs::File::create(dir.join("file")).unwrap().into(),
        "datagram" => {
            let (socket, peer) = UnixDatagram::pair().unwrap();
            drop(peer);
            socket.into()
        }
        #[cfg(target_os = "linux")]
        "seqpacket" => {
            let mut fds = [-1; 2];
            // SAFETY: socketpair writes two fresh owned fds into a live two-item array.
            assert_eq!(
                unsafe {
                    libc::socketpair(
                        libc::AF_UNIX,
                        libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
                        0,
                        fds.as_mut_ptr(),
                    )
                },
                0
            );
            // SAFETY: socketpair succeeded; wrap each returned fd exactly once.
            let socket = unsafe { OwnedFd::from_raw_fd(fds[0]) };
            // SAFETY: the other fresh fd likewise gets exactly one owner, retained through refusal.
            unix_peer = Some(UnixStream::from(unsafe { OwnedFd::from_raw_fd(fds[1]) }));
            socket
        }
        #[cfg(target_os = "macos")]
        "listener-crafted" | "listener-crafted-queued" => {
            use std::os::unix::ffi::OsStrExt;
            let (stream, peer) = UnixStream::pair().unwrap();
            let path = dir.join("crafted");
            let bytes = path.as_os_str().as_bytes();
            // SAFETY: zero initializes a valid sockaddr_un, including its path terminator.
            let mut address: libc::sockaddr_un = unsafe { mem::zeroed() };
            assert!(bytes.len() < address.sun_path.len());
            address.sun_len = mem::size_of_val(&address).try_into().unwrap();
            address.sun_family = libc::AF_UNIX as _;
            for (slot, byte) in address.sun_path.iter_mut().zip(bytes) {
                *slot = *byte as _;
            }
            // SAFETY: the test owns fd and the initialized UNIX address is fully covered.
            assert_eq!(
                unsafe {
                    libc::bind(
                        stream.as_raw_fd(),
                        (&address as *const libc::sockaddr_un).cast(),
                        mem::size_of_val(&address) as _,
                    )
                },
                0
            );
            drop(peer);
            // SAFETY: this test-owned bound socket has no connected peer; macOS permits listen.
            assert_eq!(unsafe { libc::listen(stream.as_raw_fd(), 1) }, 0);
            let listener = UnixListener::from(OwnedFd::from(stream));
            let connected = UnixStream::connect(path).unwrap();
            if case == "listener-crafted-queued" {
                unix_peer = Some(connected);
            } else {
                let (accepted, _) = listener.accept().unwrap();
                drop(accepted);
                drop(connected);
            }
            listener.into()
        }
        "listener" | "listener-queued" => {
            let path = dir.join("listener");
            let listener = UnixListener::bind(&path).unwrap();
            if case == "listener-queued" {
                unix_peer = Some(UnixStream::connect(path).unwrap());
            }
            listener.into()
        }
        "inet" => {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let stream = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
            let (peer, _) = listener.accept().unwrap();
            drop(peer);
            stream.into()
        }
        "never-connected" => {
            // SAFETY: socket creates one fd, wrapped exactly once after checking success.
            let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
            assert!(fd >= 0);
            // SAFETY: successful socket returned one fresh fd, wrapped once here.
            let descriptor = unsafe { OwnedFd::from_raw_fd(fd) };
            // SAFETY: F_SETFD updates this test-owned descriptor with a valid flag.
            assert_eq!(
                unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) },
                0
            );
            descriptor
        }
        "no-cloexec" => {
            let (stream, peer) = UnixStream::pair().unwrap();
            // SAFETY: clear flags only on this test-owned fd to construct the refusal.
            assert_eq!(
                unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_SETFD, 0) },
                0
            );
            unix_peer = Some(peer);
            stream.into()
        }
        "fault-peer"
        | "fault-peer-einval"
        | "fault-listener"
        | "fault-family"
        | "fault-flags"
        | "fault-proc-error"
        | "fault-proc-negative"
        | "fault-proc-short"
        | "fault-proc-long" => {
            let (stream, peer) = UnixStream::pair().unwrap();
            unix_peer = Some(peer);
            FAULT.with(|fault| {
                fault.set(Some(match case.as_str() {
                    "fault-peer" => Fault::Peer,
                    "fault-peer-einval" => Fault::PeerEinval,
                    #[cfg(target_os = "linux")]
                    "fault-listener" => Fault::Listener,
                    #[cfg(target_os = "macos")]
                    "fault-proc-error" => Fault::ProcError,
                    #[cfg(target_os = "macos")]
                    "fault-proc-negative" => Fault::ProcNegative,
                    #[cfg(target_os = "macos")]
                    "fault-proc-short" => Fault::ProcShort,
                    #[cfg(target_os = "macos")]
                    "fault-proc-long" => Fault::ProcLong,
                    "fault-family" => Fault::Family,
                    _ => Fault::Flags,
                }))
            });
            stream.into()
        }
        _ => panic!("unknown adoption case"),
    };
    let raw = descriptor.as_raw_fd();
    if matches!(
        case.as_str(),
        "connected"
            | "peer-closed"
            | "never-connected"
            | "listener"
            | "listener-queued"
            | "listener-crafted"
            | "listener-crafted-queued"
    ) {
        // SAFETY: zero initializes every byte of the kernel output storage.
        let mut address: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
        let mut length = std::mem::size_of_val(&address) as libc::socklen_t;
        // SAFETY: raw is live; initialized storage and length cover the kernel's output.
        let status = unsafe {
            libc::getpeername(
                raw,
                (&mut address as *mut libc::sockaddr_storage).cast(),
                &mut length,
            )
        };
        let errno = (status < 0).then(|| io::Error::last_os_error().raw_os_error().unwrap());
        let expected = match case.as_str() {
            "connected" => None,
            "peer-closed" | "listener-crafted" | "listener-crafted-queued"
                if cfg!(target_os = "macos") =>
            {
                Some(libc::EINVAL)
            }
            "peer-closed" => None,
            _ => Some(libc::ENOTCONN),
        };
        assert_eq!(errno, expected, "actual peer query for {case}");
    }
    let result = connected_unix_stream(descriptor);
    if matches!(case.as_str(), "connected" | "peer-closed") {
        let mut stream = result.unwrap();
        assert_eq!(stream.as_raw_fd(), raw);
        if case == "peer-closed" {
            assert!(crate::poll_readable(raw, Duration::from_secs(1)).unwrap());
            assert_eq!(stream.read(&mut [0]).unwrap(), 0);
        }
        drop(stream);
        // SAFETY: isolated single-test child prevents reuse of the just-closed number.
        assert_eq!(unsafe { libc::fcntl(raw, libc::F_GETFD) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EBADF));
        println!("verified and closed");
        return;
    }
    let error = result.expect_err("invalid descriptor adopted");
    FAULT.with(|fault| fault.set(None));
    // SAFETY: F_GETFD safely queries even a closed number; this child runs one test.
    assert_eq!(unsafe { libc::fcntl(raw, libc::F_GETFD) }, -1);
    assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EBADF));
    if case == "fault-peer-einval" {
        assert_eq!(error.raw_os_error(), Some(libc::EINVAL));
    } else if case.starts_with("fault-proc-") {
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    } else if case.starts_with("fault-") {
        assert_eq!(error.raw_os_error(), Some(libc::EBADF));
    } else if matches!(case.as_str(), "pipe" | "file") {
        assert_eq!(error.raw_os_error(), Some(libc::ENOTSOCK));
    } else if case == "never-connected" {
        assert_eq!(error.raw_os_error(), Some(libc::ENOTCONN));
    } else {
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
    if let Some(mut peer) = unix_peer.filter(|_| !case.starts_with("listener")) {
        assert!(crate::poll_readable(peer.as_raw_fd(), Duration::from_secs(1)).unwrap());
        assert_eq!(peer.read(&mut [0]).unwrap(), 0);
    }
    drop(dir);
    println!("verified and closed");
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
    // Skip stale/colliding entries without deleting anything this run does not own.
    use std::os::unix::fs::DirBuilderExt;
    let base = format!("org-adopt-{}", std::process::id());
    for suffix in 0u64.. {
        let name = if suffix == 0 {
            base.clone()
        } else {
            format!("{base}-{suffix}")
        };
        let path = std::env::temp_dir().join(name);
        match std::fs::DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => return TestDirectory(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("private test directory: {error}"),
        }
    }
    unreachable!("temporary directory suffix exhausted")
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
    // SAFETY: the live received descriptor owns number; F_GETFD only reads flags.
    let before = unsafe { libc::fcntl(number, libc::F_GETFD) };
    assert_ne!(before & libc::FD_CLOEXEC, 0);
    let mut stream = connected_unix_stream(descriptor).unwrap();
    assert_eq!(stream.as_raw_fd(), number);
    // SAFETY: stream still owns the same number; F_GETFD only reads flags.
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
    #[cfg(target_os = "macos")]
    ProcError,
    #[cfg(target_os = "macos")]
    ProcNegative,
    #[cfg(target_os = "macos")]
    ProcShort,
    #[cfg(target_os = "macos")]
    ProcLong,
    Peer,
    PeerEinval,
    #[cfg(target_os = "linux")]
    Listener,
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

#[test]
fn temporary_directory_retries_pid_collisions_without_touching_existing_dirs() {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let base = format!("org-adopt-{}", std::process::id());
    let mut owned = Vec::new();
    let collisions: Vec<_> = [base.clone(), format!("{base}-1")]
        .into_iter()
        .map(|name| std::env::temp_dir().join(name))
        .collect();
    for path in &collisions {
        match std::fs::DirBuilder::new().mode(0o700).create(path) {
            Ok(()) => owned.push(TestDirectory(path.clone())),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => panic!("collision fixture: {error}"),
        }
    }
    let fresh = tempfile_dir();
    assert!(!collisions.contains(&fresh.0));
    assert!(collisions
        .iter()
        .all(|path| std::fs::symlink_metadata(path).is_ok()));
    assert_eq!(
        std::fs::metadata(&fresh.0).unwrap().permissions().mode() & 0o777,
        0o700
    );
    drop(fresh);
    drop(owned);
}

// SO_TYPE always executes real libc; only an otherwise unreachable later query fails.
pub(super) unsafe fn getsockopt(
    fd: libc::c_int,
    level: libc::c_int,
    option: libc::c_int,
    value: *mut libc::c_void,
    length: *mut libc::socklen_t,
) -> libc::c_int {
    #[cfg(target_os = "linux")]
    if option == libc::SO_ACCEPTCONN && injected(Fault::Listener) {
        return -1;
    }
    // SAFETY: socket_option supplies valid buffers for a live fd and integer option.
    unsafe { libc::getsockopt(fd, level, option, value, length) }
}
#[test]
fn peer_closed_policy_accepts_only_macos_einval_and_refuses_every_other_error() {
    for macos in [false, true] {
        for errno in [
            libc::EINVAL,
            libc::ENOTCONN,
            libc::EBADF,
            libc::EACCES,
            libc::ENOTSOCK,
            libc::EINTR,
        ] {
            let error = io::Error::from_raw_os_error(errno);
            assert_eq!(
                peer_closed(&error, macos),
                macos && errno == libc::EINVAL,
                "macos={macos}, errno={errno}"
            );
        }
        // A generic InvalidInput classification cannot stand in for the exact kernel errno.
        assert!(!peer_closed(
            &io::Error::from(io::ErrorKind::InvalidInput),
            macos
        ));
    }
}

pub(super) fn peer_address(stream: &UnixStream) -> io::Result<SocketAddr> {
    if injected(Fault::Peer) {
        return Err(io::Error::last_os_error());
    }
    if FAULT.with(|fault| fault.get() == Some(Fault::PeerEinval)) {
        // SAFETY: invalid how changes no state on this live socket and sets real EINVAL.
        assert_eq!(unsafe { libc::shutdown(stream.as_raw_fd(), 99) }, -1);
        let error = io::Error::last_os_error();
        assert_eq!(error.raw_os_error(), Some(libc::EINVAL));
        return Err(error);
    }
    stream.peer_addr()
}

#[cfg(target_os = "macos")]
pub(super) unsafe fn proc_pidfdinfo(
    pid: libc::c_int,
    fd: libc::c_int,
    flavor: libc::c_int,
    buffer: *mut libc::c_void,
    size: libc::c_int,
) -> libc::c_int {
    for (fault, result) in [
        (Fault::ProcError, 0),
        (Fault::ProcNegative, -1),
        (Fault::ProcShort, size - 1),
        (Fault::ProcLong, size + 1),
    ] {
        if injected(fault) {
            return result;
        }
    }
    // SAFETY: the helper supplies an owned fd and its exact SDK-aligned writable buffer.
    unsafe { libc::proc_pidfdinfo(pid, fd, flavor, buffer, size) }
}
