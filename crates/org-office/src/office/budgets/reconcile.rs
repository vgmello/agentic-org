//! Reconciling budget holds and stops with their tasks: blocking a held task and finalizing a
//! persisted stop intent against its confirmed task transition receipt.

use org_proto::budgets::BudgetScopeKind;
use org_proto::tasks::{TaskStatus, TasksTransitionParams};
use org_proto::{ApiError, ErrorCode};
use serde_json::json;

use crate::budgets::{BudgetAggregate, PendingStopIntent};
use crate::error::OfficeError;
use crate::office::Office;
use crate::time::format_rfc3339_ms;

use super::receipts::{
    lookup_task_transition_receipt, task_transition_fingerprint, TaskReceiptOutcome,
};
use super::{
    push_budget_event, resolve_reached_notices, save_budget_aggregate, StopExecutionOutcome,
};

const MAX_TASK_TRANSITION_RETRIES: usize = 8;

/// Reconciles any active (unreleased) task budget hold by transitioning an `InProgress` task to
/// `Blocked`, retrying revision `Conflict`s cleanly (`doc-92`, `TASK-97` Pass 7 Finding #3).
pub(super) fn ensure_held_task_blocked(
    office: &Office,
    agg: &BudgetAggregate,
) -> Result<(), OfficeError> {
    if agg.budget.scope_kind != BudgetScopeKind::Task || agg.active_hold().is_none() {
        return Ok(());
    }
    if let Some(tid) = agg.budget.task_id.as_deref() {
        block_task_for_budget(office, tid)?;
    }
    Ok(())
}

fn block_task_for_budget(office: &Office, task_id: &str) -> Result<(), OfficeError> {
    for _ in 0..MAX_TASK_TRANSITION_RETRIES {
        let inspected = match office.task_inspect(task_id)? {
            Ok(v) => v,
            Err(e) if e.code == ErrorCode::NotFound => return Ok(()),
            Err(e) => return Err(OfficeError::storage(std::io::Error::other(e.message))),
        };
        if inspected.task.status != TaskStatus::InProgress {
            // Confirm durable flush & host custody of the observed non-InProgress status before
            // treating blocking as complete (`TASK-97` / Pass 11 Finding #4).
            office
                .store
                .sync()
                .map_err(|e| OfficeError::storage(std::io::Error::other(e.to_string())))?;
            if office.store.status().stopped.is_some() {
                return Err(OfficeError::storage(std::io::Error::other(
                    "store stopped before confirming task block state",
                )));
            }
            let confirmed = match office.task_inspect(task_id)? {
                Ok(v) => v,
                Err(e) if e.code == ErrorCode::NotFound => return Ok(()),
                Err(e) => return Err(OfficeError::storage(std::io::Error::other(e.message))),
            };
            if confirmed.task.revision == inspected.task.revision
                && confirmed.task.status != TaskStatus::InProgress
            {
                return Ok(());
            }
            continue;
        }
        match office.task_transition(
            office.owner.as_str(),
            TasksTransitionParams {
                command_id: office.next_uuid_v7(),
                task: task_id.to_owned(),
                expected_revision: inspected.task.revision,
                status: TaskStatus::Blocked,
                reason: None,
                blocked_reason: Some("Budget reached".into()),
                waiting_on_owner: None,
                resolution: None,
                note: Some("Task blocked because its budget was reached.".into()),
                done_anyway_reason: None,
                final_summary: None,
            },
        )? {
            Ok(_) => return Ok(()),
            Err(e) if e.code == ErrorCode::Conflict => continue,
            Err(e) if e.code == ErrorCode::NotFound => return Ok(()),
            Err(e) => return Err(OfficeError::storage(std::io::Error::other(e.message))),
        }
    }
    Err(OfficeError::storage(std::io::Error::other(
        "budget hold task transition conflicted after retries",
    )))
}

pub(super) fn stop_refusal_for_operation(
    agg: &BudgetAggregate,
    operation_id: &str,
) -> Option<ApiError> {
    let ev = agg
        .events
        .iter()
        .find(|e| e.operation_id == operation_id && e.kind == "stop_requested")?;
    let parsed: serde_json::Value = serde_json::from_str(&ev.payload_json).ok()?;
    let outcome = parsed.get("outcome").and_then(|v| v.as_str())?;
    let reason = outcome.strip_prefix("transition_refused:")?;
    Some(ApiError::new(ErrorCode::Conflict, reason.to_owned()))
}

/// Reconciles a persisted `pending_stop` intent against `org-store`'s flushed and host-confirmed
/// task transition receipt before finalizing the `stop_requested` event (`TASK-97` / Pass 7 & 8).
///
/// Resolves `budget_reached` notices ONLY on a verified closing outcome (`cancelled` or
/// `already_closed`). Retryable revision conflicts and unconfirmed receipts keep `pending_stop`
/// intact and return `Err`, while terminal task-transition refusals leave the notice open and
/// return `Ok(Some(ApiError))` so callers observe the refusal.
pub(super) fn reconcile_pending_stop(
    office: &Office,
    agg: &mut BudgetAggregate,
) -> Result<Option<ApiError>, OfficeError> {
    let Some(mut intent) = agg.pending_stop.clone() else {
        return Ok(None);
    };
    if agg.has_operation(&intent.operation_id) {
        let refusal = stop_refusal_for_operation(agg, &intent.operation_id);
        agg.pending_stop = None;
        if let Err(err) = save_budget_aggregate(office, agg)? {
            return Err(OfficeError::storage(std::io::Error::other(err.message)));
        }
        return Ok(refusal);
    }
    let outcome = execute_or_reconcile_stop_intent(office, agg, &mut intent)?;
    let now_utc = format_rfc3339_ms(office.monotonic_now_ms());
    let (outcome_str, refusal) = match outcome {
        StopExecutionOutcome::Closed(label) => {
            resolve_reached_notices(agg, &now_utc);
            (label.to_owned(), None)
        }
        StopExecutionOutcome::Refused(msg) => (
            format!("transition_refused:{msg}"),
            Some(ApiError::new(ErrorCode::Conflict, msg)),
        ),
    };
    push_budget_event(
        office,
        agg,
        &intent.operation_id,
        "stop_requested",
        &intent.actor_id,
        &now_utc,
        json!({
            "action": "budget.stop",
            "request_hash": intent.request_hash,
            "reason": intent.reason,
            "outcome": outcome_str,
            "task_command_id": intent.task_command_id,
        })
        .to_string(),
    );
    agg.pending_stop = None;
    if let Err(err) = save_budget_aggregate(office, agg)? {
        return Err(OfficeError::storage(std::io::Error::other(err.message)));
    }
    Ok(refusal)
}

fn rotate_stop_intent_command(
    office: &Office,
    agg: &mut BudgetAggregate,
    intent: &mut PendingStopIntent,
) -> Result<(), OfficeError> {
    intent.task_command_id = office.next_uuid_v7();
    intent.task_command_fingerprint = None;
    agg.pending_stop = Some(intent.clone());
    if let Err(err) = save_budget_aggregate(office, agg)? {
        return Err(OfficeError::storage(std::io::Error::other(err.message)));
    }
    Ok(())
}

fn confirm_terminal_task_state(
    office: &Office,
    task_id: &str,
    observed_revision: u64,
) -> Result<Option<StopExecutionOutcome>, OfficeError> {
    // Confirm that the observed terminal task state is flushed and host-confirmed before
    // finalizing `already_closed` (`TASK-97` / Pass 9 Finding #1).
    office
        .store
        .sync()
        .map_err(|e| OfficeError::storage(std::io::Error::other(e.to_string())))?;
    if office.store.status().stopped.is_some() {
        return Err(OfficeError::storage(std::io::Error::other(
            "store stopped before confirming terminal task state",
        )));
    }
    let confirmed = match office.task_inspect(task_id)? {
        Ok(v) => v,
        Err(e) if e.code == ErrorCode::NotFound => {
            return Ok(Some(StopExecutionOutcome::Refused("task not found".into())));
        }
        Err(e) => return Err(OfficeError::storage(std::io::Error::other(e.message))),
    };
    // Require the post-sync read to match the exact pre-sync revision so a newly appended close
    // after `sync()` started can never bypass durability (`Pass 10 #1`).
    if confirmed.task.revision == observed_revision
        && matches!(
            confirmed.task.status,
            TaskStatus::Cancelled | TaskStatus::Done
        )
    {
        return Ok(Some(StopExecutionOutcome::Closed("already_closed")));
    }
    Ok(None)
}

fn execute_or_reconcile_stop_intent(
    office: &Office,
    agg: &mut BudgetAggregate,
    intent: &mut PendingStopIntent,
) -> Result<StopExecutionOutcome, OfficeError> {
    for _ in 0..MAX_TASK_TRANSITION_RETRIES {
        if let Some(receipt_outcome) = lookup_task_transition_receipt(office, intent)? {
            match receipt_outcome {
                TaskReceiptOutcome::Final(outcome) => return Ok(outcome),
                TaskReceiptOutcome::RevisionConflict => {
                    rotate_stop_intent_command(office, agg, intent)?;
                    continue;
                }
            }
        }
        let inspected = match office.task_inspect(&intent.task_id)? {
            Ok(v) => v,
            Err(e) if e.code == ErrorCode::NotFound => {
                return Ok(StopExecutionOutcome::Refused("task not found".into()));
            }
            Err(e) => return Err(OfficeError::storage(std::io::Error::other(e.message))),
        };
        if matches!(
            inspected.task.status,
            TaskStatus::Cancelled | TaskStatus::Done
        ) {
            if let Some(outcome) =
                confirm_terminal_task_state(office, &intent.task_id, inspected.task.revision)?
            {
                return Ok(outcome);
            }
            continue;
        }
        let transition_params = TasksTransitionParams {
            command_id: intent.task_command_id.clone(),
            task: intent.task_id.clone(),
            expected_revision: inspected.task.revision,
            status: TaskStatus::Cancelled,
            reason: None,
            blocked_reason: None,
            waiting_on_owner: None,
            resolution: Some(
                intent
                    .reason
                    .as_deref()
                    .unwrap_or("Stopped at budget limit")
                    .to_owned(),
            ),
            note: Some(
                intent
                    .reason
                    .as_deref()
                    .unwrap_or("Task stopped at budget limit.")
                    .to_owned(),
            ),
            done_anyway_reason: None,
            final_summary: None,
        };
        let fp = task_transition_fingerprint(&transition_params);
        if intent.task_command_fingerprint.as_deref() != Some(fp.as_str()) {
            // Persist the exact transition fingerprint BEFORE submitting `tasks.transition` so a
            // clock-rollback command ID collision with another task can never falsely finalize
            // this stop (`TASK-97` / Final Gate Finding #3).
            intent.task_command_fingerprint = Some(fp);
            agg.pending_stop = Some(intent.clone());
            if let Err(err) = save_budget_aggregate(office, agg)? {
                return Err(OfficeError::storage(std::io::Error::other(err.message)));
            }
        }
        match office.task_transition(&intent.actor_id, transition_params)? {
            Ok(_) => return Ok(StopExecutionOutcome::Closed("cancelled")),
            Err(e) if e.code == ErrorCode::Conflict => {
                rotate_stop_intent_command(office, agg, intent)?;
                continue;
            }
            Err(e)
                if matches!(
                    e.code,
                    ErrorCode::InvalidRequest | ErrorCode::NotAuthorized | ErrorCode::NotFound
                ) =>
            {
                // Flush & confirm the journaled domain refusal receipt before clearing
                // `pending_stop` (`TASK-97` / Pass 14 Finding #2).
                office
                    .store
                    .sync()
                    .map_err(|err| OfficeError::storage(std::io::Error::other(err.to_string())))?;
                if office.store.status().stopped.is_some() {
                    return Err(OfficeError::storage(std::io::Error::other(
                        "store stopped before confirming task refusal receipt",
                    )));
                }
                return Ok(StopExecutionOutcome::Refused(e.message));
            }
            // Ambiguous/storage errors (e.g., commit succeeded before snapshot read failed)
            // keep `pending_stop` intact so reconciliation resolves via the confirmed receipt
            // (`TASK-97` / Pass 13 Finding #1).
            Err(e) => return Err(OfficeError::storage(std::io::Error::other(e.message))),
        }
    }
    Err(OfficeError::storage(std::io::Error::other(
        "budget stop task cancellation conflicted after retries",
    )))
}
