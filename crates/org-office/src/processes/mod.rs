//! Organization processes and standards domain family (`TASK-44`, `C5`, `doc-34`, `doc-75`).
//!
//! Stores authored Markdown documents with YAML front matter under `durable/state/`:
//! - Organization: `state/processes/<id>.md` or `state/standards/<id>.md`
//! - Team: `state/teams/<root>/.../processes/<id>.md` or `standards/<id>.md`
//! - Project: `state/projects/<project-id>/processes/<id>.md` or `standards/<id>.md`
//!
//! Projected into `structure_documents` on SQLite and [`testkit::ProcessesTables`] on Memory.

pub mod ports;
pub mod projector;
pub mod records;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
