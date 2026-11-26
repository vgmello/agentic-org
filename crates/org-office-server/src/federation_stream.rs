//! Event-driven persistent mTLS federation transport primitives (`TASK-140`) and
//! HQ-to-satellite session-control RPC proxy (`TASK-143` AC #3).
//!
//! Keeps `#![forbid(unsafe_code)]` intact by pairing safe non-blocking `UnixStream::pair()`
//! self-pipes with [`org_socket::poll_readable_either`] for sub-millisecond bidirectional wakeups
//! and zero idle polling CPU.

use std::collections::{HashMap, VecDeque};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

use org_office::Office;
use org_proto::{ApiError, ErrorCode, Outcome, Request, Response};
use org_sync_proto::SyncFrame;

use crate::federation::{InboundDeliveryNotifier, SatelliteSyncSession, WakeRelayHandler};
use crate::org_feed::FeedWake;

const MAX_INFLIGHT_PROXIED_RPCS: usize = 64;
const MAX_SATELLITE_RPC_WORKERS: usize = 8;
const KEEPALIVE_IDLE: Duration = Duration::from_secs(15);
const KEEPALIVE_TIMEOUT: Duration = Duration::from_secs(10);
const RECONNECT_MIN: Duration = Duration::from_millis(100);
const RECONNECT_MAX: Duration = Duration::from_secs(5);
const IDLE_POLL_SLICE: Duration = Duration::from_millis(500);

/// Returns `true` when `method` is allowed to be proxied from HQ to a satellite office where
/// the target agent is placed (`TASK-143` AC #3).
pub(crate) fn is_proxied_session_method(method: &str) -> bool {
    matches!(
        method,
        "member.session_inspect"
            | "member.session_list"
            | "member.session_approval"
            | "member.session_stop"
            | "member.session_start"
            | "member.session_transcript"
            | "member.session_clear_stall"
            | "member.session_grants"
    )
}

/// Safe, non-blocking self-pipe used to interrupt `poll_readable_either` immediately when local
/// outbox items, org deltas, relays, or proxied RPC requests arrive.
pub(crate) struct WakePipe {
    read: UnixStream,
    write: Mutex<UnixStream>,
}

impl WakePipe {
    pub(crate) fn new() -> io::Result<Self> {
        let (read, write) = UnixStream::pair()?;
        read.set_nonblocking(true)?;
        write.set_nonblocking(true)?;
        Ok(Self {
            read,
            write: Mutex::new(write),
        })
    }

    pub(crate) fn notify(&self) {
        if let Ok(mut w) = self.write.lock() {
            let _ = w.write(&[1u8]);
        }
    }

    pub(crate) fn drain(&self) {
        let mut buf = [0u8; 64];
        loop {
            match (&self.read).read(&mut buf) {
                Ok(0) => break,
                Ok(_) => continue,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    }

    pub(crate) fn read_fd(&self) -> RawFd {
        self.read.as_raw_fd()
    }
}

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
}

impl HqPeerHandle {
    fn new() -> io::Result<Self> {
        Ok(Self {
            wake: WakePipe::new()?,
            wake_gen: AtomicU64::new(0),
            rpc_queue: Mutex::new(PeerRpcQueue::default()),
        })
    }

    pub(crate) fn notify_wake(&self) {
        self.wake_gen.fetch_add(1, Ordering::Release);
        self.wake.notify();
    }

    pub(crate) fn wake_generation(&self) -> u64 {
        self.wake_gen.load(Ordering::Acquire)
    }

    fn enqueue_rpc(&self, rpc: PendingSessionRpc) -> Result<(), ApiError> {
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

    fn latest_peer(&self, office_id: &str) -> Option<Arc<HqPeerHandle>> {
        let map = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        map.get(office_id)
            .and_then(|list| list.last().map(|(_, h)| Arc::clone(h)))
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

/// Combined condvar + self-pipe wake signal for the satellite persistent mTLS loop (`TASK-140`).
/// Calling [`Self::notify`] wakes BOTH a connected `poll_readable_either` wait AND a disconnected
/// reconnect backoff wait in under 0.1 ms.
pub(crate) struct SatSyncWake {
    gen: Mutex<u64>,
    cv: Condvar,
    pipe: Option<WakePipe>,
}

impl Default for SatSyncWake {
    fn default() -> Self {
        Self {
            gen: Mutex::new(0),
            cv: Condvar::new(),
            pipe: WakePipe::new().ok(),
        }
    }
}

impl SatSyncWake {
    pub(crate) fn notify(&self) {
        let mut g = self.gen.lock().unwrap_or_else(|p| p.into_inner());
        *g = g.saturating_add(1);
        self.cv.notify_all();
        drop(g);
        if let Some(pipe) = &self.pipe {
            pipe.notify();
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        *self.gen.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub(crate) fn read_fd(&self) -> Option<RawFd> {
        self.pipe.as_ref().map(WakePipe::read_fd)
    }

    pub(crate) fn drain_pipe(&self) {
        if let Some(pipe) = &self.pipe {
            pipe.drain();
        }
    }

    pub(crate) fn wait_or_stop(&self, seen: u64, timeout: Duration, stop: &AtomicBool) -> u64 {
        let deadline = Instant::now() + timeout;
        let mut g = self.gen.lock().unwrap_or_else(|p| p.into_inner());
        while *g == seen && !stop.load(Ordering::SeqCst) {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            let step = deadline
                .saturating_duration_since(now)
                .min(Duration::from_millis(100));
            let (next, _) = self
                .cv
                .wait_timeout(g, step)
                .unwrap_or_else(|p| p.into_inner());
            g = next;
        }
        *g
    }
}

/// Runtime hooks passed into the satellite's persistent bidirectional mTLS loop.
pub(crate) struct PersistentSyncHooks {
    pub(crate) wake_handler: WakeRelayHandler,
    pub(crate) delivery_notifier: InboundDeliveryNotifier,
    pub(crate) feed_wake: Option<Arc<FeedWake>>,
    pub(crate) sessions: Weak<org_office_sessions::SessionManager>,
}

struct WorkerSlot(Arc<AtomicUsize>);
impl Drop for WorkerSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Borrowed dispatch context for HQ-proxied session RPCs executed on a satellite.
pub(crate) struct SatRpcContext<'a> {
    pub(crate) sessions: &'a Weak<org_office_sessions::SessionManager>,
    pub(crate) replies: &'a Arc<Mutex<VecDeque<SyncFrame>>>,
    pub(crate) sat_wake: &'a Arc<SatSyncWake>,
    pub(crate) active_workers: &'a Arc<AtomicUsize>,
}

fn encode_bounded_rpc_reply(request_id: String, outcome: Outcome) -> SyncFrame {
    let outcome_json = serde_json::to_string(&outcome).unwrap_or_else(|_| {
        r#"{"error":{"code":"unavailable","message":"failed encoding session RPC outcome"}}"#
            .to_owned()
    });
    let frame = SyncFrame::SessionRpcResponse {
        request_id: request_id.clone(),
        outcome_json,
    };
    if org_sync_proto::encode_sync_frame(&frame).is_ok() {
        return frame;
    }
    let fallback = Outcome::Error(ApiError::new(
        ErrorCode::StorageError,
        "session RPC response exceeds federation frame size limit",
    ));
    let fallback_json = serde_json::to_string(&fallback).unwrap_or_else(|_| {
        r#"{"error":{"code":"storage_error","message":"session RPC response exceeds federation frame size limit"}}"#
            .to_owned()
    });
    SyncFrame::SessionRpcResponse {
        request_id,
        outcome_json: fallback_json,
    }
}

/// Dispatches an HQ-proxied `SessionRpcRequest` on a bounded satellite worker thread and posts
/// the resulting `SessionRpcResponse` back onto the persistent mTLS stream (`TASK-143` AC #3).
pub(crate) fn spawn_satellite_session_rpc(
    request_id: String,
    caller_member_id: String,
    caller_is_owner: bool,
    method: String,
    params_json: String,
    ctx: &SatRpcContext<'_>,
) {
    let replies = Arc::clone(ctx.replies);
    let sat_wake = Arc::clone(ctx.sat_wake);
    let sessions = Weak::clone(ctx.sessions);
    let active_workers = Arc::clone(ctx.active_workers);
    let push_reply = move |req_id: String, outcome: Outcome| {
        replies
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push_back(encode_bounded_rpc_reply(req_id, outcome));
        sat_wake.notify();
    };
    if !is_proxied_session_method(&method) {
        push_reply(
            request_id,
            Outcome::Error(ApiError::new(
                ErrorCode::NotAuthorized,
                "method is not permitted over HQ session proxy",
            )),
        );
        return;
    }
    if active_workers.fetch_add(1, Ordering::AcqRel) >= MAX_SATELLITE_RPC_WORKERS {
        active_workers.fetch_sub(1, Ordering::AcqRel);
        push_reply(
            request_id,
            Outcome::Error(ApiError::new(
                ErrorCode::Unavailable,
                "satellite session proxy busy",
            )),
        );
        return;
    }
    let slot = WorkerSlot(active_workers);
    let replies_err = Arc::clone(ctx.replies);
    let wake_err = Arc::clone(ctx.sat_wake);
    let err_req_id = request_id.clone();
    if std::thread::Builder::new()
        .name("sat-session-rpc".into())
        .spawn(move || {
            let _slot = slot;
            let outcome = execute_satellite_session_rpc(
                &caller_member_id,
                caller_is_owner,
                &method,
                &params_json,
                &sessions,
            );
            push_reply(request_id, outcome);
        })
        .is_err()
    {
        replies_err
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push_back(encode_bounded_rpc_reply(
                err_req_id,
                Outcome::Error(ApiError::new(
                    ErrorCode::Unavailable,
                    "satellite session proxy worker unavailable",
                )),
            ));
        wake_err.notify();
    }
}

fn execute_satellite_session_rpc(
    caller_member_id: &str,
    caller_is_owner: bool,
    method: &str,
    params_json: &str,
    sessions: &Weak<org_office_sessions::SessionManager>,
) -> Outcome {
    let Some(sm) = sessions.upgrade() else {
        return Outcome::Error(ApiError::new(
            ErrorCode::Unavailable,
            "satellite session manager is shutting down",
        ));
    };
    let params: serde_json::Value = match serde_json::from_str(params_json) {
        Ok(v) => v,
        Err(_) => {
            return Outcome::Error(ApiError::new(
                ErrorCode::InvalidRequest,
                "invalid proxied session RPC parameters",
            ));
        }
    };
    let req = Request {
        id: 1,
        method: method.to_owned(),
        params,
        principal: None,
    };
    match sm.dispatch_hq_proxied_rpc(caller_member_id, caller_is_owner, &req) {
        Some(resp) => resp.outcome,
        None => Outcome::Error(ApiError::new(
            ErrorCode::UnknownMethod,
            format!("satellite does not support `{method}`"),
        )),
    }
}

/// Runs the satellite's persistent bidirectional mTLS loop with instant local outbox flush,
/// instant HQ org/relay push handling, keepalive ping/pong, and exponential backoff reconnect.
pub(crate) fn run_persistent_satellite_sync(
    office: &Arc<Office>,
    stop: &Arc<AtomicBool>,
    sat_wake: &Arc<SatSyncWake>,
    hooks: &PersistentSyncHooks,
) {
    let mut backoff = RECONNECT_MIN;
    while !stop.load(Ordering::SeqCst) {
        let mut session =
            match SatelliteSyncSession::connect_cancellable(Arc::clone(office), Some(stop)) {
                Ok(s) => {
                    backoff = RECONNECT_MIN;
                    s
                }
                Err(_) => {
                    let seen = sat_wake.generation();
                    let _ = sat_wake.wait_or_stop(seen, backoff, stop);
                    backoff = (backoff * 2).min(RECONNECT_MAX);
                    continue;
                }
            };
        if run_connected_session(&mut session, office, stop, sat_wake, hooks).is_err() {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            let seen = sat_wake.generation();
            let _ = sat_wake.wait_or_stop(seen, backoff, stop);
            backoff = (backoff * 2).min(RECONNECT_MAX);
        }
    }
}

struct PersistentLoopState {
    last_wake_gen: u64,
    buffered_flush_until: Option<Instant>,
    last_rx: Instant,
    ping_sent_at: Option<Instant>,
    ping_nonce: u64,
}

fn bootstrap_connected_session(
    session: &mut SatelliteSyncSession,
    office: &Arc<Office>,
    hooks: &PersistentSyncHooks,
) -> Result<(), String> {
    if office
        .sync_status()
        .is_ok_and(|s| s.initial_snapshot_required)
    {
        let op_id = office.next_uuid_v7();
        session.sync_initial_snapshot(&op_id)?;
        if let Some(wake) = &hooks.feed_wake {
            wake.notify();
        }
        if let Some(sm) = hooks.sessions.upgrade() {
            let _ = sm.recover_on_startup();
        }
    }
    let applied = session.pull_org_events()?;
    if applied > 0 {
        if let Some(wake) = &hooks.feed_wake {
            wake.notify();
        }
    }
    Ok(())
}

fn flush_satellite_outboxes(
    session: &mut SatelliteSyncSession,
    office: &Office,
    sat_wake: &SatSyncWake,
    state: &mut PersistentLoopState,
) -> Result<(), String> {
    let cur_gen = sat_wake.generation();
    let wake_fired = cur_gen != state.last_wake_gen;
    if wake_fired {
        state.last_wake_gen = cur_gen;
    }
    let flushed = session.flush_work_outbox()? + session.flush_message_outbox()?;
    if flushed > 0 {
        state.buffered_flush_until = None;
        state.last_rx = Instant::now();
        state.ping_sent_at = None;
    } else if wake_fired || office.has_pending_journal_pipeline() {
        // Buffered `org-store` writes (e.g. `msg.reply`) return before the background
        // flusher confirms/materializes the entry a few milliseconds later; poll briefly at
        // 5 ms until the write clears the pipeline so RTT stays < 10 ms with 0% idle CPU.
        state.buffered_flush_until = Some(Instant::now() + Duration::from_millis(80));
    } else if state
        .buffered_flush_until
        .is_some_and(|until| Instant::now() >= until)
    {
        state.buffered_flush_until = None;
    }
    Ok(())
}

fn check_keepalive(
    session: &mut SatelliteSyncSession,
    state: &mut PersistentLoopState,
) -> Result<(), String> {
    if let Some(sent_at) = state.ping_sent_at {
        if sent_at.elapsed() >= KEEPALIVE_TIMEOUT {
            return Err("HQ keepalive ping timed out".into());
        }
    } else if state.last_rx.elapsed() >= KEEPALIVE_IDLE {
        state.ping_nonce = state.ping_nonce.wrapping_add(1);
        session.write_frame(&SyncFrame::Ping {
            nonce: state.ping_nonce,
        })?;
        state.ping_sent_at = Some(Instant::now());
    }
    Ok(())
}

fn has_immediate_satellite_work(
    sat_wake: &SatSyncWake,
    last_wake_gen: u64,
    replies: &Mutex<VecDeque<SyncFrame>>,
) -> bool {
    sat_wake.generation() != last_wake_gen
        || !replies.lock().unwrap_or_else(|p| p.into_inner()).is_empty()
}

fn wait_for_satellite_activity(
    session: &SatelliteSyncSession,
    office: &Office,
    stop: &AtomicBool,
    sat_wake: &SatSyncWake,
    state: &PersistentLoopState,
) -> Result<(), String> {
    let wait = if state.buffered_flush_until.is_some() || office.has_pending_journal_pipeline() {
        Duration::from_millis(5)
    } else {
        IDLE_POLL_SLICE
    };
    if let Some(wake_fd) = sat_wake.read_fd() {
        match org_socket::poll_readable_either(session.raw_fd(), wake_fd, wait) {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => Ok(()),
            Err(e) => Err(format!("poll failed: {e}")),
        }
    } else {
        let _ = sat_wake.wait_or_stop(state.last_wake_gen, Duration::from_millis(50), stop);
        Ok(())
    }
}

fn run_connected_session(
    session: &mut SatelliteSyncSession,
    office: &Arc<Office>,
    stop: &Arc<AtomicBool>,
    sat_wake: &Arc<SatSyncWake>,
    hooks: &PersistentSyncHooks,
) -> Result<(), String> {
    bootstrap_connected_session(session, office, hooks)?;
    let replies: Arc<Mutex<VecDeque<SyncFrame>>> = Arc::default();
    let rpc_workers = Arc::new(AtomicUsize::new(0));
    let mut state = PersistentLoopState {
        last_wake_gen: sat_wake.generation().wrapping_sub(1),
        buffered_flush_until: None,
        last_rx: Instant::now(),
        ping_sent_at: None,
        ping_nonce: 1,
    };

    while !stop.load(Ordering::SeqCst) {
        flush_satellite_outboxes(session, office, sat_wake, &mut state)?;
        flush_rpc_replies(session, &replies)?;
        let rpc_ctx = SatRpcContext {
            sessions: &hooks.sessions,
            replies: &replies,
            sat_wake,
            active_workers: &rpc_workers,
        };
        let drained = crate::federation::drain_satellite_inbound_persistent(
            session,
            Some(&hooks.wake_handler),
            Some(&hooks.delivery_notifier),
            hooks.feed_wake.as_deref(),
            &rpc_ctx,
        )?;
        if drained > 0 {
            state.last_rx = Instant::now();
            state.ping_sent_at = None;
        }
        flush_rpc_replies(session, &replies)?;
        if has_immediate_satellite_work(sat_wake, state.last_wake_gen, &replies) {
            continue;
        }
        check_keepalive(session, &mut state)?;
        sat_wake.drain_pipe();
        if has_immediate_satellite_work(sat_wake, state.last_wake_gen, &replies) {
            continue;
        }
        wait_for_satellite_activity(session, office, stop, sat_wake, &state)?;
    }
    Ok(())
}

fn flush_rpc_replies(
    session: &mut SatelliteSyncSession,
    replies: &Arc<Mutex<VecDeque<SyncFrame>>>,
) -> Result<(), String> {
    loop {
        let next = replies
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .pop_front();
        let Some(frame) = next else {
            return Ok(());
        };
        session.write_frame(&frame)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn proxy_session_rpc_rejects_oversized_request_and_bounds_inflight_and_reply_frame() {
        let registry = Arc::new(HqPeerRegistry::default());
        let reg = registry.register("sat-1").unwrap();

        // Blocking #1a: oversized request is rejected BEFORE enqueueing (never reaches mTLS).
        let huge = "x".repeat(org_sync_proto::MAX_ENCODED_FRAME_BYTES);
        let huge_req = Request {
            id: 1,
            method: "member.session_start".into(),
            params: json!({ "employee_id": "emp-1", "instruction": huge }),
            principal: None,
        };
        let err = registry
            .proxy_session_rpc("sat-1", "owner-1", true, &huge_req, Duration::from_secs(1))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidRequest);
        assert!(!reg.handle().has_pending_rpcs());

        // Blocking #1b: oversized satellite reply falls back to a valid bounded error frame.
        let huge_reply = encode_bounded_rpc_reply(
            "srpc-big".into(),
            Outcome::Result(json!({ "blob": "y".repeat(org_sync_proto::MAX_ENCODED_FRAME_BYTES) })),
        );
        assert!(org_sync_proto::encode_sync_frame(&huge_reply).is_ok());
        match huge_reply {
            SyncFrame::SessionRpcResponse { outcome_json, .. } => {
                let parsed: Outcome = serde_json::from_str(&outcome_json).unwrap();
                assert!(matches!(
                    parsed,
                    Outcome::Error(ref e) if e.code == ErrorCode::StorageError
                ));
            }
            other => panic!("expected SessionRpcResponse, got {other:?}"),
        }

        // Blocking #2: active_inflight after take_rpcs still enforces MAX_INFLIGHT_PROXIED_RPCS.
        let handle = Arc::clone(reg.handle());
        for i in 0..MAX_INFLIGHT_PROXIED_RPCS {
            let (tx, _rx) = mpsc::channel();
            handle
                .enqueue_rpc(PendingSessionRpc {
                    request_id: format!("r-{i}"),
                    caller_member_id: "owner-1".into(),
                    caller_is_owner: true,
                    method: "member.session_inspect".into(),
                    params_json: "{}".into(),
                    deadline: Instant::now() + Duration::from_secs(5),
                    reply: tx,
                })
                .unwrap();
        }
        let taken = handle.take_rpcs();
        assert_eq!(taken.len(), MAX_INFLIGHT_PROXIED_RPCS);
        let (tx_over, _rx_over) = mpsc::channel();
        let over_err = handle
            .enqueue_rpc(PendingSessionRpc {
                request_id: "r-over".into(),
                caller_member_id: "owner-1".into(),
                caller_is_owner: true,
                method: "member.session_inspect".into(),
                params_json: "{}".into(),
                deadline: Instant::now() + Duration::from_secs(5),
                reply: tx_over,
            })
            .unwrap_err();
        assert_eq!(over_err.code, ErrorCode::Unavailable);

        // Releasing inflight restores capacity; dropping registration closes handle immediately.
        handle.release_inflight(1);
        let (tx_ok, _rx_ok) = mpsc::channel();
        assert!(handle
            .enqueue_rpc(PendingSessionRpc {
                request_id: "r-reclaimed".into(),
                caller_member_id: "owner-1".into(),
                caller_is_owner: true,
                method: "member.session_inspect".into(),
                params_json: "{}".into(),
                deadline: Instant::now() + Duration::from_secs(5),
                reply: tx_ok,
            })
            .is_ok());
        drop(reg);
        let (tx_closed, _rx_closed) = mpsc::channel();
        let closed_err = handle
            .enqueue_rpc(PendingSessionRpc {
                request_id: "r-after-drop".into(),
                caller_member_id: "owner-1".into(),
                caller_is_owner: true,
                method: "member.session_inspect".into(),
                params_json: "{}".into(),
                deadline: Instant::now() + Duration::from_secs(5),
                reply: tx_closed,
            })
            .unwrap_err();
        assert_eq!(closed_err.code, ErrorCode::Unavailable);
    }
}
