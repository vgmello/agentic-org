//! Domain features for organization processes & standards (`structure_documents`, `TASK-44`,
//! `C5`, `doc-34`).

pub mod write;

pub use write::{build_write_operation, decide_structure_write};
