use std::sync::{mpsc, Arc};
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
