//! Domain-neutral operation primitives shared by the `organization` and `projects` families on
//! the organization stream (acknowledgement classes, rejections, revision checks, and default
//! DoD item validation). Lives outside `organization::features` so sibling domains never reach
//! into another family's feature slice.

use org_store::{
    Ack, Actor, DurablePath, Engine, Id, OfficeIdentity, Operation, Refusal, Sha256, StoreError,
    Stream, Timestamp, UnitOfWork,
};
use serde::{Deserialize, Serialize};

use super::{
    names::name_key,
    ports::{OrganizationRead, TeamDependents, MAX_TEAM_DEPTH},
    records::{self, By, EmployeeKind, EmployeeStatus, Meta, OfficeRole, Organization},
};

/// The text of the unrecorded refusal of an actor who is not the organization's owner.
pub const NOT_AUTHORIZED: &str = "only the organization's owner changes its structure";

pub(crate) const TOO_DEEP: &str = "the team tree would be too deep";

/// What an admitted owner structure change knows before it reads its records.
pub(crate) struct Admitted {
    /// The directory generation the change becomes.
    pub generation: u64,
    pub now: Timestamp,
    pub organization: Organization,
    /// The owner, as records name who changed them.
    pub by: By,
}

/// Admits a structure change at HQ from the organization's owner (document 04, "Who may
/// change what"): `by` must be the employee `owner`, an active person on record. The
/// caller's grant authority has already checked the connection; this is the
/// organization's own check. Anything else is refused unrecorded.
pub(crate) fn admit<E: Engine>(
    uow: &mut UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    owner: &Id,
    by: &Actor,
) -> Result<Admitted, Refusal>
where
    E::Tx: OrganizationRead,
{
    let generation = uow.next(Stream::Organization)?;
    let now = uow.now();
    let view = uow.view();
    let Some(organization) = view.organization()? else {
        return invalid("there is no organization yet");
    };
    // HQ alone changes the structure: a satellite's copy is not an authority.
    let office = view.office(&here.office_id)?;
    if !office.is_some_and(|office| office.role == OfficeRole::Hq) {
        return invalid("only HQ changes the organization's structure");
    }
    let Actor::Employee(actor) = by else {
        return invalid(NOT_AUTHORIZED);
    };
    let person = view.employee(owner)?.filter(|person| {
        person.kind == EmployeeKind::Person && person.status == EmployeeStatus::Active
    });
    if actor != owner || person.is_none() {
        return invalid(NOT_AUTHORIZED);
    }
    Ok(Admitted {
        generation,
        now,
        organization,
        by: By::Employee(owner.clone()),
    })
}

/// Whether `id` names any record of the organization, whatever its kind.
pub(crate) fn id_in_use(
    view: &impl TeamDependents,
    organization: &Organization,
    id: &Id,
) -> Result<bool, StoreError> {
    Ok(*id == organization.id
        || view.team(id)?.is_some()
        || view.office(id)?.is_some()
        || view.employee(id)?.is_some()
        || view.role(id)?.is_some()
        || view.hire_request(id)?.is_some()
        || view.team_limit_request(id)?.is_some()
        || view.provider_connection(id)?.is_some()
        || view.model_profile(id)?.is_some()
        || view.project_exists(id)?
        || view.repositories()?.iter().any(|r| r.id == *id))
}

/// Whether another active team under `parent` (none: the top level) has `name`, ignoring
/// case. Archived teams keep their names without blocking them.
pub(crate) fn sibling_named(
    view: &impl OrganizationRead,
    parent: Option<&Id>,
    name: &str,
    except: &Id,
) -> Result<bool, StoreError> {
    let key = name_key(name);
    let teams = view.teams()?;
    Ok(teams.iter().any(|team| {
        team.id != *except
            && team.is_active()
            && team.parent_team_id.as_ref() == parent
            && name_key(&team.name) == key
    }))
}

/// The file of the team at the end of `chain`, or the rule's refusal when the tree is too
/// deep for the limit or for a path. A team is only where an employee's profile also fits
/// (its path is the longer), so every team can take a person or an agent.
pub(crate) fn team_file(chain: &[Id]) -> Result<DurablePath, Refusal> {
    let Some(last) = chain.last() else {
        return invalid("a team path names a team");
    };
    // Every stable ID has the same length: the team's own stands in for any employee's.
    let profiles_fit = [EmployeeKind::Person, EmployeeKind::Agent]
        .into_iter()
        .all(|kind| records::employee_path(chain, kind, last).is_ok());
    if chain.len() > MAX_TEAM_DEPTH || !profiles_fit {
        return reject(RejectCode::InvalidRequest, TOO_DEEP);
    }
    records::team_path(chain).or_else(|_| reject(RejectCode::InvalidRequest, TOO_DEEP))
}

macro_rules! define_ops {
    ($($variant:ident => $kind:literal),+ $(,)?) => {
        /// Every operation of the organization stream. The one place their acknowledgement classes
        /// are declared (ADR-0014).
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum Op { $($variant),+ }

        impl Op {
            pub const ALL: &'static [Op] = &[$(Op::$variant),+];

            /// Structure changes answer only once durable (ADR-0014).
            pub const fn ack(self) -> Ack { Ack::Durable }

            /// The kind a receipt records.
            pub const fn kind(self) -> &'static str {
                match self { $(Op::$variant => $kind),+ }
            }
        }
    };
}

define_ops!(
    CreateOrganization => "organization.create",
    ImportOrganization => "organization.import",
    CreateTeam => "organization.create_team",
    UpdateTeam => "organization.update_team",
    SetTeamMaxMembers => "organization.set_team_max_members",
    RequestTeamLimitIncrease => "organization.request_team_limit_increase",
    DecideTeamLimitRequest => "organization.decide_team_limit_request",
    MoveTeam => "organization.move_team",
    ArchiveTeam => "organization.archive_team",
    AddPerson => "organization.add_person",
    UpdatePerson => "organization.update_person",
    MovePerson => "organization.move_person",
    DeactivatePerson => "organization.deactivate_person",
    SetMayHire => "organization.set_may_hire",
    CreateRole => "organization.create_role",
    UpdateRole => "organization.update_role",
    DraftHire => "organization.draft_hire",
    ConfirmHire => "organization.confirm_hire",
    HireAgent => "organization.hire_agent",
    WithdrawHire => "organization.withdraw_hire",
    UpdateAgent => "organization.update_agent",
    MoveAgent => "organization.move_agent",
    SetAgentStatus => "organization.set_agent_status",
    OffboardAgent => "organization.offboard_agent",
    UpsertAgentExecution => "organization.upsert_agent_execution",
    CreateProviderConnection => "organization.create_provider_connection",
    UpdateProviderConnection => "organization.update_provider_connection",
    CreateModelProfile => "organization.create_model_profile",
    UpdateModelProfile => "organization.update_model_profile",
    SetWatchingModel => "organization.set_watching_model",
    CreateProject => "organization.create_project",
    AddProjectTeam => "organization.add_project_team",
    RemoveProjectTeam => "organization.remove_project_team",
    RegisterRepository => "organization.register_repository",
    AttachProjectRepository => "organization.attach_project_repository",
    RegisterSatelliteOffice => "organization.register_satellite_office",
    ApplyReplicatedFiles => "organization.apply_replicated_files",
    EnsureChiefOfStaff => "organization.ensure_chief_of_staff",
    SetOrganizationDefaultDod => "organization.set_default_dod",
    SetProjectDefaultDod => "organization.set_project_default_dod",
);

/// Which rule refused an admitted change, as the wire contract names its codes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectCode {
    NotFound,
    InvalidRequest,
    InvalidName,
    OwnerProtected,
    Cycle,
    NotEmpty,
    Conflict,
    AlreadyExists,
}

/// The reply of a change a rule refused. Recorded, so a retry gets the same answer.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrgRejection {
    pub code: RejectCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// The reply of a completed change to one team, employee, or project.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Changed {
    pub id: Id,
    /// The record's revision after the change.
    pub revision: u64,
    /// The directory generation the change is.
    pub generation: u64,
}

/// What a retry of `command` is matched on: its kind, who asked and every field of it.
pub(crate) fn operation(op: Op, id: Id, actor: Actor, command: &impl Serialize) -> Operation {
    let fields = serde_json::to_vec(command).expect("a command is plain data");
    Operation {
        id,
        kind: op.kind(),
        actor,
        fingerprint: Sha256::of_parts(&[op.kind().as_bytes(), b"\0", &fields]),
    }
}

/// A reply in its byte form: JSON.
pub(crate) fn reply(value: &impl Serialize) -> Vec<u8> {
    serde_json::to_vec(value).expect("a reply is plain data")
}

/// Refused before admission: nothing is recorded.
pub(crate) fn invalid<T>(reason: &str) -> Result<T, Refusal> {
    Err(Refusal::invalid(reason))
}

pub(crate) fn require(holds: bool, reason: &str) -> Result<(), Refusal> {
    if holds {
        return Ok(());
    }
    invalid(reason)
}

/// `id` is a stable ID, or the command is malformed.
pub(crate) fn stable(id: &Id, what: &str) -> Result<(), Refusal> {
    let reason = format!("{what} is a UUIDv7 in lowercase text");
    require(records::is_stable_id(id.as_str()), &reason)
}

/// A completed change's reply.
pub(crate) fn changed(id: &Id, revision: u64, generation: u64) -> Vec<u8> {
    reply(&Changed {
        id: id.clone(),
        revision,
        generation,
    })
}

/// A rule refused the change: recorded with its operation, so a retry gets this answer.
pub(crate) fn reject<T>(code: RejectCode, message: impl Into<String>) -> Result<T, Refusal> {
    let rejection = OrgRejection {
        code,
        message: message.into(),
        hint: None,
    };
    Err(Refusal::rejected(reply(&rejection)))
}

/// Like [`reject`], attaching an actionable client hint.
pub(crate) fn reject_with_hint<T>(
    code: RejectCode,
    message: impl Into<String>,
    hint: impl Into<String>,
) -> Result<T, Refusal> {
    let rejection = OrgRejection {
        code,
        message: message.into(),
        hint: Some(hint.into()),
    };
    Err(Refusal::rejected(reply(&rejection)))
}

/// `meta` after one more change by `by` at `now`.
pub(crate) fn touched(meta: &Meta, by: &By, now: Timestamp) -> Meta {
    Meta {
        revision: meta.revision + 1,
        updated_at: Some(records::time_text(now)),
        updated_by: by.clone(),
        ..meta.clone()
    }
}

/// A stale `expected_revision`: told after every other rule but `already_exists`.
pub(crate) fn check_revision(current: u64, expected: u64) -> Result<(), Refusal> {
    if current == expected {
        return Ok(());
    }
    reject(
        RejectCode::Conflict,
        "it changed since you last looked; refresh and try again",
    )
}

/// Default definition-of-done items (`TASK-88`) as `org_proto::tasks::normalize_dod_items`
/// leaves them, refused unrecorded otherwise: the office normalizes before it submits, so
/// only a command built around it gets here.
pub(crate) fn checked_dod_items(items: Option<&[String]>) -> Result<Option<Vec<String>>, Refusal> {
    let Some(items) = items else {
        return Ok(None);
    };
    match org_proto::tasks::normalize_dod_items(items) {
        Ok(normalized) if normalized == items => Ok(Some(normalized)),
        Ok(_) => invalid("a definition-of-done item is not trimmed"),
        Err(error) => invalid(&error.message),
    }
}
