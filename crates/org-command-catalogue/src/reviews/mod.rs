//! Declarative command rows for universal reviews (`review`: 9 verbs, `TASK-92.1`).

mod read;
mod schema;
mod write;

use org_command::Command;

/// All review command row groups in canonical table order.
pub const ALL: &[&[Command]] = &[write::ROWS, read::ROWS];
