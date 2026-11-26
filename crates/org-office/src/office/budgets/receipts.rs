//! Task transition receipts that make budget stops idempotent.

use org_proto::tasks::TasksTransitionParams;
use org_proto::{ApiError, ErrorCode};
use org_store::Sha256;

use crate::budgets::PendingStopIntent;
use crate::error::OfficeError;
use crate::office::Office;

use super::stop::StopExecutionOutcome;

pub(super) enum TaskReceiptOutcome {
    Final(StopExecutionOutcome),
    RevisionConflict,
}

fn sha256_hex(digest: &Sha256) -> String {
    let mut hex = String::with_capacity(64);
    for b in digest.as_bytes() {
        use std::fmt::Write;
        let _ = write!(&mut hex, "{b:02x}");
    }
    hex
}

/// Computes the exact `org-store` operation fingerprint produced by `task_ops::task_op`
/// for `"tasks.transition"` so stop recovery can reject command ID collisions from another task.
pub(super) fn task_transition_fingerprint(params: &TasksTransitionParams) -> String {
    let fields = serde_json::to_vec(params).unwrap_or_default();
    sha256_hex(&Sha256::of_parts(&[b"tasks.transition", b"\0", &fields]))
}

pub(super) fn lookup_task_transition_receipt(
    office: &Office,
    intent: &PendingStopIntent,
) -> Result<Option<TaskReceiptOutcome>, OfficeError> {
    let (Ok(op_id), Ok(actor_id)) = (
        org_store::Id::parse(&intent.task_command_id),
        org_store::Id::parse(&intent.actor_id),
    ) else {
        return Ok(None);
    };
    let (receipt, _) = office
        .store
        .read(crate::store::open::ReceiptById(op_id))
        .map_err(|e| OfficeError::storage(std::io::Error::other(e.to_string())))?;
    let Some(receipt) = receipt else {
        return Ok(None);
    };
    if receipt.kind != "tasks.transition" || receipt.actor != org_store::Actor::Employee(actor_id) {
        return Ok(Some(TaskReceiptOutcome::RevisionConflict));
    }
    let Some(expected_fp) = intent.task_command_fingerprint.as_deref() else {
        // An unfingerprinted intent has not yet submitted `tasks.transition`; any existing
        // receipt for `task_command_id` is a collision (e.g., clock rollback) and must rotate
        // to a fresh command ID rather than finalizing this stop (`Final Gate Pass 2 #2`).
        return Ok(Some(TaskReceiptOutcome::RevisionConflict));
    };
    if sha256_hex(&receipt.fingerprint) != expected_fp {
        return Ok(Some(TaskReceiptOutcome::RevisionConflict));
    }
    // Never finalize a budget stop from an unconfirmed projection receipt (`TASK-97` / Pass 8):
    // require both journal flush and host custody confirmation through `receipt.ordinal`.
    let mut status = office.store.status();
    if status.stopped.is_none()
        && (status.flushed < receipt.ordinal || status.confirmed < receipt.ordinal)
    {
        office
            .store
            .sync()
            .map_err(|e| OfficeError::storage(std::io::Error::other(e.to_string())))?;
        status = office.store.status();
    }
    if status.stopped.is_some()
        || status.flushed < receipt.ordinal
        || status.confirmed < receipt.ordinal
    {
        return Err(OfficeError::storage(std::io::Error::other(
            "task cancellation receipt is not yet flushed and confirmed",
        )));
    }
    match receipt.outcome {
        org_store::Outcome::Completed => {
            let reply_task_id = serde_json::from_slice::<serde_json::Value>(&receipt.reply)
                .ok()
                .and_then(|v| v.get("task_id")?.as_str().map(str::to_owned));
            if reply_task_id.as_deref() != Some(intent.task_id.as_str()) {
                return Ok(Some(TaskReceiptOutcome::RevisionConflict));
            }
            Ok(Some(TaskReceiptOutcome::Final(
                StopExecutionOutcome::Closed("cancelled"),
            )))
        }
        org_store::Outcome::Rejected => {
            let api_err = serde_json::from_slice::<ApiError>(&receipt.reply).unwrap_or_else(|_| {
                ApiError::new(ErrorCode::InvalidRequest, "task operation refused")
            });
            if matches!(
                api_err.code,
                ErrorCode::Conflict
                    | ErrorCode::StorageError
                    | ErrorCode::Unavailable
                    | ErrorCode::Timeout
            ) {
                Ok(Some(TaskReceiptOutcome::RevisionConflict))
            } else {
                Ok(Some(TaskReceiptOutcome::Final(
                    StopExecutionOutcome::Refused(api_err.message),
                )))
            }
        }
    }
}

pub(super) fn invalid_op_id() -> ApiError {
    ApiError::new(
        ErrorCode::InvalidRequest,
        "operation_id must be a canonical lowercase UUIDv7",
    )
}
