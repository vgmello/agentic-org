//! The organization family's operations as [`StoreOp`]s, and its first read. The
//! composition names the family; the family does not name the store's seam.

use org_store::{Actor, Id, OfficeIdentity, Reply, Store, StoreError};

use super::{OfficeEngine, ReadOp, StoreOp};
use crate::organization::{
    features::{create_organization, create_team, CreateOrganization, CreateTeam},
    ports::OrganizationRead,
};

/// Creates the organization. The creator is the actor.
pub struct CreateOrganizationOp {
    /// Minted by the caller: a retry with it gets the original reply.
    pub operation_id: Id,
    /// The office deciding: the store's own identity.
    pub here: OfficeIdentity,
    pub command: CreateOrganization,
}

impl StoreOp for CreateOrganizationOp {
    type Out = Result<Reply, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let Self {
            operation_id,
            here,
            command,
        } = self;
        let operation = command.operation(operation_id);
        store.submit(Some(operation), false, move |uow| {
            create_organization::decide(uow, &here, &command)
        })
    }
}

/// Creates a team under an existing one. The caller has authorized `by`.
pub struct CreateTeamOp {
    /// Minted by the caller: a retry with it gets the original reply.
    pub operation_id: Id,
    /// The office deciding: the store's own identity.
    pub here: OfficeIdentity,
    pub by: Actor,
    pub command: CreateTeam,
}

impl StoreOp for CreateTeamOp {
    type Out = Result<Reply, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let Self {
            operation_id,
            here,
            by,
            command,
        } = self;
        let operation = command.operation(operation_id, &by);
        store.submit(Some(operation), false, move |uow| {
            create_team::decide(uow, &here, &by, &command)
        })
    }
}

/// The directory generation of the snapshot read: the organization stream's high-water.
pub struct DirectoryGeneration;

impl ReadOp for DirectoryGeneration {
    type Out = u64;

    fn read<S: OrganizationRead>(self, snapshot: &S) -> Result<u64, StoreError> {
        Ok(snapshot.generation()?)
    }
}
