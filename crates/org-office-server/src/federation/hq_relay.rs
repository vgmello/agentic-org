//! HQ message relay: delivering satellite messages, forwarding relays, and message receipts.

use std::net::TcpStream;
use std::sync::Arc;

use crate::federation_stream::HqPeerRegistry;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use org_office::replication::StoredRelayItem;
use org_office::Office;
use org_sync::messages::Message;
use org_sync::organization::{PeerIdentity, PeerRole};
use org_sync_proto::{write_sync_frame, Digest, OfficeId, OrgId, SyncFrame};
use rustls::{ServerConnection, StreamOwned};

use super::relay::deliver_inbound_message_checked;
use super::{now_ms, InboundCallbacks, InboundMessage, StaticPeer};
use crate::federation_link::hq_peer::{write_sync_frame_if_peer_active, HqSessionPeer};

fn forward_hq_relay_to_satellite(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    peer: &HqSessionPeer<'_>,
    peers: &HqPeerRegistry,
    msg: InboundMessage,
) -> bool {
    let relay_frame = SyncFrame::MessageDeliver {
        org_id: msg.org_id.clone(),
        origin_office: msg.origin_office.clone(),
        destination_office: msg.destination_office.clone(),
        message_id: msg.message_id.clone(),
        fingerprint_hex: msg.fingerprint_hex.clone(),
        payload_b64: msg.payload_b64,
        deadline_ms: msg.deadline_ms,
    };
    let Ok(payload_json) = serde_json::to_string(&relay_frame) else {
        return false;
    };
    let admission = org_office::admit_relay_item_if_active(
        peer.office,
        StoredRelayItem {
            id: msg.message_id.clone(),
            origin_office: msg.origin_office.clone(),
            destination_office: msg.destination_office.clone(),
            fingerprint_hex: msg.fingerprint_hex.clone(),
            kind: "message".into(),
            payload_json,
            expires_at_ms: if msg.message_id.starts_with("close-") {
                0
            } else {
                msg.deadline_ms
            },
        },
    );
    match admission {
        Ok(org_office::RelayAdmission::OriginRemoved) => {
            let _ = write_sync_frame(
                tls,
                &SyncFrame::Error {
                    code: "revoked".into(),
                    message: "origin office has been permanently removed".into(),
                },
            );
            return false;
        }
        Ok(org_office::RelayAdmission::DestinationRemoved) => {
            return write_sync_frame_if_peer_active(
                tls,
                peer,
                &SyncFrame::Error {
                    code: "destination_removed".into(),
                    message: "destination office has been permanently removed".into(),
                },
            );
        }
        Ok(org_office::RelayAdmission::DestinationUnknown) => {
            return write_sync_frame_if_peer_active(
                tls,
                peer,
                &SyncFrame::MessageReceipt {
                    org_id: msg.org_id,
                    origin_office: msg.origin_office,
                    destination_office: msg.destination_office,
                    message_id: msg.message_id,
                    fingerprint_hex: msg.fingerprint_hex,
                    delivered: false,
                    relayed_by_hq: false,
                    deadline_ms: msg.deadline_ms,
                },
            );
        }
        Ok(org_office::RelayAdmission::QuotaExceeded) | Err(_) => {
            return write_sync_frame_if_peer_active(
                tls,
                peer,
                &SyncFrame::Error {
                    code: "destination_busy".into(),
                    message: "destination relay queue quota exceeded; retry later".into(),
                },
            );
        }
        Ok(org_office::RelayAdmission::Admitted) => {
            peers.notify_peer(&msg.destination_office);
        }
    }
    write_sync_frame_if_peer_active(
        tls,
        peer,
        &SyncFrame::MessageReceipt {
            org_id: msg.org_id,
            origin_office: msg.origin_office,
            destination_office: msg.destination_office,
            message_id: msg.message_id,
            fingerprint_hex: msg.fingerprint_hex,
            delivered: false,
            relayed_by_hq: true,
            deadline_ms: msg.deadline_ms,
        },
    )
}

pub(super) fn handle_hq_message_deliver(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    peer: &HqSessionPeer<'_>,
    callbacks: InboundCallbacks<'_>,
    peers: &HqPeerRegistry,
    msg: InboundMessage,
) -> bool {
    let office = peer.office;
    let org_id = peer.org_id;
    let sat_id = peer.sat_id;
    let cert_serial_hex = peer.cert_serial_hex;
    if msg.origin_office != sat_id.as_str() || msg.org_id != org_id.as_str() {
        return write_sync_frame_if_peer_active(
            tls,
            peer,
            &SyncFrame::Error {
                code: "not_authorized".into(),
                message: "message origin does not match authenticated satellite".into(),
            },
        );
    }
    if msg.destination_office != office.info().office_id {
        return forward_hq_relay_to_satellite(tls, peer, peers, msg);
    }
    let Ok(dest_id) = OfficeId::new(&msg.destination_office) else {
        return false;
    };
    let Ok(fp) = Digest::parse_hex(&msg.fingerprint_hex) else {
        return false;
    };
    let Ok(payload) = STANDARD.decode(&msg.payload_b64) else {
        return false;
    };
    let Ok(cp) = office.replication().org_checkpoint() else {
        return false;
    };
    let static_peer = StaticPeer(PeerIdentity {
        org_id: org_id.clone(),
        office_id: sat_id.clone(),
        role: PeerRole::Satellite,
        authority: cp.position.epoch,
    });
    let sync_msg = Message {
        org_id: org_id.clone(),
        origin: sat_id.clone(),
        destination: dest_id,
        id: msg.message_id.clone(),
        fingerprint: fp,
        payload: Arc::from(payload.as_slice()),
        deadline_ms: msg.deadline_ms,
    };
    let delivered =
        match org_office::run_if_peer_active(office, sat_id.as_str(), Some(cert_serial_hex), || {
            deliver_inbound_message_checked(
                office,
                &static_peer,
                callbacks,
                &msg,
                sync_msg,
                &payload,
                fp,
            )
        }) {
            Ok(Some(ok)) => ok,
            Ok(None) => {
                let _ = write_sync_frame(
                    tls,
                    &SyncFrame::Error {
                        code: "revoked".into(),
                        message: "origin office has been permanently removed".into(),
                    },
                );
                return false;
            }
            Err(_) => {
                let _ = write_sync_frame(
                    tls,
                    &SyncFrame::Error {
                        code: "unavailable".into(),
                        message: "revocation store unavailable".into(),
                    },
                );
                return false;
            }
        };
    write_sync_frame_if_peer_active(
        tls,
        peer,
        &SyncFrame::MessageReceipt {
            org_id: msg.org_id,
            origin_office: msg.origin_office,
            destination_office: msg.destination_office,
            message_id: msg.message_id,
            fingerprint_hex: msg.fingerprint_hex,
            delivered,
            relayed_by_hq: false,
            deadline_ms: msg.deadline_ms,
        },
    )
}

pub(super) struct InboundMessageReceipt {
    pub(super) org_id: String,
    pub(super) origin_office: String,
    pub(super) destination_office: String,
    pub(super) message_id: String,
    pub(super) fingerprint_hex: String,
    pub(super) deadline_ms: u64,
}

pub(super) fn handle_hq_message_receipt_frame(
    office: &Office,
    org_id: &OrgId,
    sat_id: &OfficeId,
    delivered: bool,
    relayed_by_hq: bool,
    peers: &HqPeerRegistry,
    receipt: InboundMessageReceipt,
) -> bool {
    if delivered {
        return handle_hq_message_receipt(office, org_id, sat_id, peers, receipt);
    }
    if !relayed_by_hq
        && !receipt.message_id.starts_with("close-")
        && receipt.org_id == org_id.as_str()
        && receipt.destination_office == sat_id.as_str()
        && receipt.deadline_ms <= now_ms()
    {
        let _ = office.replication().ack_message_relay_checked(
            &receipt.destination_office,
            &receipt.origin_office,
            &receipt.message_id,
            &receipt.fingerprint_hex,
        );
    }
    true
}

fn handle_hq_message_receipt(
    office: &Office,
    org_id: &OrgId,
    sat_id: &OfficeId,
    peers: &HqPeerRegistry,
    receipt: InboundMessageReceipt,
) -> bool {
    if receipt.org_id != org_id.as_str() || receipt.destination_office != sat_id.as_str() {
        return true;
    }
    let forward_dest =
        (receipt.origin_office != office.info().office_id).then(|| receipt.origin_office.clone());
    // Atomically remove the destination's message relay AND enqueue the forwarded delivery
    // receipt for the origin satellite in ONE durable transaction (`TASK-52` P7-#11).
    let forwarded = if receipt.origin_office != office.info().office_id {
        let forward = SyncFrame::MessageReceipt {
            org_id: receipt.org_id,
            origin_office: receipt.origin_office.clone(),
            destination_office: receipt.destination_office.clone(),
            message_id: receipt.message_id.clone(),
            fingerprint_hex: receipt.fingerprint_hex.clone(),
            delivered: true,
            relayed_by_hq: false,
            deadline_ms: receipt.deadline_ms,
        };
        let Ok(payload_json) = serde_json::to_string(&forward) else {
            return false;
        };
        Some(StoredRelayItem {
            id: format!("receipt:{}", receipt.message_id),
            origin_office: sat_id.as_str().to_owned(),
            destination_office: receipt.origin_office.clone(),
            fingerprint_hex: receipt.fingerprint_hex.clone(),
            kind: "receipt".into(),
            payload_json,
            expires_at_ms: receipt
                .deadline_ms
                .max(now_ms())
                .saturating_add(7 * 24 * 60 * 60 * 1000),
        })
    } else {
        None
    };
    let ok = org_office::ack_message_relay_and_enqueue_receipt_checked(
        office,
        &receipt.destination_office,
        &receipt.origin_office,
        &receipt.message_id,
        &receipt.fingerprint_hex,
        forwarded,
    )
    .is_ok();
    if ok {
        if let Some(dest) = forward_dest {
            peers.notify_peer(&dest);
        }
    }
    ok
}
