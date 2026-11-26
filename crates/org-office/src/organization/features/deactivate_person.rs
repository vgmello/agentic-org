//! Deactivating a person: status `inactive`, `offboarded_at` set, and the profile and its
//! handle kept (document 04). The owner is never deactivated. A person who leads a team
//! or has direct reports is refused, naming what to clear; nothing changes implicitly
//! (owner decision Q2).

use org_store::{Actor, Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};
use serde::Serialize;

use super::{
    changed, invalid,
    shared::{admit, check_revision, led_by, named, reject, touched},
    stable, Feature, Op, RejectCode,
};
use crate::organization::{
    ports::{OrganizationRead, TeamDependents},
    records::{self, Employee, EmployeeKind, EmployeeStatus},
};

#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct DeactivatePerson {
    pub id: Id,
    pub expected_revision: u64,
}

impl Feature for DeactivatePerson {
    const OP: Op = Op::DeactivatePerson;

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
    command: &DeactivatePerson,
) -> Result<Change, Refusal>
where
    E::Tx: OrganizationRead,
{
    stable(&command.id, "the person's ID")?;
    let admitted = admit(uow, here, owner, by)?;
    let view = uow.view();
    let Some(person) = view.employee(&command.id)? else {
        return reject(RejectCode::NotFound, "no such person");
    };
    if person.kind != EmployeeKind::Person {
        return reject(RejectCode::InvalidRequest, "agents change through HR");
    }
    if person.status == EmployeeStatus::Inactive {
        return reject(RejectCode::InvalidRequest, "the person is already inactive");
    }
    if person.id == *owner {
        let rule = "the organization's owner is never deactivated";
        return reject(RejectCode::OwnerProtected, rule);
    }
    let led = led_by(view, &person.id)?;
    let employees = view.employees()?;
    let reports: Vec<&str> = employees
        .iter()
        .filter(|e| e.reports_to_employee_id.as_ref() == Some(&person.id))
        .filter(|e| e.status != EmployeeStatus::Inactive)
        .map(|e| e.handle.as_str())
        .collect();
    if led.is_some() || !reports.is_empty() {
        let mut clear = Vec::new();
        if let Some(team) = &led {
            clear.push(format!("the lead of {}", named(team)));
        }
        if !reports.is_empty() {
            clear.push(format!("the manager of {}", handles(&reports)));
        }
        let message = format!(
            "{} is {}: appoint another lead or manager first",
            person.name,
            clear.join(" and ")
        );
        return reject(RejectCode::NotEmpty, message);
    }
    check_revision(person.meta.revision, command.expected_revision)?;
    let Some(chain) = view.team_chain(&person.home_team_id)? else {
        return invalid("the home team has no chain of parents to a top-level team");
    };

    let inactive = Employee {
        status: EmployeeStatus::Inactive,
        offboarded_at: Some(records::time_text(admitted.now)),
        meta: touched(&person.meta, &admitted.by, admitted.now),
        ..person
    };
    let path = records::employee_path(&chain, inactive.kind, &inactive.id)?;
    let reply = changed(&inactive.id, inactive.meta.revision, admitted.generation);
    Ok(Change::new(Op::DeactivatePerson.ack())
        .put(path, records::encode_employee(&inactive))
        .reply(reply))
}

/// The first few reports by handle (`@a, @b and 3 more`): enough to say whose line to
/// change, never an unbounded message.
fn handles(reports: &[&str]) -> String {
    const NAMED: usize = 5;
    let named: Vec<String> = reports
        .iter()
        .take(NAMED)
        .map(|h| format!("@{h}"))
        .collect();
    let more = reports.len().saturating_sub(NAMED);
    match more {
        0 => named.join(", "),
        _ => format!("{} and {more} more", named.join(", ")),
    }
}
