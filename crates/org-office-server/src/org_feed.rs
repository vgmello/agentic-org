//! Production [`OrgFeed`] backed by `org_office::Office` (`org-store` committed receipts)
//! with immediate wakeups for `subscribe`, `inbox.changed`, and `/api/v1/events/stream` (`B6`).

use std::sync::Arc;
use std::time::Duration;

use org_office::Office;
use org_proto::org::{OrgChangedData, Position};

pub(crate) use org_federation::FeedWake;

use crate::follow::OrgFeed;

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
