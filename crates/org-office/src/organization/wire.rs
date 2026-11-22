//! The native protocol's organization commands, mapped onto the family's operations, and
//! the family's answers mapped back. Pure: the office hands in a function that runs the
//! operation on its store, so this names no store and no engine.
//!
//! Until B2.2 moves the wire to one home team, the office answers `team.create` from the
//! family; every other organization command is `unsupported` and is not mapped here. A
//! top-level team is refused on the wire until then, though the family creates one.

use org_proto::{
    org::{CommandAccepted, CommandStatus, TeamCreateParams},
    ApiError, ErrorCode,
};
use org_store::{Actor, Id, Outcome, Reply, StoreError};

use super::{
    features::{CreateTeam, OrgRejection, RejectCode, TeamCreated, NOT_AUTHORIZED},
    operation_id, records, snapshot,
};

/// `team.create` as the family takes it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TeamCreateRequest {
    /// The store operation: the wire `command_id` mapped by [`operation_id::from_wire_ulid`].
    pub operation_id: Id,
    pub by: Actor,
    pub command: CreateTeam,
}

fn refused(code: ErrorCode, message: &str) -> ApiError {
    ApiError::new(code, message)
}

/// The family's request for `team.create` from `member`, the member the caller's grant is
/// bound to. Refused with the protocol's answer when the request cannot be one.
pub fn team_create_request(
    member: &str,
    params: &TeamCreateParams,
) -> Result<TeamCreateRequest, ApiError> {
    let invalid = |message: &str| refused(ErrorCode::InvalidRequest, message);
    let operation_id = operation_id::from_wire_ulid(&params.command_id)
        .map_err(|_| invalid("command_id is not a ULID in its canonical form"))?;
    // Every employee's ID is a stable ID: a member with another is nobody on record.
    let Some(by) = stable_id(member) else {
        return Err(refused(ErrorCode::NotAuthorized, NOT_AUTHORIZED));
    };
    let Some(team) = stable_id(&params.team_id) else {
        return Err(invalid("team_id is not a UUIDv7 in lowercase text"));
    };
    let Some(parent) = &params.owner_team_id else {
        return Err(invalid("a top-level team arrives on the wire with B2.2"));
    };
    let Some(parent) = stable_id(parent) else {
        return Err(invalid("owner_team_id is not a UUIDv7 in lowercase text"));
    };
    Ok(TeamCreateRequest {
        operation_id,
        by: Actor::Employee(by),
        command: CreateTeam {
            id: team,
            name: params.name.clone(),
            parent: Some(parent),
        },
    })
}

fn stable_id(text: &str) -> Option<Id> {
    records::is_stable_id(text)
        .then(|| Id::parse(text).ok())
        .flatten()
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

/// The protocol's answer to `team.create` from the store's. A failure of the store itself
/// (it stopped, its storage failed, the outcome is unknown) is not an answer: it is
/// returned as it is, for the office to report.
pub fn team_create_answer(
    answer: Result<Reply, StoreError>,
) -> Result<Result<CommandAccepted, ApiError>, StoreError> {
    let reply = match answer {
        Ok(reply) => reply,
        // The operation ID was used before for another command: no detail of that use.
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
    let unreadable = |what: &str| StoreError::Internal(format!("an unreadable {what} reply"));
    match reply.outcome {
        Outcome::Completed => {
            let created: TeamCreated =
                serde_json::from_slice(&reply.bytes).map_err(|_| unreadable("team.create"))?;
            Ok(Ok(CommandAccepted {
                status: CommandStatus::Committed,
                position: Some(snapshot::position(created.generation)),
            }))
        }
        Outcome::Rejected => {
            let rejected: OrgRejection =
                serde_json::from_slice(&reply.bytes).map_err(|_| unreadable("team.create"))?;
            let code = rejection_code(rejected.code);
            Ok(Err(ApiError::new(code, rejected.message)))
        }
    }
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

        // The store's own failures are not answers.
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
