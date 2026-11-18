use std::cell::Cell;
use std::net::{TcpListener, UdpSocket};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::TryLockError;

use super::*;

#[test]
fn a_real_pair_reports_its_kernel_uid_and_a_bad_descriptor_fails() {
    let (stream, _peer) = UnixStream::pair().unwrap();
    // SAFETY: geteuid has no preconditions; this is an independent oracle for the query.
    let uid = unsafe { libc::geteuid() };
    assert_eq!(peer_uid(stream.as_raw_fd()).unwrap(), uid);
    assert_eq!(peer_uid(-1).unwrap_err().raw_os_error(), Some(libc::EBADF));
}

fn poisoned() -> Mutex<()> {
    let fence = Mutex::new(());
    let _ = std::panic::catch_unwind(|| {
        let _guard = fence.lock().unwrap();
        panic!("inject fence poison");
    });
    fence
}

#[test]
fn a_receive_sets_cloexec_atomically_and_never_takes_the_fence() {
    let fence = Mutex::new(());
    let receiving = descriptor_receive(&fence).unwrap();
    assert_eq!(
        receiving.flags(),
        libc::MSG_DONTWAIT | libc::MSG_CMSG_CLOEXEC
    );
    assert!(fence.try_lock().is_ok(), "Linux must not acquire the fence");
    let (first, second) = UnixStream::pair().unwrap();
    let descriptors = [OwnedFd::from(first), OwnedFd::from(second)];
    receiving
        .close_on_exec(&descriptors, |_| {
            panic!("MSG_CMSG_CLOEXEC already set the flag")
        })
        .unwrap();
    // A poisoned fence is not inspected either.
    let fence = poisoned();
    let receiving = descriptor_receive(&fence).unwrap();
    assert_eq!(
        receiving.flags(),
        libc::MSG_DONTWAIT | libc::MSG_CMSG_CLOEXEC
    );
    assert!(matches!(fence.try_lock(), Err(TryLockError::Poisoned(_))));
}

#[test]
fn control_length_is_the_message_field() {
    let fence = Mutex::new(());
    let receiving = descriptor_receive(&fence).unwrap();
    // SAFETY: zero is valid for msghdr; no pointer in it is followed.
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    assert_eq!(receiving.control_len(&message), 0);
    message.msg_controllen = 24;
    assert_eq!(receiving.control_len(&message), 24);
}

#[test]
fn a_send_is_nonblocking_and_raises_no_sigpipe() {
    assert_eq!(
        send_flags().unwrap(),
        libc::MSG_DONTWAIT | libc::MSG_NOSIGNAL
    );
}

#[test]
fn listener_state_is_read_for_unix_and_inet_sockets_and_non_sockets_fail() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("listener");
    let listener = UnixListener::bind(&path).unwrap();
    assert!(listening(listener.as_raw_fd()).unwrap());
    // A queued connection does not change the listener's state.
    let queued = UnixStream::connect(&path).unwrap();
    assert!(listening(listener.as_raw_fd()).unwrap());
    assert!(!listening(queued.as_raw_fd()).unwrap());
    let (stream, peer) = UnixStream::pair().unwrap();
    assert!(!listening(stream.as_raw_fd()).unwrap());
    drop(peer);
    assert!(!listening(stream.as_raw_fd()).unwrap());
    let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
    assert!(listening(tcp.as_raw_fd()).unwrap());
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    assert!(!listening(udp.as_raw_fd()).unwrap());

    let mut fds = [-1; 2];
    // SAFETY: pipe writes exactly two new descriptors; each gets one owner below.
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    // SAFETY: successful pipe created these two fresh fds; wrap each once.
    let (read, _write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    assert_eq!(
        listening(read.as_raw_fd()).unwrap_err().raw_os_error(),
        Some(libc::ENOTSOCK)
    );
    assert_eq!(
        listening(-1).unwrap_err().raw_os_error(),
        Some(libc::EBADF)
    );
}

// Test-only syscall interposition: the option query fails with a genuine EBADF while
// the socket stays open, so the adapter's own error path is observed on a socket whose
// real state would have answered.
thread_local! { static FAULT: Cell<bool> = const { Cell::new(false) }; }

pub(super) unsafe fn getsockopt(
    fd: libc::c_int,
    level: libc::c_int,
    option: libc::c_int,
    value: *mut libc::c_void,
    length: *mut libc::socklen_t,
) -> libc::c_int {
    if option == libc::SO_ACCEPTCONN && FAULT.with(Cell::get) {
        // SAFETY: no descriptor is closed; this sets a genuine thread-local EBADF.
        assert_eq!(unsafe { libc::close(-1) }, -1);
        return -1;
    }
    // SAFETY: listening supplies valid buffers for a live fd and integer option.
    unsafe { libc::getsockopt(fd, level, option, value, length) }
}

#[test]
fn a_failed_listener_query_is_an_error_for_listeners_and_streams_alike() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let (stream, _peer) = UnixStream::pair().unwrap();
    for fd in [listener.as_raw_fd(), stream.as_raw_fd()] {
        FAULT.with(|fault| fault.set(true));
        let result = listening(fd);
        FAULT.with(|fault| fault.set(false));
        assert_eq!(result.unwrap_err().raw_os_error(), Some(libc::EBADF));
    }
    assert!(listening(listener.as_raw_fd()).unwrap());
    assert!(!listening(stream.as_raw_fd()).unwrap());
}
