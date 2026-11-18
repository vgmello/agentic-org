//! Creating a team under an existing one. HQ numbers it: the team, the advanced counter
//! and the number's immutable allocation are the files of one entry (document 06).

use org_store::{
    Actor, Change, Engine, Id, OfficeIdentity, Operation, Refusal, Stream, UnitOfWork,
};
use serde::{Deserialize, Serialize};

use super::{invalid, operation, reply, require, Op};
use crate::organization::{
    ports::{OrganizationRead, MAX_TEAM_DEPTH},
    records::{self, By, Meta, OfficeRole, Team, TeamAllocation, TeamStatus},
};

#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct CreateTeam {
    /// Minted by the caller, so a retry names the same team.
    pub id: Id,
    pub name: String,
    pub parent: Id,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamCreated {
    pub id: Id,
    /// The organization-wide `TEAM-<number>`.
    pub number: u64,
    /// The directory generation the creation is.
    pub generation: u64,
}

/// Why an admitted creation was rejected. Recorded, so a retry gets the same answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamRejection {
    ParentArchived,
}

/// The reply of a rejected creation.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamRejected {
    pub rejected: TeamRejection,
}

impl CreateTeam {
    /// The identity a retry is matched on.
    pub fn operation(&self, id: Id, by: &Actor) -> Operation {
        operation(Op::CreateTeam, id, by.clone(), self)
    }
}

/// The caller has authorized `by`; this decides whether the organization allows it.
pub fn decide<E: Engine>(
    uow: &mut UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    by: &Actor,
    command: &CreateTeam,
) -> Result<Change, Refusal>
where
    E::Tx: OrganizationRead,
{
    let stable = records::is_stable_id(command.id.as_str());
    require(stable, "the team's ID is a UUIDv7 in lowercase text")?;
    require(
        records::is_line(&command.name),
        "the name is one line of text",
    )?;
    let generation = uow.next(Stream::Organization)?;
    let now = uow.now();

    let view = uow.view();
    let Some(organization) = view.organization()? else {
        return invalid("there is no organization yet");
    };
    // HQ alone creates teams: a satellite's copy of the counter is not an authority.
    let office = view.office(&here.office_id)?;
    let headquarters = office.is_some_and(|office| office.role == OfficeRole::Hq);
    require(headquarters, "only HQ creates a team")?;
    let by = match by {
        Actor::Employee(id) => {
            let known = view.employee(id)?.is_some();
            require(known, "the actor is not an employee on record")?;
            By::Employee(id.clone())
        }
        Actor::System => By::System,
    };
    let Some(parent) = view.team(&command.parent)? else {
        return invalid("no such parent team");
    };
    // An ID names one record of the organization, whatever its kind.
    let in_use = view.team(&command.id)?.is_some()
        || command.id == organization.id
        || view.office(&command.id)?.is_some()
        || view.employee(&command.id)?.is_some()
        || view.repositories()?.iter().any(|r| r.id == command.id);
    require(!in_use, "the team's ID is in use")?;
    if !parent.is_active() {
        let rejected = TeamRejected {
            rejected: TeamRejection::ParentArchived,
        };
        return Err(Refusal::rejected(reply(&rejected)));
    }
    let Some(mut chain) = view.team_chain(&parent.id)? else {
        return invalid("the parent team has no chain of parents to a top-level team");
    };
    chain.push(command.id.clone());
    require(chain.len() <= MAX_TEAM_DEPTH, "the team tree is too deep")?;
    let Some(counter) = view.team_counter()? else {
        return invalid("the organization has no team counter");
    };
    let counter = counter.advance()?;
    let number = counter.last_number;
    if view.team_by_number(number)?.is_some() {
        return invalid("the next team number is already a team's");
    }

    let team = Team {
        id: command.id.clone(),
        name: command.name.clone(),
        number,
        parent_team_id: Some(parent.id),
        lead_employee_id: None,
        status: TeamStatus::Active,
        meta: Meta::created(&by, now),
    };
    let allocation = TeamAllocation {
        organization_id: organization.id.clone(),
        number,
        team_id: team.id.clone(),
        allocated_at: records::time_text(now),
    };
    let created = TeamCreated {
        id: team.id.clone(),
        number,
        generation,
    };
    // A tree within the depth limit but too deep for a path is refused here.
    Ok(Change::new(Op::CreateTeam.ack())
        .put(records::team_path(&chain)?, records::encode_team(&team))
        .put(
            records::team_counter_path(&organization.id),
            records::encode_team_counter(&counter),
        )
        .create(
            records::team_allocation_path(&organization.id, number),
            records::encode_team_allocation(&allocation),
        )
        .reply(reply(&created)))
}
