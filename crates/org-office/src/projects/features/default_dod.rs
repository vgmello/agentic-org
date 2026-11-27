//! A project's default definition of done (`TASK-88`): `None` inherits the organization's,
//! `Some([])` opts the project out, a list overrides. Set at HQ, at the project's revision, by
//! the owner or by the lead of the project's owning team or of any team above it;
//! `project.yaml` carries it to every office, so a satellite copies it offline.

use org_docs::MAX_FRONT_MATTER_BYTES;
use org_store::{Actor, Change, Engine, Id, OfficeIdentity, Refusal, Stream, UnitOfWork};
use serde::Serialize;

use crate::organization::{
    ops::{
        changed, check_revision, checked_dod_items, invalid, reject, stable, touched, Op,
        RejectCode,
    },
    ports::{OrganizationRead, MAX_TEAM_DEPTH},
    records::{By, EmployeeKind, EmployeeStatus, OfficeRole},
};
use crate::projects::{ports::ProjectsRead, records};

use super::ProjectFeature;

/// Why a caller who is neither the owner nor a lead above the project is refused, unrecorded.
pub const PROJECT_DOD_DENIED: &str = "only the owner, or the lead of the project's owning team \
     or of a team above it, sets the project's default definition of done";

/// Replaces a project's default definition of done.
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct SetProjectDefaultDod {
    pub project_id: Id,
    pub expected_revision: u64,
    pub default_dod: Option<Vec<String>>,
}

/// Whether `employee` leads `team` or an active team above it.
fn leads_chain<R: OrganizationRead>(view: &R, team: &Id, employee: &Id) -> Result<bool, Refusal> {
    let mut next = Some(team.clone());
    for _ in 0..MAX_TEAM_DEPTH {
        let Some(id) = next else {
            return Ok(false);
        };
        let Some(team) = view.team(&id)? else {
            return Ok(false);
        };
        if !team.is_active() {
            return Ok(false);
        }
        if team.lead_employee_id.as_ref() == Some(employee) {
            return Ok(true);
        }
        next = team.parent_team_id;
    }
    Ok(false)
}

impl ProjectFeature for SetProjectDefaultDod {
    const OP: Op = Op::SetProjectDefaultDod;

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
        let default_dod = checked_dod_items(self.default_dod.as_deref())?;
        let generation = uow.next(Stream::Organization)?;
        let now = uow.now();
        let view = uow.view();
        // HQ alone changes the structure: a satellite's copy is not an authority.
        let office = view.office(&here.office_id)?;
        if !office.is_some_and(|office| office.role == OfficeRole::Hq) {
            return invalid("only HQ changes the organization's structure");
        }
        let Actor::Employee(actor) = by else {
            return invalid(PROJECT_DOD_DENIED);
        };
        let active_person = view
            .employee(actor)?
            .is_some_and(|e| e.kind == EmployeeKind::Person && e.status == EmployeeStatus::Active);
        let Some(mut project) = view.project(&self.project_id)? else {
            return invalid(PROJECT_DOD_DENIED);
        };
        if !active_person || (actor != owner && !leads_chain(view, &project.owner_team_id, actor)?)
        {
            return invalid(PROJECT_DOD_DENIED);
        }
        if project.archived {
            return reject(RejectCode::InvalidRequest, "the project is archived");
        }
        check_revision(project.meta.revision, self.expected_revision)?;
        project.default_dod = default_dod;
        project.meta = touched(&project.meta, &By::Employee(actor.clone()), now);
        let bytes = records::encode_project(&project);
        if bytes.len() > MAX_FRONT_MATTER_BYTES {
            return reject(
                RejectCode::InvalidRequest,
                "the default definition of done does not fit the project record",
            );
        }
        let reply = changed(&project.id, project.meta.revision, generation);
        Ok(Change::new(Op::SetProjectDefaultDod.ack())
            .put(records::project_path(&project.id), bytes)
            .reply(reply))
    }
}
