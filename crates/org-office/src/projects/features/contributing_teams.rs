//! Adding and removing contributing teams on a project (data contract document 02, FC-030).

use org_store::{Actor, Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};
use serde::Serialize;

use crate::organization::{
    ops::{admit, changed, check_revision, reject, stable, touched, Op, RejectCode},
    ports::OrganizationRead,
};
use crate::projects::{ports::ProjectsRead, records};

use super::ProjectFeature;

#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct AddProjectTeam {
    pub project_id: Id,
    pub team_id: Id,
}

impl ProjectFeature for AddProjectTeam {
    const OP: Op = Op::AddProjectTeam;

    fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
        here: &OfficeIdentity,
        owner: &Id,
        by: &Actor,
    ) -> Result<Change, Refusal>
    where
        E::Tx: ProjectsRead,
    {
        stable(&self.project_id, "the project's ID")?;
        stable(&self.team_id, "the team's ID")?;
        let admitted = admit(uow, here, owner, by)?;
        let view = uow.view();
        let Some(mut project) = view.project(&self.project_id)? else {
            return reject(RejectCode::NotFound, "no such project");
        };
        if project.archived {
            return reject(RejectCode::InvalidRequest, "the project is archived");
        }
        let Some(team) = view.team(&self.team_id)? else {
            return reject(RejectCode::NotFound, "no such team");
        };
        if !team.is_active() {
            return reject(RejectCode::InvalidRequest, "the team is archived");
        }
        if project.owner_team_id == self.team_id {
            return reject(
                RejectCode::AlreadyExists,
                "the team is already the project's owning team",
            );
        }
        if project.contributing_team_ids.contains(&self.team_id) {
            return reject(
                RejectCode::AlreadyExists,
                "the team is already a contributing team on this project",
            );
        }
        project.contributing_team_ids.push(self.team_id.clone());
        project.contributing_team_ids.sort();
        project.meta = touched(&project.meta, &admitted.by, admitted.now);
        let reply = changed(&project.id, project.meta.revision, admitted.generation);
        Ok(Change::new(Op::AddProjectTeam.ack())
            .put(
                records::project_path(&project.id),
                records::encode_project(&project),
            )
            .reply(reply))
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct RemoveProjectTeam {
    pub project_id: Id,
    pub team_id: Id,
    pub expected_revision: u64,
}

impl ProjectFeature for RemoveProjectTeam {
    const OP: Op = Op::RemoveProjectTeam;

    fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
        here: &OfficeIdentity,
        owner: &Id,
        by: &Actor,
    ) -> Result<Change, Refusal>
    where
        E::Tx: ProjectsRead,
    {
        stable(&self.project_id, "the project's ID")?;
        stable(&self.team_id, "the team's ID")?;
        let admitted = admit(uow, here, owner, by)?;
        let view = uow.view();
        let Some(mut project) = view.project(&self.project_id)? else {
            return reject(RejectCode::NotFound, "no such project");
        };
        if project.archived {
            return reject(RejectCode::InvalidRequest, "the project is archived");
        }
        if project.owner_team_id == self.team_id {
            return reject(
                RejectCode::InvalidRequest,
                "cannot remove the project's accountable owning team; reassign ownership first",
            );
        }
        let Some(pos) = project
            .contributing_team_ids
            .iter()
            .position(|id| id == &self.team_id)
        else {
            return reject(
                RejectCode::NotFound,
                "that team is not a contributing team on this project",
            );
        };
        check_revision(project.meta.revision, self.expected_revision)?;
        project.contributing_team_ids.remove(pos);
        project.meta = touched(&project.meta, &admitted.by, admitted.now);
        let reply = changed(&project.id, project.meta.revision, admitted.generation);
        Ok(Change::new(Op::RemoveProjectTeam.ack())
            .put(
                records::project_path(&project.id),
                records::encode_project(&project),
            )
            .reply(reply))
    }
}
