//! A bug follows its fix tasks (§2.4, "Bug follows fix tasks"): the automatic changes the
//! library makes in the same transaction as the change that triggers them. Each is its own
//! event with `auto: true`, no actor and a `cause` naming the triggering event, and each fires
//! only on a triggering **edge**, never on a scan, so an Undo is not re-applied by the trigger
//! that caused it.
//!
//! | Edge (a fix task...) | While the bug is | Change |
//! | --- | --- | --- |
//! | goes `in_progress`, or an `in_progress` task is linked with `fixes`; or `bug.triage` commits while one is `in_progress` | `triaged` | `in_progress` |
//! | goes `done`, or `bug.triage` commits, and now every fix task that isn't deleted is `done`, at least one `completed` | `triaged`, `in_progress` | `fixed`; with a required step unticked, no change and an event saying so |
//! | goes back from `done` | `fixed` | `in_progress` |
//!
//! Unlinking or deleting a fix task is no edge, and "still happens" or a reopen return to
//! `triaged` on purpose. If every fix task is Done · Cancelled there is no change.

use org_proto::work::{EventRef, ItemKind, ItemStatus, LinkKind, Resolution, WorkEventKind};

use crate::engine::{Ctx, Draft, Origin};
use crate::fail::Fail;
use crate::model::StoredItem;

/// What happened to a fix task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Edge {
    /// It went `in_progress`, or was linked while `in_progress`.
    Started,
    /// It went `done`.
    Done,
    /// It went back from `done`.
    Reopened,
}

/// Why a rule that was due changed nothing, recorded on its event.
pub(crate) const REQUIRED_LEFT: &str = "all fix tasks are done; required steps are left";

/// The bug's fix tasks that aren't deleted, with `substitute` standing for its own row (a task
/// whose new state the call has decided but not written yet).
fn fix_tasks(
    ctx: &mut Ctx,
    bug_id: &str,
    substitute: Option<&StoredItem>,
) -> Result<Vec<StoredItem>, Fail> {
    let mut tasks = ctx.txn.linked_to(bug_id, LinkKind::Fixes)?;
    if let Some(s) = substitute {
        match tasks.iter_mut().find(|t| t.id == s.id) {
            Some(t) => *t = s.clone(),
            None if s.fixes().iter().any(|b| b == bug_id) => tasks.push(s.clone()),
            None => {}
        }
    }
    tasks.retain(|t| !t.deleted);
    Ok(tasks)
}

/// Every fix task is done and at least one completed (cancelled ones don't fix anything).
fn all_fixed(tasks: &[StoredItem]) -> bool {
    tasks.iter().all(|t| t.status == ItemStatus::Done)
        && tasks
            .iter()
            .any(|t| t.resolution == Some(Resolution::Completed))
}

/// Moves the bug to `to` as an automatic change.
fn set(draft: &mut Draft, to: ItemStatus, cause: EventRef) {
    let from = draft.item.status;
    draft.item.status = to;
    if from == ItemStatus::Fixed {
        draft.item.bug.closed_reason = None;
        draft.item.bug.done_anyway = None;
    }
    draft.record(
        WorkEventKind::StatusChanged {
            from,
            to,
            resolution: None,
            reason: None,
            anyway: None,
        },
        Origin::Auto(cause),
    );
}

/// The rule for "every fix task is done": fixed, or, with a required step left, an event that
/// says why it isn't.
fn fix_done(draft: &mut Draft, cause: EventRef) {
    if !matches!(
        draft.item.status,
        ItemStatus::Triaged | ItemStatus::InProgress
    ) {
        return;
    }
    if draft.item.required_left().is_empty() {
        set(draft, ItemStatus::Fixed, cause);
    } else {
        let status = draft.item.status;
        draft.record(
            WorkEventKind::StatusChanged {
                from: status,
                to: status,
                resolution: None,
                reason: Some(REQUIRED_LEFT.to_owned()),
                anyway: None,
            },
            Origin::Auto(cause),
        );
    }
}

fn apply(draft: &mut Draft, edge: Edge, tasks: &[StoredItem], cause: EventRef) {
    match edge {
        Edge::Started => {
            if draft.item.status == ItemStatus::Triaged {
                set(draft, ItemStatus::InProgress, cause);
            }
        }
        Edge::Done => {
            if all_fixed(tasks) {
                fix_done(draft, cause);
            }
        }
        Edge::Reopened => {
            if draft.item.status == ItemStatus::Fixed {
                set(draft, ItemStatus::InProgress, cause);
            }
        }
    }
}

/// A fix task `task` (its state after the call) had these `edges`: applies them to each bug it
/// fixes, in `drafts` when the call already changes that bug, else in a new draft. `cause` is
/// the task's event.
pub(crate) fn follow_task(
    ctx: &mut Ctx,
    drafts: &mut Vec<Draft>,
    task: &StoredItem,
    edges: &[Edge],
    cause: &EventRef,
) -> Result<(), Fail> {
    for bug_id in task.fixes() {
        let existing = drafts.iter().position(|d| d.item.id == bug_id);
        let bug = match existing {
            Some(i) => drafts[i].item.clone(),
            None => match ctx.txn.get(&bug_id)? {
                Some(b) => b,
                None => continue,
            },
        };
        if bug.kind != ItemKind::Bug || bug.deleted {
            continue;
        }
        let tasks = fix_tasks(ctx, &bug_id, Some(task))?;
        match existing {
            Some(i) => {
                for edge in edges {
                    apply(&mut drafts[i], *edge, &tasks, cause.clone());
                }
            }
            None => {
                let mut draft = ctx.draft(bug);
                for edge in edges {
                    apply(&mut draft, *edge, &tasks, cause.clone());
                }
                if !draft.events.is_empty() {
                    drafts.push(draft);
                }
            }
        }
    }
    Ok(())
}

/// `bug.triage` committed (`draft` is the bug, now `triaged`): the fix tasks it already has
/// count at once.
pub(crate) fn after_triage(ctx: &mut Ctx, draft: &mut Draft, cause: EventRef) -> Result<(), Fail> {
    let tasks = fix_tasks(ctx, &draft.item.id.clone(), None)?;
    if all_fixed(&tasks) {
        apply(draft, Edge::Done, &tasks, cause);
    } else if tasks.iter().any(|t| t.status == ItemStatus::InProgress) {
        apply(draft, Edge::Started, &tasks, cause);
    }
    Ok(())
}
