//! Renaming a team and appointing or clearing its lead. A lead is a direct, active
//! employee of the team: its home team is this one (documents 01 and 04).

use org_store::{Actor, Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};
use serde::Serialize;

use super::{
    changed, invalid,
    shared::{admit, check_revision, reject, sibling_named, team_file, touched},
    stable, Feature, Op, RejectCode,
};
use crate::organization::{
    ports::OrganizationRead,
    records::{self, EmployeeStatus, Team},
};

#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct UpdateTeam {
    pub id: Id,
    pub expected_revision: u64,
    /// The new name; `None` keeps it.
    pub name: Option<String>,
    /// The new lead; `None` keeps the lead unless `clear_lead`.
    pub lead: Option<Id>,
    pub clear_lead: bool,
}

impl UpdateTeam {
    /// An update of `id` at `expected_revision` that changes nothing (and still counts).
    pub fn new(id: Id, expected_revision: u64) -> Self {
        Self {
            id,
            expected_revision,
            name: None,
            lead: None,
            clear_lead: false,
        }
    }
}

impl Feature for UpdateTeam {
    const OP: Op = Op::UpdateTeam;

    fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
        here: &OfficeIdentity,
        owner: &Id,
        by: &Actor,
    ) -> Result<Change, Refusal>
    where
        E::Tx: crate::projects::ports::ProjectsRead,
    {
        decide(uow, here, owner, by, self)
    }
}

pub fn decide<E: Engine>(
    uow: &mut UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    by: &Actor,
    command: &UpdateTeam,
) -> Result<Change, Refusal>
where
    E::Tx: OrganizationRead,
{
    if command.lead.is_some() && command.clear_lead {
        return invalid("a lead is appointed or cleared, not both");
    }
    stable(&command.id, "the team's ID")?;
    if let Some(lead) = &command.lead {
        stable(lead, "the lead's ID")?;
    }
    let admitted = admit(uow, here, owner, by)?;
    let view = uow.view();
    let Some(team) = view.team(&command.id)? else {
        return reject(RejectCode::NotFound, "no such team");
    };
    let lead = match &command.lead {
        Some(id) => match view.employee(id)? {
            Some(lead) => Some(lead),
            None => return reject(RejectCode::NotFound, "no such employee"),
        },
        None => None,
    };
    if !team.is_active() {
        return reject(RejectCode::InvalidRequest, "the team is archived");
    }
    if let Some(lead) = &lead {
        if lead.home_team_id != team.id || lead.status != EmployeeStatus::Active {
            let rule = "a team's lead is an active employee whose home team it is";
            return reject(RejectCode::InvalidRequest, rule);
        }
    }
    if command
        .name
        .as_deref()
        .is_some_and(|name| !records::is_line(name))
    {
        return reject(RejectCode::InvalidName, "the name is one line of text");
    }
    check_revision(team.meta.revision, command.expected_revision)?;
    if let Some(name) = &command.name {
        if sibling_named(view, team.parent_team_id.as_ref(), name, &team.id)? {
            let clash = "a team with that name already exists here";
            return reject(RejectCode::AlreadyExists, clash);
        }
    }
    let Some(chain) = view.team_chain(&team.id)? else {
        return invalid("the team has no chain of parents to a top-level team");
    };

    let lead_employee_id = match (&lead, command.clear_lead) {
        (Some(lead), _) => Some(lead.id.clone()),
        (None, true) => None,
        (None, false) => team.lead_employee_id.clone(),
    };
    let updated = Team {
        name: command.name.clone().unwrap_or_else(|| team.name.clone()),
        lead_employee_id,
        meta: touched(&team.meta, &admitted.by, admitted.now),
        ..team
    };
    let reply = changed(&updated.id, updated.meta.revision, admitted.generation);
    Ok(Change::new(Op::UpdateTeam.ack())
        .put(team_file(&chain)?, records::encode_team(&updated))
        .reply(reply))
}
