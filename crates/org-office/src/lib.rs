//! # org-office: the office's domain and storage
//!
//! An office is one folder (`<home>/offices/<office-id>/`) with its own encrypted databases
//! (`org.db`: the organization's data; `office.db`: the office's own), one key, a lock that
//! makes the server the only owner, a workspace, and `office.json` marking it finished. A
//! library with no entry point: `apps/org-server` runs it.
//!
//! Knows nothing about sockets: callers hand it plain values and get plain values back.
//! `Office::create` / `open` / `discard` are the three ways into a folder; the engine
//! (`store.rs`: SQLCipher, one batching writer, keys, owner-only files) is private. The
//! organization's rules (`rules`) speak the wire contract's types (`org_proto::org`), because
//! a satellite replays the very changes HQ logged.

mod employees;
mod organization;
pub use organization::directory;
mod error;
mod key;
mod messaging;
mod model;
mod office;
mod office_file;
mod private_fs;
pub mod rules;
mod status;
mod store;
mod time;
mod ulid;

pub use error::{NameError, OfficeError};
pub use key::Key;
pub use messaging::{OfficeDirectory, OfficeMessageStore};
pub use model::{NewOffice, OfficeInfo, OfficeRole};
pub use office::{Office, LOCK_WAIT};
pub use status::presence_after_status;
pub use store::org::{OrganizationSnapshotExport, OrganizationSnapshotManifest};
