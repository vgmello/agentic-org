use std::os::unix::net::UnixStream;
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
fn a_send_is_nonblocking_and_has_no_sigpipe_flag() {
    assert_eq!(send_flags().unwrap(), libc::MSG_DONTWAIT);
}
