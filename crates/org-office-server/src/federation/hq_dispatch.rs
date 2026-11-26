//! HQ frame dispatch: data and control frames from a satellite, hello, and wake notifications.

use std::net::TcpStream;

use crate::federation_stream::HqPeerRegistry;
use crate::snapshot_transfer::{self, ActiveSnapshotTransfer, HqSnapshotTransferBudget};
use org_office::replication::StoredRelayItem;
use org_office::Office;
use org_sync_proto::{OfficeId, OrgId, SyncFrame, WireOfficeRole};
use rustls::{ServerConnection, StreamOwned};

use super::hq_peer::{send_hq_org_events_after, write_sync_frame_if_peer_active, HqSessionPeer};
use super::hq_relay::{
    handle_hq_message_deliver, handle_hq_message_receipt_frame, InboundMessageReceipt,
};
use super::hq_work::{handle_hq_work_change, WorkChangeInput};
use super::permits::{handle_hq_permit_sync, PermitSyncInput};
use super::{
    now_ms, InboundCallbacks, InboundMessage, RelayedWake, WakeRelayHandler, WAKE_RELAY_TTL_MS,
};

pub(super) fn dispatch_hq_frame(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    peer: &HqSessionPeer<'_>,
    snapshot_budget: &HqSnapshotTransferBudget,
    active_snapshot: &mut Option<ActiveSnapshotTransfer>,
    frame: SyncFrame,
    callbacks: InboundCallbacks<'_>,
    peers: &HqPeerRegistry,
) -> bool {
    if !matches!(frame, SyncFrame::OrgSnapshotRequest { .. }) {
        *active_snapshot = None;
    }
    match frame {
        SyncFrame::Hello { .. } => send_hq_hello(tls, peer),
        SyncFrame::OrgSnapshotRequest {
            snapshot_id,
            page_index,
            supports_paging,
            ..
        } => snapshot_transfer::send_hq_snapshot(
            tls,
            peer,
            snapshot_budget,
            active_snapshot,
            snapshot_id.as_deref(),
            page_index,
            supports_paging,
        ),
        SyncFrame::OrgGap {
            after_epoch,
            after_seq,
        } => send_hq_org_events_after(tls, peer, after_epoch, after_seq).is_some(),
        SyncFrame::WorkChange {
            org_id: work_org,
            origin_office,
            incarnation,
            seq,
            record_id,
            generation,
            version,
            digest_hex,
            record_digest_hex,
            codec_name,
            codec_version,
            payload_b64,
            required_org_epoch,
            required_org_seq,
            created_ms,
        } => handle_hq_work_change(
            tls,
            peer,
            WorkChangeInput {
                org_id: work_org,
                origin_office,
                incarnation,
                seq,
                record_id,
                generation,
                version,
                digest_hex,
                record_digest_hex,
                codec_name,
                codec_version,
                payload_b64,
                required_org_epoch,
                required_org_seq,
                created_ms,
            },
            peers,
            callbacks.feed_wake,
        ),
        SyncFrame::MessageDeliver {
            org_id: msg_org,
            origin_office,
            destination_office,
            message_id,
            fingerprint_hex,
            payload_b64,
            deadline_ms,
        } => handle_hq_message_deliver(
            tls,
            peer,
            callbacks,
            peers,
            InboundMessage {
                org_id: msg_org,
                origin_office,
                destination_office,
                message_id,
                fingerprint_hex,
                payload_b64,
                deadline_ms,
            },
        ),
        SyncFrame::MessageReceipt {
            org_id: r_org,
            origin_office,
            destination_office,
            message_id,
            fingerprint_hex,
            delivered,
            relayed_by_hq,
            deadline_ms,
        } => matches!(
            org_office::run_if_peer_active(
                peer.office,
                peer.sat_id.as_str(),
                Some(peer.cert_serial_hex),
                || {
                    handle_hq_message_receipt_frame(
                        peer.office,
                        peer.org_id,
                        peer.sat_id,
                        delivered,
                        relayed_by_hq,
                        peers,
                        InboundMessageReceipt {
                            org_id: r_org,
                            origin_office,
                            destination_office,
                            message_id,
                            fingerprint_hex,
                            deadline_ms,
                        },
                    )
                },
            ),
            Ok(Some(true))
        ),
        wake @ SyncFrame::WakeNotify { .. } => {
            handle_hq_wake_notify(
                peer.office,
                peer.org_id,
                peer.sat_id,
                peer.cert_serial_hex,
                callbacks.wake,
                peers,
                wake,
            );
            true
        }
        other => dispatch_hq_control_frame(tls, peer, other),
    }
}

fn dispatch_hq_control_frame(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    peer: &HqSessionPeer<'_>,
    frame: SyncFrame,
) -> bool {
    match frame {
        SyncFrame::RelayAck { relay_id } => matches!(
            org_office::run_if_peer_active(
                peer.office,
                peer.sat_id.as_str(),
                Some(peer.cert_serial_hex),
                || {
                    let _ = peer
                        .office
                        .replication()
                        .ack_relay_item_for_destination(peer.sat_id.as_str(), &relay_id);
                },
            ),
            Ok(Some(()))
        ),
        SyncFrame::PermitSyncRequest {
            request_id,
            connection_id,
            employee_id,
            task_id,
            model_id,
            is_failover_switch,
        } => handle_hq_permit_sync(
            tls,
            peer,
            PermitSyncInput {
                request_id,
                connection_id,
                employee_id,
                task_id,
                model_id,
                is_failover_switch,
            },
        ),
        SyncFrame::PermitRenewRequest {
            request_id,
            permit_id,
            coordinator_generation,
        } => {
            let Ok(Some(renewed)) = org_office::run_if_peer_active(
                peer.office,
                peer.sat_id.as_str(),
                Some(peer.cert_serial_hex),
                || {
                    peer.office
                        .renew_turn_permit_for_office(
                            peer.sat_id.as_str(),
                            &permit_id,
                            coordinator_generation,
                        )
                        .is_ok_and(|r| r.is_ok())
                },
            ) else {
                return false;
            };
            write_sync_frame_if_peer_active(
                tls,
                peer,
                &SyncFrame::PermitRenewResponse {
                    request_id,
                    renewed,
                },
            )
        }
        SyncFrame::PermitReleaseNotify {
            permit_id,
            release_pending,
        } => {
            if release_pending {
                let Ok(Some(())) = org_office::run_if_peer_active(
                    peer.office,
                    peer.sat_id.as_str(),
                    Some(peer.cert_serial_hex),
                    || {
                        let _ = peer
                            .office
                            .release_turn_permit_for_office(peer.sat_id.as_str(), &permit_id);
                    },
                ) else {
                    return false;
                };
            }
            true
        }
        SyncFrame::CooldownNotify {
            connection_id,
            resets_at,
            ..
        } => matches!(
            org_office::run_if_peer_active(
                peer.office,
                peer.sat_id.as_str(),
                Some(peer.cert_serial_hex),
                || {
                    let _ = peer.office.record_satellite_cooldown(
                        peer.sat_id.as_str(),
                        &connection_id,
                        resets_at.as_deref(),
                    );
                },
            ),
            Ok(Some(()))
        ),
        SyncFrame::Ping { nonce } => {
            write_sync_frame_if_peer_active(tls, peer, &SyncFrame::Pong { nonce })
        }
        _ => true,
    }
}

fn send_hq_hello(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    peer: &HqSessionPeer<'_>,
) -> bool {
    let Ok(cp) = peer.office.replication().org_checkpoint() else {
        return false;
    };
    write_sync_frame_if_peer_active(
        tls,
        peer,
        &SyncFrame::Hello {
            org_id: peer.office.info().org_id.clone(),
            office_id: peer.office.info().office_id.clone(),
            role: WireOfficeRole::Hq,
            epoch: cp.position.epoch.get(),
            protocol_level: cp.protocol_level,
            supported_max: 1,
            codec_max: 1,
            hq_addresses: Vec::new(),
        },
    )
}

fn handle_hq_wake_notify(
    office: &Office,
    org_id: &OrgId,
    sat_id: &OfficeId,
    cert_serial_hex: &str,
    wake_handler: Option<&WakeRelayHandler>,
    peers: &HqPeerRegistry,
    frame: SyncFrame,
) {
    let SyncFrame::WakeNotify {
        org_id: wake_org,
        origin_office,
        destination_office,
        employee_id,
        kind,
        source_id,
        sender_id,
        sender_name,
        sender_is_person,
        conversation_id,
        summary,
    } = frame
    else {
        return;
    };
    // Bind wake provenance strictly to authenticated org & satellite AND accepted activity (`TASK-52` F5).
    if wake_org != org_id.as_str() || origin_office != sat_id.as_str() {
        return;
    }
    let accepted = office
        .replication()
        .has_accepted_activity_from(sat_id.as_str(), &source_id)
        .unwrap_or(false);
    let recipient_exists = office
        .org_snapshot()
        .is_ok_and(|s| s.members.iter().any(|m| m.id == employee_id));
    if !accepted || !recipient_exists {
        return;
    }
    if destination_office == office.info().office_id {
        if let Some(cb) = wake_handler {
            let _ = org_office::run_if_peer_active(
                office,
                sat_id.as_str(),
                Some(cert_serial_hex),
                || {
                    let _ = cb(RelayedWake {
                        origin_office,
                        destination_office,
                        employee_id,
                        kind,
                        source_id,
                        sender_id,
                        sender_name,
                        sender_is_person,
                        conversation_id,
                        summary,
                        ask_answer: None,
                        ask_withdrawal: None,
                    });
                },
            );
        }
    } else {
        let relay_frame = SyncFrame::WakeNotify {
            org_id: wake_org,
            origin_office: origin_office.clone(),
            destination_office: destination_office.clone(),
            employee_id,
            kind,
            source_id: source_id.clone(),
            sender_id,
            sender_name,
            sender_is_person,
            conversation_id,
            summary,
        };
        if let Ok(payload_json) = serde_json::to_string(&relay_frame) {
            let dest_for_wake = destination_office.clone();
            if matches!(
                org_office::admit_relay_item_if_active(
                    office,
                    StoredRelayItem {
                        id: format!("wake:{source_id}:{destination_office}"),
                        origin_office,
                        destination_office,
                        fingerprint_hex: String::new(),
                        kind: "wake".into(),
                        payload_json,
                        expires_at_ms: now_ms().saturating_add(WAKE_RELAY_TTL_MS),
                    },
                ),
                Ok(org_office::RelayAdmission::Admitted)
            ) {
                peers.notify_peer(&dest_for_wake);
            }
        }
    }
}
