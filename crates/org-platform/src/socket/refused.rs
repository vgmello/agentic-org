//! The refusing socket adapter: no peer credential. Nothing here falls back to
//! another system's checks.

use std::io;
use std::os::fd::RawFd;

use crate::Refused;

pub(super) fn peer_uid(_fd: RawFd) -> io::Result<u32> {
    Err(Refused::unsupported("socket").into())
}

#[cfg(test)]
mod tests {
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;

    use super::*;

    fn assert_refused<T>(result: io::Result<T>) {
        let error = result.err().expect("the refusing adapter never succeeds");
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        let reason = error
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<Refused>())
            .expect("the refusal is the error's source");
        assert_eq!(reason, &Refused::unsupported("socket"));
    }

    #[test]
    fn a_real_connected_peer_and_a_bad_descriptor_are_both_refused() {
        let (stream, _peer) = UnixStream::pair().unwrap();
        assert_refused(peer_uid(stream.as_raw_fd()));
        assert_refused(peer_uid(-1));
    }
}
