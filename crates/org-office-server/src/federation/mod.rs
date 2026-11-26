//! Multi-office Org-CA mTLS federation listener (HQ) and full-duplex sync/relay client
//! (satellite) (`TASK-52` / `S1`, `doc-67`, `X-05`, `FC-020`, `FC-024`, `FC-145`).
//!
//! Module graph: `federation -> {federation_stream, snapshot_transfer} -> federation_link`.
//! The link primitives this module shares with the persistent stream and snapshot transfer
//! (the satellite session, the HQ peer writer, org-event application, relay callbacks, IDs
//! and deadlines) live in the leaf `federation_link` and are re-exported here; the stream
//! reaches this module's inbound drain only through the hook `federation_runtime` injects.

mod client;
mod enrollment;
mod hq_dispatch;
mod hq_relay;
mod hq_server;
mod hq_session;
mod hq_work;
mod permits;
mod relay;
mod satellite_inbound;
mod sync;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::org_feed::FeedWake;

pub use self::enrollment::{enroll_satellite_with_hq, EnrolledCertificateBundle};
pub use self::hq_server::HqFederationServer;
pub use self::permits::SatelliteHqPermitCoordinator;
pub use self::relay::sync_native_message_relays;
pub use crate::federation_stream::HqFederationHandle;

pub(crate) use self::satellite_inbound::drain_satellite_inbound_persistent;
pub(crate) use crate::federation_link::{
    deterministic_uuid_v7, SnapshotFileBatch, StaticPeer, FIRST_FRAME_DEADLINE,
    PARTIAL_FRAME_DEADLINE,
};
pub use crate::federation_link::{
    InboundDeliveryNotifier, RelayedWake, SatelliteSyncSession, WakeRelayHandler,
};

const POLL_SLEEP: Duration = Duration::from_millis(25);
const ENROLL_WAIT_MAX: Duration = Duration::from_secs(120);
const SOCKET_READ_TIMEOUT: Duration = Duration::from_millis(250);
const SOCKET_WRITE_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const HANDSHAKE_DEADLINE: Duration = Duration::from_secs(10);
const MAX_CONCURRENT_CONNECTIONS: usize = 64;
const WAKE_RELAY_TTL_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Clone, Copy, Default)]
struct InboundCallbacks<'a> {
    wake: Option<&'a WakeRelayHandler>,
    notify: Option<&'a InboundDeliveryNotifier>,
    feed_wake: Option<&'a FeedWake>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn now_unix() -> i64 {
    (now_ms() / 1000) as i64
}

struct InboundMessage {
    org_id: String,
    origin_office: String,
    destination_office: String,
    message_id: String,
    fingerprint_hex: String,
    payload_b64: String,
    deadline_ms: u64,
}

/// Request sent by a satellite over mTLS to acquire a shared `HqPool` turn permit (`TASK-52` P2-14).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HqPermitRequest {
    pub request_id: String,
    pub connection_id: String,
    pub employee_id: String,
    pub task_id: Option<String>,
    pub model_id: String,
    pub is_failover_switch: bool,
}

/// Reply returned by HQ for a satellite `HqPool` permit sync request (`TASK-52` P2-14).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HqPermitResponse {
    pub granted: bool,
    pub permit_id: Option<String>,
    pub coordinator_generation: Option<u64>,
    pub reason: Option<String>,
    pub cooldown_until: Option<String>,
    pub occupied: Option<u64>,
    pub max_capacity: Option<u64>,
}
