//! Narrow federation test hooks exposed under `feature = "testkit"` for downstream integration
//! suites (such as `org-office-server`). Never compiled into production builds.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use org_sync_proto::SyncFrame;

pub use crate::federation::{ship_blob_outbox, OutboxFlush};
pub use crate::stream::HqPeerRegistry;
use crate::SatelliteSyncSession;

pub fn write_frame(session: &mut SatelliteSyncSession, frame: &SyncFrame) -> Result<(), String> {
    session.write_frame(frame)
}

pub fn read_correlated_frame(
    session: &mut SatelliteSyncSession,
    timeout: Duration,
    expected_receipt_msg_id: Option<&str>,
) -> Result<Option<SyncFrame>, String> {
    session.read_correlated_frame(timeout, expected_receipt_msg_id)
}

pub fn read_blob_reply(
    session: &mut SatelliteSyncSession,
    request_id: &str,
    stop: Option<&AtomicBool>,
) -> Result<SyncFrame, String> {
    session.read_blob_reply(request_id, stop)
}

pub fn flush_blob_outbox(
    session: &mut SatelliteSyncSession,
    cache_max_bytes: u64,
    stop: Option<&AtomicBool>,
) -> Result<OutboxFlush, String> {
    session.flush_blob_outbox(cache_max_bytes, stop)
}

pub mod federation_link {
    pub use crate::link::StaticPeer;

    pub mod org_events {
        pub use crate::link::org_events::{
            apply_satellite_org_event_frames, replicated_files_op_id,
        };
    }
}

pub mod snapshot_transfer {
    pub use crate::snapshot_transfer::sync_initial_snapshot_in_batches;
}
