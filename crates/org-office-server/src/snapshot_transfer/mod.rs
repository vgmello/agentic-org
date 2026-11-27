//! Bounded, disk-backed, paged organization snapshot transfer over Org-CA mTLS (`TASK-52` P20-#4 / P21-#1 / P21-#2).
//!
//! - Both HQ and satellites enforce identical per-page and per-chunk limits
//!   (`MAX_SNAPSHOT_PAGE_CHUNKS * SNAPSHOT_CHUNK_RAW_BYTES == SNAPSHOT_PAGE_MAX_BYTES`).
//! - Paging is negotiated explicitly via `OrgSnapshotRequest::supports_paging`: legacy peers never
//!   receive page 0 of a multi-page snapshot.
//! - HQ bounds concurrent transfers, process-wide staged disk bytes, and idle lifetimes, streaming
//!   pages to/from disk so idle sessions retain negligible memory.
//! - Satellites validate and stage every page to disk (within a strict byte cap) BEFORE mutating
//!   `state/`, then apply staged pages in dependency order with bounded memory (`<= 8 MiB` per
//!   `org-store` commit).

mod apply;
mod budget;
mod export;
mod moves;
mod staging;
mod tiers;

use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(test)]
pub(crate) use self::apply::sync_initial_snapshot_in_batches;
pub(crate) use self::apply::sync_initial_snapshot_paged;
pub(crate) use self::budget::{ActiveSnapshotTransfer, HqSnapshotTransferBudget};
pub(crate) use self::export::{cleanup_stale_hq_snapshot_exports, send_hq_snapshot};

pub(crate) const SNAPSHOT_CHUNK_RAW_BYTES: usize = 256 * 1024;
pub(crate) const SNAPSHOT_PAGE_MAX_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_SNAPSHOT_PAGE_CHUNKS: u32 =
    (SNAPSHOT_PAGE_MAX_BYTES / SNAPSHOT_CHUNK_RAW_BYTES) as u32;
pub(crate) const MAX_SNAPSHOT_PAGES: u32 = 1024;
pub(crate) const MAX_CONCURRENT_SNAPSHOT_TRANSFERS: usize = 4;
pub(crate) const MAX_HQ_STAGED_SNAPSHOT_BYTES: u64 = 512 * 1024 * 1024;
pub(crate) const MAX_SATELLITE_STAGED_SNAPSHOT_BYTES: u64 = 512 * 1024 * 1024;
pub(crate) const SNAPSHOT_TRANSFER_IDLE_TTL: Duration = Duration::from_secs(30);
pub(crate) const SNAPSHOT_PREPARE_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(3);
pub(crate) const SNAPSHOT_PREPARE_DEADLINE: Duration = Duration::from_secs(180);
const SNAPSHOT_PREPARE_LEGACY_POLL: Duration = Duration::from_millis(250);
const MAX_NATIVE_BATCH_BYTES: usize = 8 * 1024 * 1024;

const TIER_BIT_ORG_STRUCT: u16 = 1 << 0;
const TIER_BIT_ORG_EMP: u16 = 1 << 1;

type SnapshotFileBatch = Vec<(String, Option<Vec<u8>>)>;

fn create_private_dir(path: &Path) -> std::io::Result<()> {
    org_platform::fs::create_owner_only_dir_all(path)
}

fn write_private_bytes(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    org_platform::fs::write_owner_only_file(path, bytes)
}

fn page_file_path(dir: &Path, page_index: u32) -> PathBuf {
    dir.join(format!("page-{page_index:06}.json"))
}

#[cfg(test)]
mod tests;
