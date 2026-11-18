use std::os::fd::AsRawFd;
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
