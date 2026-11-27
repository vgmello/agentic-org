//! The store's operations on default definitions of done (`TASK-88`): the organization's,
//! set by the owner, and a project's, set by the owner or a lead above the project. Both are
//! structure changes HQ decides; a task copies whichever applies when it is created.

use org_store::StoreError;

use super::operations::{ProjectOp, StructureOp};
use super::ReadOp;
use crate::organization::features::SetOrganizationDefaultDod;
use crate::projects::features::SetProjectDefaultDod;
use crate::tasks::ports::TasksRead;

/// Replaces the organization's default definition of done.
pub type SetOrganizationDefaultDodOp = StructureOp<SetOrganizationDefaultDod>;
/// Replaces a project's default definition of done.
pub type SetProjectDefaultDodOp = ProjectOp<SetProjectDefaultDod>;

/// The organization's default definition of done and its record's revision, once there is
/// an organization.
pub struct OrganizationDefaultDodRead;

impl ReadOp for OrganizationDefaultDodRead {
    type Out = Option<(Option<Vec<String>>, u64)>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        Ok(read
            .organization()?
            .map(|org| (org.default_dod, org.meta.revision)))
    }
}
