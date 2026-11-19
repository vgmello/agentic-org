//! Changing a person's profile, role and manager. Agents change through HR, never here
//! (B2 plan, D8). Reporting lines end: the manager is another active employee and no
//! line comes back to the person (document 04).

use org_store::{Actor, Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};
use serde::Serialize;

use super::{
    changed, invalid,
    shared::{admit, check_revision, reject, touched},
    Feature, Op, RejectCode,
};
use crate::organization::{
    ports::OrganizationRead,
    records::{self, Employee, EmployeeKind, EmployeeStatus},
};

/// What an update does to one optional field.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Edit<T> {
    #[default]
    Keep,
    Set(T),
    Clear,
}

impl<T: Clone> Edit<T> {
    fn applied(&self, current: &Option<T>) -> Option<T> {
        match self {
            Edit::Keep => current.clone(),
            Edit::Set(value) => Some(value.clone()),
            Edit::Clear => None,
        }
    }

    fn set(&self) -> Option<&T> {
        match self {
            Edit::Set(value) => Some(value),
            Edit::Keep | Edit::Clear => None,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct UpdatePerson {
    pub id: Id,
    pub expected_revision: u64,
    /// The new name; `None` keeps it.
    pub name: Option<String>,
    pub contact: Edit<String>,
    pub title: Edit<String>,
    pub focus: Edit<String>,
    pub role_text: Edit<String>,
    pub title_in_team: Edit<String>,
    pub role: Edit<Id>,
    /// Who the person reports to.
    pub manager: Edit<Id>,
}

impl UpdatePerson {
    /// An update of `id` at `expected_revision` that changes nothing (and still counts).
    pub fn new(id: Id, expected_revision: u64) -> Self {
        Self {
            id,
            expected_revision,
            name: None,
            contact: Edit::Keep,
            title: Edit::Keep,
            focus: Edit::Keep,
            role_text: Edit::Keep,
            title_in_team: Edit::Keep,
            role: Edit::Keep,
            manager: Edit::Keep,
        }
    }

    fn texts(&self) -> [(&'static str, &Edit<String>); 5] {
        [
            ("contact", &self.contact),
            ("title", &self.title),
            ("focus", &self.focus),
            ("role_text", &self.role_text),
            ("title_in_team", &self.title_in_team),
        ]
    }
}

impl Feature for UpdatePerson {
    const OP: Op = Op::UpdatePerson;

    fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
        here: &OfficeIdentity,
        owner: &Id,
        by: &Actor,
    ) -> Result<Change, Refusal>
    where
        E::Tx: OrganizationRead,
    {
        decide(uow, here, owner, by, self)
    }
}

pub fn decide<E: Engine>(
    uow: &mut UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    by: &Actor,
    command: &UpdatePerson,
) -> Result<Change, Refusal>
where
    E::Tx: OrganizationRead,
{
    let admitted = admit(uow, here, owner, by)?;
    let view = uow.view();
    let Some(person) = view.employee(&command.id)? else {
        return reject(RejectCode::NotFound, "no such person");
    };
    if let Some(role) = command.role.set() {
        if view.role(role)?.is_none() {
            return reject(RejectCode::NotFound, "no such role");
        }
    }
    let manager = match command.manager.set() {
        Some(id) => match view.employee(id)? {
            Some(manager) => Some(manager),
            None => return reject(RejectCode::NotFound, "no such manager"),
        },
        None => None,
    };
    if person.kind != EmployeeKind::Person {
        return reject(RejectCode::InvalidRequest, "agents change through HR");
    }
    if person.status == EmployeeStatus::Inactive {
        return reject(RejectCode::InvalidRequest, "the person is inactive");
    }
    for (field, edit) in command.texts() {
        if edit.set().is_some_and(|text| !records::is_line(text)) {
            let rule = format!("{field} is one line of text");
            return reject(RejectCode::InvalidRequest, rule);
        }
    }
    if let Some(manager) = &manager {
        if manager.id == person.id {
            return reject(RejectCode::InvalidRequest, "nobody reports to themselves");
        }
        if manager.status == EmployeeStatus::Inactive {
            let rule = "nobody new reports to an inactive employee";
            return reject(RejectCode::InvalidRequest, rule);
        }
    }
    if command.name.as_deref().is_some_and(|name| !records::is_line(name)) {
        return reject(RejectCode::InvalidName, "the name is one line of text");
    }
    if let Some(manager) = &manager {
        if reports_up_to(view, manager, &person.id)? {
            let rule = "a reporting line cannot come back to the person";
            return reject(RejectCode::Cycle, rule);
        }
    }
    check_revision(person.meta.revision, command.expected_revision)?;
    let Some(chain) = view.team_chain(&person.home_team_id)? else {
        return invalid("the home team has no chain of parents to a top-level team");
    };

    let updated = Employee {
        name: command.name.clone().unwrap_or_else(|| person.name.clone()),
        contact: command.contact.applied(&person.contact),
        title: command.title.applied(&person.title),
        focus: command.focus.applied(&person.focus),
        role_text: command.role_text.applied(&person.role_text),
        title_in_team: command.title_in_team.applied(&person.title_in_team),
        role_id: command.role.applied(&person.role_id),
        reports_to_employee_id: command.manager.applied(&person.reports_to_employee_id),
        meta: touched(&person.meta, &admitted.by, admitted.now),
        ..person
    };
    let path = records::employee_path(&chain, updated.kind, &updated.id)?;
    let reply = changed(&updated.id, updated.meta.revision, admitted.generation);
    Ok(Change::new(Op::UpdatePerson.ack())
        .put(path, records::encode_employee(&updated))
        .reply(reply))
}

/// Whether `id` is `from` or anyone `from` reports to, however far up.
fn reports_up_to(
    view: &impl OrganizationRead,
    from: &Employee,
    id: &Id,
) -> Result<bool, Refusal> {
    let mut next = Some(from.clone());
    // A line on record ends (validate); the bound only keeps a damaged one finite.
    for _ in 0..=view.employees()?.len() {
        let Some(employee) = next else {
            return Ok(false);
        };
        if employee.id == *id {
            return Ok(true);
        }
        next = match &employee.reports_to_employee_id {
            Some(above) => view.employee(above)?,
            None => None,
        };
    }
    invalid("a reporting line on record is a cycle")
}
