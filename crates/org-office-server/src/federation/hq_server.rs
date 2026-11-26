//! The HQ federation listener: connection permits, the accept loop, the server TLS handshake, and
//! per-connection routing to enrollment or a satellite sync session.

use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::federation_stream::{HqFederationHandle, HqPeerRegistry};
use crate::org_feed::FeedWake;
use crate::snapshot_transfer::{self, HqSnapshotTransferBudget};
use org_office::replication::StoredRelayItem;
use org_office::Office;
use org_pki::cert::{build_hq_tls_dual_server_config, verify_office_cert, OfficeCertRole};
use org_sync_proto::{write_sync_frame, SyncFrame};
use rustls::{ServerConnection, StreamOwned};

use super::enrollment::handle_hq_enrollment;
use super::hq_session::handle_hq_satellite_session;
use super::{
    now_ms, now_unix, InboundCallbacks, InboundDeliveryNotifier, WakeRelayHandler,
    HANDSHAKE_DEADLINE, MAX_CONCURRENT_CONNECTIONS, POLL_SLEEP, SOCKET_READ_TIMEOUT,
    SOCKET_WRITE_TIMEOUT, WAKE_RELAY_TTL_MS,
};

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

/// Running HQ federation TLS listener serving enrollment + mTLS sync & durable cross-office relay.
pub struct HqFederationServer {
    addr: SocketAddr,
    office: Arc<Office>,
    peers: Arc<HqPeerRegistry>,
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
        Self::start_with_hooks(
            office,
            bind_addr,
            san_hosts,
            wake_handler,
            delivery_notifier,
            None,
        )
    }

    /// Starts HQ's Org-CA TLS federation listener with durable wake, delivery notification, and
    /// local SSE `FeedWake` callbacks (`TASK-117`, `TASK-140`).
    pub(crate) fn start_with_hooks(
        office: Arc<Office>,
        bind_addr: SocketAddr,
        san_hosts: &[String],
        wake_handler: Option<WakeRelayHandler>,
        delivery_notifier: Option<InboundDeliveryNotifier>,
        feed_wake: Option<Arc<FeedWake>>,
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
        let peers = Arc::new(HqPeerRegistry::default());
        let stop_bg = Arc::clone(&stop);
        let office_bg = Arc::clone(&office);
        let workers_bg = Arc::clone(&workers);
        let peers_bg = Arc::clone(&peers);
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
                        feed_wake,
                        peers: peers_bg,
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
            peers,
            stop,
            handle: Some(handle),
            workers,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Returns a cloneable handle for pushing immediate sync wakeups and proxying remote
    /// satellite session-control RPCs (`TASK-140`, `TASK-143` AC #3).
    pub fn handle(&self) -> HqFederationHandle {
        HqFederationHandle::new(Arc::clone(&self.peers))
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
            org_office::RelayAdmission::Admitted => {
                self.peers.notify_peer(destination_office_id);
                Ok(())
            }
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
        self.peers.notify_all();
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
    feed_wake: Option<Arc<FeedWake>>,
    peers: Arc<HqPeerRegistry>,
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
                let _ = stream.set_nodelay(true);
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
                    feed_wake: ctx.feed_wake.as_deref(),
                },
                &ctx.peers,
                &ctx.snapshot_budget,
            );
        }
    }
}
