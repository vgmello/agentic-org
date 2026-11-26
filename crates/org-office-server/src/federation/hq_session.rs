//! The HQ satellite session loop: inbound draining, outbound flushing (relays, proxied session
//! RPCs, organization events), and poll pacing.

use std::collections::{BTreeSet, HashMap};
use std::io;
use std::net::TcpStream;
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use crate::federation_stream::HqPeerRegistry;
use crate::snapshot_transfer::{ActiveSnapshotTransfer, HqSnapshotTransferBudget};
use org_office::Office;
use org_sync_proto::{FrameCodecError, FrameReader, OfficeId, OrgId, SyncFrame};
use rustls::{ServerConnection, StreamOwned};

use super::hq_dispatch::dispatch_hq_frame;
use super::relay::sync_native_message_relays;
use super::{now_ms, InboundCallbacks};
use crate::federation_link::hq_peer::{
    check_not_revoked_fail_closed, push_hq_org_events_if_behind, read_hq_frame_nonblocking,
    send_hq_org_events_after, write_sync_frame_if_peer_active, HqSessionPeer,
};

fn flush_durable_relays_to_satellite(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    peer: &HqSessionPeer<'_>,
    sent_in_session: &mut BTreeSet<String>,
) -> bool {
    let satellite_office_id = peer.sat_id.as_str();
    sync_native_message_relays(peer.office);
    let Ok(items) = peer
        .office
        .replication()
        .pending_relays_for(satellite_office_id)
    else {
        return false;
    };
    let now = now_ms();
    for item in items {
        if peer.session_stop.load(Ordering::Acquire) {
            return false;
        }
        let Ok(mut frame) = serde_json::from_str::<SyncFrame>(&item.payload_json) else {
            continue;
        };
        let is_ask_close = item.id.starts_with("close-");
        // Prune expired relays across ALL non-close kinds before draining; authoritative ask
        // closes/withdrawals (`close-*`) stay durable until explicitly acknowledged (`TASK-52` P12-#5).
        let expired = !is_ask_close
            && ((item.expires_at_ms > 0 && item.expires_at_ms <= now)
                || matches!(&frame, SyncFrame::MessageDeliver { deadline_ms, .. } if *deadline_ms <= now));
        if expired {
            let _ = peer
                .office
                .replication()
                .ack_relay_item_for_destination(satellite_office_id, &item.id);
            continue;
        }
        if is_ask_close {
            if let SyncFrame::MessageDeliver {
                ref mut deadline_ms,
                ..
            } = frame
            {
                if *deadline_ms <= now {
                    *deadline_ms = now.saturating_add(7 * 24 * 60 * 60 * 1000);
                }
            }
        }
        if !sent_in_session.insert(item.id.clone()) {
            continue;
        }
        // Fence every outbound frame against concurrent office removal under `peer_activity_fence`
        // so a copied relay list can never leak frames after `remove_satellite_office` (`TASK-52` P19-#2).
        if !write_sync_frame_if_peer_active(tls, peer, &frame) {
            sent_in_session.remove(&item.id);
            return false;
        }
        // Keep EVERY relay item (messages, receipts, and wakes) durably queued until the receiver
        // explicitly acknowledges durable consumption via `MessageReceipt` or `RelayAck` (`TASK-52` M-19).
    }
    true
}

type InflightRpcMap = HashMap<
    String,
    (
        Instant,
        mpsc::Sender<Result<org_proto::Outcome, org_proto::ApiError>>,
    ),
>;

fn prune_expired_inflight_rpcs(
    inflight_rpcs: &mut InflightRpcMap,
    peer_handle: &crate::federation_stream::HqPeerHandle,
) {
    let now = Instant::now();
    let before = inflight_rpcs.len();
    inflight_rpcs.retain(|_, (deadline, _)| now < *deadline);
    peer_handle.release_inflight(before.saturating_sub(inflight_rpcs.len()));
}

fn flush_hq_session_rpcs(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    peer: &HqSessionPeer<'_>,
    peer_handle: &crate::federation_stream::HqPeerHandle,
    inflight_rpcs: &mut InflightRpcMap,
) -> bool {
    prune_expired_inflight_rpcs(inflight_rpcs, peer_handle);
    let mut pending = peer_handle.take_rpcs().into_iter();
    while let Some(rpc) = pending.next() {
        let frame = SyncFrame::SessionRpcRequest {
            request_id: rpc.request_id.clone(),
            caller_member_id: rpc.caller_member_id,
            caller_is_owner: rpc.caller_is_owner,
            method: rpc.method,
            params_json: rpc.params_json,
        };
        if !write_sync_frame_if_peer_active(tls, peer, &frame) {
            let remaining = 1 + pending.len();
            peer_handle.release_inflight(remaining);
            let _ = rpc.reply.send(Err(org_proto::ApiError::new(
                org_proto::ErrorCode::Unavailable,
                "failed sending session RPC to satellite office",
            )));
            for unsent in pending {
                let _ = unsent.reply.send(Err(org_proto::ApiError::new(
                    org_proto::ErrorCode::Unavailable,
                    "failed sending session RPC to satellite office",
                )));
            }
            return false;
        }
        inflight_rpcs.insert(rpc.request_id, (rpc.deadline, rpc.reply));
    }
    true
}

fn deliver_hq_session_rpc_response(
    inflight_rpcs: &mut InflightRpcMap,
    peer_handle: &crate::federation_stream::HqPeerHandle,
    request_id: &str,
    outcome_json: &str,
) {
    let Some((_, tx)) = inflight_rpcs.remove(request_id) else {
        return;
    };
    peer_handle.release_inflight(1);
    let parsed = serde_json::from_str::<org_proto::Outcome>(outcome_json).map_err(|_| {
        org_proto::ApiError::new(
            org_proto::ErrorCode::Unavailable,
            "malformed session RPC response from satellite office",
        )
    });
    let _ = tx.send(parsed);
}

struct HqSessionState<'a> {
    reader: FrameReader,
    sent_in_session: BTreeSet<String>,
    active_snapshot: Option<ActiveSnapshotTransfer>,
    peer_org_cursor: Option<(u64, u64)>,
    inflight_rpcs: InflightRpcMap,
    hello_done: bool,
    callbacks: InboundCallbacks<'a>,
}

fn drain_hq_inbound_frames(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    peer: &HqSessionPeer<'_>,
    peer_handle: &crate::federation_stream::HqPeerHandle,
    snapshot_budget: &HqSnapshotTransferBudget,
    peers: &HqPeerRegistry,
    state: &mut HqSessionState<'_>,
) -> bool {
    loop {
        match read_hq_frame_nonblocking(tls, &mut state.reader) {
            Ok(Some(frame)) => {
                if peer.session_stop.load(Ordering::Acquire)
                    || !check_not_revoked_fail_closed(
                        tls,
                        peer.office,
                        peer.sat_id.as_str(),
                        peer.cert_serial_hex,
                    )
                {
                    return false;
                }
                match frame {
                    SyncFrame::SessionRpcResponse {
                        request_id,
                        outcome_json,
                    } => {
                        deliver_hq_session_rpc_response(
                            &mut state.inflight_rpcs,
                            peer_handle,
                            &request_id,
                            &outcome_json,
                        );
                    }
                    SyncFrame::OrgGap {
                        after_epoch,
                        after_seq,
                    } => {
                        state.active_snapshot = None;
                        let Some(last_sent) =
                            send_hq_org_events_after(tls, peer, after_epoch, after_seq)
                        else {
                            return false;
                        };
                        state.peer_org_cursor = Some(last_sent);
                    }
                    other => {
                        let is_hello = matches!(other, SyncFrame::Hello { .. });
                        if !dispatch_hq_frame(
                            tls,
                            peer,
                            snapshot_budget,
                            &mut state.active_snapshot,
                            other,
                            state.callbacks,
                            peers,
                        ) {
                            return false;
                        }
                        if is_hello {
                            state.hello_done = true;
                        }
                    }
                }
            }
            Ok(None) => return false,
            Err(FrameCodecError::Io(e))
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                return true;
            }
            Err(_) => return false,
        }
    }
}

fn hq_session_flush_outbound(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    peer: &HqSessionPeer<'_>,
    peer_handle: &crate::federation_stream::HqPeerHandle,
    state: &mut HqSessionState<'_>,
    buffered_flush_until: &mut Option<Instant>,
    wake_triggered: &mut bool,
) -> bool {
    if !state.hello_done || state.reader.has_partial_frame() {
        return true;
    }
    let sent_before = state.sent_in_session.len();
    let cursor_before = state.peer_org_cursor;
    if !flush_durable_relays_to_satellite(tls, peer, &mut state.sent_in_session)
        || !push_hq_org_events_if_behind(tls, peer, &mut state.peer_org_cursor)
        || !flush_hq_session_rpcs(tls, peer, peer_handle, &mut state.inflight_rpcs)
    {
        return false;
    }
    let sent_any =
        state.sent_in_session.len() != sent_before || state.peer_org_cursor != cursor_before;
    update_buffered_flush_deadline(
        buffered_flush_until,
        sent_any,
        *wake_triggered || peer.office.has_pending_journal_pipeline(),
    );
    *wake_triggered = false;
    true
}

fn update_buffered_flush_deadline(deadline: &mut Option<Instant>, sent_any: bool, triggered: bool) {
    if sent_any {
        *deadline = None;
    } else if triggered {
        *deadline = Some(Instant::now() + Duration::from_millis(80));
    } else if deadline.is_some_and(|until| Instant::now() >= until) {
        *deadline = None;
    }
}

fn hq_session_poll_wait(
    office: &Office,
    state: &HqSessionState<'_>,
    buffered_flush_until: Option<Instant>,
) -> Duration {
    if buffered_flush_until.is_some() || office.has_pending_journal_pipeline() {
        Duration::from_millis(5)
    } else if state.reader.has_partial_frame() || state.active_snapshot.is_some() {
        Duration::from_millis(250)
    } else {
        Duration::from_millis(500)
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "HQ satellite session handler coordinates stream, office, peer identity, callbacks, registry, and snapshot budget"
)]
pub(super) fn handle_hq_satellite_session(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    office: &Office,
    satellite_office_id: &str,
    cert_serial_hex: &str,
    stop: &AtomicBool,
    callbacks: InboundCallbacks<'_>,
    peers: &Arc<HqPeerRegistry>,
    snapshot_budget: &HqSnapshotTransferBudget,
) {
    let (Ok(org_id), Ok(sat_id), Ok(peer_reg)) = (
        OrgId::new(&office.info().org_id),
        OfficeId::new(satellite_office_id),
        peers.register(satellite_office_id),
    ) else {
        return;
    };
    let session_stop = Arc::new(AtomicBool::new(false));
    let _session_guard = org_office::register_peer_session_stop(
        office,
        satellite_office_id,
        Arc::clone(&session_stop),
    );
    let peer = HqSessionPeer {
        office,
        org_id: &org_id,
        sat_id: &sat_id,
        cert_serial_hex,
        session_stop: &session_stop,
        server_stop: stop,
    };
    let mut state = HqSessionState {
        reader: FrameReader::new(),
        sent_in_session: BTreeSet::new(),
        active_snapshot: None,
        peer_org_cursor: None,
        inflight_rpcs: HashMap::new(),
        hello_done: false,
        callbacks,
    };
    let mut buffered_flush_until: Option<Instant> = None;
    let mut last_wake_gen = peer_reg.handle().wake_generation();
    let mut wake_triggered = false;
    while !stop.load(Ordering::Acquire) && !session_stop.load(Ordering::Acquire) {
        ActiveSnapshotTransfer::expire_if_idle(&mut state.active_snapshot);
        let cur_gen = peer_reg.handle().wake_generation();
        if cur_gen != last_wake_gen {
            last_wake_gen = cur_gen;
            wake_triggered = true;
        }
        let alive =
            check_not_revoked_fail_closed(tls, office, satellite_office_id, cert_serial_hex)
                && drain_hq_inbound_frames(
                    tls,
                    &peer,
                    peer_reg.handle(),
                    snapshot_budget,
                    peers,
                    &mut state,
                )
                && hq_session_flush_outbound(
                    tls,
                    &peer,
                    peer_reg.handle(),
                    &mut state,
                    &mut buffered_flush_until,
                    &mut wake_triggered,
                );
        if !alive {
            return;
        }
        peer_reg.handle().wake.drain();
        if peer_reg.handle().wake_generation() != last_wake_gen
            || peer_reg.handle().has_pending_rpcs()
        {
            continue;
        }
        let wait = hq_session_poll_wait(office, &state, buffered_flush_until);
        match org_socket::poll_readable_either(
            tls.sock.as_raw_fd(),
            peer_reg.handle().wake.read_fd(),
            wait,
        ) {
            Ok((_, wake_ready)) => wake_triggered |= wake_ready,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }
    }
}
