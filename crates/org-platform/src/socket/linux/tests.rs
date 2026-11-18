use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;

use super::*;

#[test]
fn a_real_pair_reports_its_kernel_uid_and_a_bad_descriptor_fails() {
    let (stream, _peer) = UnixStream::pair().unwrap();
    // SAFETY: geteuid has no preconditions; this is an independent oracle for the query.
    let uid = unsafe { libc::geteuid() };
    assert_eq!(peer_uid(stream.as_raw_fd()).unwrap(), uid);
    assert_eq!(peer_uid(-1).unwrap_err().raw_os_error(), Some(libc::EBADF));
}
