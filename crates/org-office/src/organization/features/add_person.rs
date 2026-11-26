//! Adding a person to a home team. People are added directly; agents come only through
//! HR (document 04). A new person is active, may hire, has no builtin key and reports to
//! nobody. The team's member limit is for hires only: adding a person is not limited
//! (owner decision Q4).

use org_store::{Actor, Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};
use serde::Serialize;

use super::{
    changed, invalid,
    shared::{admit, id_in_use, profile_file, reject},
    stable, Feature, Op, RejectCode,
};
use crate::organization::{
    ports::{OrganizationRead, TeamDependents},
    records::{self, Employee, EmployeeKind, EmployeeStatus, Meta},
};

#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct AddPerson {
    /// Minted by the caller, so a retry names the same person.
    pub id: Id,
    pub handle: String,
    pub name: String,
    pub home_team: Id,
    pub role: Option<Id>,
    pub contact: Option<String>,
    pub title: Option<String>,
    pub focus: Option<String>,
    pub role_text: Option<String>,
    pub title_in_team: Option<String>,
}

impl AddPerson {
    /// A person with only what a person must have.
    pub fn new(id: Id, handle: &str, name: &str, home_team: Id) -> Self {
        Self {
            id,
            handle: handle.to_owned(),
            name: name.to_owned(),
            home_team,
            role: None,
            contact: None,
            title: None,
            focus: None,
            role_text: None,
            title_in_team: None,
        }
    }

    fn texts(&self) -> [(&'static str, &Option<String>); 5] {
        [
            ("contact", &self.contact),
            ("title", &self.title),
            ("focus", &self.focus),
            ("role_text", &self.role_text),
            ("title_in_team", &self.title_in_team),
        ]
    }
}

impl Feature for AddPerson {
    const OP: Op = Op::AddPerson;

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
    command: &AddPerson,
) -> Result<Change, Refusal>
where
    E::Tx: TeamDependents,
{
    stable(&command.id, "the person's ID")?;
    stable(&command.home_team, "the home team's ID")?;
    if let Some(role) = &command.role {
        stable(role, "the role's ID")?;
    }
    let admitted = admit(uow, here, owner, by)?;
    let view = uow.view();
    let Some(team) = view.team(&command.home_team)? else {
        return reject(RejectCode::NotFound, "no such team");
    };
    if let Some(role) = &command.role {
        if view.role(role)?.is_none() {
            return reject(RejectCode::NotFound, "no such role");
        }
    }
    if !team.is_active() {
        return reject(RejectCode::InvalidRequest, "the team is archived");
    }
    for (field, text) in command.texts() {
        if text.as_deref().is_some_and(|text| !records::is_line(text)) {
            let rule = format!("{field} is one line of text");
            return reject(RejectCode::InvalidRequest, rule);
        }
    }
    if !records::is_line(&command.name) {
        return reject(RejectCode::InvalidName, "the name is one line of text");
    }
    if !records::is_handle(&command.handle) {
        let rule = "a handle is lowercase letters, digits, `-`, `_` and inner dots, \
                    and not a reserved word";
        return reject(RejectCode::InvalidName, rule);
    }
    // A handle is the person's for good: an inactive employee's stays taken.
    if view.handle_in_use(&command.handle)? {
        return reject(RejectCode::AlreadyExists, "the handle is taken");
    }
    if id_in_use(view, &admitted.organization, &command.id)? {
        return reject(RejectCode::AlreadyExists, "the person's ID is in use");
    }
    let Some(chain) = view.team_chain(&team.id)? else {
        return invalid("the team has no chain of parents to a top-level team");
    };

    let person = Employee {
        id: command.id.clone(),
        handle: command.handle.clone(),
        kind: EmployeeKind::Person,
        name: command.name.clone(),
        home_team_id: team.id,
        role_id: command.role.clone(),
        reports_to_employee_id: None,
        status: EmployeeStatus::Active,
        contact: command.contact.clone(),
        title: command.title.clone(),
        focus: command.focus.clone(),
        role_text: command.role_text.clone(),
        title_in_team: command.title_in_team.clone(),
        offboarded_at: None,
        may_hire: true,
        builtin_key: None,
        meta: Meta::created(&admitted.by, admitted.now),
        body: String::new(),
    };
    let path = profile_file(&chain, EmployeeKind::Person, &person.id)?;
    let reply = changed(&person.id, person.meta.revision, admitted.generation);
    Ok(Change::new(Op::AddPerson.ack())
        .put(path, records::encode_employee(&person))
        .reply(reply))
}
