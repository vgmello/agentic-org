//! Each fact this module states about the system, checked against what the system
//! does, so that a fact cannot drift from its adapter. Consumers' tests decide by these
//! facts which of their cases a system has; a wrong fact would turn such a case into a
//! test that checks nothing.

use std::io::{self, Write};
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::{UnixDatagram, UnixStream};
use std::process::Command;

use super::testing::{bind_path, seqpacket_pair, CONNECTED_STREAM_CAN_LISTEN, SEQPACKET};
use super::{
    send_flags, DATAGRAM_PEER_CLOSE_MAY_RESET, DATAGRAM_PEER_FULL, PEER_CLOSED_IS_EINVAL,
    SEND_SUPPRESSES_SIGPIPE, STREAM_PEER_CLOSE_WITH_UNREAD_RESETS,
};

#[test]
fn seqpacket_is_whether_the_system_makes_a_unix_seqpacket_pair() {
    let mut fds = [-1; 2];
    // SAFETY: socketpair writes two descriptors into the live two-item array.
    let made =
        unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_SEQPACKET, 0, fds.as_mut_ptr()) } == 0;
    if made {
        // SAFETY: socketpair succeeded; each descriptor is closed once.
        drop(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) });
    }
    assert_eq!(made, SEQPACKET);
    assert_eq!(seqpacket_pair().unwrap().is_some(), SEQPACKET);
}

#[test]
fn a_connected_stream_becomes_a_listener_exactly_where_the_fact_says() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("s");
    let (stream, peer) = UnixStream::pair().unwrap();
    bind_path(stream.as_fd(), &path).unwrap();
    drop(peer);
    // macOS has been seen to refuse `listen` (EINVAL) for a moment after the peer
    // closed, in a loaded run of the whole suite (two of five runs), so it is tried
    // again for up to half a second.
    let mut listened = false;
    for _ in 0..100 {
        // SAFETY: listen on a live socket owned by this test.
        if unsafe { libc::listen(stream.as_raw_fd(), 1) } == 0 {
            listened = true;
            break;
        }
        assert_eq!(
            io::Error::last_os_error().raw_os_error(),
            Some(libc::EINVAL)
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    // A listener only if a connection reaches it.
    let accepting = listened && UnixStream::connect(&path).is_ok();
    assert_eq!(
        accepting, CONNECTED_STREAM_CAN_LISTEN,
        "listen returned {listened}"
    );
}

#[test]
fn a_closed_peers_address_query_answers_as_the_fact_says() {
    let (stream, peer) = UnixStream::pair().unwrap();
    drop(peer);
    // SAFETY: zero is a valid sockaddr_storage; the length covers it.
    let mut address: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of_val(&address) as libc::socklen_t;
    // SAFETY: a live socket and an output buffer of `length` bytes.
    let status = unsafe {
        libc::getpeername(
            stream.as_raw_fd(),
            (&mut address as *mut libc::sockaddr_storage).cast(),
            &mut length,
        )
    };
    let error = (status != 0).then(io::Error::last_os_error);
    if PEER_CLOSED_IS_EINVAL {
        assert_eq!(error.and_then(|e| e.raw_os_error()), Some(libc::EINVAL));
    } else {
        assert!(error.is_none(), "{error:?}");
    }
}

#[test]
fn a_full_datagram_peer_answers_with_the_stated_errno() {
    let (sender, _receiver) = UnixDatagram::pair().unwrap();
    sender.set_nonblocking(true).unwrap();
    let payload = [b'x'; 866];
    let mut sent = 0;
    let full = loop {
        match sender.send(&payload) {
            Ok(_) => {
                sent += 1;
                assert!(sent < 100_000, "the datagram queue never filled");
            }
            Err(error) => break error,
        }
    };
    assert_eq!(full.raw_os_error(), Some(DATAGRAM_PEER_FULL), "{full:?}");
}

#[test]
fn a_reset_after_the_datagram_peer_closed_happens_only_where_the_fact_allows_it() {
    let (socket, peer) = UnixDatagram::pair().unwrap();
    socket.set_nonblocking(true).unwrap();
    peer.send(b"x").unwrap();
    drop(peer);
    let mut buffer = [0; 1];
    // The queued datagram is still delivered; then the receive finds nothing.
    assert_eq!(socket.recv(&mut buffer).unwrap(), 1);
    let error = socket.recv(&mut buffer).unwrap_err();
    let reset = error.raw_os_error() == Some(libc::ECONNRESET);
    assert!(
        reset && DATAGRAM_PEER_CLOSE_MAY_RESET || error.kind() == io::ErrorKind::WouldBlock,
        "{error:?}"
    );
}

/// The send flags keep a send to a closed stream from raising `SIGPIPE` exactly where
/// the fact says: a child process with `SIGPIPE` at its default action makes the send,
/// and survives it only there.
#[test]
fn the_send_flags_suppress_sigpipe_exactly_where_the_fact_says() {
    if std::env::var_os("ORG_PLATFORM_SIGPIPE_CHILD").is_some() {
        let (stream, peer) = UnixStream::pair().unwrap();
        drop(peer);
        // SAFETY: this child runs this test alone; SIGPIPE goes back to its default.
        assert_ne!(
            unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) },
            libc::SIG_ERR
        );
        let flags = send_flags().unwrap();
        // SAFETY: a live socket and a one-byte buffer.
        let sent = unsafe { libc::send(stream.as_raw_fd(), b"x".as_ptr().cast(), 1, flags) };
        assert_eq!(sent, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EPIPE));
        println!("survived the send");
        io::stdout().flush().unwrap();
        return;
    }
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "socket::adapter_tests::the_send_flags_suppress_sigpipe_exactly_where_the_fact_says",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("ORG_PLATFORM_SIGPIPE_CHILD", "1")
        .output()
        .unwrap();
    // The signal may reach another thread of the child after the send returned, so the
    // child's end, not its output, says whether one was raised.
    use std::os::unix::process::ExitStatusExt;
    if SEND_SUPPRESSES_SIGPIPE {
        assert!(output.status.success(), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stdout).contains("survived the send"));
    } else {
        assert_eq!(output.status.signal(), Some(libc::SIGPIPE), "{output:?}");
    }
}

/// Closing a Unix stream peer with unread bytes in its receive buffer causes `ECONNRESET`
/// on the remaining side exactly where `STREAM_PEER_CLOSE_WITH_UNREAD_RESETS` says, and
/// plain EOF otherwise.
#[test]
fn stream_peer_close_with_unread_bytes_matches_the_fact() {
    use std::io::Read;
    let (peer, mut local) = UnixStream::pair().unwrap();
    local.write_all(b"unread").unwrap();
    drop(peer);
    let mut byte = [0u8; 1];
    let result = local.read(&mut byte);
    if STREAM_PEER_CLOSE_WITH_UNREAD_RESETS {
        let error = result.expect_err("expected ECONNRESET on peer close with unread bytes");
        assert_eq!(error.raw_os_error(), Some(libc::ECONNRESET), "{error:?}");
    } else {
        assert_eq!(
            result.expect("expected EOF on peer close with unread bytes"),
            0
        );
    }
}

#[test]
fn peer_pid_reports_connected_process_and_refuses_bad_fd() {
    use std::os::unix::net::UnixListener;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("peer-pid.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let _client = UnixStream::connect(&path).unwrap();
    let (accepted, _) = listener.accept().unwrap();
    assert_eq!(
        super::peer_pid(accepted.as_raw_fd()).unwrap(),
        std::process::id() as libc::pid_t
    );
    assert!(super::peer_pid(-1).is_err());
}
