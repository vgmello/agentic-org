//! # org-office: the office's domain and storage
//!
//! Organizations, teams, agents, projects and tasks; encrypted SQLite (single writer: the server);
//! the workspace files agents read and write. A library with no entry point:
//! `apps/org-office-server` runs it.
//!
//! Knows nothing about sockets or the wire contract: callers hand it plain values and get
//! plain values back. Phase 1 holds the organization registry (`Office`).
//!
//! **Transitional:** this is the storage engine (encrypted SQLite, one batching writer, keys,
//! owner-only files) with the first registry-style `Office` on top of it. The office model
//! (`docs/design/offices-hq-and-satellites.md`) makes an office ONE folder with its own
//! `org.db` and `office.db`; the next piece replaces `Office`'s registry API with that, on the
//! same engine (`store.rs`, `key.rs`, `private_fs.rs`).

mod error;
mod key;
mod model;
mod name;
mod office;
mod private_fs;
mod store;
mod time;
mod ulid;

pub use error::{NameError, OfficeError};
pub use key::{InMemoryKeys, Key, KeyError, KeyProvider};
pub use model::{OfficeInfo, Organization};
pub use name::MAX_NAME_CHARS;
pub use office::Office;
