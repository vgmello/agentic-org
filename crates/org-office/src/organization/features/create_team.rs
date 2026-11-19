//! Creating a team under an existing one. HQ numbers it: the team, the advanced counter
//! and the number's immutable allocation are the files of one entry (document 06).

use org_store::{
    Actor, Change, Engine, Id, OfficeIdentity, Operation, Refusal, Stream, UnitOfWork,
};
use serde::{Deserialize, Serialize};

use super::{invalid, operation, reply, require, Op};
use crate::organization::{
    ports::{OrganizationRead, MAX_TEAM_DEPTH},
    records::{
        self, By, EmployeeKind, EmployeeStatus, Meta, OfficeRole, Team, TeamAllocation, TeamStatus,
    },
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

/// Why a creation was refused before admission. Nothing is recorded, so a retry decides
/// again. Each reason is the fixed text of a [`Refusal::Invalid`], so whoever answers the
/// caller can tell them apart ([`TeamRefusal::of`]); any other refusal is a malformed
/// command or a broken rule of the organization.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TeamRefusal {
    /// The parent team does not exist.
    NoSuchParent,
    /// The actor may not create a team there (see [`decide`]).
    NotAuthorized,
    /// The team's ID names another record of the organization.
    IdInUse,
    /// The name is not one line of text.
    BadName,
}

impl TeamRefusal {
    pub const ALL: [TeamRefusal; 4] = [
        TeamRefusal::NoSuchParent,
        TeamRefusal::NotAuthorized,
        TeamRefusal::IdInUse,
        TeamRefusal::BadName,
    ];

    pub const fn reason(self) -> &'static str {
        match self {
            TeamRefusal::NoSuchParent => "no such parent team",
            TeamRefusal::NotAuthorized => {
                "only an active person who leads the parent team or a team above it creates a team"
            }
            TeamRefusal::IdInUse => "the team's ID is in use",
            TeamRefusal::BadName => "the name is one line of text",
        }
    }

    /// The refusal whose reason is `reason`, the text of an [`org_store::StoreError::Invalid`].
    pub fn of(reason: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|refusal| refusal.reason() == reason)
    }
}

impl From<TeamRefusal> for Refusal {
    fn from(refusal: TeamRefusal) -> Self {
        Refusal::invalid(refusal.reason())
    }
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

/// Whether the organization lets `by` create a team, and where.
///
/// Until B2 brings the organization's own permissions, the rule is this one: an employee
/// creates a team only as an active person who leads the parent team or a team above it
/// (the creator of the organization leads the top-level team, so qualifies everywhere).
/// An agent never does. The system, which no wire request can act as, may. The caller has
/// authenticated `by`; the organization decides what `by` may do.
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
    if !records::is_line(&command.name) {
        return Err(TeamRefusal::BadName.into());
    }
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
    let actor = match by {
        Actor::Employee(id) => {
            let Some(employee) = view.employee(id)? else {
                return Err(TeamRefusal::NotAuthorized.into());
            };
            Some(employee)
        }
        Actor::System => None,
    };
    let Some(parent) = view.team(&command.parent)? else {
        return Err(TeamRefusal::NoSuchParent.into());
    };
    let Some(mut chain) = view.team_chain(&parent.id)? else {
        return invalid("the parent team has no chain of parents to a top-level team");
    };
    let by = match actor {
        Some(employee) => {
            let person =
                employee.kind == EmployeeKind::Person && employee.status == EmployeeStatus::Active;
            let mut led = Vec::with_capacity(chain.len());
            for team in &chain {
                led.push(view.team(team)?.and_then(|team| team.lead_employee_id));
            }
            let leads = led.iter().flatten().any(|lead| *lead == employee.id);
            if !(person && leads) {
                return Err(TeamRefusal::NotAuthorized.into());
            }
            By::Employee(employee.id)
        }
        None => By::System,
    };
    // An ID names one record of the organization, whatever its kind.
    let in_use = view.team(&command.id)?.is_some()
        || command.id == organization.id
        || view.office(&command.id)?.is_some()
        || view.employee(&command.id)?.is_some()
        || view.repositories()?.iter().any(|r| r.id == command.id);
    if in_use {
        return Err(TeamRefusal::IdInUse.into());
    }
    if !parent.is_active() {
        let rejected = TeamRejected {
            rejected: TeamRejection::ParentArchived,
        };
        return Err(Refusal::rejected(reply(&rejected)));
    }
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
        max_members: records::DEFAULT_MAX_MEMBERS,
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
