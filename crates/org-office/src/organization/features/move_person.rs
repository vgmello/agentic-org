//! Moving a person to another home team: the profile moves to that team's folder. A
//! team's lead is not moved until the appointment is cleared or another lead appointed
//! (owner decision Q3); the refusal names the team. Moving is not limited by the team's
//! member limit (owner decision Q4).

use org_store::{Actor, Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};
use serde::Serialize;

use super::{
    changed, invalid,
    shared::{admit, check_revision, led_by, named, profile_file, reject, touched},
    stable, Feature, Op, RejectCode,
};
use crate::organization::{
    ports::{OrganizationRead, TeamDependents},
    records::{self, Employee, EmployeeKind, EmployeeStatus},
};

#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct MovePerson {
    pub id: Id,
    pub expected_revision: u64,
    pub home_team: Id,
}

impl Feature for MovePerson {
    const OP: Op = Op::MovePerson;

    fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
        here: &OfficeIdentity,
        owner: &Id,
        by: &Actor,
    ) -> Result<Change, Refusal>
    where
        E::Tx: TeamDependents,
    {
        decide(uow, here, owner, by, self)
    }
}

pub fn decide<E: Engine>(
    uow: &mut UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    by: &Actor,
    command: &MovePerson,
) -> Result<Change, Refusal>
where
    E::Tx: OrganizationRead,
{
    stable(&command.id, "the person's ID")?;
    stable(&command.home_team, "the home team's ID")?;
    let admitted = admit(uow, here, owner, by)?;
    let view = uow.view();
    let Some(person) = view.employee(&command.id)? else {
        return reject(RejectCode::NotFound, "no such person");
    };
    let Some(team) = view.team(&command.home_team)? else {
        return reject(RejectCode::NotFound, "no such team");
    };
    if person.kind != EmployeeKind::Person {
        return reject(RejectCode::InvalidRequest, "agents change through HR");
    }
    if person.status == EmployeeStatus::Inactive {
        return reject(RejectCode::InvalidRequest, "the person is inactive");
    }
    if !team.is_active() {
        return reject(RejectCode::InvalidRequest, "the team is archived");
    }
    if team.id != person.home_team_id {
        if let Some(led) = led_by(view, &person.id)? {
            let message = format!(
                "{} leads {}: clear the appointment or appoint another lead first",
                person.name,
                named(&led)
            );
            return reject(RejectCode::InvalidRequest, message);
        }
    }
    check_revision(person.meta.revision, command.expected_revision)?;
    if team.id == person.home_team_id {
        return reject(
            RejectCode::AlreadyExists,
            "the person is already in that team",
        );
    }
    let (Some(old), Some(new)) = (
        view.team_chain(&person.home_team_id)?,
        view.team_chain(&team.id)?,
    ) else {
        return invalid("a team has no chain of parents to a top-level team");
    };

    let from = records::employee_path(&old, person.kind, &person.id)?;
    let to = profile_file(&new, person.kind, &person.id)?;
    let moved = Employee {
        home_team_id: team.id,
        meta: touched(&person.meta, &admitted.by, admitted.now),
        ..person
    };
    let reply = changed(&moved.id, moved.meta.revision, admitted.generation);
    Ok(Change::new(Op::MovePerson.ack())
        .remove(from)
        .put(to, records::encode_employee(&moved))
        .reply(reply))
}
