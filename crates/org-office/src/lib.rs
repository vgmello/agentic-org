//! # org-office: the office's domain and storage
//!
//! An office is one folder (`<home>/offices/<office-id>/`) with its own encrypted databases
//! (`org.db`: the organization's data; `office.db`: the office's own), one key, a lock that
//! makes the server the only owner, a workspace, and `office.json` marking it finished. A
//! library with no entry point: `apps/agentic-org-office` runs it.
//!
//! Knows nothing about sockets: callers hand it plain values and get plain values back.
//! `Office::create` / `open` / `discard` are the three ways into a folder; the engine
//! (`store.rs`: plain SQLite, one batching writer, owner-only files) is private. The
//! organization's rules (`rules`) speak the wire contract's types (`org_proto::org`), because
//! a satellite replays the very changes HQ logged.

mod employees;
pub mod hq_creation;
mod organization;
pub use organization::admission::{OwnerAccount, OwnerAuthority};
pub use organization::{activity, directory};
/// The organization family on the office store (`docs/design/office-store.md`): its
/// records and codec, its two ports and its projector.
pub mod organization_family {
    pub use crate::organization::ports::{
        OrganizationRead, OrganizationWrite, RecordRef, MAX_TEAM_DEPTH,
    };
    pub use crate::organization::projector::OrganizationFiles;
    pub use crate::organization::records;
    #[cfg(any(test, feature = "testkit"))]
    pub use crate::organization::testkit;
}
mod error;
mod key;
mod messaging;
mod model;
mod office;
mod office_file;
mod owned_processes;
mod private_fs;
pub mod rules;
mod status;
mod store;
pub use store::acceptance::{
    AcceptanceConfig, AcceptanceIdentity, JournalPosition, WriterAuthority,
};
mod time;
mod ulid;

pub use error::{NameError, OfficeError};
pub use key::Key;
pub use messaging::{OfficeDirectory, OfficeMessageStore};
pub use model::{NewOffice, OfficeInfo, OfficeRole};
pub use office::{Office, LOCK_WAIT};
pub use owned_processes::{OwnedProcess, OwnedProcessKey, SavedIdentity, SavedLeader, SavedTarget};
pub use status::presence_after_status;
pub use store::org::{OrganizationSnapshotExport, OrganizationSnapshotManifest, SnapshotStageMode};
