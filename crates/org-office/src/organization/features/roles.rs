//! Role catalog creation and update (`FC-108`, `TASK-41` AC #3).

use org_store::{Actor, Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};
use serde::Serialize;

use super::{
    changed, invalid,
    shared::{admit, check_revision, id_in_use, reject, touched},
    stable, Feature, Op, RejectCode,
};
use crate::organization::{
    ports::OrganizationRead,
    records::{self, Meta, Role},
};

#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct CreateRole {
    pub id: Id,
    pub key: Option<String>,
    pub name: String,
    pub body: String,
}

impl Feature for CreateRole {
    const OP: Op = Op::CreateRole;

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
        decide_create(uow, here, owner, by, self)
    }
}

fn decide_create<E: Engine>(
    uow: &mut UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    by: &Actor,
    command: &CreateRole,
) -> Result<Change, Refusal>
where
    E::Tx: crate::projects::ports::ProjectsRead,
{
    stable(&command.id, "the role's ID")?;
    let admitted = admit(uow, here, owner, by)?;
    let view = uow.view();
    if !records::is_line(&command.name) {
        return reject(RejectCode::InvalidName, "the role name is one line of text");
    }
    if let Some(key) = &command.key {
        if !records::is_slug(key) {
            return reject(
                RejectCode::InvalidName,
                "a role key is lowercase letters, digits, and single inner hyphens",
            );
        }
        if view
            .roles()?
            .iter()
            .any(|r| r.key.as_deref() == Some(key.as_str()))
        {
            return reject(
                RejectCode::AlreadyExists,
                "a role with that key already exists",
            );
        }
    }
    if id_in_use(view, &admitted.organization, &command.id)? {
        return reject(RejectCode::AlreadyExists, "the role's ID is in use");
    }
    let role = Role {
        id: command.id.clone(),
        key: command.key.clone(),
        name: command.name.clone(),
        meta: Meta::created(&admitted.by, admitted.now),
        body: command.body.clone(),
    };
    let reply = changed(&role.id, role.meta.revision, admitted.generation);
    Ok(Change::new(Op::CreateRole.ack())
        .put(records::role_path(&role.id), records::encode_role(&role))
        .reply(reply))
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct UpdateRole {
    pub id: Id,
    pub expected_revision: u64,
    pub key: Option<String>,
    pub clear_key: bool,
    pub name: Option<String>,
    pub body: Option<String>,
}

impl Feature for UpdateRole {
    const OP: Op = Op::UpdateRole;

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
        decide_update(uow, here, owner, by, self)
    }
}

fn decide_update<E: Engine>(
    uow: &mut UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    by: &Actor,
    command: &UpdateRole,
) -> Result<Change, Refusal>
where
    E::Tx: crate::projects::ports::ProjectsRead,
{
    if command.key.is_some() && command.clear_key {
        return invalid("a role key is set or cleared, not both");
    }
    stable(&command.id, "the role's ID")?;
    let admitted = admit(uow, here, owner, by)?;
    let view = uow.view();
    let Some(role) = view.role(&command.id)? else {
        return reject(RejectCode::NotFound, "no such role");
    };
    if let Some(name) = &command.name {
        if !records::is_line(name) {
            return reject(RejectCode::InvalidName, "the role name is one line of text");
        }
    }
    if let Some(key) = &command.key {
        if !records::is_slug(key) {
            return reject(
                RejectCode::InvalidName,
                "a role key is lowercase letters, digits, and single inner hyphens",
            );
        }
        if view
            .roles()?
            .iter()
            .any(|r| r.id != role.id && r.key.as_deref() == Some(key.as_str()))
        {
            return reject(
                RejectCode::AlreadyExists,
                "a role with that key already exists",
            );
        }
    }
    check_revision(role.meta.revision, command.expected_revision)?;
    let next_key = match (&command.key, command.clear_key) {
        (Some(k), _) => Some(k.clone()),
        (None, true) => None,
        (None, false) => role.key.clone(),
    };
    let updated = Role {
        key: next_key,
        name: command.name.clone().unwrap_or(role.name),
        body: command.body.clone().unwrap_or(role.body),
        meta: touched(&role.meta, &admitted.by, admitted.now),
        ..role
    };
    let reply = changed(&updated.id, updated.meta.revision, admitted.generation);
    Ok(Change::new(Op::UpdateRole.ack())
        .put(
            records::role_path(&updated.id),
            records::encode_role(&updated),
        )
        .reply(reply))
}
