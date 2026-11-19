//! The refusing socket adapter: no peer credential, no descriptor passing and no
//! socket-shape query. Nothing here falls back to another system's flags or checks.

use std::convert::Infallible;
use std::io;
use std::marker::PhantomData;
use std::os::fd::{OwnedFd, RawFd};
use std::sync::{Mutex, MutexGuard};

use crate::Refused;

/// No peer-query error is accepted: the caller refuses the descriptor.
pub(super) const PEER_CLOSED_IS_EINVAL: bool = false;

/// No receive begins.
pub(super) const RECEIVED_CLOSE_ON_EXEC: super::ReceivedCloseOnExec =
    super::ReceivedCloseOnExec::Unavailable;

/// Nothing is sent: no claim about `SIGPIPE`.
pub(super) const SEND_SUPPRESSES_SIGPIPE: bool = false;

/// The POSIX answer for a send that cannot proceed without blocking.
pub(super) const DATAGRAM_PEER_FULL: i32 = libc::EAGAIN;

/// No claim beyond POSIX: a receive finds nothing to read.
pub(super) const DATAGRAM_PEER_CLOSE_MAY_RESET: bool = false;

pub(super) fn peer_uid(_fd: RawFd) -> io::Result<u32> {
    Err(Refused::unsupported("socket").into())
}

/// Never constructed: [`descriptor_receive`] always refuses, so no receive begins.
pub(super) struct DescriptorReceive<'fence> {
    never: Infallible,
    _fence: PhantomData<MutexGuard<'fence, ()>>,
}

pub(super) fn descriptor_receive(_fence: &Mutex<()>) -> io::Result<DescriptorReceive<'_>> {
    Err(Refused::unsupported("socket").into())
}

impl DescriptorReceive<'_> {
    pub(super) fn flags(&self) -> libc::c_int {
        match self.never {}
    }

    pub(super) fn close_on_exec(
        &self,
        _descriptors: &[OwnedFd],
        _set: impl FnMut(RawFd) -> io::Result<()>,
    ) -> io::Result<()> {
        match self.never {}
    }
}

pub(super) fn send_flags() -> io::Result<libc::c_int> {
    Err(Refused::unsupported("socket").into())
}

pub(super) fn listening(_fd: RawFd) -> io::Result<bool> {
    Err(Refused::unsupported("socket").into())
}

/// What `socket::testing` needs of this system (`test-support`): no claim beyond POSIX.
#[cfg(any(test, feature = "test-support"))]
pub(super) mod testing {
    pub(in crate::socket) const SEQPACKET: bool = false;
    pub(in crate::socket) const CONNECTED_STREAM_CAN_LISTEN: bool = false;

    pub(in crate::socket) fn set_address_length(_address: &mut libc::sockaddr_un) {}

    /// No Unix seqpacket sockets here.
    pub(in crate::socket) fn seqpacket_pair(
    ) -> std::io::Result<Option<(std::os::fd::OwnedFd, std::os::fd::OwnedFd)>> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::os::fd::AsRawFd;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::sync::{Mutex, TryLockError};

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

    #[test]
    fn no_descriptor_receive_begins_and_the_fence_is_left_alone() {
        let fence = Mutex::new(());
        assert_refused(descriptor_receive(&fence));
        assert!(fence.try_lock().is_ok(), "the fence was locked");
        let _ = std::panic::catch_unwind(|| {
            let _guard = fence.lock().unwrap();
            panic!("inject fence poison");
        });
        // A poisoned fence gets the same refusal, not a different error.
        assert_refused(descriptor_receive(&fence));
        assert!(matches!(fence.try_lock(), Err(TryLockError::Poisoned(_))));
    }

    #[test]
    fn send_flags_are_refused() {
        assert_refused(send_flags());
    }

    #[test]
    fn listener_state_is_refused_for_listeners_streams_and_bad_descriptors() {
        let directory = tempfile::tempdir().unwrap();
        let listener = UnixListener::bind(directory.path().join("listener")).unwrap();
        let (stream, _peer) = UnixStream::pair().unwrap();
        assert_refused(listening(listener.as_raw_fd()));
        assert_refused(listening(stream.as_raw_fd()));
        assert_refused(listening(-1));
    }

    #[test]
    fn no_peer_query_error_is_accepted_as_a_closed_peer() {
        for errno in [libc::EINVAL, libc::ENOTCONN, libc::EBADF] {
            let error = io::Error::from_raw_os_error(errno);
            assert!(!super::super::peer_closed_when(
                &error,
                PEER_CLOSED_IS_EINVAL
            ));
        }
    }
}
