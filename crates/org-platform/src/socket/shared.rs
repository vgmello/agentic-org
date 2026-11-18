//! Code the macOS and GNU/Linux adapters share. The refusing adapter calls none of it,
//! so it is not compiled for a refusing target.

use std::io;

// Isolate syscall status/size decisions so a truncated credential can be tested hermetically.
pub(super) fn checked_uid(
    rc: libc::c_int,
    uid: u32,
    length: libc::socklen_t,
    expected: libc::socklen_t,
) -> io::Result<u32> {
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    if length != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected peer credential size",
        ));
    }
    Ok(uid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_status_size_and_uid_zero_are_checked_independently() {
        for uid in [0, 501, u32::MAX] {
            assert_eq!(checked_uid(0, uid, 12, 12).unwrap(), uid);
            assert!(checked_uid(-1, uid, 12, 12).is_err());
            for length in [0, 11, 13] {
                assert_eq!(
                    checked_uid(0, uid, length, 12).unwrap_err().kind(),
                    io::ErrorKind::InvalidData
                );
            }
        }
    }
}
