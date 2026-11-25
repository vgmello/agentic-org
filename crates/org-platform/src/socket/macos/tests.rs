use std::cell::Cell;
use std::net::TcpListener;
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

#[test]
fn the_fence_is_held_from_the_start_of_a_receive_through_every_flag_update() {
    let fence = Mutex::new(());
    let receiving = descriptor_receive(&fence).unwrap();
    assert!(matches!(fence.try_lock(), Err(TryLockError::WouldBlock)));
    assert_eq!(receiving.flags(), libc::MSG_DONTWAIT);
    let (first, second) = UnixStream::pair().unwrap();
    let descriptors = [OwnedFd::from(first), OwnedFd::from(second)];
    let mut seen = Vec::new();
    receiving
        .close_on_exec(&descriptors, |fd| {
            assert!(matches!(fence.try_lock(), Err(TryLockError::WouldBlock)));
            seen.push(fd);
            Ok(())
        })
        .unwrap();
    assert_eq!(
        seen,
        [descriptors[0].as_raw_fd(), descriptors[1].as_raw_fd()]
    );
    assert!(matches!(fence.try_lock(), Err(TryLockError::WouldBlock)));
    drop(receiving);
    assert!(fence.try_lock().is_ok());
}

#[test]
fn the_first_failed_flag_update_stops_the_rest_and_is_returned() {
    let fence = Mutex::new(());
    let receiving = descriptor_receive(&fence).unwrap();
    let (first, second) = UnixStream::pair().unwrap();
    let descriptors = [OwnedFd::from(first), OwnedFd::from(second)];
    let mut calls = 0;
    let error = receiving
        .close_on_exec(&descriptors, |_| {
            calls += 1;
            Err(io::Error::from_raw_os_error(libc::EIO))
        })
        .unwrap_err();
    assert_eq!(calls, 1);
    assert_eq!(error.raw_os_error(), Some(libc::EIO));
}

#[test]
fn a_poisoned_fence_refuses_the_receive() {
    let fence = Mutex::new(());
    let _ = std::panic::catch_unwind(|| {
        let _guard = fence.lock().unwrap();
        panic!("inject fence poison");
    });
    let error = descriptor_receive(&fence).err().unwrap();
    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert_eq!(error.to_string(), "descriptor fence is poisoned");
}

#[test]
fn a_send_is_nonblocking_and_has_no_sigpipe_flag() {
    assert_eq!(send_flags().unwrap(), libc::MSG_DONTWAIT);
}

#[test]
fn listener_state_is_read_for_unix_and_inet_sockets() {
    let directory = tempfile::tempdir().unwrap();
    let listener = UnixListener::bind(directory.path().join("listener")).unwrap();
    assert!(listening(listener.as_raw_fd()).unwrap());
    let (stream, _peer) = UnixStream::pair().unwrap();
    assert!(!listening(stream.as_raw_fd()).unwrap());
    let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
    assert!(listening(tcp.as_raw_fd()).unwrap());
}

// Test-only syscall interposition leaves the live fd open, so the count guard is
// observed on sockets whose genuine metadata would have answered.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fault {
    Failed,
    Negative,
    Short,
    Long,
}
thread_local! { static FAULT: Cell<Option<Fault>> = const { Cell::new(None) }; }
fn injected(fault: Fault) -> bool {
    let fail = FAULT.with(|value| value.get() == Some(fault));
    if fail {
        // SAFETY: no descriptor is closed; this sets a genuine thread-local EBADF.
        assert_eq!(unsafe { libc::close(-1) }, -1);
    }
    fail
}

pub(super) unsafe fn proc_pidfdinfo(
    pid: libc::c_int,
    fd: libc::c_int,
    flavor: libc::c_int,
    buffer: *mut libc::c_void,
    size: libc::c_int,
) -> libc::c_int {
    for (fault, result) in [
        (Fault::Failed, 0),
        (Fault::Negative, -1),
        (Fault::Short, size - 1),
        (Fault::Long, size + 1),
    ] {
        if injected(fault) {
            // Keep genuine metadata in the buffer so a deleted count guard is
            // not masked by the listener's later state check.
            // SAFETY: these are the unchanged live output buffers from the helper.
            let actual = unsafe { libc::proc_pidfdinfo(pid, fd, flavor, buffer, size) };
            assert_eq!(actual, size);
            return result;
        }
    }
    // SAFETY: the helper supplies an owned fd and its exact SDK-aligned writable buffer.
    unsafe { libc::proc_pidfdinfo(pid, fd, flavor, buffer, size) }
}

// Moved from org-socket's `fault-proc-*` and `tcp-fault-proc-*` adoption cases: a
// connected stream (genuinely not listening) and a TCP listener (genuinely listening)
// are both refused when the reply's size is wrong, whichever way a missing guard
// would have answered.
#[test]
fn an_incomplete_socket_information_reply_is_refused_whatever_the_buffer_holds() {
    let (stream, _peer) = UnixStream::pair().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    for fd in [stream.as_raw_fd(), listener.as_raw_fd()] {
        for fault in [Fault::Failed, Fault::Negative, Fault::Short, Fault::Long] {
            FAULT.with(|slot| slot.set(Some(fault)));
            let result = listening(fd);
            FAULT.with(|slot| slot.set(None));
            let error = result.expect_err("an incomplete reply was accepted");
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        }
    }
    assert!(!listening(stream.as_raw_fd()).unwrap());
    assert!(listening(listener.as_raw_fd()).unwrap());
}
