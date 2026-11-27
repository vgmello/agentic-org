//! The HQ's snapshot transfer budget: concurrent transfers and staged bytes, with the permits and
//! active-transfer leases that hold them.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::{
    MAX_CONCURRENT_SNAPSHOT_TRANSFERS, MAX_HQ_STAGED_SNAPSHOT_BYTES, SNAPSHOT_TRANSFER_IDLE_TTL,
};

#[derive(Default)]
struct SnapshotBudgetState {
    active_transfers: usize,
    staged_bytes: u64,
}

/// Process-wide HQ concurrency and staged-disk budget for active snapshot exports (`TASK-52` P21-#2).
#[derive(Clone, Default)]
pub(crate) struct HqSnapshotTransferBudget {
    inner: Arc<Mutex<SnapshotBudgetState>>,
    max_concurrent: usize,
    max_staged_bytes: u64,
}

impl HqSnapshotTransferBudget {
    pub(crate) fn production() -> Self {
        Self::with_limits(
            MAX_CONCURRENT_SNAPSHOT_TRANSFERS,
            MAX_HQ_STAGED_SNAPSHOT_BYTES,
        )
    }

    pub(crate) fn with_limits(max_concurrent: usize, max_staged_bytes: u64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(SnapshotBudgetState::default())),
            max_concurrent,
            max_staged_bytes,
        }
    }

    pub(crate) fn try_begin(&self) -> Option<SnapshotTransferPermit> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if guard.active_transfers >= self.max_concurrent {
            return None;
        }
        guard.active_transfers += 1;
        Some(SnapshotTransferPermit {
            budget: self.clone(),
            reserved_bytes: 0,
        })
    }
}

pub(crate) struct SnapshotTransferPermit {
    budget: HqSnapshotTransferBudget,
    reserved_bytes: u64,
}

impl SnapshotTransferPermit {
    pub(crate) fn try_reserve_bytes(&mut self, bytes: u64) -> bool {
        let mut guard = self.budget.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(next) = guard.staged_bytes.checked_add(bytes) else {
            return false;
        };
        if next > self.budget.max_staged_bytes {
            return false;
        }
        guard.staged_bytes = next;
        self.reserved_bytes = self.reserved_bytes.saturating_add(bytes);
        true
    }
}

impl Drop for SnapshotTransferPermit {
    fn drop(&mut self) {
        let mut guard = self.budget.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.active_transfers = guard.active_transfers.saturating_sub(1);
        guard.staged_bytes = guard.staged_bytes.saturating_sub(self.reserved_bytes);
    }
}

/// Disk-backed active HQ snapshot transfer with RAII cleanup and idle expiry.
pub(crate) struct ActiveSnapshotTransfer {
    pub(super) snapshot_id: String,
    pub(super) checkpoint: org_sync::organization::OrgCheckpoint,
    pub(super) staging_dir: PathBuf,
    pub(super) total_pages: u32,
    pub(super) last_activity: Instant,
    pub(super) _permit: SnapshotTransferPermit,
}

impl ActiveSnapshotTransfer {
    pub(crate) fn is_expired(&self) -> bool {
        self.last_activity.elapsed() >= SNAPSHOT_TRANSFER_IDLE_TTL
    }

    pub(crate) fn expire_if_idle(slot: &mut Option<Self>) {
        if slot.as_ref().is_some_and(Self::is_expired) {
            *slot = None;
        }
    }
}

impl Drop for ActiveSnapshotTransfer {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.staging_dir);
    }
}
