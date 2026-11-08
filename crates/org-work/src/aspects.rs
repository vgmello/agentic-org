//! Per-field conflicts (§2.3). Each event changes a set of **aspects**; a call is refused with
//! `conflict` only when an event after its `expected_version` changed an aspect the call
//! writes. So a comment or a tick never makes a title edit stale, while two status moves do.

use org_proto::work::{FieldChange, WorkEvent, WorkEventKind};

/// Something an event can change.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Aspect {
    /// A plain field, by its wire name (`title`, `description`, `priority`, `team_id`, …).
    Field(String),
    Assignee,
    /// The status and what goes with it (resolution, blocked reason, done anyway).
    Status,
    Deleted,
    Watchers,
    Links,
    Comments,
    /// The office's waiting bookkeeping, which no call writes.
    Waiting,
    /// The checklist's set of steps (an add).
    Steps,
    StepContent(String),
    StepDone(String),
    StepPosition(String),
    StepTask(String),
    /// An event of a kind this build doesn't know: it may have changed anything.
    Any,
}

/// The aspects one event changed. An `Undone` event changed what the event it undid changed:
/// `undone` looks that event up.
pub(crate) fn aspects_of(
    event: &WorkEvent,
    undone: &mut dyn FnMut(u64) -> Option<WorkEvent>,
) -> Vec<Aspect> {
    use Aspect::*;
    match &event.change {
        WorkEventKind::Created => vec![],
        WorkEventKind::FieldsChanged { fields } => fields
            .iter()
            .map(|FieldChange { field, .. }| Field(field.clone()))
            .collect(),
        WorkEventKind::StatusChanged { .. } => vec![Status],
        WorkEventKind::Assigned { .. } => vec![Assignee],
        WorkEventKind::Linked { .. } | WorkEventKind::Unlinked { .. } => vec![Links],
        WorkEventKind::Comment { watch, .. } => {
            let mut a = vec![Comments];
            if *watch {
                a.push(Watchers);
            }
            a
        }
        WorkEventKind::StepsAdded { .. } => vec![Steps],
        WorkEventKind::StepEdited { step_id, .. } => vec![StepContent(step_id.clone())],
        WorkEventKind::StepChecked { step_id, .. } => vec![StepDone(step_id.clone())],
        WorkEventKind::StepMoved { step_id, .. } => vec![StepPosition(step_id.clone())],
        WorkEventKind::StepRemoved { step_id } => vec![
            StepContent(step_id.clone()),
            StepDone(step_id.clone()),
            StepPosition(step_id.clone()),
            StepTask(step_id.clone()),
        ],
        // Making a task also unticks the step (it follows the open task from then on).
        WorkEventKind::StepMadeTask { step_id, .. } => {
            vec![StepTask(step_id.clone()), StepDone(step_id.clone())]
        }
        WorkEventKind::StepTaskUnlinked { step_id, .. } => vec![StepTask(step_id.clone())],
        WorkEventKind::WaitingStarted { .. } | WorkEventKind::WaitingEnded { .. } => {
            vec![Waiting]
        }
        WorkEventKind::Watched { .. } => vec![Watchers],
        WorkEventKind::Undone { event_version } => match undone(*event_version) {
            // An undo never undoes an undo, so this doesn't recurse further.
            Some(target) => aspects_of(&target, &mut |_| None),
            None => vec![],
        },
        WorkEventKind::Deleted => vec![Deleted],
        // A kind this build doesn't know: assume it changed anything a call might write.
        _ => vec![Any],
    }
}
