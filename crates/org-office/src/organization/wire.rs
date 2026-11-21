//! The native protocol's organization commands, mapped onto the organization family's
//! operations, and the family's answers mapped back. Pure: the office hands in a function
//! that runs the operation on its store, so this names no store and no engine.

use org_proto::{
    org::{
        CommandAccepted, CommandStatus, MemberAddPersonParams, MemberMoveParams,
        MemberRemoveParams, MemberSetReportsToParams, MemberUpdatePersonParams, TeamCreateParams,
        TeamDeleteParams, TeamMoveParams, TeamRenameParams, TeamSetLeadParams, TeamSetTitleParams,
    },
    ApiError, ErrorCode,
};
use org_store::{Actor, Id, Outcome, Reply, StoreError};

use super::{
    features::{
        AddPerson, ArchiveTeam, Changed, CreateTeam, DeactivatePerson, Edit, MovePerson, MoveTeam,
        OrgRejection, RejectCode, TeamCreated, UpdatePerson, UpdateTeam, NOT_AUTHORIZED,
    },
    operation_id, records, snapshot,
};
use crate::rules::OrgCommand;

/// A validated wire command mapped onto one organization-family feature.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StructureRequest<C> {
    /// The store operation: the wire `command_id` mapped by [`operation_id::from_wire_ulid`].
    pub operation_id: Id,
    pub by: Actor,
    pub command: C,
}

/// Backwards-compatible alias for [`StructureRequest<CreateTeam>`].
pub type TeamCreateRequest = StructureRequest<CreateTeam>;

/// Every organization-family operation reachable from wire `OrgCommand`s.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum WireOp {
    CreateTeam(StructureRequest<CreateTeam>),
    UpdateTeam(StructureRequest<UpdateTeam>),
    MoveTeam(StructureRequest<MoveTeam>),
    ArchiveTeam(StructureRequest<ArchiveTeam>),
    AddPerson(StructureRequest<AddPerson>),
    UpdatePerson(StructureRequest<UpdatePerson>),
    MovePerson(StructureRequest<MovePerson>),
    DeactivatePerson(StructureRequest<DeactivatePerson>),
}

fn refused(code: ErrorCode, message: &str) -> ApiError {
    ApiError::new(code, message)
}

fn invalid(message: &str) -> ApiError {
    refused(ErrorCode::InvalidRequest, message)
}

fn stable_id(text: &str) -> Option<Id> {
    records::is_stable_id(text)
        .then(|| Id::parse(text).ok())
        .flatten()
}

fn require_id(text: &str, field: &str) -> Result<Id, ApiError> {
    stable_id(text).ok_or_else(|| invalid(&format!("{field} is not a UUIDv7 in lowercase text")))
}

fn parse_header(member: &str, command_id: &str) -> Result<(Id, Actor), ApiError> {
    let op_id = operation_id::from_wire_ulid(command_id)
        .map_err(|_| invalid("command_id is not a ULID in its canonical form"))?;
    let Some(by) = stable_id(member) else {
        return Err(refused(ErrorCode::NotAuthorized, NOT_AUTHORIZED));
    };
    Ok((op_id, Actor::Employee(by)))
}

/// Derives a canonical lowercase handle from a person's display name when a caller omits
/// `handle` on `member.add_person`.
fn derive_handle(name: &str) -> String {
    let mut out = String::new();
    let mut prev_sep = false;
    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_sep = false;
        } else if !out.is_empty() && !prev_sep && matches!(ch, ' ' | '-' | '_' | '.') {
            out.push('-');
            prev_sep = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// The family's request for `team.create` from `member`, the member the caller's grant is
/// bound to. Refused with the protocol's answer when the request cannot be one.
pub fn team_create_request(
    member: &str,
    params: &TeamCreateParams,
) -> Result<TeamCreateRequest, ApiError> {
    let (operation_id, by) = parse_header(member, &params.command_id)?;
    let team = require_id(&params.team_id, "team_id")?;
    let Some(parent) = &params.owner_team_id else {
        return Err(invalid("a top-level team arrives on the wire with B2.2"));
    };
    let parent = require_id(parent, "owner_team_id")?;
    Ok(StructureRequest {
        operation_id,
        by,
        command: CreateTeam {
            id: team,
            name: params.name.clone(),
            parent: Some(parent),
        },
    })
}

fn team_rename_request(
    member: &str,
    params: &TeamRenameParams,
) -> Result<StructureRequest<UpdateTeam>, ApiError> {
    let (operation_id, by) = parse_header(member, &params.command_id)?;
    let id = require_id(&params.team_id, "team_id")?;
    let mut command = UpdateTeam::new(id, params.expected_version);
    command.name = Some(params.name.clone());
    Ok(StructureRequest {
        operation_id,
        by,
        command,
    })
}

fn team_set_lead_request(
    member: &str,
    params: &TeamSetLeadParams,
) -> Result<StructureRequest<UpdateTeam>, ApiError> {
    let (operation_id, by) = parse_header(member, &params.command_id)?;
    let id = require_id(&params.team_id, "team_id")?;
    let mut command = UpdateTeam::new(id, params.expected_version);
    match &params.lead_member_id {
        Some(lead) => command.lead = Some(require_id(lead, "lead_member_id")?),
        None => command.clear_lead = true,
    }
    Ok(StructureRequest {
        operation_id,
        by,
        command,
    })
}

fn team_move_request(
    member: &str,
    params: &TeamMoveParams,
) -> Result<StructureRequest<MoveTeam>, ApiError> {
    let (operation_id, by) = parse_header(member, &params.command_id)?;
    let id = require_id(&params.team_id, "team_id")?;
    let Some(parent) = &params.owner_team_id else {
        return Err(invalid("owner_team_id is required when moving a team"));
    };
    let parent = require_id(parent, "owner_team_id")?;
    Ok(StructureRequest {
        operation_id,
        by,
        command: MoveTeam {
            id,
            expected_revision: params.expected_version,
            parent,
        },
    })
}

fn team_delete_request(
    member: &str,
    params: &TeamDeleteParams,
) -> Result<StructureRequest<ArchiveTeam>, ApiError> {
    let (operation_id, by) = parse_header(member, &params.command_id)?;
    let id = require_id(&params.team_id, "team_id")?;
    Ok(StructureRequest {
        operation_id,
        by,
        command: ArchiveTeam {
            id,
            expected_revision: params.expected_version,
        },
    })
}

fn add_person_request(
    member: &str,
    params: &MemberAddPersonParams,
    default_home_team: Option<Id>,
) -> Result<StructureRequest<AddPerson>, ApiError> {
    let (operation_id, by) = parse_header(member, &params.command_id)?;
    let id = require_id(&params.member_id, "member_id")?;
    let home_team = match &params.home_team_id {
        Some(team) => require_id(team, "home_team_id")?,
        None => default_home_team
            .ok_or_else(|| invalid("home_team_id is required when no top-level team exists"))?,
    };
    let handle = params
        .handle
        .clone()
        .unwrap_or_else(|| derive_handle(&params.name));
    let mut command = AddPerson::new(id, &handle, &params.name, home_team);
    command.contact = params.contact.clone();
    Ok(StructureRequest {
        operation_id,
        by,
        command,
    })
}

fn update_person_request(
    member: &str,
    params: &MemberUpdatePersonParams,
) -> Result<StructureRequest<UpdatePerson>, ApiError> {
    let (operation_id, by) = parse_header(member, &params.command_id)?;
    if params.contact.is_some() && params.clear_contact {
        return Err(invalid("contact and clear_contact cannot both be set"));
    }
    let id = require_id(&params.member_id, "member_id")?;
    let mut command = UpdatePerson::new(id, params.expected_version);
    command.name = params.name.clone();
    command.contact = if params.clear_contact {
        Edit::Clear
    } else if let Some(contact) = &params.contact {
        Edit::Set(contact.clone())
    } else {
        Edit::Keep
    };
    Ok(StructureRequest {
        operation_id,
        by,
        command,
    })
}

fn set_reports_to_request(
    member: &str,
    params: &MemberSetReportsToParams,
) -> Result<StructureRequest<UpdatePerson>, ApiError> {
    let (operation_id, by) = parse_header(member, &params.command_id)?;
    let id = require_id(&params.member_id, "member_id")?;
    let mut command = UpdatePerson::new(id, params.expected_version);
    command.manager = match &params.reports_to_member_id {
        Some(manager) => Edit::Set(require_id(manager, "reports_to_member_id")?),
        None => Edit::Clear,
    };
    Ok(StructureRequest {
        operation_id,
        by,
        command,
    })
}

fn set_title_request(
    member: &str,
    params: &TeamSetTitleParams,
) -> Result<StructureRequest<UpdatePerson>, ApiError> {
    let (operation_id, by) = parse_header(member, &params.command_id)?;
    let id = require_id(&params.membership_id, "membership_id")?;
    let mut command = UpdatePerson::new(id, params.expected_version);
    command.title_in_team = match &params.title_in_team {
        Some(title) => Edit::Set(title.clone()),
        None => Edit::Clear,
    };
    Ok(StructureRequest {
        operation_id,
        by,
        command,
    })
}

fn move_person_request(
    member: &str,
    params: &MemberMoveParams,
) -> Result<StructureRequest<MovePerson>, ApiError> {
    let (operation_id, by) = parse_header(member, &params.command_id)?;
    let id = require_id(&params.membership_id, "membership_id")?;
    let home_team = require_id(&params.to_team_id, "to_team_id")?;
    Ok(StructureRequest {
        operation_id,
        by,
        command: MovePerson {
            id,
            expected_revision: params.expected_version,
            home_team,
        },
    })
}

fn remove_person_request(
    member: &str,
    params: &MemberRemoveParams,
) -> Result<StructureRequest<DeactivatePerson>, ApiError> {
    let (operation_id, by) = parse_header(member, &params.command_id)?;
    let id = require_id(&params.member_id, "member_id")?;
    Ok(StructureRequest {
        operation_id,
        by,
        command: DeactivatePerson {
            id,
            expected_revision: params.expected_version,
        },
    })
}

/// Decodes any implemented [`OrgCommand`] into a typed [`WireOp`]. Retired many-to-many
/// membership commands return `invalid_request`; unbuilt HR/project commands return
/// `unsupported`.
pub fn parse_command(
    member: &str,
    command: &OrgCommand,
    default_home_team: impl FnOnce() -> Result<Option<Id>, StoreError>,
) -> Result<Result<WireOp, ApiError>, StoreError> {
    let mapped = match command {
        OrgCommand::TeamCreate(p) => team_create_request(member, p).map(WireOp::CreateTeam),
        OrgCommand::TeamRename(p) => team_rename_request(member, p).map(WireOp::UpdateTeam),
        OrgCommand::TeamSetLead(p) => team_set_lead_request(member, p).map(WireOp::UpdateTeam),
        OrgCommand::TeamMove(p) => team_move_request(member, p).map(WireOp::MoveTeam),
        OrgCommand::TeamDelete(p) => team_delete_request(member, p).map(WireOp::ArchiveTeam),
        OrgCommand::MemberAddPerson(p) => {
            let team = if p.home_team_id.is_some() {
                None
            } else {
                default_home_team()?
            };
            add_person_request(member, p, team).map(WireOp::AddPerson)
        }
        OrgCommand::MemberUpdatePerson(p) => {
            update_person_request(member, p).map(WireOp::UpdatePerson)
        }
        OrgCommand::MemberSetReportsTo(p) => {
            set_reports_to_request(member, p).map(WireOp::UpdatePerson)
        }
        OrgCommand::TeamSetTitle(p) => set_title_request(member, p).map(WireOp::UpdatePerson),
        OrgCommand::MemberMove(p) => move_person_request(member, p).map(WireOp::MovePerson),
        OrgCommand::MemberRemove(p) => {
            remove_person_request(member, p).map(WireOp::DeactivatePerson)
        }
        OrgCommand::TeamAddMember(_) | OrgCommand::TeamRemoveMember(_) => Err(invalid(
            "employees belong to one home team: use member.add_person or member.move",
        )),
        OrgCommand::MemberAddAgent(_)
        | OrgCommand::MemberUpdateAgent(_)
        | OrgCommand::MemberSetAgentStatus(_)
        | OrgCommand::MemberOffboard(_)
        | OrgCommand::MemberSetRunMode(_)
        | OrgCommand::ProjectAddTeam(_)
        | OrgCommand::ProjectRemoveTeam(_) => Err(refused(
            ErrorCode::Unsupported,
            "this office does not serve this organization command yet",
        )),
    };
    Ok(mapped)
}

/// The protocol's code of a rule's refusal.
pub fn rejection_code(code: RejectCode) -> ErrorCode {
    match code {
        RejectCode::NotFound => ErrorCode::NotFound,
        RejectCode::InvalidRequest => ErrorCode::InvalidRequest,
        RejectCode::InvalidName => ErrorCode::InvalidName,
        RejectCode::OwnerProtected => ErrorCode::OwnerProtected,
        RejectCode::Cycle => ErrorCode::Cycle,
        RejectCode::NotEmpty => ErrorCode::NotEmpty,
        RejectCode::Conflict => ErrorCode::Conflict,
        RejectCode::AlreadyExists => ErrorCode::AlreadyExists,
    }
}

fn decode_answer(
    label: &str,
    answer: Result<Reply, StoreError>,
    generation_of: impl FnOnce(&[u8]) -> Option<u64>,
) -> Result<Result<CommandAccepted, ApiError>, StoreError> {
    let reply = match answer {
        Ok(reply) => reply,
        Err(StoreError::Conflict) => {
            let message = "command_id was used before with other params";
            return Ok(Err(refused(ErrorCode::InvalidRequest, message)));
        }
        Err(StoreError::Invalid(reason)) => {
            let code = if reason == NOT_AUTHORIZED {
                ErrorCode::NotAuthorized
            } else {
                ErrorCode::InvalidRequest
            };
            return Ok(Err(ApiError::new(code, reason)));
        }
        Err(error) => return Err(error),
    };
    let unreadable = || StoreError::Internal(format!("an unreadable {label} reply"));
    match reply.outcome {
        Outcome::Completed => {
            let generation = generation_of(&reply.bytes).ok_or_else(unreadable)?;
            Ok(Ok(CommandAccepted {
                status: CommandStatus::Committed,
                position: Some(snapshot::position(generation)),
            }))
        }
        Outcome::Rejected => {
            let rejected: OrgRejection =
                serde_json::from_slice(&reply.bytes).map_err(|_| unreadable())?;
            Ok(Err(ApiError::new(
                rejection_code(rejected.code),
                rejected.message,
            )))
        }
    }
}

/// The protocol's answer to `team.create` from the store's.
pub fn team_create_answer(
    answer: Result<Reply, StoreError>,
) -> Result<Result<CommandAccepted, ApiError>, StoreError> {
    decode_answer("team.create", answer, |bytes| {
        serde_json::from_slice::<TeamCreated>(bytes)
            .ok()
            .map(|c| c.generation)
    })
}

/// The protocol's answer to any organization mutation returning [`Changed`].
pub fn changed_answer(
    label: &str,
    answer: Result<Reply, StoreError>,
) -> Result<Result<CommandAccepted, ApiError>, StoreError> {
    decode_answer(label, answer, |bytes| {
        serde_json::from_slice::<Changed>(bytes)
            .ok()
            .map(|c| c.generation)
    })
}

/// `team.create` from `member`: the request, run by `run` (the office's store), and the
/// protocol's answer.
pub fn team_create(
    member: &str,
    params: &TeamCreateParams,
    run: impl FnOnce(TeamCreateRequest) -> Result<Reply, StoreError>,
) -> Result<Result<CommandAccepted, ApiError>, StoreError> {
    match team_create_request(member, params) {
        Ok(request) => team_create_answer(run(request)),
        Err(refusal) => Ok(Err(refusal)),
    }
}

#[cfg(test)]
mod tests {
    use org_store::testkit::id;

    use super::*;

    fn params() -> TeamCreateParams {
        TeamCreateParams {
            command_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            team_id: id(0x21).to_string(),
            name: "Core".into(),
            owner_team_id: Some(id(0x11).to_string()),
        }
    }

    fn reply(outcome: Outcome, bytes: &[u8]) -> Result<Reply, StoreError> {
        Ok(Reply {
            outcome,
            bytes: bytes.into(),
        })
    }

    #[test]
    fn the_request_is_the_mapped_operation_the_member_and_the_command() {
        let request = team_create_request(id(0x12).as_str(), &params()).unwrap();
        let expected = TeamCreateRequest {
            operation_id: operation_id::from_wire_ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap(),
            by: Actor::Employee(id(0x12)),
            command: CreateTeam {
                id: id(0x21),
                name: "Core".into(),
                parent: Some(id(0x11)),
            },
        };
        assert_eq!(request, expected);
    }

    #[test]
    fn a_request_that_cannot_be_one_is_answered_without_the_store() {
        let member = id(0x12).to_string();
        let refused = |params: TeamCreateParams, member: &str| {
            let ran = std::cell::Cell::new(false);
            let answer = team_create(member, &params, |_| {
                ran.set(true);
                Err(StoreError::Internal("not run".into()))
            });
            assert!(!ran.get());
            answer.expect("an answer").expect_err("a refusal").code
        };
        let top = TeamCreateParams {
            owner_team_id: None,
            ..params()
        };
        assert_eq!(refused(top, &member), ErrorCode::InvalidRequest);
        let lower = TeamCreateParams {
            command_id: "01arz3ndektsv4rrffq69g5fav".into(),
            ..params()
        };
        assert_eq!(refused(lower, &member), ErrorCode::InvalidRequest);
        let team = TeamCreateParams {
            team_id: "Team".into(),
            ..params()
        };
        assert_eq!(refused(team, &member), ErrorCode::InvalidRequest);
        let parent = TeamCreateParams {
            owner_team_id: Some("01ARZ3NDEKTSV4RRFFQ69G5FAV".into()),
            ..params()
        };
        assert_eq!(refused(parent, &member), ErrorCode::InvalidRequest);
        let ulid_member = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        assert_eq!(refused(params(), ulid_member), ErrorCode::NotAuthorized);
    }

    #[test]
    fn each_answer_of_the_store_has_its_protocol_answer() {
        let created = TeamCreated {
            id: id(0x21),
            number: 2,
            generation: 7,
        };
        let created = serde_json::to_vec(&created).unwrap();
        let committed = team_create_answer(reply(Outcome::Completed, &created));
        let accepted = CommandAccepted {
            status: CommandStatus::Committed,
            position: Some(snapshot::position(7)),
        };
        assert_eq!(committed, Ok(Ok(accepted)));

        let changed = serde_json::to_vec(&Changed {
            id: id(0x21),
            revision: 2,
            generation: 8,
        })
        .unwrap();
        assert_eq!(
            changed_answer("team.rename", reply(Outcome::Completed, &changed)),
            Ok(Ok(CommandAccepted {
                status: CommandStatus::Committed,
                position: Some(snapshot::position(8)),
            }))
        );

        let rejection = |code: RejectCode| OrgRejection {
            code,
            message: "the parent team is archived".into(),
        };
        for (code, expected) in [
            (RejectCode::NotFound, ErrorCode::NotFound),
            (RejectCode::InvalidRequest, ErrorCode::InvalidRequest),
            (RejectCode::InvalidName, ErrorCode::InvalidName),
            (RejectCode::OwnerProtected, ErrorCode::OwnerProtected),
            (RejectCode::Cycle, ErrorCode::Cycle),
            (RejectCode::NotEmpty, ErrorCode::NotEmpty),
            (RejectCode::Conflict, ErrorCode::Conflict),
            (RejectCode::AlreadyExists, ErrorCode::AlreadyExists),
        ] {
            let bytes = serde_json::to_vec(&rejection(code)).unwrap();
            let rejected = team_create_answer(reply(Outcome::Rejected, &bytes));
            let error = rejected.unwrap().unwrap_err();
            assert_eq!(error.code, expected);
            assert_eq!(error.message, "the parent team is archived");
        }

        let code = |error: StoreError| team_create_answer(Err(error)).unwrap().unwrap_err().code;
        assert_eq!(code(StoreError::Conflict), ErrorCode::InvalidRequest);
        assert_eq!(
            code(StoreError::invalid(NOT_AUTHORIZED)),
            ErrorCode::NotAuthorized
        );
        let other = StoreError::invalid("the team tree is too deep");
        let answer = team_create_answer(Err(other)).unwrap().unwrap_err();
        assert_eq!(answer.code, ErrorCode::InvalidRequest);
        assert_eq!(answer.message, "the team tree is too deep");

        for failure in [
            StoreError::Stopped {
                path: None,
                reason: "closed".into(),
            },
            StoreError::OutcomeUnknown { reason: "x".into() },
            StoreError::Custody("closed".into()),
        ] {
            assert_eq!(team_create_answer(Err(failure.clone())), Err(failure));
        }
        let garbled = team_create_answer(reply(Outcome::Completed, b"{}"));
        assert!(
            matches!(garbled, Err(StoreError::Internal(_))),
            "{garbled:?}"
        );
    }
}
