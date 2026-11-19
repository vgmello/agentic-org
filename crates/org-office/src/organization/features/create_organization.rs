//! Creating the organization: the first entry of its acceptance journal (document 01,
//! "Creating an organization"). One operation writes the organization, its repository
//! registration, the HQ office and its code, one top-level team named after the
//! organization, the creator's person record as that team's lead, the built-in HR lead,
//! and the team's number with its counter and allocation. Every record is attributed to
//! the creator.

use org_store::{
    Actor, Change, Engine, Id, OfficeIdentity, Operation, Refusal, Stream, UnitOfWork,
};
use serde::{Deserialize, Serialize};

use super::{invalid, operation, reply, require, Op};
use crate::organization::{
    ports::OrganizationRead,
    records::{
        self, By, Employee, EmployeeKind, EmployeeStatus, Meta, Office, OfficeCode, OfficeRole,
        Organization, Repository, RepositoryKind, Team, TeamAllocation, TeamCounter, TeamStatus,
    },
};

/// The built-in HR lead's handle and name.
pub const HR_HANDLE: &str = "hr";
pub const HR_NAME: &str = "HR";
/// The body of the HR lead's profile.
pub const HR_BODY: &str =
    "HR bootstrap identity. Execution requires a separate verified configuration.\n";
/// The HQ office's slug and the code HQ issues itself.
pub const HQ_SLUG: &str = "hq";
pub const HQ_CODE: &str = "A";
/// The key of the organization repository's entry in `repositories.yaml` (data contract
/// document 01, "Repository types").
pub const ORGANIZATION_REPOSITORY_KEY: &str = "organization";

/// The person who creates the organization: the lead of its top-level team.
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct Creator {
    pub employee_id: Id,
    pub name: String,
    pub handle: String,
}

/// Every ID is minted by the caller, so a retry names the same records.
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct CreateOrganization {
    pub organization_id: Id,
    pub office_id: Id,
    pub repository_id: Id,
    pub team_id: Id,
    pub creator: Creator,
    pub hr_employee_id: Id,
    pub name: String,
    pub slug: String,
    pub repository_remote_url: String,
    pub state_branch: String,
    pub office_name: String,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrganizationCreated {
    pub organization_id: Id,
    /// The directory generation the creation is: always 1.
    pub generation: u64,
    pub office_code: String,
    pub team_id: Id,
    pub team_number: u64,
}

impl CreateOrganization {
    /// The identity a retry is matched on. The creator is the actor.
    pub fn operation(&self, id: Id) -> Operation {
        let actor = Actor::Employee(self.creator.employee_id.clone());
        operation(Op::CreateOrganization, id, actor, self)
    }

    /// The rules the command alone can break.
    fn check(&self) -> Result<(), Refusal> {
        let ids = [
            &self.organization_id,
            &self.office_id,
            &self.repository_id,
            &self.team_id,
            &self.creator.employee_id,
            &self.hr_employee_id,
        ];
        let stable = ids.iter().all(|id| records::is_stable_id(id.as_str()));
        require(stable, "every ID is a UUIDv7 in lowercase text")?;
        let distinct = ids.iter().enumerate().all(|(i, id)| !ids[..i].contains(id));
        require(distinct, "every record has its own ID")?;
        require(records::is_line(&self.name), "the name is one line of text")?;
        require(records::is_slug(&self.slug), "the slug is not a slug")?;
        let remote = records::is_remote_url(&self.repository_remote_url);
        require(remote, "the repository's remote URL is not a URL")?;
        let branch = records::is_branch(&self.state_branch);
        require(branch, "the state branch is not a branch name")?;
        let office = records::is_line(&self.office_name);
        require(office, "the office's name is one line of text")?;
        let creator = records::is_line(&self.creator.name);
        require(creator, "the creator's name is one line of text")?;
        let handle = records::is_handle(&self.creator.handle);
        require(handle, "the creator's handle is not a handle")?;
        let distinct = self.creator.handle != HR_HANDLE;
        require(distinct, "the creator's handle is the HR lead's")
    }
}

/// A new employee of the top-level team, at its first revision.
fn employee(
    (id, kind): (&Id, EmployeeKind),
    (name, handle): (&str, &str),
    team: &Id,
    meta: &Meta,
    body: &str,
) -> Employee {
    Employee {
        id: id.clone(),
        handle: handle.to_owned(),
        kind,
        name: name.to_owned(),
        home_team_id: team.clone(),
        role_id: None,
        reports_to_employee_id: None,
        status: EmployeeStatus::Active,
        contact: None,
        title: None,
        focus: None,
        role_text: None,
        title_in_team: None,
        offboarded_at: None,
        // A person's record is written with hiring power; the HR lead never has it.
        may_hire: kind == EmployeeKind::Person,
        meta: meta.clone(),
        body: body.to_owned(),
    }
}

/// Refuses with `Invalid` only: before the organization exists there is nothing to
/// record a rejection against, so a retry of a failed creation decides afresh.
pub fn decide<E: Engine>(
    uow: &mut UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    command: &CreateOrganization,
) -> Result<Change, Refusal>
where
    E::Tx: OrganizationRead,
{
    command.check()?;
    let ours =
        command.organization_id == here.organization_id && command.office_id == here.office_id;
    require(ours, "this store is another organization's or office's")?;
    if uow.view().organization()?.is_some() {
        return invalid("the organization already exists");
    }
    let generation = uow.next(Stream::Organization)?;
    require(
        generation == 1,
        "creation is the organization's first entry",
    )?;

    let now = uow.now();
    let at = records::time_text(now);
    let meta = Meta::created(&By::Employee(command.creator.employee_id.clone()), now);
    let organization = Organization {
        id: command.organization_id.clone(),
        slug: command.slug.clone(),
        name: command.name.clone(),
        organization_repository_id: command.repository_id.clone(),
        state_branch: command.state_branch.clone(),
        meta: meta.clone(),
    };
    let repository = Repository {
        id: command.repository_id.clone(),
        key: Some(ORGANIZATION_REPOSITORY_KEY.to_owned()),
        kind: RepositoryKind::Remote,
        remote_url: Some(command.repository_remote_url.clone()),
        meta: meta.clone(),
    };
    let office = Office {
        id: command.office_id.clone(),
        slug: HQ_SLUG.to_owned(),
        name: command.office_name.clone(),
        role: OfficeRole::Hq,
        meta: meta.clone(),
    };
    let code = OfficeCode {
        code: HQ_CODE.to_owned(),
        office_id: command.office_id.clone(),
        issued_at: at.clone(),
        retired_at: None,
    };
    // Named after the organization, and led by its creator.
    let team = Team {
        id: command.team_id.clone(),
        name: command.name.clone(),
        number: 1,
        parent_team_id: None,
        lead_employee_id: Some(command.creator.employee_id.clone()),
        status: TeamStatus::Active,
        meta: meta.clone(),
    };
    let creator = employee(
        (&command.creator.employee_id, EmployeeKind::Person),
        (&command.creator.name, &command.creator.handle),
        &team.id,
        &meta,
        "",
    );
    let hr = employee(
        (&command.hr_employee_id, EmployeeKind::Agent),
        (HR_NAME, HR_HANDLE),
        &team.id,
        &meta,
        HR_BODY,
    );
    let counter = TeamCounter {
        organization_id: organization.id.clone(),
        last_number: team.number,
    };
    let allocation = TeamAllocation {
        organization_id: organization.id.clone(),
        number: team.number,
        team_id: team.id.clone(),
        allocated_at: at,
    };
    let created = OrganizationCreated {
        organization_id: organization.id.clone(),
        generation,
        office_code: code.code.clone(),
        team_id: team.id.clone(),
        team_number: team.number,
    };

    let chain = [team.id.clone()];
    let profile = |employee: &Employee| records::employee_path(&chain, employee.kind, &employee.id);
    Ok(Change::new(Op::CreateOrganization.ack())
        .put(
            records::organization_path(),
            records::encode_organization(&organization),
        )
        .put(
            records::repositories_path(),
            records::encode_repositories(&[repository]),
        )
        .put(
            records::office_path(&office.id),
            records::encode_office(&office),
        )
        .put(
            records::office_codes_path(),
            records::encode_office_codes(&[code]),
        )
        .put(records::team_path(&chain)?, records::encode_team(&team))
        .put(profile(&creator)?, records::encode_employee(&creator))
        .put(profile(&hr)?, records::encode_employee(&hr))
        .put(
            records::team_counter_path(&organization.id),
            records::encode_team_counter(&counter),
        )
        .create(
            records::team_allocation_path(&organization.id, allocation.number),
            records::encode_team_allocation(&allocation),
        )
        .reply(reply(&created)))
}
