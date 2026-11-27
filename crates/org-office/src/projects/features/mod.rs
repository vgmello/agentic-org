//! Operations of the projects family: `CreateProject`, `AddProjectTeam`, `RemoveProjectTeam`,
//! `SetProjectDefaultDod`.

pub mod contributing_teams;
pub mod create_project;
pub mod default_dod;
pub mod repositories;

pub use contributing_teams::{AddProjectTeam, RemoveProjectTeam};
pub use create_project::{CreateProject, OwnerTeamChoice, ProjectCreated};
pub use default_dod::{SetProjectDefaultDod, PROJECT_DOD_DENIED};
pub use repositories::{resolve_repository, AttachProjectRepository, RegisterRepository};

use org_store::{Actor, Change, Engine, Id, OfficeIdentity, Operation, Refusal, UnitOfWork};
use serde::Serialize;

use crate::organization::features::{operation, Op};
use crate::projects::ports::ProjectsRead;

/// A project-family operation the owner asks for.
pub trait ProjectFeature: Serialize + Clone + Send + 'static {
    const OP: Op;

    fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
        here: &OfficeIdentity,
        owner: &Id,
        by: &Actor,
    ) -> Result<Change, Refusal>
    where
        E::Tx: ProjectsRead;

    fn operation(&self, id: Id, by: &Actor) -> Operation {
        operation(Self::OP, id, by.clone(), self)
    }
}
