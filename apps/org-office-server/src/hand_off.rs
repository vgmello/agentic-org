//! The key hand-off (launch v3, "The key hand-off" in `org_office_launch`): the app token and
//! the database key reach the server only over a connection it opens itself, right after its
//! first line, to the one-shot socket the host listens on. Nothing secret is on stdin.
//!
//! The server builds the socket's path itself from its own office home and the id on the first
//! line (it never takes a path from the host), checks that the file is a socket it could trust
//! (owned by this user, mode 0600, not a link), connects, reads the one line through a buffer it
//! wipes, and closes the connection (close-on-exec, one way, no ack). The whole exchange has a
//! 5 s deadline. It uses `poll(2)` and a non-blocking socket, never a socket read timeout:
//! on macOS setting `SO_RCVTIMEO` fails once the host has already written and closed.
#![allow(unsafe_code)]

use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::Path;
use std::time::{Duration, Instant};

use org_office_launch::{
    key_socket_path, read_hand_off, FirstLine, HandOff, KeySocketId, LaunchError,
};
use org_proto::frame::FrameError;
use org_proto::location::check_socket_path;

use crate::peer::PeerRule;
use crate::zeroed::ZeroedReader;

/// How long the connect and the read may take together.
pub(crate) const DEADLINE: Duration = Duration::from_secs(5);

/// Receives the hand-off for the first line `expected` from the key socket named `id`.
///
/// `Err(e)` with [`LaunchError::is_host_gone`] means the host left (exit 0, no `failed` line).
/// Any other error is reported with fixed text and `e.fail_code()`, never the error's own text.
pub(crate) fn receive(
    home: &Path,
    id: &KeySocketId,
    expected: FirstLine,
) -> Result<HandOff, LaunchError> {
    receive_within(home, id, expected, DEADLINE, PeerRule::Host)
}

fn receive_within(
    home: &Path,
    id: &KeySocketId,
    expected: FirstLine,
    deadline: Duration,
    rule: PeerRule,
) -> Result<HandOff, LaunchError> {
    let path = key_socket_path(home, id);
    if check_socket_path(&path).is_err() {
        return Err(LaunchError::Invalid("the key socket path is too long"));
    }
    check_socket_file(&path)?;
    let until = Instant::now() + deadline;
    let fd = connect(&path, until).map_err(|e| LaunchError::Frame(FrameError::Io(e)))?;
    // Who is listening, checked NOW: the host writes the line and closes at once, and after the
    // peer has closed the kernel no longer answers (`ENOTCONN`). A swapped socket is a listener
    // of another process, and nothing is read from it.
    if !crate::peer::admits(fd.as_raw_fd(), rule) {
        return Err(LaunchError::Invalid(
            "the key socket is not the host's listener",
        ));
    }
    // A small buffer we control; it is wiped when the reader goes, and the connection closes
    // with it.
    let mut reader = ZeroedReader::new(Polled { fd, until });
    read_hand_off(&mut reader, expected)
}

/// A fixed word for the log saying what failed (never the error's text, which could name a path).
pub(crate) fn category(error: &LaunchError) -> &'static str {
    match error {
        LaunchError::Invalid(_) => "refused",
        LaunchError::UnsupportedVersion(_) | LaunchError::UnsupportedOfficeFileVersion(_) => {
            "version"
        }
        LaunchError::Json(_) => "line",
        LaunchError::Frame(FrameError::Io(e)) if e.kind() == io::ErrorKind::TimedOut => "deadline",
        LaunchError::Frame(FrameError::Io(_)) => "connect or read",
        LaunchError::Frame(_) => "line",
    }
}

/// The file must be a socket, owned by this user, mode 0600, and not a link (`lstat`).
fn check_socket_file(path: &Path) -> Result<(), LaunchError> {
    let refused = || LaunchError::Invalid("the key socket is not the host's");
    let meta = std::fs::symlink_metadata(path).map_err(|_| refused())?;
    // SAFETY: `geteuid` has no preconditions and cannot fail.
    let me = unsafe { libc::geteuid() };
    if !meta.file_type().is_socket()
        || meta.uid() != me
        || meta.permissions().mode() & 0o777 != 0o600
    {
        return Err(refused());
    }
    Ok(())
}

/// Connects to the socket at `path` without blocking past `until`. The descriptor is
/// close-on-exec and non-blocking.
fn connect(path: &Path, until: Instant) -> io::Result<OwnedFd> {
    use std::os::unix::ffi::OsStrExt;
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: a zeroed `sockaddr_un` is a valid value; only the family and path are set below.
    let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if bytes.len() >= addr.sun_path.len() {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (slot, byte) in addr.sun_path.iter_mut().zip(bytes) {
        *slot = *byte as libc::c_char;
    }
    // SAFETY: plain system calls on a descriptor this function owns.
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `fd` is a fresh descriptor nothing else owns.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let raw = fd.as_raw_fd();
    // SAFETY: `fcntl` on a valid descriptor.
    let flags = unsafe { libc::fcntl(raw, libc::F_GETFL) };
    // SAFETY: as above.
    if flags < 0
        || unsafe { libc::fcntl(raw, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
        || unsafe { libc::fcntl(raw, libc::F_SETFD, libc::FD_CLOEXEC) } < 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `addr` is a valid `sockaddr_un` and the length is its size.
    let connected = unsafe {
        libc::connect(
            raw,
            std::ptr::addr_of!(addr).cast::<libc::sockaddr>(),
            std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t,
        )
    };
    if connected == 0 {
        return Ok(fd);
    }
    let error = io::Error::last_os_error();
    // Only a connect that is under way is waited for. (A unix socket with a full backlog is
    // refused, or answers `EAGAIN` without starting: both are errors here, not waited for.)
    if error.raw_os_error() != Some(libc::EINPROGRESS) {
        return Err(error);
    }
    wait(raw, libc::POLLOUT, until)?;
    let mut failure: libc::c_int = 0;
    let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    // SAFETY: `failure` and `len` are valid for writes of their sizes.
    let got = unsafe {
        libc::getsockopt(
            raw,
            libc::SOL_SOCKET,
            libc::SO_ERROR,
            std::ptr::addr_of_mut!(failure).cast(),
            &mut len,
        )
    };
    if got < 0 {
        return Err(io::Error::last_os_error());
    }
    if failure != 0 {
        return Err(io::Error::from_raw_os_error(failure));
    }
    Ok(fd)
}

/// Waits until `fd` is ready for `events`, or `until` passes (`TimedOut`).
fn wait(fd: libc::c_int, events: libc::c_short, until: Instant) -> io::Result<()> {
    loop {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::Error::from(io::ErrorKind::TimedOut));
        }
        let mut poll = libc::pollfd {
            fd,
            events,
            revents: 0,
        };
        let millis = left.as_millis().clamp(1, i32::MAX as u128) as libc::c_int;
        // SAFETY: one valid `pollfd`.
        let ready = unsafe { libc::poll(&mut poll, 1, millis) };
        match ready {
            r if r > 0 => return Ok(()),
            0 => {}
            _ => {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::Interrupted {
                    return Err(error);
                }
            }
        }
    }
}

/// A non-blocking descriptor read with `poll(2)` up to a deadline.
struct Polled {
    fd: OwnedFd,
    until: Instant,
}

impl Read for Polled {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let raw = self.fd.as_raw_fd();
        loop {
            // SAFETY: `out` is valid for writes of its length; `raw` is an open descriptor.
            let n = unsafe { libc::read(raw, out.as_mut_ptr().cast(), out.len()) };
            if n >= 0 {
                return Ok(n as usize);
            }
            let error = io::Error::last_os_error();
            match error.kind() {
                io::ErrorKind::Interrupted => {}
                io::ErrorKind::WouldBlock => wait(raw, libc::POLLIN, self.until)?,
                _ => return Err(error),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::os::unix::net::{UnixListener, UnixStream};

    use org_office_launch::write_hand_off;
    use org_proto::{AppToken, DbKey};

    use super::*;

    /// A short office home with a `run/` folder (socket paths are limited to ~100 bytes).
    fn home() -> tempfile::TempDir {
        let dir = tempfile::Builder::new().prefix("ho").tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("run")).unwrap();
        dir
    }

    /// The tests play the host in-process: its pid is this process's.
    fn in_process() -> PeerRule {
        PeerRule::Pid(std::process::id() as libc::pid_t)
    }

    fn id(n: u8) -> KeySocketId {
        KeySocketId::generate_from([n; 8])
    }

    fn listen(home: &Path, id: &KeySocketId) -> UnixListener {
        let path = key_socket_path(home, id);
        let listener = UnixListener::bind(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        listener
    }

    /// What the host does after it has written the line: stop writing, and keep the connection
    /// open until the server closes its end (bounded), so the server can still ask the kernel who
    /// the peer is. Then it closes.
    fn finish_like_the_host(stream: &mut UnixStream) {
        let _ = stream.shutdown(std::net::Shutdown::Write);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut sink = [0u8; 64];
        while matches!(std::io::Read::read(stream, &mut sink), Ok(n) if n > 0) {}
    }

    fn token() -> AppToken {
        AppToken::from_bytes([3; org_proto::APP_TOKEN_BYTES])
    }

    #[test]
    fn the_line_is_read_even_when_the_host_wrote_it_and_closed_before_the_server_looked() {
        // The macOS race: a read timeout set after the peer closed fails with EINVAL. Without
        // one, the buffered line is still there to read, every time.
        let home = home();
        for n in 0..100u8 {
            let id = id(n);
            let listener = listen(home.path(), &id);
            let host = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                write_hand_off(&mut stream, &token(), Some(&DbKey::from_bytes([9; 32]))).unwrap();
                finish_like_the_host(&mut stream);
                // Closed at once, with the line unread.
            });
            let got = receive_within(
                home.path(),
                &id,
                FirstLine::Start,
                Duration::from_secs(5),
                in_process(),
            )
            .unwrap_or_else(|e| panic!("round {n}: {e}"));
            assert!(got.db_key.is_some());
            host.join().unwrap();
        }
    }

    #[test]
    fn a_socket_that_is_not_owner_only_a_link_or_not_a_socket_is_refused_before_a_connect() {
        let home = home();
        let refused = |id: &KeySocketId| {
            matches!(
                receive_within(
                    home.path(),
                    id,
                    FirstLine::Start,
                    Duration::from_millis(300),
                    in_process()
                ),
                Err(LaunchError::Invalid(_))
            )
        };
        for (n, mode) in [(1, 0o644), (2, 0o666), (3, 0o700), (4, 0o400)] {
            let id = id(n);
            let listener = listen(home.path(), &id);
            std::fs::set_permissions(
                key_socket_path(home.path(), &id),
                std::fs::Permissions::from_mode(mode),
            )
            .unwrap();
            listener.set_nonblocking(true).unwrap();
            assert!(refused(&id), "{mode:o}");
            assert!(listener.accept().is_err(), "{mode:o}: it was connected to");
        }
        // A link to a good socket.
        let target = id(5);
        let _good = listen(home.path(), &target);
        let link = id(6);
        std::os::unix::fs::symlink(
            key_socket_path(home.path(), &target),
            key_socket_path(home.path(), &link),
        )
        .unwrap();
        assert!(refused(&link));
        // A file, and nothing.
        let file = id(7);
        std::fs::write(key_socket_path(home.path(), &file), b"x").unwrap();
        assert!(refused(&file));
        assert!(refused(&id(8)));
    }

    #[test]
    fn a_host_that_says_nothing_hits_the_deadline_and_one_that_closes_is_gone() {
        let home = home();
        let silent = id(1);
        let listener = listen(home.path(), &silent);
        let held = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            std::thread::sleep(Duration::from_secs(2));
            drop(stream);
        });
        let started = Instant::now();
        let err = receive_within(
            home.path(),
            &silent,
            FirstLine::Start,
            Duration::from_millis(300),
            in_process(),
        )
        .unwrap_err();
        assert!(!err.is_host_gone(), "{err}");
        assert!(matches!(
            err.fail_code(),
            org_office_launch::FailCode::StorageError
        ));
        assert!(started.elapsed() < Duration::from_secs(2));
        held.join().unwrap();

        let gone = id(2);
        let listener = listen(home.path(), &gone);
        let host = std::thread::spawn(move || drop(listener.accept().unwrap()));
        let err = receive_within(
            home.path(),
            &gone,
            FirstLine::Start,
            Duration::from_secs(5),
            in_process(),
        )
        .unwrap_err();
        assert!(err.is_host_gone(), "{err}");
        host.join().unwrap();
    }

    #[test]
    fn the_log_category_names_what_failed_without_the_errors_text() {
        let home = home();
        let word = |id: &KeySocketId, wait: Duration| {
            category(
                &receive_within(home.path(), id, FirstLine::Start, wait, in_process()).unwrap_err(),
            )
        };
        assert_eq!(word(&id(1), Duration::from_millis(100)), "refused");
        let silent = id(2);
        let listener = listen(home.path(), &silent);
        let held = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            std::thread::sleep(Duration::from_millis(600));
            drop(stream);
        });
        assert_eq!(word(&silent, Duration::from_millis(200)), "deadline");
        held.join().unwrap();
        let stale = id(3);
        drop(listen(home.path(), &stale));
        assert_eq!(word(&stale, Duration::from_secs(2)), "connect or read");
        let garbage = id(4);
        let listener = listen(home.path(), &garbage);
        let host = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = stream.write_all(b"not json\n");
        });
        assert_eq!(word(&garbage, Duration::from_secs(2)), "line");
        host.join().unwrap();
    }

    #[test]
    fn a_listener_that_is_not_the_host_is_refused_before_anything_is_read() {
        // The tests' listener is this process, not the parent that started it: under the host
        // rule it is a stranger, and the hand-off it would write is never read.
        let home = home();
        let id = id(1);
        let listener = listen(home.path(), &id);
        let host = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = write_hand_off(&mut stream, &token(), Some(&DbKey::from_bytes([9; 32])));
            finish_like_the_host(&mut stream);
        });
        let err = receive_within(
            home.path(),
            &id,
            FirstLine::Start,
            Duration::from_secs(5),
            PeerRule::Host,
        )
        .unwrap_err();
        assert!(matches!(err, LaunchError::Invalid(_)), "{err}");
        assert_eq!(category(&err), "refused");
        // Another pid is a stranger too.
        let other = id_for_other_pid();
        let listener = listen(home.path(), &other);
        let host2 = std::thread::spawn(move || drop(listener.accept()));
        let err = receive_within(
            home.path(),
            &other,
            FirstLine::Start,
            Duration::from_secs(5),
            PeerRule::Pid(std::process::id() as libc::pid_t + 1),
        )
        .unwrap_err();
        assert!(matches!(err, LaunchError::Invalid(_)), "{err}");
        let _ = host.join();
        let _ = host2.join();
    }

    fn id_for_other_pid() -> KeySocketId {
        id(77)
    }

    #[test]
    fn nothing_listening_on_a_socket_file_is_an_error_not_a_hang() {
        // A stale socket file nobody listens on: the connect is refused at once.
        let home = home();
        let stale = id(1);
        drop(listen(home.path(), &stale));
        let err = receive_within(
            home.path(),
            &stale,
            FirstLine::Start,
            Duration::from_secs(5),
            in_process(),
        )
        .unwrap_err();
        assert!(!err.is_host_gone());
    }

    #[test]
    fn the_connection_is_close_on_exec_and_non_blocking_and_a_long_path_is_refused() {
        let home = home();
        let id = id(1);
        let _listener = listen(home.path(), &id);
        let path = key_socket_path(home.path(), &id);
        let fd = connect(&path, Instant::now() + Duration::from_secs(5)).unwrap();
        let raw = fd.as_raw_fd();
        // SAFETY: `fcntl` queries on a descriptor this test owns.
        let (fd_flags, fl_flags) = unsafe {
            (
                libc::fcntl(raw, libc::F_GETFD),
                libc::fcntl(raw, libc::F_GETFL),
            )
        };
        assert!(fd_flags & libc::FD_CLOEXEC != 0, "close-on-exec");
        assert!(fl_flags & libc::O_NONBLOCK != 0, "non-blocking");

        let long = Path::new("/").join("x".repeat(200));
        assert!(matches!(
            receive_within(
                &long,
                &id,
                FirstLine::Start,
                Duration::from_millis(100),
                in_process()
            ),
            Err(LaunchError::Invalid(_))
        ));
    }

    #[test]
    fn extra_bytes_after_the_line_are_not_read_as_part_of_it() {
        let home = home();
        let id = id(1);
        let listener = listen(home.path(), &id);
        let host = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            // The server may read the line and hang up before the rest is written: that write is
            // allowed to fail.
            write_hand_off(&mut stream, &token(), Some(&DbKey::from_bytes([9; 32]))).unwrap();
            let _ = stream.write_all(b"{\"extra\":true}\n");
            finish_like_the_host(&mut stream);
        });
        let got = receive_within(
            home.path(),
            &id,
            FirstLine::Start,
            Duration::from_secs(5),
            in_process(),
        )
        .unwrap();
        assert!(got.db_key.is_some());
        host.join().unwrap();
    }
}
