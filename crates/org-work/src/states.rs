//! The state machines (§1.10): which statuses each kind has, which are terminal, and the task's
//! moves. Bugs and requests add their moves in their phases.

use org_proto::work::{ItemKind, ItemStatus};

/// Whether `status` ends a round for `kind` (§6): a task's `done`, a bug's `closed`, a request's
/// `approved`, `declined` or `merged`.
pub fn is_terminal(kind: ItemKind, status: ItemStatus) -> bool {
    use ItemStatus::*;
    match kind {
        ItemKind::Task => status == Done,
        ItemKind::Bug => status == Closed,
        ItemKind::Request => matches!(status, Approved | Declined | Merged),
        _ => false,
    }
}

/// Whether entering `status` starts a round's cycle time (§6): `in_progress` for tasks and
/// bugs (and a bug's `fixed`, when fixed by hand without passing through it), a request's
/// `scoping`.
pub fn starts_cycle(kind: ItemKind, status: ItemStatus) -> bool {
    use ItemStatus::*;
    match kind {
        ItemKind::Task => status == InProgress,
        ItemKind::Bug => matches!(status, InProgress | Fixed),
        ItemKind::Request => status == Scoping,
        _ => false,
    }
}

/// The statuses a task has.
pub fn is_task_status(status: ItemStatus) -> bool {
    use ItemStatus::*;
    matches!(status, Todo | InProgress | Blocked | Done)
}

/// The task's moves (`todo → in_progress ⇄ blocked → done`, and back; leaving `done` is a
/// reopen). Staying where it is isn't a move.
pub fn task_move_allowed(from: ItemStatus, to: ItemStatus) -> bool {
    use ItemStatus::*;
    matches!(
        (from, to),
        (Todo, InProgress)
            | (Todo, Blocked)
            | (Todo, Done)
            | (InProgress, Todo)
            | (InProgress, Blocked)
            | (InProgress, Done)
            | (Blocked, InProgress)
            | (Blocked, Todo)
            | (Blocked, Done)
            | (Done, Todo)
            | (Done, InProgress)
    )
}
