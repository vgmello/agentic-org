//! # org-office: the office's domain and storage
//!
//! An office is one folder (`<home>/offices/<office-id>/`) holding its identity, its lock
//! and its creation intent, and the organization's data under two data roots, written
//! through one acceptance journal (`docs/design/office-store.md`, ADR-0021). A library with
//! no entry point: `apps/agentic-org-office` runs it.
//!
//! Knows nothing about sockets: callers hand it plain values and get plain values back.
//! [`Office::provision_hq`], [`Office::open`] and [`Office::discard`] are the three ways into
//! a folder. The organization family (`organization_family`) holds the organization's
//! rules; `rules` reads the native protocol's organization commands.

pub mod hq_creation;
mod organization;
pub use organization::directory;
/// The organization family on the office store (`docs/design/office-store.md`): its
/// operations, its records and codec, its two ports and its projector. With the `testkit`
/// feature: its memory port and `organization_conformance_tests!`.
pub mod organization_family {
    pub use crate::organization::features::{
        self, add_person, archive_team, create_organization, create_team, deactivate_person,
        move_person, move_team, update_person, update_team, AddPerson, ArchiveTeam, Changed,
        CreateOrganization, CreateTeam, Creator, DeactivatePerson, Edit, Feature, MovePerson,
        MoveTeam, Op, OrgRejection, OrganizationCreated, RejectCode, TeamCreated, UpdatePerson,
        UpdateTeam, NOT_AUTHORIZED,
    };
    pub use crate::organization::ports::{
        OrganizationEngine, OrganizationRead, OrganizationWrite, RecordRef, MAX_TEAM_DEPTH,
    };
    pub use crate::organization::projector::OrganizationFiles;
    pub use crate::organization::records;
    #[cfg(any(test, feature = "testkit"))]
    pub use crate::organization::testkit;
    /// The native protocol's side: `org.snapshot` from the records and organization commands
    /// mapped onto the family and back.
    pub use crate::organization::{snapshot, wire};
}
mod error;
mod key;
mod model;
mod office;
mod office_file;
/// The office-local family on the office store: the HQ's wrapped CA key record and the
/// clean-stop marker under `durable/local/offices/<office-id>/`. With the `testkit`
/// feature: a CA fixture.
pub mod office_local;
mod private_fs;
pub mod projects;
pub mod rules;
mod store;
#[cfg(any(test, feature = "testkit"))]
pub use store::open::Testing;
/// Opening an office's store on the engine its configuration names, and the operations
/// run on it whichever engine that is.
pub use store::open::{
    open_store, AddPersonOp, ArchiveTeamOp, CreateOrganizationOp, CreateTeamOp, DeactivatePersonOp,
    DirectoryGeneration, DirectorySnapshot, EngineChoice, MovePersonOp, MoveTeamOp, OfficeEngine,
    OfficeStanding, OfficeStore, ReadOp, Standing, StoreConfig, StoreOp, StructureOp,
    UpdatePersonOp, UpdateTeamOp,
};
mod time;

pub use error::{NameError, OfficeError};
pub use key::Key;
pub use model::{NewOffice, OfficeInfo, OfficeRole};
#[cfg(any(test, feature = "testkit"))]
pub use office::TestOffice;
pub use office::{Custody, DataRoots, Office, LOCK_WAIT};
