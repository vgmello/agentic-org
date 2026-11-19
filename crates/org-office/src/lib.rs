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
/// operations, its records and codec, its two ports and its projector. With the `testkit`
/// feature: its memory port and `organization_conformance_tests!`.
pub mod organization_family {
    pub use crate::organization::features::{
        self, create_organization, create_team, CreateOrganization, CreateTeam, Creator, Op,
        OrganizationCreated, TeamCreated, TeamRefusal, TeamRejected, TeamRejection,
    };
    pub use crate::organization::ports::{
        OrganizationEngine, OrganizationRead, OrganizationWrite, RecordRef, MAX_TEAM_DEPTH,
    };
    pub use crate::organization::projector::OrganizationFiles;
    pub use crate::organization::records;
    #[cfg(any(test, feature = "testkit"))]
    pub use crate::organization::testkit;
    /// The native protocol's side: wire command IDs as store operation IDs, `org.snapshot`
    /// from the records, and `team.create` onto the family and back.
    pub use crate::organization::{operation_id, snapshot, wire};
}
mod error;
mod key;
mod messaging;
mod model;
mod office;
mod office_file;
/// The office-local family on the office store: the HQ's wrapped CA key record and the
/// clean-stop marker under `durable/local/offices/<office-id>/`. With the `testkit`
/// feature: a CA fixture.
pub mod office_local;
mod owned_processes;
mod private_fs;
pub mod rules;
mod status;
mod store;
pub use store::acceptance::{
    AcceptanceConfig, AcceptanceIdentity, JournalPosition, WriterAuthority,
};
#[cfg(any(test, feature = "testkit"))]
pub use store::open::Testing;
/// Opening an office's store on the engine its configuration names, and the operations
/// run on it whichever engine that is.
pub use store::open::{
    open_store, CreateOrganizationOp, CreateTeamOp, DirectoryGeneration, DirectorySnapshot,
    EngineChoice, OfficeEngine, OfficeStore, Opened, ReadOp, StoreConfig, StoreOp,
};
/// The SQLite engine of the office store: the projection as one plain database file.
pub use store::sqlite::{SqliteEngine, SqliteSnapshot, SqliteTx};
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
