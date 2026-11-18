//! A writer custody that keeps its head in memory and applies exactly the host's rule.

use std::sync::Mutex;

use crate::custody::{CustodyError, JournalHead, OfficeIdentity, WriterAuthority};

#[derive(Default)]
struct State {
    stored: Option<(OfficeIdentity, JournalHead)>,
    closed: bool,
    confirmations: usize,
}

#[derive(Default)]
pub struct FakeCustody(Mutex<State>);

impl FakeCustody {
    pub fn new() -> Self {
        Self::default()
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The host channel is gone: every call fails.
    pub fn close(&self) {
        self.state().closed = true;
    }

    pub fn reopen(&self) {
        self.state().closed = false;
    }

    /// Sets the confirmed head directly, as if an earlier process had confirmed it.
    pub fn hold(&self, identity: &OfficeIdentity, head: JournalHead) {
        self.state().stored = Some((identity.clone(), head));
    }

    pub fn head(&self) -> Option<JournalHead> {
        self.state().stored.as_ref().map(|(_, head)| head.clone())
    }

    /// How many confirmations were accepted.
    pub fn confirmations(&self) -> usize {
        self.state().confirmations
    }
}

impl WriterAuthority for FakeCustody {
    fn checkpoint(&self, identity: &OfficeIdentity) -> Result<Option<JournalHead>, CustodyError> {
        let state = self.state();
        if state.closed {
            return Err(CustodyError("the host channel is closed".into()));
        }
        match &state.stored {
            Some((stored, _)) if stored != identity => {
                Err(CustodyError("another writer holds this office".into()))
            }
            Some((_, head)) => Ok(Some(head.clone())),
            None => Ok(None),
        }
    }

    fn confirm(
        &self,
        identity: &OfficeIdentity,
        before: Option<&JournalHead>,
        after: &JournalHead,
    ) -> Result<(), CustodyError> {
        let mut state = self.state();
        if state.closed {
            return Err(CustodyError("the host channel is closed".into()));
        }
        let refuse = |why: &str| Err(CustodyError(why.into()));
        match &state.stored {
            Some((stored, _)) if stored != identity => return refuse("another incarnation"),
            Some((_, head)) => {
                if before != Some(head) {
                    return refuse("`before` is not the stored head");
                }
                let identical = after.ordinal == head.ordinal && after.sha256 == head.sha256;
                if after.ordinal <= head.ordinal && !identical {
                    return refuse("the ordinal does not advance");
                }
                if !after.high_waters.none_behind(&head.high_waters) {
                    return refuse("a stream high-water went back");
                }
            }
            None if before.is_some() => return refuse("`before` is not the stored head"),
            None => {}
        }
        state.stored = Some((identity.clone(), after.clone()));
        state.confirmations += 1;
        Ok(())
    }
}
