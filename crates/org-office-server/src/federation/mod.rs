//! Multi-office Org-CA mTLS federation listener (HQ) and full-duplex sync/relay client
//! (satellite) (`TASK-52` / `S1`, `doc-67`, `X-05`, `FC-020`, `FC-024`, `FC-145`).

mod client;
mod enrollment;
mod hq_dispatch;
mod hq_peer;
mod hq_relay;
mod hq_server;
mod hq_session;
mod hq_work;
mod org_events;
mod permits;
mod relay;
mod satellite_inbound;
mod sync;

use std::collections::VecDeque;
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::org_feed::FeedWake;
use org_office::Office;
use org_sync::organization::{PeerIdentity, VerifiedPeer};
use org_sync_proto::{FrameReader, SyncFrame};
use rustls::{ClientConnection, StreamOwned};
use sha2::{Digest as _, Sha256};

pub use self::enrollment::{enroll_satellite_with_hq, EnrolledCertificateBundle};
pub use self::hq_server::HqFederationServer;
pub use self::permits::SatelliteHqPermitCoordinator;
pub use self::relay::sync_native_message_relays;
pub use crate::federation_stream::HqFederationHandle;

pub(crate) use self::hq_peer::{write_sync_frame_if_peer_active, HqSessionPeer};
pub(crate) use self::org_events::collect_pending_work_paths_checked;
pub(crate) use self::satellite_inbound::drain_satellite_inbound_persistent;

const POLL_SLEEP: Duration = Duration::from_millis(25);
const ENROLL_WAIT_MAX: Duration = Duration::from_secs(120);
const SOCKET_READ_TIMEOUT: Duration = Duration::from_millis(250);
const SOCKET_WRITE_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const HANDSHAKE_DEADLINE: Duration = Duration::from_secs(10);
pub(crate) const FIRST_FRAME_DEADLINE: Duration = Duration::from_secs(15);
const PARTIAL_FRAME_DEADLINE: Duration = Duration::from_secs(10);
const MAX_CONCURRENT_CONNECTIONS: usize = 64;
const WAKE_RELAY_TTL_MS: u64 = 24 * 60 * 60 * 1000;

/// Relayed cross-office wake notification delivered to an office's local wake callback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelayedWake {
    pub origin_office: String,
    pub destination_office: String,
    pub employee_id: String,
    pub kind: String,
    pub source_id: String,
    pub sender_id: String,
    pub sender_name: String,
    pub sender_is_person: bool,
    pub conversation_id: Option<String>,
    pub summary: String,
    pub ask_answer: Option<org_office::RelayedAskAnswer>,
    pub ask_withdrawal: Option<org_office::RelayedAskWithdrawal>,
}

/// Optional callback invoked when HQ or a satellite receives a relayed wake notification.
/// Returns `Ok(())` only after the wake has been durably persisted on the destination (`TASK-52` #14).
pub type WakeRelayHandler = Arc<dyn Fn(RelayedWake) -> Result<(), String> + Send + Sync>;

/// Optional callback invoked after an inbound cross-office native message/answer/withdrawal/accept
/// is durably committed on this office, notifying local ask waiters, mailbox waiters, live SSE
/// streams, and the channel watcher (`TASK-117`).
pub type InboundDeliveryNotifier = Arc<dyn Fn(&org_office::RelayedMessageOutcome) + Send + Sync>;

#[derive(Clone, Copy, Default)]
struct InboundCallbacks<'a> {
    wake: Option<&'a WakeRelayHandler>,
    notify: Option<&'a InboundDeliveryNotifier>,
    feed_wake: Option<&'a FeedWake>,
}

struct StaticPeer(PeerIdentity);

impl VerifiedPeer for StaticPeer {
    fn identity(&self) -> PeerIdentity {
        self.0.clone()
    }
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

pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
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

/// Active mTLS federation client session from a satellite office to HQ.
pub struct SatelliteSyncSession {
    pub(crate) office: Arc<Office>,
    pub(crate) tls: StreamOwned<ClientConnection, TcpStream>,
    reader: FrameReader,
    pub(crate) hq_peer: PeerIdentity,
    pending_inbound: VecDeque<SyncFrame>,
}

type SnapshotFileBatch = Vec<(String, Option<Vec<u8>>)>;

/// Deterministic valid canonical lowercase RFC 9562 UUIDv7 derived from an idempotent replication key
/// so crash-recovery replays deduplicate cleanly in `org-store` (`TASK-52` H-7/H-10).
pub(crate) fn deterministic_uuid_v7(key: &str) -> String {
    let hash = Sha256::digest(key.as_bytes());
    let mut b = [0u8; 16];
    b.copy_from_slice(&hash[..16]);
    b[6] = (b[6] & 0x0f) | 0x70;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}
