//! Multi-office Org-CA mTLS federation listener (HQ) and full-duplex sync/relay client
//! (satellite) (`TASK-52` / `S1`, `doc-67`, `X-05`, `FC-020`, `FC-024`, `FC-145`).

use std::collections::{BTreeSet, VecDeque};
use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_office::{
    replication::StoredRelayItem, InvitationPollStatus, JoinRequestAccepted, JoinRequestParams,
    Office, OrgDeltaPayload,
};
use org_pki::{
    cert::{
        build_hq_tls_dual_server_config, build_satellite_tls_client_config,
        export_enrollment_binding, sign_satellite_key_pop, verify_office_cert, OfficeCertRole,
    },
    invitation::{sign_exporter_proof, InvitationToken},
    SigningKey,
};
use org_sync::{
    messages::{acknowledge, next as next_message, receive, Message, MessageOutput, Receipt},
    organization::{
        apply as apply_org_event, OrgEvent, OrgOutput, PeerIdentity, PeerRole, VerifiedPeer,
    },
    work::{Origin, SourceKey},
};
use org_sync_proto::{
    work_ack_proof_digest_v1, work_record_digest_v1, work_source_digest_v1, write_sync_frame,
    Codec, Digest, EnrollTerminalStatus, Epoch, FrameCodecError, FrameReader, OfficeId, OrgId,
    OriginIncarnation, Position, SourceSeq, SyncFrame, WireOfficeRole,
};
use rustls::pki_types::ServerName;
use rustls::{ClientConnection, ServerConnection, StreamOwned};
use sha2::{Digest as _, Sha256};

use crate::snapshot_transfer::{self, ActiveSnapshotTransfer, HqSnapshotTransferBudget};

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
}

struct StaticPeer(PeerIdentity);

impl VerifiedPeer for StaticPeer {
    fn identity(&self) -> PeerIdentity {
        self.0.clone()
    }
}

struct ConnPermit(Arc<AtomicUsize>);

impl ConnPermit {
    fn try_acquire(counter: &Arc<AtomicUsize>) -> Option<Self> {
        let mut cur = counter.load(Ordering::Acquire);
        loop {
            if cur >= MAX_CONCURRENT_CONNECTIONS {
                return None;
            }
            match counter.compare_exchange_weak(cur, cur + 1, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return Some(Self(Arc::clone(counter))),
                Err(actual) => cur = actual,
            }
        }
    }
}

impl Drop for ConnPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
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

/// Running HQ federation TLS listener serving enrollment + mTLS sync & durable cross-office relay.
pub struct HqFederationServer {
    addr: SocketAddr,
    office: Arc<Office>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    workers: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl HqFederationServer {
    /// Starts HQ's Org-CA TLS federation listener bound to `bind_addr`.
    pub fn start(
        office: Arc<Office>,
        bind_addr: SocketAddr,
        san_hosts: &[String],
        wake_handler: Option<WakeRelayHandler>,
    ) -> Result<Self, String> {
        Self::start_with_notifier(office, bind_addr, san_hosts, wake_handler, None)
    }

    /// Starts HQ's Org-CA TLS federation listener with both durable wake and local delivery
    /// notification callbacks (`TASK-117`).
    pub fn start_with_notifier(
        office: Arc<Office>,
        bind_addr: SocketAddr,
        san_hosts: &[String],
        wake_handler: Option<WakeRelayHandler>,
        delivery_notifier: Option<InboundDeliveryNotifier>,
    ) -> Result<Self, String> {
        let hq_tls = office
            .ensure_hq_tls_identity(san_hosts)
            .map_err(|e| format!("failed preparing HQ TLS identity: {e}"))?;
        let tls_cfg = build_hq_tls_dual_server_config(
            &hq_tls.ca_cert_der,
            &hq_tls.hq_cert_der,
            &hq_tls.hq_key_der,
        )
        .map_err(|e| format!("failed building HQ TLS server config: {e}"))?;
        let listener = TcpListener::bind(bind_addr)
            .map_err(|e| format!("failed binding HQ federation listener: {e}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("failed configuring nonblocking listener: {e}"))?;
        let addr = listener
            .local_addr()
            .map_err(|e| format!("failed reading listener address: {e}"))?;
        let stop = Arc::new(AtomicBool::new(false));
        let active_conns = Arc::new(AtomicUsize::new(0));
        let workers = Arc::new(Mutex::new(Vec::new()));
        let stop_bg = Arc::clone(&stop);
        let office_bg = Arc::clone(&office);
        let workers_bg = Arc::clone(&workers);
        let ca_der = Arc::new(hq_tls.ca_cert_der);
        let cached_tls = Arc::new(Mutex::new((hq_tls.hq_cert_der.clone(), tls_cfg)));
        let handle = thread::Builder::new()
            .name("hq-federation".into())
            .spawn(move || {
                run_hq_accept_loop(
                    listener,
                    AcceptCtx {
                        office: office_bg,
                        cached_tls,
                        ca_der,
                        stop: stop_bg,
                        wake_handler,
                        delivery_notifier,
                        snapshot_budget: HqSnapshotTransferBudget::production(),
                    },
                    active_conns,
                    workers_bg,
                );
            })
            .map_err(|e| format!("failed spawning HQ federation thread: {e}"))?;
        Ok(Self {
            addr,
            office,
            stop,
            handle: Some(handle),
            workers,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Durably enqueues a cross-office frame for delivery to a connected satellite office.
    pub fn queue_for_satellite(
        &self,
        destination_office_id: &str,
        relay_id: &str,
        kind: &str,
        frame: &SyncFrame,
    ) -> Result<(), String> {
        let payload_json = serde_json::to_string(frame).map_err(|e| e.to_string())?;
        match org_office::admit_relay_item_if_active(
            &self.office,
            StoredRelayItem {
                id: relay_id.to_owned(),
                origin_office: self.office.info().office_id.clone(),
                destination_office: destination_office_id.to_owned(),
                fingerprint_hex: String::new(),
                kind: kind.to_owned(),
                payload_json,
                expires_at_ms: if kind == "wake" {
                    now_ms().saturating_add(WAKE_RELAY_TTL_MS)
                } else {
                    0
                },
            },
        )
        .map_err(|e| e.to_string())?
        {
            org_office::RelayAdmission::Admitted => Ok(()),
            org_office::RelayAdmission::OriginRemoved => Err("origin office removed".into()),
            org_office::RelayAdmission::DestinationRemoved => {
                Err("destination office removed".into())
            }
            org_office::RelayAdmission::DestinationUnknown => {
                Err("destination office not enrolled".into())
            }
            org_office::RelayAdmission::QuotaExceeded => Err("relay queue quota exceeded".into()),
        }
    }

    pub fn stop(mut self) {
        self.shutdown_inner();
    }

    fn shutdown_inner(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        let handles = std::mem::take(&mut *self.workers.lock().unwrap_or_else(|p| p.into_inner()));
        for h in handles {
            let _ = h.join();
        }
    }
}

impl Drop for HqFederationServer {
    fn drop(&mut self) {
        self.shutdown_inner();
    }
}

#[derive(Clone)]
struct AcceptCtx {
    office: Arc<Office>,
    cached_tls: Arc<Mutex<(Vec<u8>, Arc<rustls::ServerConfig>)>>,
    ca_der: Arc<Vec<u8>>,
    stop: Arc<AtomicBool>,
    wake_handler: Option<WakeRelayHandler>,
    delivery_notifier: Option<InboundDeliveryNotifier>,
    snapshot_budget: HqSnapshotTransferBudget,
}

/// Returns the live HQ `ServerConfig`, transparently reloading if `create_satellite_invitation`
/// rotated HQ's TLS certificate on disk with new SANs (`TASK-52` #16).
fn current_hq_tls_config(ctx: &AcceptCtx) -> Arc<rustls::ServerConfig> {
    if let Ok(hq_tls) = ctx.office.ensure_hq_tls_identity(&[]) {
        let mut guard = ctx.cached_tls.lock().unwrap_or_else(|p| p.into_inner());
        if guard.0 != hq_tls.hq_cert_der {
            if let Ok(new_cfg) = build_hq_tls_dual_server_config(
                &hq_tls.ca_cert_der,
                &hq_tls.hq_cert_der,
                &hq_tls.hq_key_der,
            ) {
                *guard = (hq_tls.hq_cert_der, Arc::clone(&new_cfg));
                return new_cfg;
            }
        }
        return Arc::clone(&guard.1);
    }
    Arc::clone(&ctx.cached_tls.lock().unwrap_or_else(|p| p.into_inner()).1)
}

fn run_hq_accept_loop(
    listener: TcpListener,
    base_ctx: AcceptCtx,
    active_conns: Arc<AtomicUsize>,
    workers: Arc<Mutex<Vec<JoinHandle<()>>>>,
) {
    // Reconcile any interrupted removed-office relay purges and provisional withdrawals at HQ
    // startup, clean stale snapshot export dirs, and run periodic reconciliation on EVERY loop
    // iteration so busy listeners cannot starve retries (`TASK-52` P18-#3 / P19-#4 / P21-#2).
    snapshot_transfer::cleanup_stale_hq_snapshot_exports(&base_ctx.office);
    let _ = org_office::promote_pending_removed_origin_withdrawals(&base_ctx.office);
    let mut last_reconcile = Instant::now();
    while !base_ctx.stop.load(Ordering::Acquire) {
        if last_reconcile.elapsed() >= Duration::from_secs(2) {
            let _ = org_office::promote_pending_removed_origin_withdrawals(&base_ctx.office);
            last_reconcile = Instant::now();
        }
        match listener.accept() {
            Ok((stream, _peer)) => {
                let Some(permit) = ConnPermit::try_acquire(&active_conns) else {
                    drop(stream);
                    continue;
                };
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(SOCKET_READ_TIMEOUT));
                let _ = stream.set_write_timeout(Some(SOCKET_WRITE_TIMEOUT));
                let ctx = base_ctx.clone();
                if let Ok(h) = thread::Builder::new()
                    .name("hq-fed-conn".into())
                    .spawn(move || {
                        let _permit = permit;
                        handle_hq_connection(stream, ctx);
                    })
                {
                    let mut list = workers.lock().unwrap_or_else(|p| p.into_inner());
                    list.retain(|w| !w.is_finished());
                    list.push(h);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => thread::sleep(POLL_SLEEP),
            Err(_) => break,
        }
    }
}

fn complete_server_handshake(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    stop: &AtomicBool,
) -> bool {
    let deadline = Instant::now() + HANDSHAKE_DEADLINE;
    while tls.conn.is_handshaking() {
        if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
            return false;
        }
        match tls.conn.complete_io(&mut tls.sock) {
            Ok(_) => {}
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(_) => return false,
        }
    }
    true
}

fn handle_hq_connection(stream: TcpStream, ctx: AcceptCtx) {
    let tls_cfg = current_hq_tls_config(&ctx);
    let Ok(conn) = ServerConnection::new(tls_cfg) else {
        return;
    };
    let mut tls = StreamOwned::new(conn, stream);
    if !complete_server_handshake(&mut tls, &ctx.stop) {
        return;
    }
    let peer_certs = tls.conn.peer_certificates().map(|c| c.to_vec());
    match peer_certs.as_deref() {
        None | Some([]) => handle_hq_enrollment(&mut tls, &ctx.office, &ctx.stop),
        Some([leaf, ..]) => {
            // Fail closed on revocation storage failure (`TASK-52` F10).
            let Ok(revoked) = ctx.office.revoked_cert_serials() else {
                let _ = write_sync_frame(
                    &mut tls,
                    &SyncFrame::Error {
                        code: "unavailable".into(),
                        message: "revocation store unavailable".into(),
                    },
                );
                return;
            };
            let verified = match verify_office_cert(
                &ctx.ca_der,
                leaf.as_ref(),
                &ctx.office.info().org_id,
                Some(OfficeCertRole::Satellite),
                &revoked,
                now_unix(),
            ) {
                Ok(v) => v,
                Err(e) => {
                    let _ = write_sync_frame(
                        &mut tls,
                        &SyncFrame::Error {
                            code: "not_authorized".into(),
                            message: e.to_string(),
                        },
                    );
                    return;
                }
            };
            handle_hq_satellite_session(
                &mut tls,
                &ctx.office,
                &verified.office_id,
                &verified.serial_hex,
                &ctx.stop,
                InboundCallbacks {
                    wake: ctx.wake_handler.as_ref(),
                    notify: ctx.delivery_notifier.as_ref(),
                },
                &ctx.snapshot_budget,
            );
        }
    }
}

fn handle_hq_enrollment(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    office: &Office,
    stop: &AtomicBool,
) {
    let Ok(exporter) = export_enrollment_binding(&tls.conn) else {
        return;
    };
    let mut reader = FrameReader::new();
    let Some(frame) = read_frame_with_deadline(tls, &mut reader, stop, FIRST_FRAME_DEADLINE) else {
        return;
    };
    let SyncFrame::EnrollRequest {
        invitation_id,
        request_id,
        office_id,
        joining_pub_key_b64,
        joining_key_pop_b64,
        exporter_proof_b64,
    } = frame
    else {
        let _ = write_sync_frame(
            tls,
            &SyncFrame::Error {
                code: "not_authorized".into(),
                message: "unauthenticated connection may only send enroll_request".into(),
            },
        );
        return;
    };
    let proofs = match decode_enroll_proofs(
        &joining_pub_key_b64,
        &joining_key_pop_b64,
        &exporter_proof_b64,
    ) {
        Ok(v) => v,
        Err(msg) => {
            send_enroll_terminal(tls, EnrollTerminalStatus::Rejected, msg);
            return;
        }
    };
    match office.register_satellite_join_request(JoinRequestParams {
        invitation_id: invitation_id.clone(),
        request_id: request_id.clone(),
        office_id,
        joining_pub_key: proofs.pub_key,
        pop_signature_der: proofs.pop_der,
        exporter_binding: exporter,
        exporter_proof_der: proofs.proof_der,
    }) {
        Ok(Ok(JoinRequestAccepted::Pending { check_code, .. })) => {
            if write_sync_frame(
                tls,
                &SyncFrame::EnrollPending {
                    request_id,
                    check_code,
                },
            )
            .is_err()
            {
                return;
            }
            wait_for_invitation_decision(tls, office, &invitation_id, stop);
        }
        Ok(Ok(JoinRequestAccepted::AlreadyApproved {
            cert_der,
            ca_cert_der,
            hq_office_id,
            epoch,
        })) => {
            let _ = write_sync_frame(
                tls,
                &SyncFrame::EnrollApproved {
                    cert_der_b64: STANDARD.encode(cert_der),
                    ca_cert_der_b64: STANDARD.encode(ca_cert_der),
                    hq_office_id,
                    epoch,
                },
            );
        }
        Ok(Err(api)) => {
            let status = if api.code == org_proto::ErrorCode::Expired {
                EnrollTerminalStatus::Expired
            } else {
                EnrollTerminalStatus::Rejected
            };
            send_enroll_terminal(tls, status, api.message);
        }
        Err(e) => send_enroll_terminal(tls, EnrollTerminalStatus::Rejected, e.to_string()),
    }
}

fn send_enroll_terminal(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    status: EnrollTerminalStatus,
    reason: String,
) {
    let _ = write_sync_frame(tls, &SyncFrame::EnrollTerminal { status, reason });
}

struct DecodedEnrollProofs {
    pub_key: [u8; 65],
    pop_der: Vec<u8>,
    proof_der: Vec<u8>,
}

fn decode_enroll_proofs(
    pub_b64: &str,
    pop_b64: &str,
    proof_b64: &str,
) -> Result<DecodedEnrollProofs, String> {
    let pub_vec = STANDARD
        .decode(pub_b64)
        .map_err(|_| "invalid joining_pub_key base64".to_owned())?;
    let pub_key: [u8; 65] = pub_vec
        .try_into()
        .map_err(|_| "joining_pub_key must be 65 bytes".to_owned())?;
    let pop_der = STANDARD
        .decode(pop_b64)
        .map_err(|_| "invalid joining_key_pop base64".to_owned())?;
    let proof_der = STANDARD
        .decode(proof_b64)
        .map_err(|_| "invalid exporter_proof base64".to_owned())?;
    Ok(DecodedEnrollProofs {
        pub_key,
        pop_der,
        proof_der,
    })
}

fn wait_for_invitation_decision(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    office: &Office,
    invitation_id: &str,
    stop: &AtomicBool,
) {
    let deadline = Instant::now() + ENROLL_WAIT_MAX;
    while !stop.load(Ordering::Acquire) && Instant::now() < deadline {
        match office.poll_satellite_invitation(invitation_id) {
            Ok(InvitationPollStatus::Pending) => thread::sleep(POLL_SLEEP),
            Ok(InvitationPollStatus::Approved {
                cert_der,
                ca_cert_der,
                hq_office_id,
                epoch,
            }) => {
                let _ = write_sync_frame(
                    tls,
                    &SyncFrame::EnrollApproved {
                        cert_der_b64: STANDARD.encode(cert_der),
                        ca_cert_der_b64: STANDARD.encode(ca_cert_der),
                        hq_office_id,
                        epoch,
                    },
                );
                return;
            }
            Ok(InvitationPollStatus::Terminal { code, message }) => {
                let status = match code.as_str() {
                    "expired" => EnrollTerminalStatus::Expired,
                    "cancelled" => EnrollTerminalStatus::Cancelled,
                    _ => EnrollTerminalStatus::Rejected,
                };
                send_enroll_terminal(tls, status, message);
                return;
            }
            Err(_) => return,
        }
    }
}

pub(crate) struct HqSessionPeer<'a> {
    pub(crate) office: &'a Office,
    pub(crate) org_id: &'a OrgId,
    pub(crate) sat_id: &'a OfficeId,
    pub(crate) cert_serial_hex: &'a str,
    pub(crate) session_stop: &'a AtomicBool,
    pub(crate) server_stop: &'a AtomicBool,
}

fn check_not_revoked_fail_closed(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    office: &Office,
    satellite_office_id: &str,
    cert_serial_hex: &str,
) -> bool {
    match org_office::is_peer_revoked(office, satellite_office_id, Some(cert_serial_hex)) {
        Ok(false) => true,
        Ok(true) => {
            let _ = write_sync_frame(
                tls,
                &SyncFrame::Error {
                    code: "revoked".into(),
                    message: "office certificate revoked".into(),
                },
            );
            false
        }
        Err(_) => {
            let _ = write_sync_frame(
                tls,
                &SyncFrame::Error {
                    code: "unavailable".into(),
                    message: "revocation store unavailable".into(),
                },
            );
            false
        }
    }
}

pub(crate) fn write_sync_frame_if_peer_active(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    peer: &HqSessionPeer<'_>,
    frame: &SyncFrame,
) -> bool {
    if peer.session_stop.load(Ordering::Acquire) || peer.server_stop.load(Ordering::Acquire) {
        return false;
    }
    matches!(
        org_office::run_if_peer_active(
            peer.office,
            peer.sat_id.as_str(),
            Some(peer.cert_serial_hex),
            || write_sync_frame(tls, frame),
        ),
        Ok(Some(Ok(())))
    )
}

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

fn handle_hq_satellite_session(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    office: &Office,
    satellite_office_id: &str,
    cert_serial_hex: &str,
    stop: &AtomicBool,
    callbacks: InboundCallbacks<'_>,
    snapshot_budget: &HqSnapshotTransferBudget,
) {
    let Ok(org_id) = OrgId::new(&office.info().org_id) else {
        return;
    };
    let Ok(sat_id) = OfficeId::new(satellite_office_id) else {
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
    let mut reader = FrameReader::new();
    let mut sent_in_session = BTreeSet::new();
    let mut active_snapshot: Option<ActiveSnapshotTransfer> = None;
    // Complete `Hello` BEFORE pushing queued relays so the client's first reply is always `Hello` (P2-10).
    let mut hello_done = false;
    while !stop.load(Ordering::Acquire) && !session_stop.load(Ordering::Acquire) {
        // Expire idle staged snapshot transfers on every loop tick (including read timeouts) so
        // paused sessions cannot hold disk or budget slots indefinitely (`TASK-52` P21-#2).
        ActiveSnapshotTransfer::expire_if_idle(&mut active_snapshot);
        if !check_not_revoked_fail_closed(tls, office, satellite_office_id, cert_serial_hex) {
            return;
        }
        if hello_done
            && !reader.has_partial_frame()
            && !flush_durable_relays_to_satellite(tls, &peer, &mut sent_in_session)
        {
            return;
        }
        match reader.read_frame(tls, PARTIAL_FRAME_DEADLINE) {
            Ok(Some(frame)) => {
                // Re-check revocation at dispatch right after blocking read (`TASK-52` F10).
                if session_stop.load(Ordering::Acquire)
                    || !check_not_revoked_fail_closed(
                        tls,
                        office,
                        satellite_office_id,
                        cert_serial_hex,
                    )
                {
                    return;
                }
                let is_hello = matches!(frame, SyncFrame::Hello { .. });
                if !dispatch_hq_frame(
                    tls,
                    &peer,
                    snapshot_budget,
                    &mut active_snapshot,
                    frame,
                    callbacks,
                ) {
                    return;
                }
                if is_hello {
                    hello_done = true;
                }
            }
            Ok(None) => return,
            Err(FrameCodecError::Io(e))
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(_) => return,
        }
    }
}

fn dispatch_hq_frame(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    peer: &HqSessionPeer<'_>,
    snapshot_budget: &HqSnapshotTransferBudget,
    active_snapshot: &mut Option<ActiveSnapshotTransfer>,
    frame: SyncFrame,
    callbacks: InboundCallbacks<'_>,
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
        } => send_hq_org_events_after(tls, peer, after_epoch, after_seq),
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
            let _ = org_office::admit_relay_item_if_active(
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
            );
        }
    }
}

fn send_hq_org_events_after(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    peer: &HqSessionPeer<'_>,
    epoch: u64,
    seq: u64,
) -> bool {
    // Publish any accepted HQ `org-store` entries via the `ARCH-03` consumer before streaming (`TASK-52` F2).
    if peer.office.sync_org_journal_to_replication().is_err() {
        return false;
    }
    let Ok(events) = peer.office.replication().org_events_after(epoch, seq) else {
        return false;
    };
    for ev in events {
        let frame = SyncFrame::OrgEvent {
            org_id: ev.org_id.as_str().to_owned(),
            epoch: ev.position.epoch.get(),
            seq: ev.position.seq,
            previous_hex: ev.previous.to_hex(),
            digest_hex: ev.digest.to_hex(),
            codec_name: ev.codec.id().to_owned(),
            codec_version: ev.codec.version(),
            payload_b64: STANDARD.encode(&ev.payload),
        };
        // Fence every outbound org event against concurrent removal (`TASK-52` P20-#1).
        if !write_sync_frame_if_peer_active(tls, peer, &frame) {
            return false;
        }
    }
    write_sync_frame_if_peer_active(tls, peer, &SyncFrame::Pong { nonce: seq })
}

struct WorkChangeInput {
    org_id: String,
    origin_office: String,
    incarnation: String,
    seq: u64,
    record_id: String,
    generation: u64,
    version: u64,
    digest_hex: String,
    record_digest_hex: String,
    codec_name: String,
    codec_version: u32,
    payload_b64: String,
    required_org_epoch: u64,
    required_org_seq: u64,
    created_ms: u64,
}

fn verify_work_change_digests(
    sat_id: &OfficeId,
    inc: &OriginIncarnation,
    src_seq: SourceSeq,
    input: &WorkChangeInput,
    payload: &[u8],
) -> bool {
    let Ok(expected_digest) = Digest::parse_hex(&input.digest_hex) else {
        return false;
    };
    let Ok(expected_rec_digest) = Digest::parse_hex(&input.record_digest_hex) else {
        return false;
    };
    let Ok(codec) = Codec::new(&input.codec_name, input.codec_version) else {
        return false;
    };
    let Ok(req_epoch) = Epoch::new(input.required_org_epoch.max(1)) else {
        return false;
    };
    let Ok(actual_rec) = work_record_digest_v1(
        &input.record_id,
        input.generation,
        input.version,
        &codec,
        payload,
    ) else {
        return false;
    };
    if actual_rec != expected_rec_digest {
        return false;
    }
    let req_org = Position {
        epoch: req_epoch,
        seq: input.required_org_seq,
    };
    let actual_src =
        work_source_digest_v1(sat_id, inc, src_seq, actual_rec, req_org, input.created_ms);
    actual_src == expected_digest
}

struct ValidatedWorkChange {
    inc: OriginIncarnation,
    src_seq: SourceSeq,
    files: SnapshotFileBatch,
}

enum WorkChangeValidation {
    Valid(ValidatedWorkChange),
    ReplyError(&'static str, &'static str),
    CloseSession,
}

fn validate_hq_work_change(
    peer: &HqSessionPeer<'_>,
    input: &WorkChangeInput,
) -> WorkChangeValidation {
    if input.origin_office != peer.sat_id.as_str() || input.org_id != peer.org_id.as_str() {
        return WorkChangeValidation::ReplyError(
            "not_authorized",
            "work change origin does not match authenticated satellite",
        );
    }
    let (Ok(inc), Ok(src_seq), Ok(payload)) = (
        OriginIncarnation::new(&input.incarnation),
        SourceSeq::new(input.seq),
        STANDARD.decode(&input.payload_b64),
    ) else {
        return WorkChangeValidation::CloseSession;
    };
    if payload.len() > org_sync::organization::MAX_PAYLOAD_BYTES {
        return WorkChangeValidation::ReplyError(
            "invalid_request",
            "work change payload exceeds org-sync event limit",
        );
    }
    if !verify_work_change_digests(peer.sat_id, &inc, src_seq, input, &payload) {
        return WorkChangeValidation::ReplyError(
            "invalid_request",
            "work change record/source digest verification failed",
        );
    }
    let Ok(cp) = peer.office.replication().org_checkpoint() else {
        return WorkChangeValidation::CloseSession;
    };
    if input.required_org_epoch > cp.position.epoch.get()
        || (input.required_org_epoch == cp.position.epoch.get()
            && input.required_org_seq > cp.position.seq)
    {
        return WorkChangeValidation::ReplyError(
            "conflict",
            "work change depends on unreached HQ organization position",
        );
    }
    let Ok(delta) = serde_json::from_slice::<OrgDeltaPayload>(&payload) else {
        return WorkChangeValidation::ReplyError(
            "invalid_request",
            "work change payload must be a valid OrgDeltaPayload",
        );
    };
    let Ok(files) = delta.decode_files() else {
        return WorkChangeValidation::ReplyError(
            "invalid_request",
            "work change delta contains invalid paths or base64",
        );
    };
    WorkChangeValidation::Valid(ValidatedWorkChange {
        inc,
        src_seq,
        files,
    })
}

fn handle_hq_work_change(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    peer: &HqSessionPeer<'_>,
    input: WorkChangeInput,
) -> bool {
    let ValidatedWorkChange {
        inc,
        src_seq,
        files,
    } = match validate_hq_work_change(peer, &input) {
        WorkChangeValidation::Valid(v) => v,
        WorkChangeValidation::ReplyError(code, message) => {
            return write_sync_frame_if_peer_active(
                tls,
                peer,
                &SyncFrame::Error {
                    code: code.into(),
                    message: message.into(),
                },
            );
        }
        WorkChangeValidation::CloseSession => return false,
    };
    let origin = Origin {
        office: peer.sat_id.clone(),
        incarnation: inc.clone(),
    };
    let op_id = deterministic_uuid_v7(&format!(
        "work:{}:{}:{}:{}",
        peer.org_id.as_str(),
        peer.sat_id.as_str(),
        inc.as_str(),
        input.seq
    ));
    let ingest = match org_office::run_if_peer_active(
        peer.office,
        peer.sat_id.as_str(),
        Some(peer.cert_serial_hex),
        || {
            peer.office.ingest_validated_satellite_work(
                &origin,
                src_seq,
                &input.record_id,
                &op_id,
                files,
            )
        },
    ) {
        Ok(Some(res)) => res,
        _ => return false,
    };
    let prev_hw = match ingest {
        Ok(hw) => hw,
        Err(err) => {
            return write_sync_frame_if_peer_active(
                tls,
                peer,
                &SyncFrame::Error {
                    code: "not_authorized".into(),
                    message: err.to_string(),
                },
            );
        }
    };
    let prev = prev_hw.min(input.seq.saturating_sub(1));
    let ack = org_sync_proto::WorkAck {
        origin_office_id: peer.sat_id.clone(),
        incarnation: inc,
        from_exclusive: prev,
        through_seq: input.seq,
        rejected: Vec::new(),
    };
    let Ok(proof) = work_ack_proof_digest_v1(&ack) else {
        return false;
    };
    write_sync_frame_if_peer_active(
        tls,
        peer,
        &SyncFrame::WorkAck {
            origin_office: input.origin_office,
            incarnation: input.incarnation,
            seq: input.seq,
            proof_hex: proof.to_hex(),
        },
    )
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

fn forward_hq_relay_to_satellite(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    peer: &HqSessionPeer<'_>,
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
        Ok(org_office::RelayAdmission::Admitted) => {}
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

fn handle_hq_message_deliver(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    peer: &HqSessionPeer<'_>,
    callbacks: InboundCallbacks<'_>,
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
        return forward_hq_relay_to_satellite(tls, peer, msg);
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

struct InboundMessageReceipt {
    org_id: String,
    origin_office: String,
    destination_office: String,
    message_id: String,
    fingerprint_hex: String,
    deadline_ms: u64,
}

fn handle_hq_message_receipt_frame(
    office: &Office,
    org_id: &OrgId,
    sat_id: &OfficeId,
    delivered: bool,
    relayed_by_hq: bool,
    receipt: InboundMessageReceipt,
) -> bool {
    if delivered {
        return handle_hq_message_receipt(office, org_id, sat_id, receipt);
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
    receipt: InboundMessageReceipt,
) -> bool {
    if receipt.org_id != org_id.as_str() || receipt.destination_office != sat_id.as_str() {
        return true;
    }
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
    org_office::ack_message_relay_and_enqueue_receipt_checked(
        office,
        &receipt.destination_office,
        &receipt.origin_office,
        &receipt.message_id,
        &receipt.fingerprint_hex,
        forwarded,
    )
    .is_ok()
}

struct PermitSyncInput {
    request_id: String,
    connection_id: String,
    employee_id: String,
    task_id: Option<String>,
    model_id: String,
    is_failover_switch: bool,
}

fn handle_hq_permit_sync(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    peer: &HqSessionPeer<'_>,
    input: PermitSyncInput,
) -> bool {
    let Ok(Some(outcome)) = org_office::run_if_peer_active(
        peer.office,
        peer.sat_id.as_str(),
        Some(peer.cert_serial_hex),
        || {
            peer.office
                .acquire_turn_permit_for_office(org_office::OfficePermitSpec {
                    target_office_id: peer.sat_id.as_str(),
                    connection_id: &input.connection_id,
                    employee_id: &input.employee_id,
                    task_id: input.task_id.as_deref(),
                    model_id: &input.model_id,
                    authority: org_proto::providers::PermitAuthority::HqPool,
                    is_failover_switch: input.is_failover_switch,
                })
        },
    ) else {
        return false;
    };
    let resp = match outcome {
        Ok(Ok(org_office::providers::PermitAcquireOutcome::Granted(permit))) => {
            SyncFrame::PermitSyncResponse {
                request_id: input.request_id,
                granted: true,
                permit_id: Some(permit.id),
                coordinator_generation: Some(permit.coordinator_generation),
                reason: None,
                cooldown_until: None,
                occupied: None,
                max_capacity: None,
            }
        }
        Ok(Ok(org_office::providers::PermitAcquireOutcome::CoolingDown { cooldown_until })) => {
            SyncFrame::PermitSyncResponse {
                request_id: input.request_id,
                granted: false,
                permit_id: None,
                coordinator_generation: None,
                reason: Some(format!("cooling down until {cooldown_until}")),
                cooldown_until: Some(cooldown_until),
                occupied: None,
                max_capacity: None,
            }
        }
        Ok(Ok(org_office::providers::PermitAcquireOutcome::CapacityFull { occupied, max })) => {
            SyncFrame::PermitSyncResponse {
                request_id: input.request_id,
                granted: false,
                permit_id: None,
                coordinator_generation: None,
                reason: Some(format!("capacity full ({occupied}/{max})")),
                cooldown_until: None,
                occupied: Some(occupied),
                max_capacity: Some(max),
            }
        }
        Ok(Err(e)) => SyncFrame::PermitSyncResponse {
            request_id: input.request_id,
            granted: false,
            permit_id: None,
            coordinator_generation: None,
            reason: Some(e.message),
            cooldown_until: None,
            occupied: None,
            max_capacity: None,
        },
        Err(e) => SyncFrame::PermitSyncResponse {
            request_id: input.request_id,
            granted: false,
            permit_id: None,
            coordinator_generation: None,
            reason: Some(e.to_string()),
            cooldown_until: None,
            occupied: None,
            max_capacity: None,
        },
    };
    write_sync_frame_if_peer_active(tls, peer, &resp)
}

fn read_frame_with_deadline(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    reader: &mut FrameReader,
    stop: &AtomicBool,
    max_wait: Duration,
) -> Option<SyncFrame> {
    let deadline = Instant::now() + max_wait;
    while !stop.load(Ordering::Acquire) && Instant::now() < deadline {
        match reader.read_frame(tls, PARTIAL_FRAME_DEADLINE) {
            Ok(Some(f)) => return Some(f),
            Ok(None) => return None,
            Err(FrameCodecError::Io(e))
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(_) => return None,
        }
    }
    None
}

/// Output of a successful satellite enrollment exchange with HQ.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnrolledCertificateBundle {
    pub cert_der: Vec<u8>,
    pub ca_cert_der: Vec<u8>,
    pub hq_office_id: String,
    pub epoch: u64,
    pub check_code: String,
}

fn server_name_from_addr(addr: &str) -> Result<ServerName<'static>, String> {
    let stripped = addr
        .strip_prefix("https://")
        .or_else(|| addr.strip_prefix("http://"))
        .or_else(|| addr.strip_prefix("tls://"))
        .unwrap_or(addr);
    let host = if let Some(rest) = stripped.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        stripped.split(':').next().unwrap_or("")
    };
    ServerName::try_from(host.to_owned()).map_err(|_| format!("invalid HQ host `{host}`"))
}

/// Verifies HQ's TLS leaf certificate against the pinned Organization CA, org ID, HQ office ID,
/// and HQ role/epoch (`TASK-52` F17).
fn verify_hq_peer_leaf(
    tls: &StreamOwned<ClientConnection, TcpStream>,
    ca_cert_der: &[u8],
    expected_org_id: &str,
    expected_hq_office_id: &str,
    min_epoch: u64,
) -> Result<u64, String> {
    let leaf = tls
        .conn
        .peer_certificates()
        .and_then(|c| c.first())
        .ok_or_else(|| "HQ presented no TLS leaf certificate".to_owned())?;
    let verified = verify_office_cert(
        ca_cert_der,
        leaf.as_ref(),
        expected_org_id,
        None,
        &Default::default(),
        now_unix(),
    )
    .map_err(|e| format!("HQ leaf certificate verification failed: {e}"))?;
    if verified.office_id != expected_hq_office_id {
        return Err(format!(
            "HQ leaf office_id `{}` does not match pinned `{expected_hq_office_id}`",
            verified.office_id
        ));
    }
    match verified.role {
        OfficeCertRole::Hq { epoch } if epoch >= min_epoch => Ok(epoch),
        OfficeCertRole::Hq { epoch } => Err(format!(
            "HQ leaf epoch {epoch} is below pinned epoch {min_epoch}"
        )),
        OfficeCertRole::Satellite => Err("peer presented a satellite cert instead of HQ".into()),
    }
}

fn complete_client_handshake(
    tls: &mut StreamOwned<ClientConnection, TcpStream>,
) -> Result<(), String> {
    let deadline = Instant::now() + HANDSHAKE_DEADLINE;
    while tls.conn.is_handshaking() {
        if Instant::now() >= deadline {
            return Err("TLS handshake with HQ timed out".into());
        }
        match tls.conn.complete_io(&mut tls.sock) {
            Ok(_) => {}
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(e) => return Err(format!("TLS handshake with HQ failed: {e}")),
        }
    }
    Ok(())
}

fn read_client_frame(
    tls: &mut StreamOwned<ClientConnection, TcpStream>,
    reader: &mut FrameReader,
    wait: Duration,
) -> Result<Option<SyncFrame>, String> {
    let deadline = Instant::now() + wait;
    loop {
        match reader.read_frame(tls, PARTIAL_FRAME_DEADLINE) {
            Ok(frame) => return Ok(frame),
            Err(FrameCodecError::Io(e))
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                if Instant::now() >= deadline {
                    return Err("timed out waiting for HQ frame".into());
                }
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Performs satellite enrollment against HQ over Org-CA-pinned TLS using RFC 5705 exporter
/// binding + one-time invitation proof without the satellite's private key ever leaving it.
pub fn enroll_satellite_with_hq(
    token: &InvitationToken,
    local_tls_key: &SigningKey,
    on_check_code: impl FnOnce(&str),
) -> Result<EnrolledCertificateBundle, String> {
    let client_cfg = build_satellite_tls_client_config(&token.ca_cert_der, None)
        .map_err(|e| format!("failed building enrollment TLS config: {e}"))?;
    let (mut tls, verified_hq_epoch) =
        connect_verified_enrollment_hq(&token.hq_addresses, client_cfg, token)?;
    let exporter = export_enrollment_binding(&tls.conn)
        .map_err(|e| format!("TLS exporter binding failed: {e}"))?;
    let inv_key = token
        .signing_key()
        .map_err(|e| format!("invalid invitation signing key: {e}"))?;
    let pop = sign_satellite_key_pop(
        local_tls_key,
        &token.org_id,
        &token.office_id,
        &token.invitation_id,
    )
    .map_err(|e| format!("failed signing satellite key PoP: {e}"))?;
    let proof = sign_exporter_proof(
        &inv_key,
        &exporter,
        &token.invitation_id,
        &token.office_id,
        &local_tls_key.public_key(),
    )
    .map_err(|e| format!("failed signing exporter proof: {e}"))?;

    write_sync_frame(
        &mut tls,
        &SyncFrame::EnrollRequest {
            invitation_id: token.invitation_id.clone(),
            request_id: format!("req-{}", token.office_id),
            office_id: token.office_id.clone(),
            joining_pub_key_b64: STANDARD.encode(local_tls_key.public_key()),
            joining_key_pop_b64: STANDARD.encode(pop),
            exporter_proof_b64: STANDARD.encode(proof),
        },
    )
    .map_err(|e| format!("failed sending EnrollRequest: {e}"))?;

    let mut reader = FrameReader::new();
    let check_code = match read_client_frame(&mut tls, &mut reader, FIRST_FRAME_DEADLINE)? {
        Some(SyncFrame::EnrollPending { check_code, .. }) => {
            on_check_code(&check_code);
            check_code
        }
        Some(SyncFrame::EnrollApproved {
            cert_der_b64,
            ca_cert_der_b64,
            hq_office_id,
            epoch,
        }) => {
            return validate_enrolled_bundle(
                token,
                verified_hq_epoch,
                cert_der_b64,
                ca_cert_der_b64,
                hq_office_id,
                epoch,
                String::new(),
            );
        }
        Some(SyncFrame::EnrollTerminal { status, reason }) => {
            return Err(format!("enrollment refused ({status:?}): {reason}"));
        }
        other => return Err(format!("unexpected enrollment frame: {other:?}")),
    };

    match read_client_frame(&mut tls, &mut reader, ENROLL_WAIT_MAX)? {
        Some(SyncFrame::EnrollApproved {
            cert_der_b64,
            ca_cert_der_b64,
            hq_office_id,
            epoch,
        }) => validate_enrolled_bundle(
            token,
            verified_hq_epoch,
            cert_der_b64,
            ca_cert_der_b64,
            hq_office_id,
            epoch,
            check_code,
        ),
        Some(SyncFrame::EnrollTerminal { status, reason }) => {
            Err(format!("enrollment terminal ({status:?}): {reason}"))
        }
        other => Err(format!("unexpected final enrollment frame: {other:?}")),
    }
}

fn validate_enrolled_bundle(
    token: &InvitationToken,
    verified_hq_epoch: u64,
    cert_der_b64: String,
    ca_cert_der_b64: String,
    hq_office_id: String,
    epoch: u64,
    check_code: String,
) -> Result<EnrolledCertificateBundle, String> {
    let cert_der = STANDARD
        .decode(&cert_der_b64)
        .map_err(|_| "invalid cert_der base64".to_owned())?;
    let ca_cert_der = STANDARD
        .decode(&ca_cert_der_b64)
        .map_err(|_| "invalid ca_cert_der base64".to_owned())?;
    if ca_cert_der != token.ca_cert_der || hq_office_id != token.hq_office_id {
        return Err("EnrollApproved CA or HQ office ID does not match pinned invitation".into());
    }
    if epoch != verified_hq_epoch {
        return Err("EnrollApproved epoch does not match authenticated HQ leaf cert".into());
    }
    Ok(EnrolledCertificateBundle {
        cert_der,
        ca_cert_der,
        hq_office_id,
        epoch,
        check_code,
    })
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

    /// Reads the next correlated reply frame while buffering any unsolicited HQ-pushed relays
    /// (`MessageDeliver`, `WakeNotify`, or non-matching `MessageReceipt`) so queued relays never
    /// disrupt snapshot/gap/work/permit RPCs (`TASK-52` H-14).
    pub(crate) fn read_correlated_frame(
        &mut self,
        timeout: Duration,
        expected_receipt_msg_id: Option<&str>,
    ) -> Result<Option<SyncFrame>, String> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err("timed out waiting for HQ reply".into());
            }
            match read_client_frame(&mut self.tls, &mut self.reader, left)? {
                Some(frame @ (SyncFrame::MessageDeliver { .. } | SyncFrame::WakeNotify { .. })) => {
                    self.pending_inbound.push_back(frame);
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
                    self.pending_inbound.push_back(frame);
                }
                other => return Ok(other),
            }
        }
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
        let peer = StaticPeer(self.hq_peer.clone());
        let mut applied = 0usize;
        loop {
            match self.read_correlated_frame(FIRST_FRAME_DEADLINE, None)? {
                Some(SyncFrame::OrgEvent {
                    org_id,
                    epoch,
                    seq,
                    previous_hex,
                    digest_hex,
                    codec_name,
                    codec_version,
                    payload_b64,
                }) => {
                    let payload_bytes = STANDARD
                        .decode(&payload_b64)
                        .map_err(|_| "payload b64".to_owned())?;
                    let ev = OrgEvent {
                        org_id: OrgId::new(&org_id).map_err(|_| "org_id".to_owned())?,
                        position: Position {
                            epoch: Epoch::new(epoch).map_err(|_| "epoch".to_owned())?,
                            seq,
                        },
                        previous: Digest::parse_hex(&previous_hex)
                            .map_err(|_| "prev".to_owned())?,
                        digest: Digest::parse_hex(&digest_hex).map_err(|_| "digest".to_owned())?,
                        codec: Codec::new(&codec_name, codec_version)
                            .map_err(|_| "codec".to_owned())?,
                        payload: Arc::from(payload_bytes),
                    };
                    let cur = self
                        .office
                        .replication()
                        .org_checkpoint()
                        .map_err(|e| e.to_string())?;
                    if let Some(files) = verify_org_event_before_apply(&cur, &self.hq_peer, &ev)? {
                        let filtered = self
                            .office
                            .filter_self_echoed_org_event_files(files)
                            .map_err(|e| format!("filter self-echoed org event failed: {e}"))?;
                        if !filtered.is_empty() {
                            // Scope op ID to authenticated HQ office so D25 replacement HQ starting
                            // at epoch 1 never collides with old HQ receipts (`TASK-52` #7).
                            let op_id = deterministic_uuid_v7(&format!(
                                "org-event:{}:{}:{}:{}",
                                self.hq_peer.office_id.as_str(),
                                ev.org_id.as_str(),
                                ev.position.epoch.get(),
                                ev.position.seq
                            ));
                            self.office
                                .apply_replicated_state_files(&op_id, filtered)
                                .map_err(|e| format!("apply replicated delta failed: {e}"))?;
                        }
                    }
                    match apply_org_event(self.office.replication().as_ref(), &peer, ev)
                        .map_err(|e| format!("apply_org_event failed: {e}"))?
                    {
                        OrgOutput::Ack {
                            duplicate: false, ..
                        } => applied += 1,
                        OrgOutput::Ack {
                            duplicate: true, ..
                        } => {}
                        OrgOutput::Gap { .. } => {
                            return Err("org event gap detected; resync required".into());
                        }
                        OrgOutput::SnapshotRequired => {
                            return Err("snapshot required before applying org events".into());
                        }
                        other => return Err(format!("unexpected org apply outcome: {other:?}")),
                    }
                }
                Some(SyncFrame::Pong { .. }) => break,
                other => return Err(format!("unexpected frame during org event pull: {other:?}")),
            }
        }
        Ok(applied)
    }

    /// Pushes all pending local work outbox entries to HQ (draining any un-enqueued local satellite
    /// task journal entries first, `TASK-52` H-6) and verifies `WorkAck` proof before pruning.
    pub fn flush_work_outbox(&mut self) -> Result<usize, String> {
        let _ = self.office.sync_satellite_work_outbox();
        let outbox = self
            .office
            .replication()
            .load_work_outbox()
            .map_err(|e| e.to_string())?;
        let mut acked = 0usize;
        let mut prev_acked = outbox.acknowledged_seq();
        for change in outbox.pending() {
            let SourceKey { origin, seq } = &change.key;
            write_sync_frame(
                &mut self.tls,
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
            match self.read_correlated_frame(FIRST_FRAME_DEADLINE, None)? {
                Some(SyncFrame::WorkAck {
                    origin_office,
                    incarnation,
                    seq: ack_seq,
                    proof_hex,
                }) => {
                    if origin_office != origin.office.as_str()
                        || incarnation != origin.incarnation.as_str()
                        || ack_seq != seq.get()
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
                    self.office
                        .replication()
                        .ack_work_through(ack_seq)
                        .map_err(|e| format!("failed acking work outbox: {e}"))?;
                    prev_acked = ack_seq;
                    acked += 1;
                }
                other => return Err(format!("unexpected WorkAck reply: {other:?}")),
            }
        }
        Ok(acked)
    }

    /// Flushes all pending cross-office messages from the satellite's durable message outbox to HQ
    /// (draining any un-enqueued committed native messaging journal entries first, `TASK-52` #10)
    /// and matches returned delivery receipts against the original message (`TASK-52` F4 / P3-#8).
    pub fn flush_message_outbox(&mut self) -> Result<usize, String> {
        sync_native_message_relays(&self.office);
        let peer = StaticPeer(self.hq_peer.clone());
        expire_stale_non_close_head_messages(&self.office)?;
        let pending = self
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
                &mut self.tls,
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
            let reply = self.read_correlated_frame(FIRST_FRAME_DEADLINE, Some(&msg.id))?;
            delivered_count += apply_message_outbox_reply(&self.office, &peer, msg, reply)?;
        }
        Ok(delivered_count)
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
        let callbacks = InboundCallbacks {
            wake: wake_handler,
            notify: delivery_notifier,
        };
        let mut received = 0usize;
        let peer = StaticPeer(self.hq_peer.clone());
        loop {
            let next_frame = if let Some(buffered) = self.pending_inbound.pop_front() {
                Some(buffered)
            } else {
                match self
                    .reader
                    .read_frame(&mut self.tls, PARTIAL_FRAME_DEADLINE)
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
            match next_frame {
                Some(SyncFrame::MessageDeliver {
                    org_id,
                    origin_office,
                    destination_office,
                    message_id,
                    fingerprint_hex,
                    payload_b64,
                    deadline_ms,
                }) => {
                    if handle_inbound_relayed_message(
                        &mut self.tls,
                        &self.office,
                        &peer,
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
                    )? {
                        received += 1;
                    }
                }
                Some(SyncFrame::MessageReceipt {
                    org_id,
                    origin_office,
                    destination_office,
                    message_id,
                    fingerprint_hex,
                    delivered: true,
                    ..
                }) => {
                    if apply_inbound_relayed_receipt(
                        &self.office,
                        &peer,
                        &org_id,
                        &origin_office,
                        &destination_office,
                        &message_id,
                        &fingerprint_hex,
                    ) {
                        let _ = write_sync_frame(
                            &mut self.tls,
                            &SyncFrame::RelayAck {
                                relay_id: format!("receipt:{message_id}"),
                            },
                        );
                        received += 1;
                    }
                }
                Some(SyncFrame::WakeNotify {
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
                }) => {
                    let relay_id = format!("wake:{source_id}:{destination_office}");
                    // Only acknowledge the wake relay after the destination durably persists it (`TASK-52` #14).
                    let persisted = match wake_handler {
                        Some(cb) => cb(RelayedWake {
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
                        .is_ok(),
                        None => false,
                    };
                    if persisted {
                        let _ = write_sync_frame(&mut self.tls, &SyncFrame::RelayAck { relay_id });
                        received += 1;
                    }
                }
                Some(_) => {}
                None => break,
            }
        }
        Ok(received)
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

pub(crate) fn collect_pending_work_paths_checked(
    office: &Office,
) -> Result<BTreeSet<String>, String> {
    let mut keep = BTreeSet::new();
    let outbox = office
        .replication()
        .load_work_outbox()
        .map_err(|e| format!("failed loading local work outbox: {e}"))?;
    for change in outbox.pending() {
        let delta = serde_json::from_slice::<OrgDeltaPayload>(&change.payload)
            .map_err(|e| format!("malformed pending work delta in outbox: {e}"))?;
        for (path, _) in delta.files {
            keep.insert(path);
        }
    }
    Ok(keep)
}

fn verify_org_event_before_apply(
    cur: &org_sync::organization::OrgCheckpoint,
    hq_peer: &PeerIdentity,
    ev: &OrgEvent,
) -> Result<Option<SnapshotFileBatch>, String> {
    if ev.org_id != cur.org_id || hq_peer.org_id != cur.org_id {
        return Err("org event organization mismatch".into());
    }
    if ev.position.epoch < cur.position.epoch {
        return Err("stale org event epoch".into());
    }
    if ev.position.epoch == cur.position.epoch && ev.position.seq <= cur.position.seq {
        return Ok(None);
    }
    if ev.position.epoch != cur.position.epoch
        || ev.position.seq != cur.position.seq.saturating_add(1)
        || ev.previous != cur.digest
    {
        return Err("org event gap or predecessor mismatch".into());
    }
    let expected_digest = org_sync_proto::event_digest_v1(
        &ev.org_id,
        ev.position,
        ev.previous,
        &ev.codec,
        &ev.payload,
    )
    .map_err(|_| "invalid org event digest parameters".to_owned())?;
    if ev.digest != expected_digest {
        return Err("org event digest verification failed".into());
    }
    if ev.codec.id() == "org.native.changes" && ev.codec.version() == 1 {
        let delta: OrgDeltaPayload = serde_json::from_slice(&ev.payload)
            .map_err(|e| format!("malformed org.native.changes payload: {e}"))?;
        let files = delta.decode_files().map_err(|e| e.to_string())?;
        return Ok(Some(files));
    }
    Ok(None)
}

fn apply_native_relayed_message_if_applicable(
    office: &Office,
    org_id: &str,
    origin_office: &str,
    message_id: &str,
    payload: &[u8],
) -> Result<Option<org_office::RelayedMessageOutcome>, ()> {
    let Ok(delta) = serde_json::from_slice::<OrgDeltaPayload>(payload) else {
        return Ok(None);
    };
    if delta.v != 1 || delta.files.is_empty() {
        return Ok(None);
    }
    let Ok(files) = delta.decode_files() else {
        return Err(());
    };
    let op_id = deterministic_uuid_v7(&format!("msg:{org_id}:{origin_office}:{message_id}"));
    office
        .apply_relayed_messaging_files(origin_office, message_id, &op_id, files)
        .map(Some)
        .map_err(|_| ())
}

/// Derives and enqueues deduplicated local agent wakes at the destination office immediately
/// after an inbound relayed native message is durably committed (`TASK-52` P4-#8 / P5-#10).
/// Returns `true` iff every required local agent wake durably succeeded.
fn emit_local_wakes_for_delivered_message(
    wake_handler: Option<&WakeRelayHandler>,
    origin_office: &str,
    destination_office: &str,
    outcome: &org_office::RelayedMessageOutcome,
) -> bool {
    let Some(handler) = wake_handler else {
        return true;
    };
    if let Some(w) = &outcome.withdrawn_ask {
        if handler(RelayedWake {
            origin_office: origin_office.to_owned(),
            destination_office: destination_office.to_owned(),
            employee_id: w.asker_id.clone(),
            kind: "ask_withdrawal".to_owned(),
            source_id: format!("withdraw:{}:{}", w.ask_id, w.asker_id),
            sender_id: outcome.sender_id.clone(),
            sender_name: outcome.sender_name.clone(),
            sender_is_person: outcome.sender_is_person,
            conversation_id: Some(outcome.conversation_id.clone()),
            summary: String::new(),
            ask_answer: None,
            ask_withdrawal: Some(w.clone()),
        })
        .is_err()
        {
            return false;
        }
    }
    if let Some(ans) = &outcome.answered_ask {
        if handler(RelayedWake {
            origin_office: origin_office.to_owned(),
            destination_office: destination_office.to_owned(),
            employee_id: ans.asker_id.clone(),
            kind: "ask_answer".to_owned(),
            source_id: format!("ans:{}:{}", ans.ask_id, ans.asker_id),
            sender_id: outcome.sender_id.clone(),
            sender_name: outcome.sender_name.clone(),
            sender_is_person: outcome.sender_is_person,
            conversation_id: Some(outcome.conversation_id.clone()),
            summary: ans.answer_summary.clone(),
            ask_answer: Some(ans.clone()),
            ask_withdrawal: None,
        })
        .is_err()
        {
            return false;
        }
    }
    if outcome.message_id.is_empty() || outcome.local_recipient_wakes.is_empty() {
        return true;
    }
    let answered_asker = outcome.answered_ask.as_ref().map(|a| a.asker_id.as_str());
    for wake in &outcome.local_recipient_wakes {
        if answered_asker == Some(wake.employee_id.as_str()) {
            continue;
        }
        if handler(RelayedWake {
            origin_office: origin_office.to_owned(),
            destination_office: destination_office.to_owned(),
            employee_id: wake.employee_id.clone(),
            kind: wake.kind.clone(),
            source_id: format!("{}:{}", outcome.message_id, wake.employee_id),
            sender_id: outcome.sender_id.clone(),
            sender_name: outcome.sender_name.clone(),
            sender_is_person: outcome.sender_is_person,
            conversation_id: Some(outcome.conversation_id.clone()),
            summary: outcome.summary.clone(),
            ask_answer: None,
            ask_withdrawal: None,
        })
        .is_err()
        {
            return false;
        }
    }
    true
}

/// Performs verified enrollment TCP + TLS + pinned HQ leaf verification on a single resolved IP
/// while preserving the hostname for SNI/certificate verification (`TASK-52` P4-#12).
fn try_enroll_sock_addr(
    sock_addr: std::net::SocketAddr,
    server_name: rustls::pki_types::ServerName<'static>,
    client_cfg: Arc<rustls::ClientConfig>,
    token: &InvitationToken,
) -> Result<(StreamOwned<ClientConnection, TcpStream>, u64), String> {
    let tcp = TcpStream::connect_timeout(&sock_addr, CONNECT_TIMEOUT)
        .map_err(|e| format!("TCP connect {sock_addr}: {e}"))?;
    let _ = tcp.set_read_timeout(Some(SOCKET_READ_TIMEOUT));
    let _ = tcp.set_write_timeout(Some(SOCKET_WRITE_TIMEOUT));
    let conn = ClientConnection::new(client_cfg, server_name)
        .map_err(|e| format!("failed creating TLS client connection: {e}"))?;
    let mut tls = StreamOwned::new(conn, tcp);
    complete_client_handshake(&mut tls)?;
    let verified_hq_epoch = verify_hq_peer_leaf(
        &tls,
        &token.ca_cert_der,
        &token.org_id,
        &token.hq_office_id,
        1,
    )?;
    Ok((tls, verified_hq_epoch))
}

/// Iterates every invitation `hq_addresses` entry AND every resolved `SocketAddr` for multi-IP
/// hostnames through full TCP + TLS handshake + pinned HQ peer-leaf verification (`TASK-52` P3-#13 / P4-#12).
fn connect_verified_enrollment_hq(
    hq_addresses: &[String],
    client_cfg: Arc<rustls::ClientConfig>,
    token: &InvitationToken,
) -> Result<(StreamOwned<ClientConnection, TcpStream>, u64), String> {
    let mut last_err = String::from("no HQ addresses configured in invitation");
    for addr in hq_addresses {
        let Ok(server_name) = server_name_from_addr(addr) else {
            last_err = format!("{addr}: invalid server name");
            continue;
        };
        let Ok(resolved) = addr.to_socket_addrs() else {
            last_err = format!("{addr}: DNS resolution failed");
            continue;
        };
        for sock_addr in resolved {
            match try_enroll_sock_addr(
                sock_addr,
                server_name.clone(),
                Arc::clone(&client_cfg),
                token,
            ) {
                Ok(pair) => return Ok(pair),
                Err(err) => last_err = format!("{addr} ({sock_addr}): {err}"),
            }
        }
    }
    Err(format!(
        "failed verified enrollment TLS handshake to all HQ addresses: {last_err}"
    ))
}

/// Attempts full TCP + Org-CA mTLS + leaf identity + `Hello` against every resolved IP of a
/// single HQ endpoint while preserving the hostname for certificate verification (`TASK-52` #17 / P4-#12).
fn try_connect_hq_endpoint(
    office: Arc<Office>,
    ident: &org_office::SatelliteTlsIdentity,
    cfg: Arc<rustls::ClientConfig>,
    cp: &org_sync::organization::OrgCheckpoint,
    addr: &str,
    stop: Option<&AtomicBool>,
) -> Result<SatelliteSyncSession, String> {
    let server_name = server_name_from_addr(addr)?;
    let resolved: Vec<std::net::SocketAddr> = addr
        .to_socket_addrs()
        .map_err(|e| format!("DNS resolution for {addr}: {e}"))?
        .collect();
    let mut last_err = format!("no resolved addresses for {addr}");
    for sock_addr in resolved {
        if stop.is_some_and(|s| s.load(Ordering::Acquire)) {
            return Err("federation connect cancelled by shutdown".into());
        }
        match try_connect_hq_sock_addr(
            Arc::clone(&office),
            ident,
            Arc::clone(&cfg),
            cp,
            sock_addr,
            server_name.clone(),
        ) {
            Ok(session) => return Ok(session),
            Err(err) => last_err = format!("{addr} ({sock_addr}): {err}"),
        }
    }
    Err(last_err)
}

fn try_connect_hq_sock_addr(
    office: Arc<Office>,
    ident: &org_office::SatelliteTlsIdentity,
    cfg: Arc<rustls::ClientConfig>,
    cp: &org_sync::organization::OrgCheckpoint,
    sock_addr: std::net::SocketAddr,
    server_name: rustls::pki_types::ServerName<'static>,
) -> Result<SatelliteSyncSession, String> {
    let tcp = TcpStream::connect_timeout(&sock_addr, CONNECT_TIMEOUT)
        .map_err(|e| format!("TCP connect: {e}"))?;
    let _ = tcp.set_read_timeout(Some(SOCKET_READ_TIMEOUT));
    let _ = tcp.set_write_timeout(Some(SOCKET_WRITE_TIMEOUT));
    let conn =
        ClientConnection::new(cfg, server_name).map_err(|e| format!("mTLS client init: {e}"))?;
    let mut tls = StreamOwned::new(conn, tcp);
    complete_client_handshake(&mut tls)?;
    let verified_hq_epoch = verify_hq_peer_leaf(
        &tls,
        &ident.ca_cert_der,
        &office.info().org_id,
        &ident.federation.hq_office_id,
        ident.federation.epoch,
    )?;
    // Verify TLS peer HQ ID and CA digest match the durable replication checkpoint BEFORE sending
    // Hello or installing any snapshot so a crash mid-re-enrollment can never restore trust in an
    // old HQ (`TASK-52` P9-#2).
    let ident_ca_digest = Digest::from_bytes(Sha256::digest(&ident.ca_cert_der).into());
    if ident.federation.hq_office_id != cp.hq_office.as_str() || ident_ca_digest != cp.ca {
        return Err(
            "satellite TLS identity does not match durable checkpoint HQ/CA; finish re-enrollment"
                .into(),
        );
    }
    write_sync_frame(
        &mut tls,
        &SyncFrame::Hello {
            org_id: office.info().org_id.clone(),
            office_id: office.info().office_id.clone(),
            role: WireOfficeRole::Satellite,
            epoch: cp.position.epoch.get(),
            protocol_level: cp.protocol_level,
            supported_max: 1,
            codec_max: 1,
            hq_addresses: Vec::new(),
        },
    )
    .map_err(|e| format!("failed sending Hello: {e}"))?;
    let mut reader = FrameReader::new();
    let hello_reply = read_client_frame(&mut tls, &mut reader, FIRST_FRAME_DEADLINE)?;
    let (hq_office_id, hq_epoch) = match hello_reply {
        Some(SyncFrame::Hello {
            org_id,
            office_id,
            role,
            epoch,
            ..
        }) => {
            if org_id != office.info().org_id
                || office_id != ident.federation.hq_office_id
                || role != WireOfficeRole::Hq
                || epoch != verified_hq_epoch
            {
                return Err("HQ Hello identity does not match verified TLS leaf".into());
            }
            (office_id, epoch)
        }
        Some(SyncFrame::Error { code, message }) => {
            return Err(format!("HQ rejected mTLS session ({code}): {message}"));
        }
        other => return Err(format!("unexpected Hello reply: {other:?}")),
    };
    let hq_peer = PeerIdentity {
        org_id: OrgId::new(&office.info().org_id).map_err(|_| "invalid org_id".to_owned())?,
        office_id: OfficeId::new(&hq_office_id).map_err(|_| "invalid hq_office_id".to_owned())?,
        role: PeerRole::Hq,
        authority: Epoch::new(hq_epoch.max(1)).map_err(|_| "invalid hq_epoch".to_owned())?,
    };
    Ok(SatelliteSyncSession {
        office,
        tls,
        reader,
        hq_peer,
        pending_inbound: VecDeque::new(),
    })
}

/// Expires stale non-`close-` head entries in the satellite message outbox while preserving
/// authoritative ask closes/withdrawals (`close-*`) indefinitely (`TASK-52` P12-#5).
fn expire_stale_non_close_head_messages(office: &Office) -> Result<(), String> {
    loop {
        let head_is_close = office
            .replication()
            .message_queue_snapshot()
            .ok()
            .and_then(|q| q.pending.front().map(|m| m.id.starts_with("close-")))
            .unwrap_or(false);
        if head_is_close {
            break;
        }
        match next_message(office.replication().as_ref(), now_ms())
            .map_err(|e| format!("message outbox error: {e}"))?
        {
            MessageOutput::Receipt(_) => continue,
            MessageOutput::Idle | MessageOutput::Deliver(_) => break,
            other => return Err(format!("unexpected message outbox state: {other:?}")),
        }
    }
    Ok(())
}

fn apply_message_outbox_reply(
    office: &Office,
    peer: &StaticPeer,
    msg: Message,
    reply: Option<SyncFrame>,
) -> Result<usize, String> {
    match reply {
        Some(SyncFrame::MessageReceipt {
            org_id,
            origin_office,
            destination_office,
            message_id,
            fingerprint_hex,
            delivered,
            relayed_by_hq,
            ..
        }) => {
            let matches_sent = org_id == msg.org_id.as_str()
                && origin_office == msg.origin.as_str()
                && destination_office == msg.destination.as_str()
                && message_id == msg.id
                && fingerprint_hex == msg.fingerprint.to_hex();
            if !matches_sent {
                return Err("MessageReceipt does not match sent message".into());
            }
            if delivered {
                acknowledge(
                    office.replication().as_ref(),
                    peer,
                    Receipt {
                        message: msg,
                        delivered: true,
                    },
                )
                .map_err(|e| format!("failed applying message receipt: {e}"))?;
                Ok(1)
            } else if relayed_by_hq {
                Ok(0)
            } else {
                Err("HQ returned non-delivered message receipt".into())
            }
        }
        Some(SyncFrame::Error { code, .. }) if code == "destination_busy" => Ok(0),
        Some(SyncFrame::Error { code, .. }) if code == "destination_removed" => {
            // Destination office was permanently removed at HQ; retire the undeliverable outbox
            // entry durably so non-expiring closes never retry forever (`TASK-52` P15-#4).
            let _ = office
                .replication()
                .retire_undeliverable_message(&msg.id, msg.destination.as_str());
            Ok(0)
        }
        other => Err(format!("unexpected message receipt: {other:?}")),
    }
}

/// Validates and delivers an inbound cross-office message safely on both HQ and satellites:
/// short-circuits exact replays, refuses native digest mismatches WITHOUT recording `delivered: true`,
/// persists local wakes before recording terminal receipt, and notifies local waiters/live streams
/// (`TASK-52` P5-#10 / P6-#11 / P7-#10, `TASK-117`).
fn deliver_inbound_message_checked(
    office: &Office,
    peer: &StaticPeer,
    callbacks: InboundCallbacks<'_>,
    msg: &InboundMessage,
    mut sync_msg: Message,
    payload: &[u8],
    fp: Digest,
) -> bool {
    let now = now_ms();
    if office
        .replication()
        .has_received_message_receipt(&msg.origin_office, &msg.message_id, &msg.fingerprint_hex)
        .unwrap_or(false)
    {
        return true;
    }
    let clock_high = office
        .replication()
        .message_queue_snapshot()
        .map_or(0, |q| q.clock_high_ms)
        .max(now);
    // Authoritative ask closes/recoveries (`close-*`) retain identity independently of transport
    // deadlines so a deadline crossing or refresh never wedges delivery on expired-envelope
    // divergence (`TASK-52` P14-#1).
    if msg.message_id.starts_with("close-") {
        sync_msg.deadline_ms = sync_msg
            .deadline_ms
            .max(clock_high.saturating_add(7 * 24 * 60 * 60 * 1000));
    } else if clock_high >= sync_msg.deadline_ms {
        let _ = receive(office.replication().as_ref(), peer, sync_msg, now);
        return false;
    }
    if !validate_inbound_message_before_native_apply(office, &sync_msg, payload, fp, now) {
        // Never invoke `receive` on a corrupt/digest-mismatched native payload, or `org-sync`
        // could persist `delivered: true` for an empty inbox (`TASK-52` P7-#10).
        return false;
    }
    let Ok(outcome) = apply_native_relayed_message_if_applicable(
        office,
        &msg.org_id,
        &msg.origin_office,
        &msg.message_id,
        payload,
    ) else {
        return false;
    };
    let wakes_ok = outcome.as_ref().is_none_or(|info| {
        emit_local_wakes_for_delivered_message(
            callbacks.wake,
            &msg.origin_office,
            &office.info().office_id,
            info,
        )
    });
    let delivered = wakes_ok
        && matches!(
            receive(office.replication().as_ref(), peer, sync_msg, now),
            Ok(MessageOutput::Receipt(Receipt {
                delivered: true,
                ..
            }))
        );
    if delivered {
        if let (Some(notify), Some(info)) = (callbacks.notify, outcome.as_ref()) {
            notify(info);
        }
    }
    delivered
}

fn validate_inbound_message_before_native_apply(
    office: &Office,
    sync_msg: &Message,
    payload: &[u8],
    fp: Digest,
    now_ms: u64,
) -> bool {
    if sync_msg.validate().is_err() {
        return false;
    }
    let clock_high = office
        .replication()
        .message_queue_snapshot()
        .map_or(0, |q| q.clock_high_ms)
        .max(now_ms);
    if clock_high >= sync_msg.deadline_ms {
        return false;
    }
    if is_native_messaging_delta(payload)
        && Digest::from_bytes(Sha256::digest(payload).into()) != fp
    {
        return false;
    }
    true
}

fn is_native_messaging_delta(payload: &[u8]) -> bool {
    serde_json::from_slice::<OrgDeltaPayload>(payload).is_ok_and(|d| {
        !d.files.is_empty()
            && d.files
                .iter()
                .all(|(p, _)| p.starts_with("messaging/conversations/"))
    })
}

fn build_native_relay_targets(
    office: &Office,
    org_id: &OrgId,
    local_id: &OfficeId,
    message_id: &str,
    destinations: Vec<String>,
    payload_bytes: &[u8],
    deadline_ms: u64,
) -> (Vec<Message>, Vec<StoredRelayItem>) {
    let fp = Digest::from_bytes(Sha256::digest(payload_bytes).into());
    let mut messages = Vec::new();
    let mut relays = Vec::new();
    if office.info().role == org_office::OfficeRole::Satellite {
        let multi_dest = destinations.len() > 1;
        for target_id in destinations {
            let Ok(dest_office) = OfficeId::new(&target_id) else {
                continue;
            };
            let wire_id = if multi_dest {
                format!("{message_id}_{target_id}")
            } else {
                message_id.to_owned()
            };
            messages.push(Message {
                org_id: org_id.clone(),
                origin: local_id.clone(),
                destination: dest_office,
                id: wire_id,
                fingerprint: fp,
                payload: Arc::from(payload_bytes),
                deadline_ms,
            });
        }
    } else {
        let payload_b64 = STANDARD.encode(payload_bytes);
        for target_id in destinations {
            let frame = SyncFrame::MessageDeliver {
                org_id: office.info().org_id.clone(),
                origin_office: office.info().office_id.clone(),
                destination_office: target_id.clone(),
                message_id: message_id.to_owned(),
                fingerprint_hex: fp.to_hex(),
                payload_b64: payload_b64.clone(),
                deadline_ms,
            };
            if let Ok(payload_json) = serde_json::to_string(&frame) {
                relays.push(StoredRelayItem {
                    id: format!("{message_id}:{target_id}"),
                    origin_office: office.info().office_id.clone(),
                    destination_office: target_id,
                    fingerprint_hex: fp.to_hex(),
                    kind: "message".into(),
                    payload_json,
                    expires_at_ms: if message_id.starts_with("close-") {
                        0
                    } else {
                        deadline_ms
                    },
                });
            }
        }
    }
    (messages, relays)
}

type DestRelayBuckets = std::collections::BTreeMap<String, (Vec<Message>, Vec<StoredRelayItem>)>;

fn group_ordinal_relays_by_dest(
    office: &Office,
    org_id: &OrgId,
    local_id: &OfficeId,
    items: Vec<org_office::PendingMessageJournalEntry>,
    now: u64,
) -> DestRelayBuckets {
    let default_deadline_ms = now.saturating_add(7 * 24 * 60 * 60 * 1000);
    let max_deadline_ms = now.saturating_add(org_sync::messages::MAX_MESSAGE_TTL_MS);
    let mut by_dest: DestRelayBuckets = std::collections::BTreeMap::new();
    for item in items {
        let deadline_ms = if item.message_id.starts_with("close-") {
            max_deadline_ms
        } else {
            item.expires_at_ms
                .unwrap_or(default_deadline_ms)
                .min(max_deadline_ms)
        };
        if item.files.is_empty() || item.destination_offices.is_empty() || deadline_ms <= now {
            continue;
        }
        let Ok(payload_bytes) = serde_json::to_vec(&OrgDeltaPayload {
            v: 1,
            files: item.files,
        }) else {
            continue;
        };
        if payload_bytes.len() > org_sync::messages::MAX_PAYLOAD_BYTES {
            continue;
        }
        let (msgs, rels) = build_native_relay_targets(
            office,
            org_id,
            local_id,
            &item.message_id,
            item.destination_offices,
            &payload_bytes,
            deadline_ms,
        );
        for m in msgs {
            by_dest
                .entry(m.destination.as_str().to_owned())
                .or_default()
                .0
                .push(m);
        }
        for r in rels {
            by_dest
                .entry(r.destination_office.clone())
                .or_default()
                .1
                .push(r);
        }
    }
    by_dest
}

/// Crash-safe, retry-safe `ARCH-03` journal publisher for native cross-office chat/ask/answer
/// messages (`TASK-52` #9, #10 & P3-#1 / P7-#1): groups all relay parts belonging to the same
/// journal ordinal (such as split `conversation.json` + `message.json`) into ONE atomic publish.
pub fn sync_native_message_relays(office: &Office) {
    if !office.is_multi_office().unwrap_or(false) {
        return;
    }
    let _ = org_office::promote_pending_removed_origin_withdrawals(office);
    let Ok(batch) = office.collect_pending_message_journal_entries() else {
        return;
    };
    let incarnation = batch.incarnation;
    let last_ord = batch.last_ordinal;
    let Ok(org_id) = OrgId::new(&office.info().org_id) else {
        return;
    };
    let Ok(local_id) = OfficeId::new(&office.info().office_id) else {
        return;
    };
    let now = now_ms();
    let mut by_ordinal: std::collections::BTreeMap<u64, Vec<_>> = std::collections::BTreeMap::new();
    for item in batch.entries {
        by_ordinal.entry(item.ordinal).or_default().push(item);
    }
    let mut completed_ord = 0u64;
    let mut blocked_dests: BTreeSet<String> = BTreeSet::new();
    for (ord, items) in by_ordinal {
        let by_dest = group_ordinal_relays_by_dest(office, &org_id, &local_id, items, now);
        // Publish per destination; once a destination fails an ordinal, stop advancing THAT
        // destination at later ordinals while continuing to publish for healthy destinations
        // (`TASK-52` P8-#7 / P9-#1).
        let mut ord_all_ok = true;
        for (dest, (dest_msgs, dest_rels)) in by_dest {
            if blocked_dests.contains(&dest) {
                ord_all_ok = false;
                continue;
            }
            if org_office::publish_message_journal_relay_for_dest_checked(
                office,
                &dest,
                dest_msgs,
                dest_rels,
                &incarnation,
                ord,
                now,
            )
            .is_err()
            {
                blocked_dests.insert(dest);
                ord_all_ok = false;
            }
        }
        if ord_all_ok && blocked_dests.is_empty() {
            completed_ord = completed_ord.max(ord);
        }
    }
    let advance_to = if !blocked_dests.is_empty() {
        completed_ord
    } else {
        completed_ord.max(last_ord)
    };
    if advance_to > 0 {
        let _ = office.advance_message_relay_consumer(advance_to);
    }
}

/// Production satellite adapter connecting `org-office-sessions`'s router to HQ's shared `HqPool`
/// permit coordinator over Org-CA mTLS (`TASK-52` M-21).
pub struct SatelliteHqPermitCoordinator {
    office: Arc<Office>,
}

impl SatelliteHqPermitCoordinator {
    pub fn new(office: Arc<Office>) -> Self {
        Self { office }
    }
}

impl org_office_sessions::HqPermitCoordinator for SatelliteHqPermitCoordinator {
    fn acquire_permit(
        &self,
        req: org_office_sessions::HqPermitAcquireSpec,
    ) -> Result<org_office::providers::PermitAcquireOutcome, String> {
        let mut session = SatelliteSyncSession::connect(Arc::clone(&self.office))?;
        let resp = session.request_hq_permit(HqPermitRequest {
            request_id: self.office.next_uuid_v7(),
            connection_id: req.connection_id.clone(),
            employee_id: req.employee_id.clone(),
            task_id: req.task_id.clone(),
            model_id: req.model_id.clone(),
            is_failover_switch: req.is_failover_switch,
        })?;
        if resp.granted {
            let permit_id = resp
                .permit_id
                .ok_or_else(|| "HQ granted permit without permit_id".to_owned())?;
            let now_rfc = org_office::format_rfc3339_ms(now_ms());
            return Ok(org_office::providers::PermitAcquireOutcome::Granted(
                org_office::providers::TurnPermitRecord {
                    organization_id: self.office.info().org_id.clone(),
                    id: permit_id,
                    connection_id: req.connection_id,
                    office_id: self.office.info().office_id.clone(),
                    employee_id: req.employee_id,
                    placement_generation: 1,
                    model_id: req.model_id,
                    authority: org_proto::providers::PermitAuthority::HqPool,
                    state: org_proto::providers::TurnPermitState::Active,
                    issued_at: now_rfc.clone(),
                    expires_at: now_rfc,
                    coordinator_generation: resp.coordinator_generation.unwrap_or(1),
                },
            ));
        }
        if let Some(until) = resp.cooldown_until {
            return Ok(org_office::providers::PermitAcquireOutcome::CoolingDown {
                cooldown_until: until,
            });
        }
        Ok(org_office::providers::PermitAcquireOutcome::CapacityFull {
            occupied: resp.occupied.unwrap_or(1),
            max: resp.max_capacity.unwrap_or(1),
        })
    }

    fn renew_permit(&self, permit_id: &str, coordinator_generation: u64) -> Result<bool, String> {
        let mut session = SatelliteSyncSession::connect(Arc::clone(&self.office))?;
        session.renew_hq_permit(
            self.office.next_uuid_v7(),
            permit_id.to_owned(),
            coordinator_generation,
        )
    }

    fn release_permit(&self, permit_id: &str) -> Result<(), String> {
        let mut session = SatelliteSyncSession::connect(Arc::clone(&self.office))?;
        session.release_hq_permit(permit_id.to_owned())
    }

    fn report_cooldown(
        &self,
        connection_id: &str,
        resets_at: Option<&str>,
        reason: Option<&str>,
    ) -> Result<(), String> {
        let mut session = SatelliteSyncSession::connect(Arc::clone(&self.office))?;
        session.report_hq_cooldown(
            connection_id.to_owned(),
            resets_at.map(str::to_owned),
            reason.map(str::to_owned),
        )
    }
}
