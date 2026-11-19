//! The refusing presence adapter: no peer is local, this process is not local, and
//! nothing is confirmed. No query is made and nobody is prompted.

use std::os::unix::net::UnixStream;

use org_platform::Refused;

use super::{refusal, Confirmation, Unconfirmed};

pub(super) fn local_peer(_peer: &UnixStream) -> Result<bool, Refused> {
    Err(refusal())
}

pub(super) fn current_context() -> Result<bool, Refused> {
    Err(refusal())
}

pub(super) fn confirm_presence(_cancelled: impl Fn() -> bool) -> Result<Confirmation, Unconfirmed> {
    Err(Unconfirmed::Refused(refusal()))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn no_peer_and_no_process_is_in_a_local_context() {
        let (stream, _peer) = UnixStream::pair().unwrap();
        assert_eq!(local_peer(&stream), Err(refusal()));
        assert_eq!(current_context(), Err(refusal()));
        let Refused::Unsupported { capability, os } = refusal();
        assert_eq!(capability, "recovery presence");
        assert_eq!(os, std::env::consts::OS);
    }

    #[test]
    fn nothing_is_confirmed_and_the_caller_is_not_polled() {
        let polls = Cell::new(0);
        for answer in [false, true] {
            let outcome = confirm_presence(|| {
                polls.set(polls.get() + 1);
                answer
            });
            assert_eq!(outcome, Err(Unconfirmed::Refused(refusal())));
        }
        assert_eq!(polls.get(), 0, "the refusing adapter waited for an answer");
    }
}
