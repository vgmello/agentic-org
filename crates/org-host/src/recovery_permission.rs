//! A shown recovery code's acknowledgement permission (REC-DEL-1): what `recovery.release`
//! leaves on the one connection that showed the code, and what `recovery.acknowledge` on that
//! connection checks before it records the acknowledgement.
//!
//! A leaf below both of its users: the control listener's connection holds it and clears it,
//! and `recovery` grants and checks it, so neither has to know the other.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Weak;

use crate::hostdb::recovery::Ledger;

/// Owned by exactly one connection. Not Clone/Debug/Serialize, and never a
/// caller-visible capability. A dropped/revoked session invalidates it in place.
pub(crate) struct Permission {
    session: [u8; 32],
    live: Weak<AtomicBool>,
    account: String,
    ledger: Ledger,
}

impl Permission {
    /// Granted to `account`, signed in through the session bound as `session` and `live`, for
    /// the delivery `ledger` records.
    pub(crate) fn new(
        session: [u8; 32],
        live: Weak<AtomicBool>,
        account: String,
        ledger: Ledger,
    ) -> Self {
        Self {
            session,
            live,
            account,
            ledger,
        }
    }

    /// Whether this permission admits `account`'s acknowledgement of `ledger` through the
    /// session now bound as `session` and `live`: the account, session and live binding it was
    /// granted to, a session that is still live, and the delivery it was granted for.
    pub(crate) fn admits(
        &self,
        account: &str,
        session: &[u8; 32],
        live: &Weak<AtomicBool>,
        ledger: &Ledger,
    ) -> bool {
        self.account == account
            && self.session == *session
            && self.live.ptr_eq(live)
            && live.upgrade().is_some_and(|l| l.load(Ordering::SeqCst))
            && self.ledger == *ledger
    }
}

#[cfg(test)]
mod tests;
