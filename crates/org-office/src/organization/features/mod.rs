//! The organization family's operations: one file each, with its command and its
//! decision. A decision reads through the unit of work and returns the files the
//! operation leaves behind, or a refusal; it holds every rule, because an engine may have
//! no constraint of its own.
//!
//! Every decision of this family takes one position of the organization stream: that
//! stream's high-water is the directory generation. A refusal gives the position back.
//!
//! Two kinds of refusal (B2 plan, D3). Before admission, unrecorded, so a retry decides
//! again: an ID that is not a stable ID, a malformed command, no organization, an office
//! that is not HQ, and an actor who is not the owner ([`NOT_AUTHORIZED`]). After
//! admission, every rule of the organization refuses with an [`OrgRejection`] that is
//! recorded, so a retry with the same operation ID gets the same answer. Rules are told in
//! the wire contract's order (`org_proto::org`, "When several errors apply"): `not_found`,
//! `invalid_request`, `invalid_name`, the domain codes (`owner_protected` first), a stale
//! revision (`conflict`), then `already_exists`.

pub mod add_person;
pub mod agent_lifecycle;
pub mod archive_team;
pub mod authority;
pub mod create_organization;
pub mod create_team;
pub mod deactivate_person;
pub mod ensure_chief_of_staff;
pub mod hiring;
pub mod may_hire;
pub mod move_person;
pub mod move_team;
pub mod offices;
pub mod provider_pools;
pub mod roles;
pub(crate) mod shared;
pub mod team_limit;
pub mod update_person;
pub mod update_team;

use org_store::UnitOfWork;
use org_store::{Ack, Actor, Change, Engine, Id, OfficeIdentity, Operation, Refusal, Sha256};
use serde::{Deserialize, Serialize};

pub use add_person::AddPerson;
pub use agent_lifecycle::{
    CreateProviderConnection, MoveAgent, OffboardAgent, SetAgentStatus, SetWatchingModel,
    UpdateAgent, UpsertAgentExecution,
};
pub use archive_team::ArchiveTeam;
pub use authority::HIRE_DENIED;
pub use create_organization::{CreateOrganization, Creator, OrganizationCreated};
pub use create_team::{CreateTeam, TeamCreated};
pub use deactivate_person::DeactivatePerson;
pub use ensure_chief_of_staff::{ChiefOfStaffEnsured, EnsureChiefOfStaff};
pub use hiring::{ConfirmHire, DraftHire, HireAgent, HireChanged, WithdrawHire};
pub use may_hire::SetMayHire;
pub use move_person::MovePerson;
pub use move_team::MoveTeam;
pub use provider_pools::{
    validate_endpoint_url, CreateModelProfile, UpdateModelProfile, UpdateProviderConnection,
};
pub use roles::{CreateRole, UpdateRole};
pub use team_limit::SetTeamMaxMembers;
pub use update_person::{Edit, UpdatePerson};
pub use update_team::UpdateTeam;

use crate::organization::ports::TeamDependents;

/// Every operation of the family. The one place their acknowledgement classes are
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
}

impl Op {
    pub const ALL: [Op; 36] = [
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
        }
    }
}

/// The text of the unrecorded refusal of an actor who is not the organization's owner.
pub const NOT_AUTHORIZED: &str = "only the organization's owner changes its structure";

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

/// The reply of a completed change to one team or employee.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Changed {
    pub id: Id,
    /// The record's revision after the change.
    pub revision: u64,
    /// The directory generation the change is.
    pub generation: u64,
}

/// A structure change the owner asks for: its operation and its decision. `owner` is the
/// owner's employee ID, which the office knows from the creation; `by` is who asked.
pub trait Feature: Serialize + Clone + Send + 'static {
    const OP: Op;

    fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
        here: &OfficeIdentity,
        owner: &Id,
        by: &Actor,
    ) -> Result<Change, Refusal>
    where
        E::Tx: TeamDependents;

    /// The identity a retry is matched on.
    fn operation(&self, id: Id, by: &Actor) -> Operation {
        operation(Self::OP, id, by.clone(), self)
    }
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

fn require(holds: bool, reason: &str) -> Result<(), Refusal> {
    if holds {
        return Ok(());
    }
    invalid(reason)
}

/// `id` is a stable ID, or the command is malformed.
pub(crate) fn stable(id: &Id, what: &str) -> Result<(), Refusal> {
    let reason = format!("{what} is a UUIDv7 in lowercase text");
    require(super::records::is_stable_id(id.as_str()), &reason)
}

/// A completed change's reply.
pub(crate) fn changed(id: &Id, revision: u64, generation: u64) -> Vec<u8> {
    reply(&Changed {
        id: id.clone(),
        revision,
        generation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_operation_declares_its_class_and_a_kind_of_its_own() {
        for op in Op::ALL {
            // Structure changes answer only once durable (ADR-0014).
            assert_eq!(op.ack(), Ack::Durable, "{op:?}");
            assert!(op.kind().starts_with("organization."), "{op:?}");
            let same = Op::ALL.iter().filter(|other| other.kind() == op.kind());
            assert_eq!(same.count(), 1, "{op:?}");
        }
    }

    #[test]
    fn a_retry_is_matched_on_the_kind_the_actor_and_every_field() {
        let id = |n: u64| Id::parse(&format!("01900000-0000-7000-8000-{n:012x}")).unwrap();
        let command = CreateTeam {
            id: id(1),
            name: "Core".into(),
            parent: Some(id(2)),
        };
        let by = Actor::Employee(id(3));
        let first = command.operation(id(9), &by);
        assert_eq!(first, command.clone().operation(id(9), &by));
        assert_eq!(first.kind, "organization.create_team");
        assert_eq!(first.actor, by);
        let renamed = CreateTeam {
            name: "Care".into(),
            ..command.clone()
        };
        assert_ne!(renamed.operation(id(9), &by).fingerprint, first.fingerprint);
        let moved = CreateTeam {
            parent: Some(id(4)),
            ..command.clone()
        };
        assert_ne!(moved.operation(id(9), &by).fingerprint, first.fingerprint);
        let top = CreateTeam {
            parent: None,
            ..command.clone()
        };
        assert_ne!(top.operation(id(9), &by).fingerprint, first.fingerprint);
        assert_ne!(command.operation(id(9), &Actor::System).actor, first.actor);

        // Keeping, setting and clearing a field are three different commands.
        let edit = |manager: Edit<Id>| UpdatePerson {
            manager,
            ..UpdatePerson::new(id(5), 1)
        };
        let prints: Vec<_> = [Edit::Keep, Edit::Set(id(6)), Edit::Clear]
            .into_iter()
            .map(|manager| edit(manager).operation(id(9), &by).fingerprint)
            .collect();
        assert!(prints[0] != prints[1] && prints[1] != prints[2] && prints[0] != prints[2]);
        let lead = |lead: Option<Id>, clear_lead: bool| UpdateTeam {
            lead,
            clear_lead,
            ..UpdateTeam::new(id(1), 1)
        };
        assert_ne!(
            lead(None, false).operation(id(9), &by).fingerprint,
            lead(None, true).operation(id(9), &by).fingerprint
        );
    }
}
