//! # org-office: the office's domain and storage
//!
//! Organizations, teams, agents, projects and tasks; SQLite (single writer: the server);
//! the workspace files agents read and write. A library with no entry point:
//! `apps/org-office-server` runs it.
//!
//! Knows nothing about sockets or the wire contract: callers hand it plain values and get
//! plain values back. Phase 1 holds the organization registry (`Office`).

mod error;
mod model;
mod name;
mod office;
mod private_fs;
mod store;
mod time;
mod ulid;

pub use error::{NameError, OfficeError};
pub use model::{OfficeInfo, Organization};
pub use name::MAX_NAME_CHARS;
pub use office::Office;
