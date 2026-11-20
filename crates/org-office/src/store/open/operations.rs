//! The office's operations as [`StoreOp`]s, and its reads as [`ReadOp`]s. The composition
//! names the families; a family does not name the store's seam.

use org_store::{Actor, Id, OfficeIdentity, Reply, Store, StoreError};

use super::{OfficeEngine, ReadOp, StoreOp};
use crate::{
    office_local::{self, CaMaterial},
    organization::{
        features::{
            create_organization, AddPerson, ArchiveTeam, CreateOrganization, CreateTeam,
            DeactivatePerson, Feature, MovePerson, MoveTeam, UpdatePerson, UpdateTeam,
        },
        ports::OrganizationRead,
        records, snapshot,
    },
};

/// Creates the organization, with the HQ's CA records in the same entry: the
/// organization family's public escrow package and the office-local family's wrapped CA
/// key (ADR-0007, step 3). The creator is the actor.
pub struct CreateOrganizationOp {
    /// Minted by the caller: a retry with it gets the original reply.
    pub operation_id: Id,
    /// The office deciding: the store's own identity.
    pub here: OfficeIdentity,
    pub command: CreateOrganization,
    /// Frozen before the first attempt: a retry with other material conflicts.
    pub ca: CaMaterial,
}

impl StoreOp for CreateOrganizationOp {
    type Out = Result<Reply, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let Self {
            operation_id,
            here,
            command,
            ca,
        } = self;
        let key = ca.record(&here, &operation_id);
        key.check()
            .map_err(|why| StoreError::invalid(format!("the CA key record: {why}")))?;
        // The package must be this organization's and carry this public key: a mismatch
        // committed in entry 1 would leave an HQ that can never open (and nothing is
        // generated again).
        let sealed = records::ca_escrow_public_key(&ca.package, &here.organization_id)
            .map_err(|why| StoreError::invalid(format!("the CA escrow package: {why}")))?;
        if sealed != ca.public_key {
            let why = "the CA public key is not the escrow package's";
            return Err(StoreError::invalid(why));
        }
        let operation = key.bind(command.operation(operation_id));
        let key_path = office_local::records::ca_key_path(&here.office_id);
        let key = office_local::records::encode_ca_key(&key);
        store.submit(Some(operation), false, move |uow| {
            let change = create_organization::decide(uow, &here, &command)?;
            Ok(change
                .create(records::ca_escrow_path(), ca.package.clone())
                .create(key_path.clone(), key.clone()))
        })
    }
}

/// One structure change the organization's owner asks for: the family decides it, and
/// with it whether `by` may (only the owner, `owner`, does). The caller has authenticated
/// `by`; the organization decides what `by` may do.
pub struct StructureOp<C> {
    /// Minted by the caller: a retry with it gets the original reply.
    pub operation_id: Id,
    /// The office deciding: the store's own identity.
    pub here: OfficeIdentity,
    /// The owner's employee ID, as the creation binding names them.
    pub owner: Id,
    pub by: Actor,
    pub command: C,
}

impl<C: Feature> StoreOp for StructureOp<C> {
    type Out = Result<Reply, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let Self {
            operation_id,
            here,
            owner,
            by,
            command,
        } = self;
        let operation = command.operation(operation_id, &by);
        store.submit(Some(operation), false, move |uow| {
            command.decide(uow, &here, &owner, &by)
        })
    }
}

/// Creates a team, under another or at the top level.
pub type CreateTeamOp = StructureOp<CreateTeam>;
/// Renames a team, or appoints or clears its lead.
pub type UpdateTeamOp = StructureOp<UpdateTeam>;
/// Moves a team, its subtree and their profiles under another team.
pub type MoveTeamOp = StructureOp<MoveTeam>;
/// Archives an empty team.
pub type ArchiveTeamOp = StructureOp<ArchiveTeam>;
/// Adds a person to a home team.
pub type AddPersonOp = StructureOp<AddPerson>;
/// Changes a person's profile, role or manager.
pub type UpdatePersonOp = StructureOp<UpdatePerson>;
/// Moves a person to another home team.
pub type MovePersonOp = StructureOp<MovePerson>;
/// Deactivates a person.
pub type DeactivatePersonOp = StructureOp<DeactivatePerson>;

/// The directory generation of the snapshot read: the organization stream's high-water.
pub struct DirectoryGeneration;

impl ReadOp for DirectoryGeneration {
    type Out = u64;

    fn read<S: OrganizationRead>(self, snapshot: &S) -> Result<u64, StoreError> {
        Ok(snapshot.generation()?)
    }
}

/// The organization's structure, as `org.snapshot` answers it.
pub struct DirectorySnapshot {
    /// The member whose account owns the organization: the creator, as the creation
    /// binding names them.
    pub owner: Id,
}

impl ReadOp for DirectorySnapshot {
    type Out = org_proto::OrgSnapshot;

    fn read<S: OrganizationRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        Ok(snapshot::project(read, &self.owner)?)
    }
}

/// Where an office stands in the organization its projection holds.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Standing {
    /// The organization's ID, when there is an organization.
    pub organization: Option<Id>,
    /// Whether the office is on record as the organization's HQ.
    pub hq: bool,
}

/// What an opening checks before it serves: the organization and the office's role.
pub struct OfficeStanding {
    pub office: Id,
}

impl ReadOp for OfficeStanding {
    type Out = Standing;

    fn read<S: OrganizationRead>(self, read: &S) -> Result<Standing, StoreError> {
        let organization = read.organization()?.map(|record| record.id);
        let office = read.office(&self.office)?;
        let hq = office.is_some_and(|office| office.role == records::OfficeRole::Hq);
        Ok(Standing { organization, hq })
    }
}
