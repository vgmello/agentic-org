//! Declarative command rows for the work domain: `task`, `checklist`, and `task-note`
//! (shorthand `task note`).

mod checklist;
mod schema;
mod task_note;
mod task_read;
mod task_write;

use org_command::Command;

pub use checklist::CHECKLIST_COMMANDS;
pub use task_note::{TASK_NOTE_ALIAS, TASK_NOTE_COMMANDS};
pub use task_read::TASK_READ_COMMANDS;
pub use task_write::TASK_WRITE_COMMANDS;

/// All command row slices in the work domain, in canonical table order.
pub const ALL: &[&[Command]] = &[
    TASK_WRITE_COMMANDS,
    TASK_READ_COMMANDS,
    CHECKLIST_COMMANDS,
    TASK_NOTE_COMMANDS,
];
