//! Domain-neutral operation primitives shared by the `organization` and `projects` families on
//! the organization stream (acknowledgement classes, rejections, revision checks, and default
//! DoD item validation). Lives outside `organization::features` so sibling domains never reach
//! into another family's feature slice.

use org_store::{Ack, Actor, Id, Operation, Refusal, Sha256, Timestamp};
use serde::{Deserialize, Serialize};

use super::records::{self, By, Meta};

/// Every operation of the organization stream. The one place their acknowledgement classes are
/// declared (ADR-0014).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    CreateOrganization,
    ImportOrganization,
    CreateTeam,
    UpdateTeam,
    SetTeamMaxMembers,
    MoveTeam,
    ArchiveTeam,
    AddPerson,
    UpdatePerson,
    MovePerson,
    DeactivatePerson,
    SetMayHire,
    CreateRole,
    UpdateRole,
    DraftHire,
    ConfirmHire,
    HireAgent,
    WithdrawHire,
    UpdateAgent,
    MoveAgent,
    SetAgentStatus,
    OffboardAgent,
    UpsertAgentExecution,
    CreateProviderConnection,
    UpdateProviderConnection,
    CreateModelProfile,
    UpdateModelProfile,
    SetWatchingModel,
    CreateProject,
    AddProjectTeam,
    RemoveProjectTeam,
    RegisterRepository,
    AttachProjectRepository,
    RegisterSatelliteOffice,
    ApplyReplicatedFiles,
    EnsureChiefOfStaff,
    SetOrganizationDefaultDod,
    SetProjectDefaultDod,
}

impl Op {
    pub const ALL: [Op; 38] = [
        Op::CreateOrganization,
        Op::ImportOrganization,
        Op::CreateTeam,
        Op::UpdateTeam,
        Op::SetTeamMaxMembers,
        Op::MoveTeam,
        Op::ArchiveTeam,
        Op::AddPerson,
        Op::UpdatePerson,
        Op::MovePerson,
        Op::DeactivatePerson,
        Op::SetMayHire,
        Op::CreateRole,
        Op::UpdateRole,
        Op::DraftHire,
        Op::ConfirmHire,
        Op::HireAgent,
        Op::WithdrawHire,
        Op::UpdateAgent,
        Op::MoveAgent,
        Op::SetAgentStatus,
        Op::OffboardAgent,
        Op::UpsertAgentExecution,
        Op::CreateProviderConnection,
        Op::UpdateProviderConnection,
        Op::CreateModelProfile,
        Op::UpdateModelProfile,
        Op::SetWatchingModel,
        Op::CreateProject,
        Op::AddProjectTeam,
        Op::RemoveProjectTeam,
        Op::RegisterRepository,
        Op::AttachProjectRepository,
        Op::RegisterSatelliteOffice,
        Op::ApplyReplicatedFiles,
        Op::EnsureChiefOfStaff,
        Op::SetOrganizationDefaultDod,
        Op::SetProjectDefaultDod,
    ];

    /// Structure changes answer only once durable (ADR-0014).
    pub const fn ack(self) -> Ack {
        Ack::Durable
    }

    /// The kind a receipt records.
    pub const fn kind(self) -> &'static str {
        match self {
            Op::CreateOrganization => "organization.create",
            Op::ImportOrganization => "organization.import",
            Op::CreateTeam => "organization.create_team",
            Op::UpdateTeam => "organization.update_team",
            Op::SetTeamMaxMembers => "organization.set_team_max_members",
            Op::MoveTeam => "organization.move_team",
            Op::ArchiveTeam => "organization.archive_team",
            Op::AddPerson => "organization.add_person",
            Op::UpdatePerson => "organization.update_person",
            Op::MovePerson => "organization.move_person",
            Op::DeactivatePerson => "organization.deactivate_person",
            Op::SetMayHire => "organization.set_may_hire",
            Op::CreateRole => "organization.create_role",
            Op::UpdateRole => "organization.update_role",
            Op::DraftHire => "organization.draft_hire",
            Op::ConfirmHire => "organization.confirm_hire",
            Op::HireAgent => "organization.hire_agent",
            Op::WithdrawHire => "organization.withdraw_hire",
            Op::UpdateAgent => "organization.update_agent",
            Op::MoveAgent => "organization.move_agent",
            Op::SetAgentStatus => "organization.set_agent_status",
            Op::OffboardAgent => "organization.offboard_agent",
            Op::UpsertAgentExecution => "organization.upsert_agent_execution",
            Op::CreateProviderConnection => "organization.create_provider_connection",
            Op::UpdateProviderConnection => "organization.update_provider_connection",
            Op::CreateModelProfile => "organization.create_model_profile",
            Op::UpdateModelProfile => "organization.update_model_profile",
            Op::SetWatchingModel => "organization.set_watching_model",
            Op::CreateProject => "organization.create_project",
            Op::AddProjectTeam => "organization.add_project_team",
            Op::RemoveProjectTeam => "organization.remove_project_team",
            Op::RegisterRepository => "organization.register_repository",
            Op::AttachProjectRepository => "organization.attach_project_repository",
            Op::RegisterSatelliteOffice => "organization.register_satellite_office",
            Op::ApplyReplicatedFiles => "organization.apply_replicated_files",
            Op::EnsureChiefOfStaff => "organization.ensure_chief_of_staff",
            Op::SetOrganizationDefaultDod => "organization.set_default_dod",
            Op::SetProjectDefaultDod => "organization.set_project_default_dod",
        }
    }
}

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
