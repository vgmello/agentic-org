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
