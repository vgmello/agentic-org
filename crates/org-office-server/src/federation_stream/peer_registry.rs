//! The HQ's registry of connected satellite sessions: per-peer proxied session RPC queues, peer
//! registrations, and the federation handle the office holds.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use org_proto::{ApiError, ErrorCode, Outcome, Request, Response};
use org_sync_proto::SyncFrame;

use super::{is_proxied_session_method, WakePipe, MAX_INFLIGHT_PROXIED_RPCS};

/// Outbound HQ->Satellite proxied session RPC awaiting transmission and reply.
pub(crate) struct PendingSessionRpc {
    pub(crate) request_id: String,
    pub(crate) caller_member_id: String,
    pub(crate) caller_is_owner: bool,
    pub(crate) method: String,
    pub(crate) params_json: String,
    pub(crate) deadline: Instant,
    pub(crate) reply: mpsc::Sender<Result<Outcome, ApiError>>,
}

#[derive(Default)]
struct PeerRpcQueue {
    closed: bool,
    active_inflight: usize,
    inbox: VecDeque<PendingSessionRpc>,
}

/// Live HQ-side wakeup and RPC queue handle for one connected satellite mTLS session.
pub(crate) struct HqPeerHandle {
    pub(crate) wake: WakePipe,
    wake_gen: AtomicU64,
    rpc_queue: Mutex<PeerRpcQueue>,
    /// Set once the session, after its `Hello`, asked for the organization stream
    /// (`SyncFrame::OrgGap`) and was sent it: it is the satellite's persistent sync session,
    /// not a short-lived permit or RPC connection (`TASK-174` AC #1).
    persistent: AtomicBool,
}

impl HqPeerHandle {
    fn new() -> io::Result<Self> {
        Ok(Self {
            wake: WakePipe::new()?,
            wake_gen: AtomicU64::new(0),
            rpc_queue: Mutex::new(PeerRpcQueue::default()),
            persistent: AtomicBool::new(false),
        })
    }

    /// Marks this session as the satellite's persistent sync session (`TASK-174` AC #1).
    pub(crate) fn mark_persistent(&self) {
        self.persistent.store(true, Ordering::Release);
    }

    pub(crate) fn is_persistent(&self) -> bool {
        self.persistent.load(Ordering::Acquire)
    }

    pub(crate) fn notify_wake(&self) {
        self.wake_gen.fetch_add(1, Ordering::Release);
        self.wake.notify();
    }

    pub(crate) fn wake_generation(&self) -> u64 {
        self.wake_gen.load(Ordering::Acquire)
    }

    pub(super) fn enqueue_rpc(&self, rpc: PendingSessionRpc) -> Result<(), ApiError> {
        let mut q = self.rpc_queue.lock().unwrap_or_else(|p| p.into_inner());
        if q.closed {
            return Err(ApiError::new(
                ErrorCode::Unavailable,
                "satellite office disconnected before accepting session request",
            ));
        }
        if q.inbox.len().saturating_add(q.active_inflight) >= MAX_INFLIGHT_PROXIED_RPCS {
            return Err(ApiError::new(
                ErrorCode::Unavailable,
                "too many concurrent proxied session requests for satellite office",
            ));
        }
        q.inbox.push_back(rpc);
        drop(q);
        self.notify_wake();
        Ok(())
    }

    pub(crate) fn take_rpcs(&self) -> Vec<PendingSessionRpc> {
        let mut q = self.rpc_queue.lock().unwrap_or_else(|p| p.into_inner());
        let taken: Vec<PendingSessionRpc> = q.inbox.drain(..).collect();
        q.active_inflight = q.active_inflight.saturating_add(taken.len());
        taken
    }

    pub(crate) fn release_inflight(&self, count: usize) {
        if count == 0 {
            return;
        }
        let mut q = self.rpc_queue.lock().unwrap_or_else(|p| p.into_inner());
        q.active_inflight = q.active_inflight.saturating_sub(count);
    }

    pub(crate) fn has_pending_rpcs(&self) -> bool {
        !self
            .rpc_queue
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .inbox
            .is_empty()
    }

    fn close_and_clear(&self) {
        let mut q = self.rpc_queue.lock().unwrap_or_else(|p| p.into_inner());
        q.closed = true;
        q.active_inflight = 0;
        q.inbox.clear();
        drop(q);
        self.notify_wake();
    }
}

type PeerSessionList = Vec<(u64, Arc<HqPeerHandle>)>;

/// Registry of active satellite mTLS sessions connected to HQ, providing sub-millisecond push
/// wakeups and synchronous HQ->Satellite session RPC proxying (`TASK-140`, `TASK-143` AC #3).
#[derive(Default)]
pub(crate) struct HqPeerRegistry {
    sessions: Mutex<HashMap<String, PeerSessionList>>,
    next_conn_id: AtomicU64,
    next_rpc_id: AtomicU64,
    /// Bytes of satellite blob uploads HQ holds in memory across all sessions (`TASK-98.6`).
    blob_bytes_in_flight: AtomicU64,
}

pub(crate) struct HqPeerRegistration {
    registry: Arc<HqPeerRegistry>,
    office_id: String,
    conn_id: u64,
    handle: Arc<HqPeerHandle>,
}

impl HqPeerRegistration {
    pub(crate) fn handle(&self) -> &Arc<HqPeerHandle> {
        &self.handle
    }
}

impl Drop for HqPeerRegistration {
    fn drop(&mut self) {
        let mut map = self
            .registry
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if let Some(list) = map.get_mut(&self.office_id) {
            list.retain(|(id, _)| *id != self.conn_id);
            if list.is_empty() {
                map.remove(&self.office_id);
            }
        }
        drop(map);
        self.handle.close_and_clear();
    }
}

impl HqPeerRegistry {
    /// Reserves `bytes` of in-memory blob upload room if the total stays within `cap`.
    pub(crate) fn try_reserve_blob_bytes(&self, bytes: u64, cap: u64) -> bool {
        self.blob_bytes_in_flight
            .try_update(Ordering::AcqRel, Ordering::Acquire, |held| {
                held.checked_add(bytes).filter(|total| *total <= cap)
            })
            .is_ok()
    }

    /// Returns room [`Self::try_reserve_blob_bytes`] reserved.
    pub(crate) fn release_blob_bytes(&self, bytes: u64) {
        let _ = self
            .blob_bytes_in_flight
            .try_update(Ordering::AcqRel, Ordering::Acquire, |held| {
                Some(held.saturating_sub(bytes))
            });
    }

    pub(crate) fn register(self: &Arc<Self>, office_id: &str) -> io::Result<HqPeerRegistration> {
        let handle = Arc::new(HqPeerHandle::new()?);
        let conn_id = self.next_conn_id.fetch_add(1, Ordering::Relaxed) + 1;
        let mut map = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        map.entry(office_id.to_owned())
            .or_default()
            .push((conn_id, Arc::clone(&handle)));
        Ok(HqPeerRegistration {
            registry: Arc::clone(self),
            office_id: office_id.to_owned(),
            conn_id,
            handle,
        })
    }

    pub(crate) fn notify_peer(&self, office_id: &str) {
        let handles: Vec<Arc<HqPeerHandle>> = {
            let map = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
            map.get(office_id)
                .map(|list| list.iter().map(|(_, h)| Arc::clone(h)).collect())
                .unwrap_or_default()
        };
        for handle in handles {
            handle.notify_wake();
        }
    }

    pub(crate) fn notify_all(&self) {
        let handles: Vec<Arc<HqPeerHandle>> = {
            let map = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
            map.values()
                .flat_map(|list| list.iter().map(|(_, h)| Arc::clone(h)))
                .collect()
        };
        for handle in handles {
            handle.notify_wake();
        }
    }

    /// The session to proxy a session RPC over: the newest persistent sync session, so a
    /// short-lived connection (a permit coordinator, ~1-10 ms) that registered later is never
    /// chosen while the persistent one is up; the newest session only while none is marked
    /// persistent yet (`TASK-174` AC #1).
    pub(super) fn latest_peer(&self, office_id: &str) -> Option<Arc<HqPeerHandle>> {
        let map = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        let list = map.get(office_id)?;
        list.iter()
            .rev()
            .find(|(_, h)| h.is_persistent())
            .or_else(|| list.last())
            .map(|(_, h)| Arc::clone(h))
    }

    pub(crate) fn proxy_session_rpc(
        &self,
        target_office_id: &str,
        caller_member_id: &str,
        caller_is_owner: bool,
        request: &Request,
        timeout: Duration,
    ) -> Result<Response, ApiError> {
        if !is_proxied_session_method(&request.method) {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                format!(
                    "method `{}` cannot be proxied to a satellite",
                    request.method
                ),
            ));
        }
        let params_json = serde_json::to_string(&request.params).map_err(|_| {
            ApiError::new(
                ErrorCode::InvalidRequest,
                "unable to serialize session RPC parameters",
            )
        })?;
        let request_id = format!(
            "srpc-{}",
            self.next_rpc_id.fetch_add(1, Ordering::Relaxed) + 1
        );
        // Pre-validate the encoded SyncFrame size before enqueueing so an oversized request can
        // never trigger FrameCodecError::TooLarge on the persistent mTLS transport.
        let probe = SyncFrame::SessionRpcRequest {
            request_id: request_id.clone(),
            caller_member_id: caller_member_id.to_owned(),
            caller_is_owner,
            method: request.method.clone(),
            params_json: params_json.clone(),
        };
        org_sync_proto::encode_sync_frame(&probe).map_err(|_| {
            ApiError::new(
                ErrorCode::InvalidRequest,
                "proxied session RPC request exceeds federation frame size limit",
            )
        })?;
        let Some(handle) = self.latest_peer(target_office_id) else {
            return Err(ApiError::new(
                ErrorCode::Unavailable,
                format!("satellite office `{target_office_id}` is not connected to HQ"),
            ));
        };
        let (tx, rx) = mpsc::channel();
        handle.enqueue_rpc(PendingSessionRpc {
            request_id,
            caller_member_id: caller_member_id.to_owned(),
            caller_is_owner,
            method: request.method.clone(),
            params_json,
            deadline: Instant::now() + timeout,
            reply: tx,
        })?;
        drop(handle);
        match rx.recv_timeout(timeout) {
            Ok(Ok(outcome)) => Ok(Response {
                id: request.id,
                outcome,
                waiting_messages: None,
            }),
            Ok(Err(err)) => Err(err),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(ApiError::new(
                ErrorCode::Unavailable,
                "timed out waiting for satellite office session response",
            )),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(ApiError::new(
                ErrorCode::Unavailable,
                "satellite office disconnected before answering session request",
            )),
        }
    }
}

/// Cloneable handle to a running [`crate::federation::HqFederationServer`] allowing local HQ
/// services to push immediate sync wakeups and proxy remote session RPCs (`TASK-140`, `TASK-143`).
#[derive(Clone)]
pub struct HqFederationHandle {
    peers: Arc<HqPeerRegistry>,
}

impl HqFederationHandle {
    pub(crate) fn new(peers: Arc<HqPeerRegistry>) -> Self {
        Self { peers }
    }

    pub fn notify_peer(&self, office_id: &str) {
        self.peers.notify_peer(office_id);
    }

    pub fn notify_all_peers(&self) {
        self.peers.notify_all();
    }

    pub fn proxy_session_rpc(
        &self,
        target_office_id: &str,
        caller_member_id: &str,
        caller_is_owner: bool,
        request: &Request,
        timeout: Duration,
    ) -> Result<Response, ApiError> {
        self.peers.proxy_session_rpc(
            target_office_id,
            caller_member_id,
            caller_is_owner,
            request,
            timeout,
        )
    }
}
