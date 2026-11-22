//! Creating a project at HQ (data contract document 02, implementation plan B3).
//!
//! By default, creating a project also creates its accountable owning team named after the
//! project (under `parent_team_id`, defaulting to `TEAM-1`) in the same atomic transaction.
//! Naming an existing active team instead binds `owner_team_id` without creating a team.

use std::collections::BTreeSet;

use org_store::{Actor, Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};
use serde::{Deserialize, Serialize};

use crate::organization::{
    features::{
        invalid, reply,
        shared::{admit, id_in_use, reject, sibling_named, team_file},
        stable, Op, RejectCode,
    },
    names::name_key,
    records::{self as org_records, Meta, Team, TeamAllocation, TeamStatus},
};
use crate::projects::{
    ports::ProjectsRead,
    records::{self, Project, ProjectAllocation, ProjectCounter},
};

use super::ProjectFeature;

/// How the project's accountable owning team is chosen on creation.
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub enum OwnerTeamChoice {
    /// Create a new team named after the project under `parent_team_id` (or `TEAM-1` when `None`).
    CreateDefault {
        team_id: Id,
        parent_team_id: Option<Id>,
    },
    /// Bind an existing active team as the project's accountable owner.
    Existing(Id),
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct CreateProject {
    /// Caller-minted project UUIDv7.
    pub id: Id,
    pub name: String,
    /// Optional explicit lowercase slug; derived from `name` and deduplicated when `None`.
    pub slug: Option<String>,
    pub owner_team: OwnerTeamChoice,
    /// Optional active contributing teams (must be distinct and not the owning team).
    pub contributing_team_ids: Vec<Id>,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectCreated {
    pub id: Id,
    pub number: u64,
    pub key: String,
    pub slug: String,
    pub owner_team_id: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_team_number: Option<u64>,
    pub generation: u64,
}

impl ProjectFeature for CreateProject {
    const OP: Op = Op::CreateProject;

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
        decide(uow, here, owner, by, self)
    }
}

fn is_reserved_project_slug(slug: &str) -> bool {
    if matches!(slug, "add-team" | "remove-team") {
        return true;
    }
    if let Some(rest) = slug.strip_prefix("p-") {
        if !rest.is_empty() && !rest.starts_with('0') && rest.bytes().all(|b| b.is_ascii_digit()) {
            return true;
        }
    }
    false
}

fn allocate_slug(
    view: &impl ProjectsRead,
    explicit: Option<&str>,
    name: &str,
) -> Result<String, Refusal> {
    if let Some(slug) = explicit {
        if !org_records::is_slug(slug) {
            return reject(
                RejectCode::InvalidRequest,
                "project slug must be lowercase ASCII letters, digits, and single inner hyphens",
            );
        }
        if is_reserved_project_slug(slug) {
            return reject(
                RejectCode::InvalidName,
                "project slug is reserved and cannot match a project key or action route",
            );
        }
        if view.project_by_slug(slug)?.is_some() {
            return reject(
                RejectCode::AlreadyExists,
                "a project with that slug already exists",
            );
        }
        return Ok(slug.to_owned());
    }
    let base = records::derive_slug(name);
    if !is_reserved_project_slug(&base) && view.project_by_slug(&base)?.is_none() {
        return Ok(base);
    }
    for suffix in 2_u64..10_000 {
        let candidate = format!("{base}-{suffix}");
        if candidate.len() <= 64
            && !is_reserved_project_slug(&candidate)
            && view.project_by_slug(&candidate)?.is_none()
        {
            return Ok(candidate);
        }
    }
    invalid("could not allocate a unique project slug")
}

type CreatedTeamParts = (
    org_store::DurablePath,
    Team,
    org_records::TeamCounter,
    TeamAllocation,
);

fn validate_command_ids(command: &CreateProject) -> Result<(), Refusal> {
    stable(&command.id, "the project's ID")?;
    match &command.owner_team {
        OwnerTeamChoice::CreateDefault {
            team_id,
            parent_team_id,
        } => {
            stable(team_id, "the new owning team's ID")?;
            if team_id == &command.id {
                return reject(
                    RejectCode::InvalidRequest,
                    "the project and its new owning team must have distinct IDs",
                );
            }
            if let Some(parent) = parent_team_id {
                stable(parent, "the parent team's ID")?;
            }
        }
        OwnerTeamChoice::Existing(team_id) => stable(team_id, "the owning team's ID")?,
    }
    for team_id in &command.contributing_team_ids {
        stable(team_id, "a contributing team's ID")?;
    }
    Ok(())
}

fn resolve_owner_team(
    view: &impl ProjectsRead,
    organization: &org_records::Organization,
    by: &org_records::By,
    now: org_store::Timestamp,
    command: &CreateProject,
) -> Result<(Id, Option<CreatedTeamParts>), Refusal> {
    match &command.owner_team {
        OwnerTeamChoice::Existing(team_id) => {
            let Some(team) = view.team(team_id)? else {
                return reject(RejectCode::NotFound, "no such owning team");
            };
            if !team.is_active() {
                return reject(RejectCode::InvalidRequest, "the owning team is archived");
            }
            Ok((team.id, None))
        }
        OwnerTeamChoice::CreateDefault {
            team_id,
            parent_team_id,
        } => create_default_owner_team(
            view,
            organization,
            by,
            now,
            &command.name,
            team_id,
            parent_team_id.as_ref(),
        ),
    }
}

fn create_default_owner_team(
    view: &impl ProjectsRead,
    organization: &org_records::Organization,
    by: &org_records::By,
    now: org_store::Timestamp,
    name: &str,
    team_id: &Id,
    parent_team_id: Option<&Id>,
) -> Result<(Id, Option<CreatedTeamParts>), Refusal> {
    if id_in_use(view, organization, team_id)? {
        return reject(RejectCode::AlreadyExists, "the owning team's ID is in use");
    }
    let parent = match parent_team_id {
        Some(parent_id) => {
            let Some(parent) = view.team(parent_id)? else {
                return reject(RejectCode::NotFound, "no such parent team");
            };
            if !parent.is_active() {
                return reject(RejectCode::InvalidRequest, "the parent team is archived");
            }
            Some(parent)
        }
        None => view.team_by_number(1)?.filter(Team::is_active),
    };
    let (parent_id, mut chain) = match parent {
        Some(parent) => {
            let Some(above) = view.team_chain(&parent.id)? else {
                return invalid("the parent team has no chain of parents to a top-level team");
            };
            (Some(parent.id), above)
        }
        None => (None, Vec::new()),
    };
    if sibling_named(view, parent_id.as_ref(), name, team_id)? {
        return reject(
            RejectCode::AlreadyExists,
            "a team with that name already exists under the parent team",
        );
    }
    chain.push(team_id.clone());
    let path = team_file(&chain)?;
    let Some(team_counter) = view.team_counter()? else {
        return invalid("the organization has no team counter");
    };
    let next_team_counter = team_counter.advance()?;
    let team_number = next_team_counter.last_number;
    if view.team_by_number(team_number)?.is_some() {
        return invalid("the next team number is already a team's");
    }
    let team = Team {
        id: team_id.clone(),
        name: name.to_owned(),
        number: team_number,
        parent_team_id: parent_id,
        lead_employee_id: None,
        status: TeamStatus::Active,
        max_members: org_records::DEFAULT_MAX_MEMBERS,
        meta: Meta::created(by, now),
    };
    let team_allocation = TeamAllocation {
        organization_id: organization.id.clone(),
        number: team_number,
        team_id: team.id.clone(),
        allocated_at: records::time_text(now),
    };
    Ok((
        team.id.clone(),
        Some((path, team, next_team_counter, team_allocation)),
    ))
}

fn validate_contributing_teams(
    view: &impl ProjectsRead,
    owner_team_id: &Id,
    contributing_team_ids: &[Id],
) -> Result<Vec<Id>, Refusal> {
    let mut seen_contrib = BTreeSet::new();
    for team_id in contributing_team_ids {
        if team_id == owner_team_id {
            return reject(
                RejectCode::InvalidRequest,
                "the owning team is already accountable and cannot also be a contributing team",
            );
        }
        if !seen_contrib.insert(team_id) {
            return reject(
                RejectCode::InvalidRequest,
                "duplicate contributing team in project creation",
            );
        }
        let Some(team) = view.team(team_id)? else {
            return reject(RejectCode::NotFound, "no such contributing team");
        };
        if !team.is_active() {
            return reject(
                RejectCode::InvalidRequest,
                "a contributing team is archived",
            );
        }
    }
    let mut sorted = contributing_team_ids.to_vec();
    sorted.sort();
    Ok(sorted)
}

pub fn decide<E: Engine>(
    uow: &mut UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    by: &Actor,
    command: &CreateProject,
) -> Result<Change, Refusal>
where
    E::Tx: ProjectsRead,
{
    validate_command_ids(command)?;

    let admitted = admit(uow, here, owner, by)?;
    let view = uow.view();
    let organization = &admitted.organization;

    if !org_records::is_line(&command.name) {
        return reject(
            RejectCode::InvalidName,
            "the project name is one line of text",
        );
    }
    if id_in_use(view, organization, &command.id)? {
        return reject(RejectCode::AlreadyExists, "the project's ID is in use");
    }
    let wanted_name = name_key(&command.name);
    if view
        .projects()?
        .iter()
        .any(|existing| name_key(&existing.name) == wanted_name)
    {
        return reject(
            RejectCode::AlreadyExists,
            "a project with that name already exists",
        );
    }

    let slug = allocate_slug(view, command.slug.as_deref(), &command.name)?;
    let (owner_team_id, created_team) =
        resolve_owner_team(view, organization, &admitted.by, admitted.now, command)?;
    let contributing_team_ids =
        validate_contributing_teams(view, &owner_team_id, &command.contributing_team_ids)?;

    let project_counter = view.project_counter()?.unwrap_or(ProjectCounter {
        organization_id: organization.id.clone(),
        last_number: 0,
    });
    let next_project_counter = project_counter.advance()?;
    let project_number = next_project_counter.last_number;
    if view.project_by_number(project_number)?.is_some() {
        return invalid("the next project number is already a project's");
    }

    let project = Project {
        id: command.id.clone(),
        number: project_number,
        slug: slug.clone(),
        name: command.name.clone(),
        owner_team_id: owner_team_id.clone(),
        archived: false,
        development_process_id: None,
        contributing_team_ids,
        repositories: Vec::new(),
        meta: Meta::created(&admitted.by, admitted.now),
    };
    let project_allocation = ProjectAllocation {
        organization_id: organization.id.clone(),
        number: project_number,
        project_id: project.id.clone(),
        allocated_at: records::time_text(admitted.now),
    };

    let created_team_number = created_team.as_ref().map(|(_, t, _, _)| t.number);
    let created = ProjectCreated {
        id: project.id.clone(),
        number: project.number,
        key: project.key(),
        slug,
        owner_team_id,
        created_team_number,
        generation: admitted.generation,
    };

    let mut change = Change::new(Op::CreateProject.ack());
    if let Some((team_path, team, team_counter, team_alloc)) = created_team {
        change = change
            .put(team_path, org_records::encode_team(&team))
            .put(
                org_records::team_counter_path(&organization.id),
                org_records::encode_team_counter(&team_counter),
            )
            .create(
                org_records::team_allocation_path(&organization.id, team_alloc.number),
                org_records::encode_team_allocation(&team_alloc),
            );
    }
    Ok(change
        .put(
            records::project_path(&project.id),
            records::encode_project(&project),
        )
        .put(
            records::project_counter_path(&organization.id),
            records::encode_project_counter(&next_project_counter),
        )
        .create(
            records::project_allocation_path(&organization.id, project_number),
            records::encode_project_allocation(&project_allocation),
        )
        .reply(reply(&created)))
}
