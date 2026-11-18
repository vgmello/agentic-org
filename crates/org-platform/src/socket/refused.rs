//! The refusing socket adapter: no peer credential and no descriptor passing. Nothing
//! here falls back to another system's flags or checks.

use std::convert::Infallible;
use std::io;
use std::marker::PhantomData;
use std::os::fd::{OwnedFd, RawFd};
use std::sync::Mutex;

use crate::Refused;

pub(super) fn peer_uid(_fd: RawFd) -> io::Result<u32> {
    Err(Refused::unsupported("socket").into())
}

/// Never constructed: [`descriptor_receive`] always refuses, so no receive begins.
pub(super) struct DescriptorReceive<'fence> {
    never: Infallible,
    _fence: PhantomData<&'fence Mutex<()>>,
}

pub(super) fn descriptor_receive(_fence: &Mutex<()>) -> io::Result<DescriptorReceive<'_>> {
    Err(Refused::unsupported("socket").into())
}

impl DescriptorReceive<'_> {
    pub(super) fn flags(&self) -> libc::c_int {
        match self.never {}
    }

    pub(super) fn control_len(&self, _message: &libc::msghdr) -> usize {
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

#[cfg(test)]
mod tests {
    use std::io;
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;
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

}
