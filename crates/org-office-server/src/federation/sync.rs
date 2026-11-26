//! The satellite sync session: connect, handshake, outbox flushing, and correlated replies.

use std::os::fd::{AsRawFd, RawFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::snapshot_transfer;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use org_office::Office;
use org_pki::cert::build_satellite_tls_client_config;
use org_sync::work::{Origin, SourceKey};
use org_sync_proto::{work_ack_proof_digest_v1, write_sync_frame, SyncFrame};

use super::client::{read_client_frame, try_connect_hq_endpoint};
use super::relay::{
    apply_message_outbox_reply, expire_stale_non_close_head_messages, sync_native_message_relays,
};
use super::satellite_inbound::receive_satellite_inbound_relays;
use super::{
    now_ms, HqPermitRequest, HqPermitResponse, InboundCallbacks, InboundDeliveryNotifier,
    RelayedWake, SatelliteSyncSession, StaticPeer, WakeRelayHandler, FIRST_FRAME_DEADLINE,
};
use crate::federation_link::org_events::{apply_satellite_org_event_frames, ORG_EVENT_BATCH_MAX};

impl SatelliteSyncSession {
    /// Connects to HQ using this satellite's persisted Org-CA-pinned mTLS credentials, verifies
    /// HQ's leaf identity & epoch, and runs the `Hello` handshake.
    pub fn connect(office: Arc<Office>) -> Result<Self, String> {
        Self::connect_cancellable(office, None)
    }

    /// Bounded, cancellable multi-endpoint mTLS connection to HQ (`TASK-52` M-22 & #17): tries
    /// each configured HQ address through full TCP + TLS + identity + `Hello` before failing over.
    pub fn connect_cancellable(
        office: Arc<Office>,
        stop: Option<&AtomicBool>,
    ) -> Result<Self, String> {
        let ident = office
            .load_satellite_tls_identity()
            .map_err(|e| format!("failed loading satellite TLS identity: {e}"))?;
        let cfg = build_satellite_tls_client_config(
            &ident.ca_cert_der,
            Some((&ident.office_cert_der, &ident.office_key_der)),
        )
        .map_err(|e| format!("failed building satellite mTLS config: {e}"))?;
        let cp = office
            .replication()
            .org_checkpoint()
            .map_err(|e| format!("failed reading replication checkpoint: {e}"))?;
        let mut last_err = String::from("no HQ addresses configured");
        for addr in &ident.federation.hq_addresses {
            if stop.is_some_and(|s| s.load(Ordering::Acquire)) {
                return Err("federation connect cancelled by shutdown".into());
            }
            match try_connect_hq_endpoint(
                Arc::clone(&office),
                &ident,
                Arc::clone(&cfg),
                &cp,
                addr,
                stop,
            ) {
                Ok(session) => return Ok(session),
                Err(e) => last_err = format!("{addr}: {e}"),
            }
        }
        Err(format!("cannot establish HQ mTLS session: {last_err}"))
    }

    pub(crate) fn raw_fd(&self) -> RawFd {
        self.tls.sock.as_raw_fd()
    }

    pub(crate) fn write_frame(&mut self, frame: &SyncFrame) -> Result<(), String> {
        write_sync_frame(&mut self.tls, frame).map_err(|e| format!("federation write failed: {e}"))
    }

    /// Reads the next correlated reply frame while buffering any unsolicited HQ-pushed relays,
    /// `OrgEvent`s, or `SessionRpcRequest`s so queued pushes never disrupt snapshot/work/message/
    /// permit RPCs (`TASK-52` H-14, `TASK-140`, `TASK-143` AC #3).
    pub(crate) fn read_correlated_frame(
        &mut self,
        timeout: Duration,
        expected_receipt_msg_id: Option<&str>,
    ) -> Result<Option<SyncFrame>, String> {
        read_correlated_frame_opts(self, timeout, expected_receipt_msg_id, true)
    }

    /// Streams, validates, stages to disk, applies in bounded dependency-safe batches, and installs
    /// HQ's authoritative `state/` snapshot into `org-office` and `org-sync` (`TASK-52` P21-#2).
    pub fn sync_initial_snapshot(&mut self, operation_id: &str) -> Result<usize, String> {
        snapshot_transfer::sync_initial_snapshot_paged(self, operation_id)
    }

    /// Pulls and applies any new HQ organization events since our current checkpoint, verifying
    /// initial-snapshot state, hash chain, codec, and typed delta BEFORE mutating files (`TASK-52` H-10).
    pub fn pull_org_events(&mut self) -> Result<usize, String> {
        if self
            .office
            .sync_status()
            .map_err(|e| e.to_string())?
            .initial_snapshot_required
        {
            return Err("snapshot required before applying org events".into());
        }
        let cp = self
            .office
            .replication()
            .org_checkpoint()
            .map_err(|e| e.to_string())?;
        write_sync_frame(
            &mut self.tls,
            &SyncFrame::OrgGap {
                after_epoch: cp.position.epoch.get(),
                after_seq: cp.position.seq,
            },
        )
        .map_err(|e| format!("failed sending OrgGap: {e}"))?;
        // Events arrive back to back up to the closing `Pong`: apply them in verified batches
        // rather than one durable commit each (`TASK-140`), and always apply the ones already
        // received before reporting a later failure, as applying each on arrival did.
        let mut applied = 0usize;
        let mut burst = Vec::new();
        loop {
            match read_correlated_frame_opts(self, FIRST_FRAME_DEADLINE, None, false) {
                Ok(Some(ev @ SyncFrame::OrgEvent { .. })) => {
                    burst.push(ev);
                    if burst.len() >= ORG_EVENT_BATCH_MAX {
                        applied += self.apply_org_event_burst(&mut burst)?;
                    }
                }
                Ok(Some(SyncFrame::Pong { .. })) => {
                    applied += self.apply_org_event_burst(&mut burst)?;
                    return Ok(applied);
                }
                other => {
                    self.apply_org_event_burst(&mut burst)?;
                    return Err(match other {
                        Err(e) => e,
                        Ok(other) => format!("unexpected frame during org event pull: {other:?}"),
                    });
                }
            }
        }
    }

    fn apply_org_event_burst(&self, burst: &mut Vec<SyncFrame>) -> Result<usize, String> {
        apply_satellite_org_event_frames(&self.office, &self.hq_peer, std::mem::take(burst))
    }

    /// Pushes all pending local work outbox entries to HQ (draining any un-enqueued local satellite
    /// task journal entries first, `TASK-52` H-6) and verifies `WorkAck` proof before pruning.
    pub fn flush_work_outbox(&mut self) -> Result<usize, String> {
        flush_satellite_work_outbox(self)
    }

    /// Flushes all pending cross-office messages from the satellite's durable message outbox to HQ
    /// (draining any un-enqueued committed native messaging journal entries first, `TASK-52` #10)
    /// and matches returned delivery receipts against the original message (`TASK-52` F4 / P3-#8).
    pub fn flush_message_outbox(&mut self) -> Result<usize, String> {
        flush_satellite_message_outbox(self)
    }

    /// Receives and commits any inbound relayed messages/receipts/wakes pushed by HQ (draining
    /// any frames buffered during correlated RPCs first) and sends explicit `RelayAck` after
    /// durable consumption (`TASK-52` H-14 + M-19).
    pub fn receive_inbound_relays(
        &mut self,
        wake_handler: Option<&WakeRelayHandler>,
    ) -> Result<usize, String> {
        self.receive_inbound_relays_with_notifier(wake_handler, None)
    }

    /// Receives inbound relays and invokes `delivery_notifier` after each durably committed
    /// message/answer/withdrawal/accept (`TASK-117`).
    pub fn receive_inbound_relays_with_notifier(
        &mut self,
        wake_handler: Option<&WakeRelayHandler>,
        delivery_notifier: Option<&InboundDeliveryNotifier>,
    ) -> Result<usize, String> {
        receive_satellite_inbound_relays(
            self,
            InboundCallbacks {
                wake: wake_handler,
                notify: delivery_notifier,
                feed_wake: None,
            },
        )
    }

    /// Requests a shared `HqPool` turn permit from HQ over the authenticated mTLS session (`TASK-52` P2-14).
    pub fn request_hq_permit(&mut self, req: HqPermitRequest) -> Result<HqPermitResponse, String> {
        let request_id = req.request_id.clone();
        write_sync_frame(
            &mut self.tls,
            &SyncFrame::PermitSyncRequest {
                request_id: req.request_id,
                connection_id: req.connection_id,
                employee_id: req.employee_id,
                task_id: req.task_id,
                model_id: req.model_id,
                is_failover_switch: req.is_failover_switch,
            },
        )
        .map_err(|e| format!("failed sending PermitSyncRequest: {e}"))?;
        match self.read_correlated_frame(FIRST_FRAME_DEADLINE, None)? {
            Some(SyncFrame::PermitSyncResponse {
                request_id: resp_id,
                granted,
                permit_id,
                coordinator_generation,
                reason,
                cooldown_until,
                occupied,
                max_capacity,
            }) if resp_id == request_id => Ok(HqPermitResponse {
                granted,
                permit_id,
                coordinator_generation,
                reason,
                cooldown_until,
                occupied,
                max_capacity,
            }),
            other => Err(format!("unexpected PermitSyncResponse: {other:?}")),
        }
    }

    /// Renews an HQ-issued `HqPool` turn permit during turn heartbeat (`TASK-52` #12).
    pub fn renew_hq_permit(
        &mut self,
        request_id: String,
        permit_id: String,
        coordinator_generation: u64,
    ) -> Result<bool, String> {
        write_sync_frame(
            &mut self.tls,
            &SyncFrame::PermitRenewRequest {
                request_id: request_id.clone(),
                permit_id,
                coordinator_generation,
            },
        )
        .map_err(|e| format!("failed sending PermitRenewRequest: {e}"))?;
        match self.read_correlated_frame(FIRST_FRAME_DEADLINE, None)? {
            Some(SyncFrame::PermitRenewResponse {
                request_id: resp_id,
                renewed,
            }) if resp_id == request_id => Ok(renewed),
            other => Err(format!("unexpected PermitRenewResponse: {other:?}")),
        }
    }

    /// Releases an HQ-issued turn permit held by this satellite (`TASK-52` P2-14).
    pub fn release_hq_permit(&mut self, permit_id: String) -> Result<(), String> {
        write_sync_frame(
            &mut self.tls,
            &SyncFrame::PermitReleaseNotify {
                permit_id,
                release_pending: true,
            },
        )
        .map_err(|e| format!("failed sending PermitReleaseNotify: {e}"))
    }

    /// Reports a provider cooldown observed by this satellite to HQ (`TASK-52` P2-14).
    pub fn report_hq_cooldown(
        &mut self,
        connection_id: String,
        resets_at: Option<String>,
        reason: Option<String>,
    ) -> Result<(), String> {
        write_sync_frame(
            &mut self.tls,
            &SyncFrame::CooldownNotify {
                connection_id,
                resets_at,
                reason,
            },
        )
        .map_err(|e| format!("failed sending CooldownNotify: {e}"))
    }

    /// Sends a cross-office wake notification through HQ.
    pub fn send_wake(&mut self, wake: RelayedWake) -> Result<(), String> {
        write_sync_frame(
            &mut self.tls,
            &SyncFrame::WakeNotify {
                org_id: self.office.info().org_id.clone(),
                origin_office: wake.origin_office,
                destination_office: wake.destination_office,
                employee_id: wake.employee_id,
                kind: wake.kind,
                source_id: wake.source_id,
                sender_id: wake.sender_id,
                sender_name: wake.sender_name,
                sender_is_person: wake.sender_is_person,
                conversation_id: wake.conversation_id,
                summary: wake.summary,
            },
        )
        .map_err(|e| format!("failed sending WakeNotify: {e}"))
    }
}

fn read_correlated_frame_opts(
    session: &mut SatelliteSyncSession,
    timeout: Duration,
    expected_receipt_msg_id: Option<&str>,
    buffer_org_events: bool,
) -> Result<Option<SyncFrame>, String> {
    let deadline = Instant::now() + timeout;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err("timed out waiting for HQ reply".into());
        }
        match read_client_frame(&mut session.tls, &mut session.reader, left)? {
            Some(
                frame @ (SyncFrame::MessageDeliver { .. }
                | SyncFrame::WakeNotify { .. }
                | SyncFrame::SessionRpcRequest { .. }),
            ) => {
                session.pending_inbound.push_back(frame);
            }
            Some(frame @ SyncFrame::OrgEvent { .. }) if buffer_org_events => {
                session.pending_inbound.push_back(frame);
            }
            Some(frame @ SyncFrame::Pong { .. }) if buffer_org_events => {
                session.pending_inbound.push_back(frame);
            }
            Some(SyncFrame::Ping { nonce }) => {
                let _ = write_sync_frame(&mut session.tls, &SyncFrame::Pong { nonce });
            }
            Some(frame)
                if matches!(
                    &frame,
                    SyncFrame::MessageReceipt {
                        message_id,
                        delivered: true,
                        relayed_by_hq: false,
                        ..
                    } if expected_receipt_msg_id != Some(message_id.as_str())
                ) =>
            {
                session.pending_inbound.push_back(frame);
            }
            other => return Ok(other),
        }
    }
}

fn verify_and_ack_work_change(
    office: &Office,
    origin: &Origin,
    expected_seq: u64,
    prev_acked: u64,
    reply: Option<SyncFrame>,
) -> Result<u64, String> {
    let Some(SyncFrame::WorkAck {
        origin_office,
        incarnation,
        seq: ack_seq,
        proof_hex,
    }) = reply
    else {
        return Err(format!("unexpected WorkAck reply: {reply:?}"));
    };
    if origin_office != origin.office.as_str()
        || incarnation != origin.incarnation.as_str()
        || ack_seq != expected_seq
    {
        return Err("WorkAck identity or sequence mismatch".into());
    }
    let expected_ack = org_sync_proto::WorkAck {
        origin_office_id: origin.office.clone(),
        incarnation: origin.incarnation.clone(),
        from_exclusive: prev_acked.min(ack_seq.saturating_sub(1)),
        through_seq: ack_seq,
        rejected: Vec::new(),
    };
    let expected_proof = work_ack_proof_digest_v1(&expected_ack)
        .map_err(|_| "failed computing expected WorkAck proof".to_owned())?;
    if proof_hex != expected_proof.to_hex() {
        return Err("WorkAck proof digest verification failed".into());
    }
    office
        .replication()
        .ack_work_through(ack_seq)
        .map_err(|e| format!("failed acking work outbox: {e}"))?;
    Ok(ack_seq)
}

fn flush_satellite_work_outbox(session: &mut SatelliteSyncSession) -> Result<usize, String> {
    let _ = session.office.sync_satellite_work_outbox();
    let outbox = session
        .office
        .replication()
        .load_work_outbox()
        .map_err(|e| e.to_string())?;
    let mut acked = 0usize;
    let mut prev_acked = outbox.acknowledged_seq();
    for change in outbox.pending() {
        let SourceKey { origin, seq } = &change.key;
        write_sync_frame(
            &mut session.tls,
            &SyncFrame::WorkChange {
                org_id: change.org_id.as_str().to_owned(),
                origin_office: origin.office.as_str().to_owned(),
                incarnation: origin.incarnation.as_str().to_owned(),
                seq: seq.get(),
                record_id: change.record_id.clone(),
                generation: change.version.generation,
                version: change.version.version,
                digest_hex: change.digest.to_hex(),
                record_digest_hex: change.record_digest.to_hex(),
                codec_name: change.codec.id().to_owned(),
                codec_version: change.codec.version(),
                payload_b64: STANDARD.encode(&change.payload),
                required_org_epoch: change.required_org.epoch.get(),
                required_org_seq: change.required_org.seq,
                created_ms: change.created_ms,
            },
        )
        .map_err(|e| format!("failed sending WorkChange: {e}"))?;
        let reply = session.read_correlated_frame(FIRST_FRAME_DEADLINE, None)?;
        prev_acked =
            verify_and_ack_work_change(&session.office, origin, seq.get(), prev_acked, reply)?;
        acked += 1;
    }
    Ok(acked)
}

fn flush_satellite_message_outbox(session: &mut SatelliteSyncSession) -> Result<usize, String> {
    sync_native_message_relays(&session.office);
    let peer = StaticPeer(session.hq_peer.clone());
    expire_stale_non_close_head_messages(&session.office)?;
    let pending = session
        .office
        .replication()
        .message_queue_snapshot()
        .map_err(|e| format!("message queue snapshot error: {e}"))?
        .pending;
    let now = now_ms();
    let mut delivered_count = 0usize;
    for msg in pending {
        let is_close = msg.id.starts_with("close-");
        if !is_close && msg.deadline_ms <= now {
            continue;
        }
        let wire_deadline_ms = if is_close && msg.deadline_ms <= now {
            now.saturating_add(7 * 24 * 60 * 60 * 1000)
        } else {
            msg.deadline_ms
        };
        write_sync_frame(
            &mut session.tls,
            &SyncFrame::MessageDeliver {
                org_id: msg.org_id.as_str().to_owned(),
                origin_office: msg.origin.as_str().to_owned(),
                destination_office: msg.destination.as_str().to_owned(),
                message_id: msg.id.clone(),
                fingerprint_hex: msg.fingerprint.to_hex(),
                payload_b64: STANDARD.encode(&msg.payload),
                deadline_ms: wire_deadline_ms,
            },
        )
        .map_err(|e| format!("failed sending MessageDeliver: {e}"))?;
        let reply = session.read_correlated_frame(FIRST_FRAME_DEADLINE, Some(&msg.id))?;
        delivered_count += apply_message_outbox_reply(&session.office, &peer, msg, reply)?;
    }
    Ok(delivered_count)
}
