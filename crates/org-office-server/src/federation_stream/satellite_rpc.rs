//! Proxied session RPCs executed on a satellite: bounded worker slots and bounded reply encoding.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};

use org_proto::{ApiError, ErrorCode, Outcome, Request};
use org_sync_proto::SyncFrame;

use super::{is_proxied_session_method, SatSyncWake, MAX_SATELLITE_RPC_WORKERS};

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

pub(super) fn encode_bounded_rpc_reply(request_id: String, outcome: Outcome) -> SyncFrame {
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
