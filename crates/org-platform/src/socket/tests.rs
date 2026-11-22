use std::io;

use super::*;

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
                peer_closed_when(&error, macos),
                macos && errno == libc::EINVAL,
                "macos={macos}, errno={errno}"
            );
        }
        // A generic InvalidInput classification cannot stand in for the exact kernel errno.
        assert!(!peer_closed_when(
            &io::Error::from(io::ErrorKind::InvalidInput),
            macos
        ));
    }
}

#[test]
fn the_close_on_exec_fact_is_what_a_receive_does() {
    let fence = std::sync::Mutex::new(());
    match RECEIVED_CLOSE_ON_EXEC {
        ReceivedCloseOnExec::WithTheReceive(flag) => {
            let receive = descriptor_receive(&fence).unwrap();
            assert_ne!(flag, 0);
            assert_eq!(receive.flags() & flag, flag);
            assert!(fence.try_lock().is_ok(), "the fence is not taken");
            let mut called = false;
            receive
                .close_on_exec(&[], |_| {
                    called = true;
                    Ok(())
                })
                .unwrap();
            assert!(!called);
        }
        ReceivedCloseOnExec::UnderTheFence => {
            let _receive = descriptor_receive(&fence).unwrap();
            assert!(fence.try_lock().is_err(), "the fence is held");
        }
        ReceivedCloseOnExec::Unavailable => assert!(descriptor_receive(&fence).is_err()),
    }
}
