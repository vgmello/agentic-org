use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use org_proto::{ErrorCode, Outcome, Request};
use org_sync_proto::SyncFrame;
use serde_json::json;

use super::peer_registry::PendingSessionRpc;
use super::satellite_rpc::encode_bounded_rpc_reply;
use super::*;

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

/// `TASK-174` AC #1: a short-lived connection that registers after the satellite's
/// persistent sync session (a permit coordinator, ~1-10 ms) never takes its proxied
/// session RPCs; the newest session is chosen only while none is persistent yet.
#[test]
fn latest_peer_prefers_the_newest_persistent_session_over_a_later_short_lived_one() {
    let registry = Arc::new(HqPeerRegistry::default());
    assert!(registry.latest_peer("sat-1").is_none());

    let persistent = registry.register("sat-1").unwrap();
    let permit = registry.register("sat-1").unwrap();
    // Nothing marked yet: the newest registration.
    assert!(Arc::ptr_eq(
        &registry.latest_peer("sat-1").unwrap(),
        permit.handle()
    ));

    persistent.handle().mark_persistent();
    assert!(Arc::ptr_eq(
        &registry.latest_peer("sat-1").unwrap(),
        persistent.handle()
    ));
    let later_permit = registry.register("sat-1").unwrap();
    assert!(Arc::ptr_eq(
        &registry.latest_peer("sat-1").unwrap(),
        persistent.handle()
    ));

    // A reconnect: the newer persistent session wins over the older one.
    let reconnected = registry.register("sat-1").unwrap();
    reconnected.handle().mark_persistent();
    assert!(Arc::ptr_eq(
        &registry.latest_peer("sat-1").unwrap(),
        reconnected.handle()
    ));

    // Once every persistent session is gone, the newest remaining one is used.
    drop(reconnected);
    drop(persistent);
    assert!(Arc::ptr_eq(
        &registry.latest_peer("sat-1").unwrap(),
        later_permit.handle()
    ));
    drop(later_permit);
    drop(permit);
    assert!(registry.latest_peer("sat-1").is_none());
    assert!(registry.latest_peer("sat-2").is_none());
}

fn take_reply(replies: &Mutex<VecDeque<SyncFrame>>) -> Option<Outcome> {
    let frame = replies.lock().unwrap().pop_front()?;
    match frame {
        SyncFrame::SessionRpcResponse { outcome_json, .. } => {
            Some(serde_json::from_str(&outcome_json).unwrap())
        }
        other => panic!("expected SessionRpcResponse, got {other:?}"),
    }
}

fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(30);
    while !ready() {
        assert!(Instant::now() < until, "{what} did not happen");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// `TASK-174` AC #2: the RPC worker budget belongs to the persistent sync loop, not to one
/// connection. Workers a dropped connection left running still hold their slots, so a
/// reconnected session sharing the budget answers "busy" instead of starting a ninth.
#[test]
fn satellite_rpc_workers_share_one_budget_across_reconnected_sessions() {
    let active_workers = Arc::new(AtomicUsize::new(0));
    let sat_wake = Arc::new(SatSyncWake::default());
    let sessions = std::sync::Weak::<org_office_sessions::SessionManager>::new();

    // The previous connection's workers are still running: every slot is held.
    let held: Vec<_> = (0..MAX_SATELLITE_RPC_WORKERS)
        .map(|_| satellite_rpc::WorkerSlot::try_acquire(&active_workers).unwrap())
        .collect();
    assert!(satellite_rpc::WorkerSlot::try_acquire(&active_workers).is_none());
    assert_eq!(
        active_workers.load(Ordering::Acquire),
        MAX_SATELLITE_RPC_WORKERS
    );

    // A reconnected session: new reply queue, the same budget.
    let replies: Arc<Mutex<VecDeque<SyncFrame>>> = Arc::default();
    let ctx = SatRpcContext {
        sessions: &sessions,
        replies: &replies,
        sat_wake: &sat_wake,
        active_workers: &active_workers,
    };
    spawn_satellite_session_rpc(
        "srpc-1".into(),
        "owner-1".into(),
        true,
        "member.session_inspect".into(),
        "{}".into(),
        &ctx,
    );
    match take_reply(&replies) {
        Some(Outcome::Error(e)) => {
            assert_eq!(e.code, ErrorCode::Unavailable);
            assert!(e.message.contains("busy"), "{}", e.message);
        }
        other => panic!("expected a busy refusal, got {other:?}"),
    }
    assert_eq!(
        active_workers.load(Ordering::Acquire),
        MAX_SATELLITE_RPC_WORKERS
    );

    // The old workers finish: the reconnected session gets a worker, which releases its
    // slot when done.
    drop(held);
    assert_eq!(active_workers.load(Ordering::Acquire), 0);
    spawn_satellite_session_rpc(
        "srpc-2".into(),
        "owner-1".into(),
        true,
        "member.session_inspect".into(),
        "{}".into(),
        &ctx,
    );
    let mut reply = None;
    wait_for("the worker's reply", || {
        reply = take_reply(&replies);
        reply.is_some()
    });
    match reply {
        Some(Outcome::Error(e)) => assert!(e.message.contains("shutting down"), "{}", e.message),
        other => panic!("expected the worker's answer, got {other:?}"),
    }
    wait_for("the worker's slot release", || {
        active_workers.load(Ordering::Acquire) == 0
    });
}
