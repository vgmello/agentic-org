//! Satellite inbound frames: relayed messages, receipts, wakes, and the persistent drain.

use std::io;
use std::net::TcpStream;
use std::sync::Arc;

use crate::federation_stream::{spawn_satellite_session_rpc, SatRpcContext};
use crate::org_feed::FeedWake;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use org_office::Office;
use org_sync::messages::{acknowledge, Message, Receipt};
use org_sync_proto::{write_sync_frame, Digest, FrameCodecError, OfficeId, SyncFrame};
use rustls::{ClientConnection, StreamOwned};

use super::relay::deliver_inbound_message_checked;
use super::{
    InboundCallbacks, InboundDeliveryNotifier, InboundMessage, RelayedWake, SatelliteSyncSession,
    StaticPeer, WakeRelayHandler, PARTIAL_FRAME_DEADLINE,
};
use crate::federation_link::org_events::{apply_satellite_org_event_frames, ORG_EVENT_BATCH_MAX};

fn dispatch_inbound_relay_frame(
    session: &mut SatelliteSyncSession,
    peer: &StaticPeer,
    callbacks: InboundCallbacks<'_>,
    frame: SyncFrame,
) -> Result<usize, String> {
    match frame {
        SyncFrame::MessageDeliver {
            org_id,
            origin_office,
            destination_office,
            message_id,
            fingerprint_hex,
            payload_b64,
            deadline_ms,
        } => {
            let applied = handle_inbound_relayed_message(
                &mut session.tls,
                &session.office,
                peer,
                callbacks,
                InboundMessage {
                    org_id,
                    origin_office,
                    destination_office,
                    message_id,
                    fingerprint_hex,
                    payload_b64,
                    deadline_ms,
                },
            )?;
            Ok(usize::from(applied))
        }
        SyncFrame::MessageReceipt {
            org_id,
            origin_office,
            destination_office,
            message_id,
            fingerprint_hex,
            delivered: true,
            ..
        } => {
            if !apply_inbound_relayed_receipt(
                &session.office,
                peer,
                &org_id,
                &origin_office,
                &destination_office,
                &message_id,
                &fingerprint_hex,
            ) {
                return Ok(0);
            }
            let _ = write_sync_frame(
                &mut session.tls,
                &SyncFrame::RelayAck {
                    relay_id: format!("receipt:{message_id}"),
                },
            );
            Ok(1)
        }
        SyncFrame::WakeNotify {
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
            ..
        } => {
            let relay_id = format!("wake:{source_id}:{destination_office}");
            // Only acknowledge the wake relay after the destination durably persists it (`TASK-52` #14).
            let persisted = callbacks.wake.is_some_and(|cb| {
                cb(RelayedWake {
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
                })
                .is_ok()
            });
            if !persisted {
                return Ok(0);
            }
            let _ = write_sync_frame(&mut session.tls, &SyncFrame::RelayAck { relay_id });
            Ok(1)
        }
        _ => Ok(0),
    }
}

pub(super) fn receive_satellite_inbound_relays(
    session: &mut SatelliteSyncSession,
    callbacks: InboundCallbacks<'_>,
) -> Result<usize, String> {
    let mut received = 0usize;
    let peer = StaticPeer(session.hq_peer.clone());
    loop {
        let next_frame = if let Some(buffered) = session.pending_inbound.pop_front() {
            Some(buffered)
        } else {
            match session
                .reader
                .read_frame(&mut session.tls, PARTIAL_FRAME_DEADLINE)
            {
                Ok(frame) => frame,
                Err(FrameCodecError::Io(e))
                    if e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::TimedOut =>
                {
                    break;
                }
                Err(e) => return Err(e.to_string()),
            }
        };
        let Some(frame) = next_frame else { break };
        received += dispatch_inbound_relay_frame(session, &peer, callbacks, frame)?;
    }
    Ok(received)
}

fn read_satellite_frame_nonblocking(
    session: &mut SatelliteSyncSession,
) -> Result<Option<SyncFrame>, FrameCodecError> {
    let _ = session.tls.sock.set_nonblocking(true);
    let res = session
        .reader
        .read_frame(&mut session.tls, PARTIAL_FRAME_DEADLINE);
    let _ = session.tls.sock.set_nonblocking(false);
    res
}

/// Non-blocking drain of all buffered and socket-ready inbound frames on a persistent
/// bidirectional mTLS session (`TASK-140`, `TASK-143` AC #3).
pub(crate) fn drain_satellite_inbound_persistent(
    session: &mut SatelliteSyncSession,
    wake_handler: Option<&WakeRelayHandler>,
    delivery_notifier: Option<&InboundDeliveryNotifier>,
    feed_wake: Option<&FeedWake>,
    rpc_ctx: &SatRpcContext<'_>,
) -> Result<usize, String> {
    let callbacks = InboundCallbacks {
        wake: wake_handler,
        notify: delivery_notifier,
        feed_wake,
    };
    let peer = StaticPeer(session.hq_peer.clone());
    let mut handled = 0usize;
    loop {
        let next_frame = if let Some(buffered) = session.pending_inbound.pop_front() {
            Some(buffered)
        } else {
            match read_satellite_frame_nonblocking(session) {
                Ok(Some(frame)) => Some(frame),
                Ok(None) => return Err("HQ closed persistent mTLS session".into()),
                Err(FrameCodecError::Io(e))
                    if e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::TimedOut =>
                {
                    break;
                }
                Err(e) => return Err(e.to_string()),
            }
        };
        let Some(frame) = next_frame else {
            break;
        };
        handled += 1;
        handle_persistent_inbound_frame(session, frame, &peer, callbacks, feed_wake, rpc_ctx)?;
    }
    Ok(handled)
}

/// Takes the `OrgEvent` frames that follow `first` back to back, buffered or already readable
/// on the socket, up to [`ORG_EVENT_BATCH_MAX`], so a burst HQ pushes (a catch-up of hundreds
/// of tasks) applies in verified batches instead of one durable commit per event
/// (`TASK-140`). The first other frame goes back to the front of the inbound buffer, so frames
/// are still handled in arrival order. A read failure is returned beside the burst so the
/// events received before it are applied first.
fn collect_org_event_burst(
    session: &mut SatelliteSyncSession,
    first: SyncFrame,
) -> (Vec<SyncFrame>, Option<String>) {
    let mut burst = vec![first];
    while burst.len() < ORG_EVENT_BATCH_MAX {
        let next = match session.pending_inbound.pop_front() {
            Some(buffered) => buffered,
            None => match read_satellite_frame_nonblocking(session) {
                Ok(Some(frame)) => frame,
                Ok(None) => return (burst, Some("HQ closed persistent mTLS session".into())),
                Err(FrameCodecError::Io(e))
                    if e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::TimedOut =>
                {
                    break;
                }
                Err(e) => return (burst, Some(e.to_string())),
            },
        };
        if matches!(next, SyncFrame::OrgEvent { .. }) {
            burst.push(next);
        } else {
            session.pending_inbound.push_front(next);
            break;
        }
    }
    (burst, None)
}

fn handle_persistent_wake_frame(
    tls: &mut StreamOwned<ClientConnection, TcpStream>,
    wake_handler: Option<&WakeRelayHandler>,
    wake: RelayedWake,
) {
    let relay_id = format!("wake:{}:{}", wake.source_id, wake.destination_office);
    let persisted = wake_handler.is_some_and(|cb| cb(wake).is_ok());
    if persisted {
        let _ = write_sync_frame(tls, &SyncFrame::RelayAck { relay_id });
    }
}

fn handle_persistent_inbound_frame(
    session: &mut SatelliteSyncSession,
    frame: SyncFrame,
    peer: &StaticPeer,
    callbacks: InboundCallbacks<'_>,
    feed_wake: Option<&FeedWake>,
    rpc_ctx: &SatRpcContext<'_>,
) -> Result<(), String> {
    match frame {
        ev @ SyncFrame::OrgEvent { .. } => {
            let (burst, read_error) = collect_org_event_burst(session, ev);
            let applied =
                apply_satellite_org_event_frames(&session.office, &session.hq_peer, burst)?;
            if applied > 0 {
                if let Some(wake) = feed_wake {
                    wake.notify();
                }
            }
            if let Some(error) = read_error {
                return Err(error);
            }
        }
        SyncFrame::MessageDeliver {
            org_id,
            origin_office,
            destination_office,
            message_id,
            fingerprint_hex,
            payload_b64,
            deadline_ms,
        } => {
            let _ = handle_inbound_relayed_message(
                &mut session.tls,
                &session.office,
                peer,
                callbacks,
                InboundMessage {
                    org_id,
                    origin_office,
                    destination_office,
                    message_id,
                    fingerprint_hex,
                    payload_b64,
                    deadline_ms,
                },
            )?;
        }
        SyncFrame::MessageReceipt {
            org_id,
            origin_office,
            destination_office,
            message_id,
            fingerprint_hex,
            delivered: true,
            ..
        } => {
            if apply_inbound_relayed_receipt(
                &session.office,
                peer,
                &org_id,
                &origin_office,
                &destination_office,
                &message_id,
                &fingerprint_hex,
            ) {
                let _ = write_sync_frame(
                    &mut session.tls,
                    &SyncFrame::RelayAck {
                        relay_id: format!("receipt:{message_id}"),
                    },
                );
            }
        }
        SyncFrame::WakeNotify {
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
            ..
        } => handle_persistent_wake_frame(
            &mut session.tls,
            callbacks.wake,
            RelayedWake {
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
            },
        ),
        SyncFrame::SessionRpcRequest {
            request_id,
            caller_member_id,
            caller_is_owner,
            method,
            params_json,
        } => spawn_satellite_session_rpc(
            request_id,
            caller_member_id,
            caller_is_owner,
            method,
            params_json,
            rpc_ctx,
        ),
        SyncFrame::Ping { nonce } => {
            let _ = write_sync_frame(&mut session.tls, &SyncFrame::Pong { nonce });
        }
        SyncFrame::Error { code, .. } if code == "revoked" => {
            return Err("satellite certificate revoked by HQ".into());
        }
        _ => {}
    }
    Ok(())
}

fn handle_inbound_relayed_message(
    tls: &mut StreamOwned<ClientConnection, TcpStream>,
    office: &Office,
    peer: &StaticPeer,
    callbacks: InboundCallbacks<'_>,
    msg: InboundMessage,
) -> Result<bool, String> {
    if msg.org_id != office.info().org_id || msg.destination_office != office.info().office_id {
        return Err("inbound relayed message org or destination mismatch".into());
    }
    let origin = OfficeId::new(&msg.origin_office).map_err(|_| "origin".to_owned())?;
    let dest = OfficeId::new(&msg.destination_office).map_err(|_| "dest".to_owned())?;
    let fp = Digest::parse_hex(&msg.fingerprint_hex).map_err(|_| "fp".to_owned())?;
    let payload = STANDARD
        .decode(&msg.payload_b64)
        .map_err(|_| "payload b64".to_owned())?;
    let sync_msg = Message {
        org_id: peer.0.org_id.clone(),
        origin,
        destination: dest,
        id: msg.message_id.clone(),
        fingerprint: fp,
        payload: Arc::from(payload.as_slice()),
        deadline_ms: msg.deadline_ms,
    };
    let delivered =
        deliver_inbound_message_checked(office, peer, callbacks, &msg, sync_msg, &payload, fp);
    write_sync_frame(
        tls,
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
    .map_err(|e| format!("failed sending inbound MessageReceipt: {e}"))?;
    Ok(delivered)
}

fn apply_inbound_relayed_receipt(
    office: &Office,
    peer: &StaticPeer,
    org_id: &str,
    origin_office: &str,
    destination_office: &str,
    message_id: &str,
    fingerprint_hex: &str,
) -> bool {
    let Ok(queue) = office.replication().message_queue_snapshot() else {
        return false;
    };
    if let Some(sent) = queue.pending.iter().find(|m| {
        m.org_id.as_str() == org_id
            && m.origin.as_str() == origin_office
            && m.destination.as_str() == destination_office
            && m.id == message_id
            && m.fingerprint.to_hex() == fingerprint_hex
    }) {
        return acknowledge(
            office.replication().as_ref(),
            peer,
            Receipt {
                message: sent.clone(),
                delivered: true,
            },
        )
        .is_ok();
    }
    // If a prior RelayAck was dropped after durable receipt commit, or a late successful receipt
    // arrived after local expiry, verify matching envelope and upgrade expired receipt to delivered
    // so HQ clears its relay queue (`TASK-52` P3-#12 / P6-#10).
    let Some(existing) = office
        .replication()
        .outgoing_receipt_snapshot(message_id)
        .ok()
        .flatten()
    else {
        return false;
    };
    let matches_envelope = existing.message.org_id.as_str() == org_id
        && existing.message.origin.as_str() == origin_office
        && existing.message.destination.as_str() == destination_office
        && existing.message.id == message_id
        && existing.message.fingerprint.to_hex() == fingerprint_hex;
    if !matches_envelope {
        return false;
    }
    if existing.delivered {
        return true;
    }
    acknowledge(
        office.replication().as_ref(),
        peer,
        Receipt {
            message: existing.message,
            delivered: true,
        },
    )
    .is_ok()
}
