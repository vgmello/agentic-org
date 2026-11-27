//! The mTLS federation link primitives shared by the federation listener and client
//! (`federation`), the satellite's persistent stream (`federation_stream`) and snapshot
//! transfer (`snapshot_transfer`): the satellite session, the HQ peer writer ([`hq_peer`]),
//! org-event application ([`org_events`]), the relay callbacks, deterministic IDs and frame
//! deadlines.
//!
//! A leaf below all three: it imports none of them, so `federation -> {federation_stream,
//! snapshot_transfer} -> federation_link` is a DAG.

pub(crate) mod hq_peer;
pub mod org_events;

use std::collections::VecDeque;
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use org_office::Office;
use org_sync::organization::{PeerIdentity, VerifiedPeer};
use org_sync_proto::{FrameReader, SyncFrame};
use rustls::{ClientConnection, StreamOwned};
use sha2::{Digest as _, Sha256};

pub(crate) use self::hq_peer::{write_sync_frame_if_peer_active, HqSessionPeer};
pub(crate) use self::org_events::collect_pending_work_paths_checked;

/// How long the first frame of an exchange may take.
pub(crate) const FIRST_FRAME_DEADLINE: Duration = Duration::from_secs(15);
/// How long the rest of a frame may take once it has started.
pub(crate) const PARTIAL_FRAME_DEADLINE: Duration = Duration::from_secs(10);

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
    pub attachment_ids: Vec<String>,
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

/// The verified peer of an established session.
pub struct StaticPeer(pub PeerIdentity);

impl VerifiedPeer for StaticPeer {
    fn identity(&self) -> PeerIdentity {
        self.0.clone()
    }
}

pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Active mTLS federation client session from a satellite office to HQ.
pub struct SatelliteSyncSession {
    pub(crate) office: Arc<Office>,
    pub(crate) tls: StreamOwned<ClientConnection, TcpStream>,
    pub(crate) reader: FrameReader,
    pub(crate) hq_peer: PeerIdentity,
    pub(crate) pending_inbound: VecDeque<SyncFrame>,
}

pub type SnapshotFileBatch = Vec<(String, Option<Vec<u8>>)>;

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
