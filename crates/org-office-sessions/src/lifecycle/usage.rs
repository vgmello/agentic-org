//! Durable session usage staging, flush into office accounting, and dead-lettering (`TASK-97`).

use super::*;

pub(super) fn record_session_usage(
    office: &Office,
    store: &SessionStore,
    emp_id: &str,
    run_id: &str,
    seq: u64,
    usage: &org_agent_runtime::Usage,
) {
    let entry = crate::recovery::DeferredUsageEntry {
        run_id: run_id.to_owned(),
        seq,
        occurred_at: rfc3339_now(),
        usage: usage.clone(),
    };
    if let Err(err) = store.stage_usage_observation(emp_id, entry) {
        eprintln!(
            "org-office-sessions: warning: failed to durably stage usage {run_id}:{seq} for {emp_id}: {err} (buffered in memory for retry)"
        );
        return;
    }
    let _ = flush_pending_usage(office, store, emp_id);
}

pub(super) fn is_permanent_usage_rejection(code: org_proto::ErrorCode) -> bool {
    matches!(
        code,
        org_proto::ErrorCode::InvalidRequest | org_proto::ErrorCode::NotAuthorized
    )
}

pub(super) fn record_permanent_usage_rejection(
    store: &SessionStore,
    emp_id: &str,
    params: &org_proto::costs::CostUsageRecordParams,
    err: &org_proto::ApiError,
) -> bool {
    let rejected = crate::recovery::RejectedUsageEntry {
        params: params.clone(),
        error_code: format!("{:?}", err.code),
        error_message: err.message.clone(),
        rejected_at: rfc3339_now(),
    };
    if let Err(io_err) = store.append_rejected_usage(emp_id, &rejected) {
        eprintln!(
            "org-office-sessions: warning: failed to persist rejected usage dead-letter {}:{} for {emp_id}: {io_err}",
            params.producer_id, params.source_event_id
        );
        return false;
    }
    eprintln!(
        "org-office-sessions: warning: usage {}:{} for {emp_id} permanently rejected ({:?}: {}); dead-lettered",
        params.producer_id, params.source_event_id, err.code, err.message
    );
    true
}

/// Drains any durably staged usage events for `emp_id` into office accounting (`TASK-97`).
/// First flushes any in-memory overflow into `usage-capture.v1.json` in strict FIFO order, then
/// submits pending records idempotently by `(producer_id, source_event_id)`. Transient failures
/// keep remaining events pending for later retry; permanent validation errors are durably
/// dead-lettered into `rejected-usage.v1.json` before dequeuing.
pub(crate) fn flush_pending_usage(office: &Office, store: &SessionStore, emp_id: &str) -> usize {
    if store.pending_usage_overflow_count(emp_id) == 0
        && !store.usage_capture_path(emp_id).is_ok_and(|p| p.exists())
    {
        return 0;
    }
    if let Err(err) = store.flush_overflow_usage(emp_id) {
        eprintln!(
            "org-office-sessions: warning: failed to flush usage overflow for {emp_id}: {err}"
        );
    }
    let Ok(pending) = store.read_pending_usage(emp_id) else {
        return 0;
    };
    if pending.is_empty() {
        return 0;
    }
    let mut acked: Vec<(String, String)> = Vec::new();
    let mut flushed = 0usize;
    for params in pending {
        let key = (params.producer_id.clone(), params.source_event_id.clone());
        match office.record_office_usage(params.clone()) {
            Ok(Ok(_)) => {
                acked.push(key);
                flushed += 1;
            }
            Ok(Err(api_err)) if is_permanent_usage_rejection(api_err.code) => {
                if record_permanent_usage_rejection(store, emp_id, &params, &api_err) {
                    acked.push(key);
                } else {
                    break;
                }
            }
            Ok(Err(_)) | Err(_) => {
                break;
            }
        }
    }
    if !acked.is_empty() {
        let _ = store.remove_pending_usage_by_keys(emp_id, &acked);
    }
    flushed
}
