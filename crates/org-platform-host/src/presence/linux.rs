//! GNU/Linux has no presence adapter yet. The design's adapter (the peer's controlling
//! terminal and foreground process group, and `Confirmation::BrowserApproval`; Linux
//! host design section 4) is written later. Until then every function is the refusing
//! adapter's, so Linux answers as a target without an adapter does.

pub(super) use super::refused::{confirm_presence, current_context, local_peer};

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixStream;

    use crate::presence::{self, Unconfirmed};

    #[test]
    fn linux_refuses_until_its_adapter_is_written() {
        let refusal = presence::refusal();
        assert_eq!(
            refusal.to_string(),
            "recovery presence is not available on linux in this release; nothing was changed"
        );
        let (stream, _peer) = UnixStream::pair().unwrap();
        assert_eq!(presence::local(&stream), Err(refusal.clone()));
        assert_eq!(presence::current(), Err(refusal.clone()));
        assert_eq!(
            presence::confirm(|| false),
            Err(Unconfirmed::Refused(refusal))
        );
    }
}
