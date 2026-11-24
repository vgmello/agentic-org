//! Production [`OrgFeed`] backed by `org_office::Office` (`org-store` committed receipts)
//! with immediate wakeups for `subscribe`, `inbox.changed`, and `/api/v1/events/stream` (`B6`).

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use org_office::Office;
use org_proto::org::{OrgChangedData, Position};

use crate::follow::OrgFeed;

/// Shared monotonic wake token notified on every committed organization change or live event.
#[derive(Default)]
pub(crate) struct FeedWake {
    seq: Mutex<u64>,
    cv: Condvar,
}

impl FeedWake {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn notify(&self) {
        let mut seq = self.seq.lock().unwrap_or_else(|p| p.into_inner());
        *seq = seq.saturating_add(1);
        self.cv.notify_all();
    }

    pub(crate) fn token(&self) -> u64 {
        *self.seq.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub(crate) fn wait(&self, seen: u64, timeout: Duration) -> u64 {
        let guard = self.seq.lock().unwrap_or_else(|p| p.into_inner());
        if *guard != seen {
            return *guard;
        }
        let (guard, _) = self
            .cv
            .wait_timeout_while(guard, timeout, |cur| *cur == seen)
            .unwrap_or_else(|p| p.into_inner());
        *guard
    }
}

/// [`OrgFeed`] backed by [`Office`] durable receipts (`org_changes_after`) and [`FeedWake`].
pub(crate) struct OfficeOrgFeed {
    office: Arc<Office>,
    wake: Arc<FeedWake>,
}

impl OfficeOrgFeed {
    pub(crate) fn new(office: Arc<Office>, wake: Arc<FeedWake>) -> Self {
        Self { office, wake }
    }
}

impl OrgFeed for OfficeOrgFeed {
    fn head(&self) -> Result<Position, String> {
        self.office
            .org_snapshot()
            .map(|s| s.position)
            .map_err(|e| crate::logged("feed.head", &e))
    }

    fn can_resume_from(&self, after: Position) -> Result<bool, String> {
        let head = self.head()?;
        Ok(after.epoch == head.epoch && after.seq <= head.seq)
    }

    fn changes_after(&self, after: Position, limit: usize) -> Result<Vec<OrgChangedData>, String> {
        self.office
            .org_changes_after(after.seq, limit)
            .map_err(|e| crate::logged("feed.changes_after", &e))
    }

    fn token(&self) -> u64 {
        self.wake.token()
    }

    fn wait(&self, seen: u64, timeout: Duration) -> u64 {
        self.wake.wait(seen, timeout)
    }
}
