//! Which fd of a process holds the other end of a connection the host accepted, and whether that
//! process holds it the way the admitted server must (`handoff`'s peer-fd check). Public libproc
//! only, and with `sys` the one other module (besides `spawn`) with unsafe code.
//!
//! Why: the peer's audit token (or pid) names the process that **last used** the connection, not
//! the one that reads from it. An early resumer can put a socket on the genuine server's fd 2
//! that its helper also holds and connects to the key socket; the server's runtime then writes to
//! fd 2 before `main` (libobjc does, under `OBJC_HELP` and the other `OBJC_PRINT_*` variables, which
//! the hardened runtime doesn't strip), and even a write that fails makes the server that socket's
//! last user. The token check at accept then passes, and the line would go to the helper. What the
//! kernel's fd tables show tells the two apart: the admitted server's own connection is on one fd
//! it made itself, close-on-exec, held by nobody else; the attacker's socket is one the server
//! **inherited** across `exec`, which can never be close-on-exec (and usually sits on 0, 1 or 2),
//! and which the helper still holds, so its open file is shared.
//!
//! [`own_conn`] reads the identity of the host's accepted end: its socket, its protocol control
//! block and the socket it is connected to. [`check_holder`] lists the process's fds and finds
//! the ones that are that connection's other end (all three identities must agree), then requires
//! **exactly one**, not 0, 1 or 2, close-on-exec, and not shared ([`judge`]); anything else is a
//! [`Refusal`]. An error from libproc (`EPERM`, `ESRCH`, a short read, a process with no fds at
//! all, which libproc reports as a zero return) is an `Err`, and the caller fails closed on it too.
//!
//! Limits:
//! - It is a snapshot: it says what the fd tables looked like during the scan, which takes tens of
//!   microseconds. The caller re-checks who the peer is afterwards (`handoff`).
//! - The kernel handles it compares are obfuscated addresses, stable for one boot and comparable
//!   between processes, but **reused once a socket is freed**. So a [`Conn`] borrows the accepted
//!   fd: it can't outlive it, and the host keeps that fd open across the whole check.
//! - `SHARED` means the open file has more than one reference: another fd in any process, or a
//!   descriptor in flight (`SCM_RIGHTS`). It is also set, for a moment, on every close-on-exec fd
//!   of a process that is in the middle of `posix_spawn` (or `fork`): measured, about one scan in
//!   seven of a process spawning in a tight loop. Hence the contract: the server doesn't fork or
//!   spawn while its key connection is open.
//! - A copy made by a library inside the server is caught by `SHARED` alone (the match is
//!   close-on-exec and above 2), which is why that flag is essential. The spike found none: with
//!   fd 2 a socket and every `OBJC_*` printing variable set, neither a Foundation/libobjc test
//!   program nor the real server held another fd on fd 2's socket.

mod sys;

use std::fmt;
use std::io;
use std::marker::PhantomData;
use std::mem::{size_of, MaybeUninit};
use std::os::fd::{AsRawFd, BorrowedFd, RawFd};

use sys::{PROC_FP_CLEXEC, PROC_FP_SHARED, PROC_PIDFDSOCKETINFO, SOCKINFO_UN};

/// The kernel's identity of one end of a connected Unix-domain stream socket of this process:
/// what the other end must match. It borrows the fd it was read from, because the handles can be
/// reused once that socket is freed. Its `Debug` doesn't show them (they are kernel addresses,
/// if obfuscated).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Conn<'fd> {
    /// Our socket.
    so: u64,
    /// Our protocol control block.
    pcb: u64,
    /// The socket ours is connected to: the one the peer must hold.
    conn: u64,
    fd: PhantomData<BorrowedFd<'fd>>,
}

impl fmt::Debug for Conn<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Conn").finish_non_exhaustive()
    }
}

impl Conn<'_> {
    /// Whether `info` is the other end of this connection: connected to our socket and our
    /// control block, and itself the socket ours is connected to. All three must agree.
    fn is_other_end(&self, info: &sys::socket_fdinfo) -> bool {
        if info.psi.soi_kind != SOCKINFO_UN {
            return false;
        }
        // SAFETY: `soi_kind == SOCKINFO_UN` means the kernel filled `pri_un`, the union's only
        // member here; every bit pattern is a valid `un_sockinfo` anyway (plain integers).
        let un = unsafe { info.psi.soi_proto.pri_un };
        un.unsi_conn_so == self.so && un.unsi_conn_pcb == self.pcb && info.psi.soi_so == self.conn
    }
}

/// Reads the identity of a connected Unix-domain stream socket of this process (the host's end of
/// an accepted connection). Valid only while `fd` stays open, which the borrow enforces.
pub(crate) fn own_conn(fd: BorrowedFd<'_>) -> io::Result<Conn<'_>> {
    let info = socket_info(std::process::id() as i32, fd.as_raw_fd())?;
    if info.psi.soi_kind != SOCKINFO_UN || info.psi.soi_type != libc::SOCK_STREAM {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a Unix-domain stream socket",
        ));
    }
    // SAFETY: as in `is_other_end`.
    let un = unsafe { info.psi.soi_proto.pri_un };
    if un.unsi_conn_so == 0 || info.psi.soi_so == 0 || info.psi.soi_pcb == 0 {
        return Err(io::Error::new(
            io::ErrorKind::NotConnected,
            "the socket isn't connected",
        ));
    }
    Ok(Conn {
        so: info.psi.soi_so,
        pcb: info.psi.soi_pcb,
        conn: un.unsi_conn_so,
        fd: PhantomData,
    })
}

/// Why a process doesn't hold a connection the way the admitted server must.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// No fd of the process is the other end of the connection.
    NoMatch,
    /// Several fds are (listed): it could hand one away.
    SeveralMatches(Vec<i32>),
    /// The one match is fd 0, 1 or 2: the mark of a socket it inherited.
    StdFd(i32),
    /// The match isn't close-on-exec: it may have survived an `exec` (an inherited fd can't be
    /// close-on-exec), and would leak into anything the process runs.
    NotCloexec(i32),
    /// The match's open file has another reference: another process (or another fd, or a
    /// descriptor in flight) holds the connection too.
    Shared(i32),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::NoMatch => f.write_str("it holds no end of the connection"),
            Refusal::SeveralMatches(fds) => {
                write!(f, "it holds the connection on several fds ({fds:?})")
            }
            Refusal::StdFd(fd) => write!(f, "it holds the connection on standard fd {fd}"),
            Refusal::NotCloexec(fd) => {
                write!(f, "its fd {fd} holding the connection isn't close-on-exec")
            }
            Refusal::Shared(fd) => write!(
                f,
                "its fd {fd} holding the connection is shared with another holder"
            ),
        }
    }
}

/// Checks that process `pid` holds the other end of `conn` on exactly one fd, not 0, 1 or 2,
/// close-on-exec and not shared; returns that fd, or why not ([`judge`]).
///
/// `Err` means libproc couldn't answer: the process is gone, it isn't ours to inspect, a short
/// read, or no fds at all (a zero return). An fd that goes away during the scan, or whose number
/// is reused by something that isn't a socket, is skipped ([`on_fd_error`]): a holder hiding
/// that way still leaves `SHARED` on the match.
pub(crate) fn check_holder(pid: i32, conn: &Conn<'_>) -> io::Result<Result<i32, Refusal>> {
    let mut matches = Vec::new();
    let mut check_process = false;
    for entry in list_fds(pid)? {
        if entry.proc_fdtype != libc::PROX_FDTYPE_SOCKET as u32 {
            continue;
        }
        match socket_info(pid, entry.proc_fd) {
            Ok(info) if conn.is_other_end(&info) => {
                matches.push((entry.proc_fd, info.pfi.fi_status));
            }
            Ok(_) => {}
            Err(e) => match on_fd_error(e.raw_os_error()) {
                FdError::Skip => {}
                FdError::CheckProcess => check_process = true,
                FdError::Fail => return Err(e),
            },
        }
    }
    if check_process {
        // A per-fd `ESRCH` is only an fd going away if the process itself is still there.
        list_size_hint(pid)?;
    }
    Ok(judge(&matches))
}

/// What a scan does about an error reading one fd.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FdError {
    /// The fd is no longer a socket: closed (`EBADF`), or its number reused by something else
    /// (`ENOTSOCK`). Skipped.
    Skip,
    /// `ESRCH`: the fd went away, or the whole process did. Skipped if the process is still
    /// there after the scan, else an error.
    CheckProcess,
    /// Anything else (`EPERM`, an unexpected size, …): the scan fails.
    Fail,
}

/// The per-fd error policy, by errno.
fn on_fd_error(errno: Option<i32>) -> FdError {
    match errno {
        Some(libc::EBADF | libc::ENOTSOCK) => FdError::Skip,
        Some(libc::ESRCH) => FdError::CheckProcess,
        _ => FdError::Fail,
    }
}

/// The verdict on the `(fd, fi_status)` of every fd that matched. In order: how many matched,
/// then fd 0-2, then close-on-exec, then shared.
fn judge(matches: &[(i32, u32)]) -> Result<i32, Refusal> {
    match *matches {
        [] => Err(Refusal::NoMatch),
        [(fd, _)] if (0..=2).contains(&fd) => Err(Refusal::StdFd(fd)),
        [(fd, status)] if status & PROC_FP_CLEXEC == 0 => Err(Refusal::NotCloexec(fd)),
        [(fd, status)] if status & PROC_FP_SHARED != 0 => Err(Refusal::Shared(fd)),
        [(fd, _)] => Ok(fd),
        _ => Err(Refusal::SeveralMatches(
            matches.iter().map(|&(fd, _)| fd).collect(),
        )),
    }
}

const FDINFO_SIZE: usize = size_of::<libc::proc_fdinfo>();

/// The kernel's estimate of the size of `pid`'s fd list, in bytes (with some slack). A process
/// with no fds at all gives a zero return, which is an error here.
fn list_size_hint(pid: i32) -> io::Result<usize> {
    clear_errno();
    // SAFETY: a null buffer of size 0 asks only for the size.
    let n = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0) };
    if n <= 0 {
        return Err(libproc_error("proc_pidinfo(PROC_PIDLISTFDS) size"));
    }
    Ok(n as usize)
}

/// Every open fd of `pid`, retried with a larger buffer if the list filled it.
fn list_fds(pid: i32) -> io::Result<Vec<libc::proc_fdinfo>> {
    let mut room = list_size_hint(pid)? / FDINFO_SIZE + 32;
    for _ in 0..4 {
        let mut list: Vec<libc::proc_fdinfo> = Vec::with_capacity(room);
        let bytes = i32::try_from(room * FDINFO_SIZE)
            .map_err(|_| io::Error::other("the fd list is too large"))?;
        clear_errno();
        // SAFETY: the buffer has room for `bytes` bytes, and the kernel writes at most that many.
        let n = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDLISTFDS,
                0,
                list.as_mut_ptr().cast(),
                bytes,
            )
        };
        if n <= 0 {
            return Err(libproc_error("proc_pidinfo(PROC_PIDLISTFDS)"));
        }
        let n = n as usize;
        if !n.is_multiple_of(FDINFO_SIZE) || n > room * FDINFO_SIZE {
            return Err(io::Error::other(
                "proc_pidinfo(PROC_PIDLISTFDS) returned a malformed size",
            ));
        }
        if n < room * FDINFO_SIZE {
            // SAFETY: the kernel wrote the first `n / FDINFO_SIZE` entries, all plain integers.
            unsafe { list.set_len(n / FDINFO_SIZE) };
            return Ok(list);
        }
        room *= 2;
    }
    Err(io::Error::other(
        "proc_pidinfo(PROC_PIDLISTFDS): the fd list kept growing",
    ))
}

/// `PROC_PIDFDSOCKETINFO` for fd `fd` of `pid`. An error keeps its errno (`EBADF`: no such fd,
/// `ENOTSOCK`: not a socket, `ESRCH`: no such process, `EPERM`: not allowed).
fn socket_info(pid: i32, fd: RawFd) -> io::Result<sys::socket_fdinfo> {
    const SIZE: usize = size_of::<sys::socket_fdinfo>();
    let mut info = MaybeUninit::<sys::socket_fdinfo>::zeroed();
    clear_errno();
    // SAFETY: the buffer is SIZE bytes, and the kernel writes at most that many.
    let n = unsafe {
        libc::proc_pidfdinfo(
            pid,
            fd,
            PROC_PIDFDSOCKETINFO,
            info.as_mut_ptr().cast(),
            SIZE as i32,
        )
    };
    if n <= 0 {
        return Err(libproc_error("proc_pidfdinfo(PROC_PIDFDSOCKETINFO)"));
    }
    if n as usize != SIZE {
        return Err(io::Error::other(format!(
            "proc_pidfdinfo(PROC_PIDFDSOCKETINFO) returned {n} of {SIZE} bytes"
        )));
    }
    // SAFETY: zeroed, then written whole by the kernel; every field is a plain integer.
    Ok(unsafe { info.assume_init() })
}

fn clear_errno() {
    // SAFETY: `__error` returns this thread's errno slot.
    unsafe { *libc::__error() = 0 };
}

/// libproc reports a failure as a zero (or negative) return with errno set; a zero without an
/// errno is a failure here too.
fn libproc_error(what: &str) -> io::Error {
    let e = io::Error::last_os_error();
    match e.raw_os_error() {
        Some(0) | None => io::Error::other(format!("{what}: a zero return")),
        Some(_) => e,
    }
}

#[cfg(test)]
mod tests {
    //! Each test plays the host: it listens, accepts a connection from a real peer process and
    //! checks it. A peer is this test binary run again in a peer mode ([`peer_entry`]), spawned
    //! with `posix_spawn` (`Command`, no `pre_exec`), so this process never forks. The attack as
    //! a whole (an inherited fd 2 that a helper holds, the genuine server's pre-`main` write) is
    //! `tests/binary.rs`'s, against the real host; these cover each rule.

    use super::*;
    use std::io::{BufRead, BufReader, Read};
    use std::os::fd::AsFd;
    use std::os::unix::net::{UnixDatagram, UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::process::{Child, ChildStdin, Command, Stdio};
    use std::time::{Duration, Instant};

    const MODE_ENV: &str = "ORG_PEER_FDS_TEST_MODE";
    const SOCKET_ENV: &str = "ORG_PEER_FDS_TEST_SOCKET";

    /// The host's side: a listener in a fresh short folder under the temporary folder (well within
    /// `sun_path`'s 104 bytes), removed with it.
    struct Host {
        listener: UnixListener,
        path: PathBuf,
        _dir: tempfile::TempDir,
    }

    impl Host {
        fn new() -> Self {
            let dir = tempfile::Builder::new()
                .prefix("pfd")
                .tempdir_in(std::env::temp_dir())
                .unwrap();
            let path = dir.path().join("s");
            assert!(path.as_os_str().len() < 100, "{}", path.display());
            let listener = UnixListener::bind(&path).unwrap();
            listener.set_nonblocking(true).unwrap();
            Host {
                listener,
                path,
                _dir: dir,
            }
        }

        fn accept(&self) -> UnixStream {
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                match self.listener.accept() {
                    Ok((stream, _)) => return stream,
                    Err(e)
                        if e.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(e) => panic!("accept: {e}"),
                }
            }
        }
    }

    /// A peer process. It (and anything it forked) exits when its stdin closes; dropped, it is
    /// killed and reaped.
    struct Peer {
        child: Child,
        stdin: Option<ChildStdin>,
        /// The numbers it reported: its fds, or a forked pid.
        report: Vec<i32>,
    }

    impl Peer {
        fn pid(&self) -> i32 {
            self.child.id() as i32
        }
    }

    impl Drop for Peer {
        fn drop(&mut self) {
            drop(self.stdin.take());
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    /// Runs this test binary again as a peer in `mode`, connected to `host`, and waits for its
    /// report.
    fn spawn_peer(mode: &str, host: &Host) -> Peer {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "peer_fds::tests::peer_entry",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(MODE_ENV, mode)
            .env(SOCKET_ENV, &host.path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut out = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        let report = loop {
            line.clear();
            assert!(
                out.read_line(&mut line).unwrap() > 0,
                "the peer exited before it reported"
            );
            if let Some(rest) = line.split("PEERFD").nth(1) {
                break rest
                    .split_whitespace()
                    .map(|n| n.parse().unwrap())
                    .collect();
            }
        };
        let stdin = child.stdin.take();
        Peer {
            child,
            stdin,
            report,
        }
    }

    /// The peers' entry point: does nothing unless `spawn_peer` ran it.
    #[test]
    #[ignore = "the entry point of the peer processes the other tests spawn"]
    fn peer_entry() {
        let Ok(mode) = std::env::var(MODE_ENV) else {
            return;
        };
        let path = std::env::var(SOCKET_ENV).unwrap();
        let report = |numbers: &[i32]| {
            let words: Vec<String> = numbers.iter().map(i32::to_string).collect();
            println!("PEERFD {}", words.join(" "));
        };
        // std's sockets are close-on-exec, on the lowest free fd (3 or more).
        let stream = UnixStream::connect(&path).unwrap();
        let fd = stream.as_raw_fd();
        match mode.as_str() {
            "clean" => report(&[fd]),
            "dup" => {
                // SAFETY: duplicates an fd this process owns; the copy is left open on purpose.
                let copy = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 20) };
                assert!(copy >= 0);
                report(&[fd, copy]);
            }
            "fork" => {
                // SAFETY: the forked child only calls async-signal-safe `read` and `_exit`.
                let pid = unsafe { libc::fork() };
                if pid == 0 {
                    exit_on_stdin_eof();
                }
                assert!(pid > 0);
                report(&[fd, pid]);
            }
            "no-cloexec" => {
                // SAFETY: clears only the close-on-exec flag of an fd this process owns.
                assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETFD, 0) }, 0);
                report(&[fd]);
            }
            "stderr" => {
                // SAFETY: moves the connection onto fd 2 (`dup2` leaves it without
                // close-on-exec) and closes the original: fd 2 is then its only fd.
                unsafe {
                    assert_eq!(libc::dup2(fd, 2), 2);
                    libc::close(std::os::fd::IntoRawFd::into_raw_fd(stream));
                }
                report(&[2]);
                let _ = io::stdin().read_to_end(&mut Vec::new());
                return;
            }
            other => panic!("unknown peer mode {other}"),
        }
        let _ = io::stdin().read_to_end(&mut Vec::new());
        drop(stream);
    }

    /// Waits for stdin's EOF, then exits without running anything else. Async-signal-safe.
    fn exit_on_stdin_eof() -> ! {
        let mut byte = 0u8;
        // SAFETY: reads into a valid one-byte buffer; `_exit` never returns.
        unsafe {
            while libc::read(0, (&mut byte as *mut u8).cast(), 1) > 0 {}
            libc::_exit(0)
        }
    }

    /// The host's end of a connection from `peer`, and the verdict on it.
    fn verdict(host: &Host, peer: &Peer) -> Result<i32, Refusal> {
        let accepted = host.accept();
        let conn = own_conn(accepted.as_fd()).unwrap();
        check_holder(peer.pid(), &conn).unwrap()
    }

    #[test]
    fn a_peer_that_made_its_own_close_on_exec_connection_holds_it_properly() {
        let host = Host::new();
        let peer = spawn_peer("clean", &host);
        assert!(peer.report[0] >= 3);
        assert_eq!(verdict(&host, &peer), Ok(peer.report[0]));
    }

    #[test]
    fn a_connection_on_two_fds_is_refused() {
        let host = Host::new();
        let peer = spawn_peer("dup", &host);
        let (fd, copy) = (peer.report[0], peer.report[1]);
        assert_eq!(
            verdict(&host, &peer),
            Err(Refusal::SeveralMatches(vec![fd, copy]))
        );
    }

    /// The peer forked after connecting: two processes hold the same open file. Each is refused.
    #[test]
    fn a_connection_another_process_also_holds_is_refused_as_shared() {
        let host = Host::new();
        let peer = spawn_peer("fork", &host);
        let (fd, forked) = (peer.report[0], peer.report[1]);
        let accepted = host.accept();
        let conn = own_conn(accepted.as_fd()).unwrap();
        assert_eq!(
            check_holder(peer.pid(), &conn).unwrap(),
            Err(Refusal::Shared(fd))
        );
        assert_eq!(
            check_holder(forked, &conn).unwrap(),
            Err(Refusal::Shared(fd))
        );
    }

    #[test]
    fn a_connection_without_close_on_exec_is_refused() {
        let host = Host::new();
        let peer = spawn_peer("no-cloexec", &host);
        let fd = peer.report[0];
        assert!(fd >= 3);
        assert_eq!(verdict(&host, &peer), Err(Refusal::NotCloexec(fd)));
    }

    #[test]
    fn a_connection_on_a_standard_fd_is_refused() {
        let host = Host::new();
        let peer = spawn_peer("stderr", &host);
        assert_eq!(verdict(&host, &peer), Err(Refusal::StdFd(2)));
    }

    /// The peer holds a connection to the same listener, but not the one being checked.
    #[test]
    fn another_connection_is_no_match() {
        let host = Host::new();
        let ours = UnixStream::connect(&host.path).unwrap();
        let other_accepted = host.accept();
        let other = own_conn(other_accepted.as_fd()).unwrap();
        let peer = spawn_peer("clean", &host);
        let accepted = host.accept();
        let conn = own_conn(accepted.as_fd()).unwrap();
        assert_eq!(
            check_holder(peer.pid(), &other).unwrap(),
            Err(Refusal::NoMatch)
        );
        assert_eq!(check_holder(peer.pid(), &conn).unwrap(), Ok(peer.report[0]));
        drop(ours);
    }

    /// A pid that exited and was reaped is an error (the caller fails closed), not a verdict.
    #[test]
    fn a_process_that_is_gone_is_an_error() {
        let host = Host::new();
        let _client = UnixStream::connect(&host.path).unwrap();
        let accepted = host.accept();
        let conn = own_conn(accepted.as_fd()).unwrap();
        let mut child = Command::new("/usr/bin/true").spawn().unwrap();
        let pid = child.id() as i32;
        child.wait().unwrap();
        let err = check_holder(pid, &conn).unwrap_err();
        assert_eq!(err.raw_os_error(), Some(libc::ESRCH), "{err}");
    }

    #[test]
    fn own_conn_takes_only_a_connected_unix_stream_socket() {
        let file = std::fs::File::open(Path::new("/dev/null")).unwrap();
        assert!(own_conn(file.as_fd()).is_err());
        let host = Host::new();
        assert_eq!(
            own_conn(host.listener.as_fd()).unwrap_err().kind(),
            io::ErrorKind::NotConnected
        );
        let (datagram, _other) = UnixDatagram::pair().unwrap();
        assert_eq!(
            own_conn(datagram.as_fd()).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        let (stream, _other) = UnixStream::pair().unwrap();
        let conn = own_conn(stream.as_fd()).unwrap();
        assert_eq!(format!("{conn:?}"), "Conn { .. }");
    }

    /// Each branch of the verdict, and their order: the count first, then fd 0-2, then
    /// close-on-exec, then shared.
    #[test]
    fn the_verdict_takes_exactly_one_close_on_exec_unshared_fd_above_2() {
        const C: u32 = PROC_FP_CLEXEC;
        const S: u32 = PROC_FP_SHARED;
        assert_eq!(judge(&[]), Err(Refusal::NoMatch));
        assert_eq!(judge(&[(1, C)]), Err(Refusal::StdFd(1)));
        assert_eq!(judge(&[(2, C | S)]), Err(Refusal::StdFd(2)));
        assert_eq!(judge(&[(5, 0)]), Err(Refusal::NotCloexec(5)));
        assert_eq!(judge(&[(5, S)]), Err(Refusal::NotCloexec(5)));
        assert_eq!(judge(&[(5, C | S)]), Err(Refusal::Shared(5)));
        assert_eq!(judge(&[(5, C)]), Ok(5));
        assert_eq!(judge(&[(3, C)]), Ok(3));
        assert_eq!(
            judge(&[(5, C), (9, C)]),
            Err(Refusal::SeveralMatches(vec![5, 9]))
        );
        assert_eq!(
            judge(&[(2, 0), (9, C)]),
            Err(Refusal::SeveralMatches(vec![2, 9]))
        );
    }

    #[test]
    fn an_fd_that_went_away_is_skipped_and_any_other_error_fails() {
        assert_eq!(on_fd_error(Some(libc::EBADF)), FdError::Skip);
        assert_eq!(on_fd_error(Some(libc::ENOTSOCK)), FdError::Skip);
        assert_eq!(on_fd_error(Some(libc::ESRCH)), FdError::CheckProcess);
        for errno in [Some(libc::EPERM), Some(libc::EINVAL), Some(0), None] {
            assert_eq!(on_fd_error(errno), FdError::Fail, "{errno:?}");
        }
    }

    #[test]
    fn refusals_say_why_without_a_secret() {
        assert_eq!(
            Refusal::StdFd(2).to_string(),
            "it holds the connection on standard fd 2"
        );
        assert_eq!(
            Refusal::SeveralMatches(vec![3, 20]).to_string(),
            "it holds the connection on several fds ([3, 20])"
        );
    }

    /// 200 checks (the host's whole check: `own_conn`, then `check_holder`) against a live peer.
    /// Run with `--nocapture` to see p50 and p99.
    #[test]
    fn two_hundred_checks_take_microseconds_each() {
        let host = Host::new();
        let peer = spawn_peer("clean", &host);
        let accepted = host.accept();
        let mut took: Vec<Duration> = (0..200)
            .map(|_| {
                let start = Instant::now();
                let conn = own_conn(accepted.as_fd()).unwrap();
                assert_eq!(check_holder(peer.pid(), &conn).unwrap(), Ok(peer.report[0]));
                start.elapsed()
            })
            .collect();
        took.sort();
        println!(
            "peer-fd check x200: p50 {:?}, p99 {:?}, max {:?}",
            took[99], took[197], took[199]
        );
        assert!(took[99] < Duration::from_millis(50), "{:?}", took[99]);
    }
}
