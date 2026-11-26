//! The turn input for a batch of queued wakes (`doc-27` §1, `doc-34` §3.1, `INSTR-5`).
//!
//! One choke-point composes it for every path that sends queued wakes to an agent: a fresh or
//! resumed start, the mutual-deadlock breaker, and the turn-end drain into a live warm session.
//! Each path then sends exactly the same text, so none can drop a task's must-follow rules.

use org_office::Office;
use org_proto::ApiError;

use crate::wake::{compose_coalesced_wake_objective, PendingWakeItem, SuspendedAskEntry, WakeKind};

/// The distinct, non-empty task references of the batch's task assignments, in queue order.
pub(super) fn assigned_task_refs(items: &[PendingWakeItem]) -> Vec<&str> {
    let mut refs: Vec<&str> = Vec::new();
    for item in items {
        let task_ref = item.source_id.as_str();
        if item.kind == WakeKind::TaskAssignment
            && !task_ref.is_empty()
            && !refs.contains(&task_ref)
        {
            refs.push(task_ref);
        }
    }
    refs
}

/// Whether an office budget hold covers the agent, or the agent on any task the batch assigns.
pub(super) fn office_budget_held(office: &Office, employee_id: &str, task_refs: &[&str]) -> bool {
    let held = |task: Option<&str>| {
        office
            .active_budget_holds(Some(employee_id), task)
            .is_ok_and(|holds| !holds.is_empty())
    };
    if task_refs.is_empty() {
        held(None)
    } else {
        task_refs.iter().any(|task| held(Some(task)))
    }
}

/// The turn input for `items` (with the asks still open): the fenced objective, then what the
/// chain of every distinct task the batch assigns changes about the agent's standing processes.
///
/// Fails closed: when the must-follow documents exceed the instruction budget, or the processes
/// cannot be read, the batch must stay queued rather than reach the agent without them.
pub(super) fn compose_wake_turn(
    office: &Office,
    employee_id: &str,
    items: &[PendingWakeItem],
    still_open_asks: &[SuspendedAskEntry],
) -> Result<String, ApiError> {
    let objective = compose_coalesced_wake_objective(items, still_open_asks);
    let task_refs = assigned_task_refs(items);
    Ok(
        match crate::manager::task_process_context(office, employee_id, &task_refs)? {
            Some(delta) => format!("{objective}\n\n{delta}"),
            None => objective,
        },
    )
}
