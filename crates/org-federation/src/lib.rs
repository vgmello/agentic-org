//! Multi-office Org-CA mTLS federation listener (HQ), persistent full-duplex sync/relay stream,
//! shared attachment blob shipping/fetching, and bounded paged snapshot transfer.
//!
//! Module graph: `federation -> {stream, snapshot_transfer} -> link`, with leaf `feed_wake`.

#![forbid(unsafe_code)]

mod federation;
pub mod feed_wake;
mod link;
mod snapshot_transfer;
pub mod stream;

#[cfg(any(test, feature = "testkit"))]
pub mod testkit;

pub use self::federation::{
    blob_cache_max_bytes, drain_satellite_inbound_persistent, enroll_satellite_with_hq,
    ship_blob_outbox, spawn_satellite_blob_shipper, sync_native_message_relays,
    EnrolledCertificateBundle, HqBlobRemote, HqFederationHandle, HqFederationServer,
    HqPermitRequest, HqPermitResponse, InboundDeliveryNotifier, OutboxFlush, RelayedWake,
    SatelliteHqPermitCoordinator, SatelliteSyncSession, WakeRelayHandler, BLOB_CACHE_MAX_BYTES_ENV,
};
pub use self::feed_wake::FeedWake;
