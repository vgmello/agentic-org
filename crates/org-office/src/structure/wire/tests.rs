use org_store::testkit::id;

use super::*;

fn params() -> TeamCreateParams {
    TeamCreateParams {
        command_id: id(0x01).to_string(),
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
        operation_id: id(0x01),
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
    assert_eq!(
        team_create_request(&member, &top).unwrap().command.parent,
        None
    );
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
    assert_eq!(code(StoreError::Conflict), ErrorCode::OperationReused);
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

#[test]
fn derive_handle_normalizes_punctuation_and_non_ascii() {
    assert_eq!(derive_handle("Ada Lovelace"), "ada-lovelace");
    assert_eq!(
        derive_handle("  Dr. Mary_Jane--O'Connor! "),
        "dr-mary-jane-oconnor"
    );
    assert_eq!(derive_handle("Éva 42"), "va-42");
    assert_eq!(derive_handle("   ---   "), "");
}

#[test]
fn parse_command_validates_all_wired_operations_and_defers_default_home_team_read() {
    let actor = id(0x12).to_string();
    let cmd_id = id(0x01).to_string();
    let no_read =
        || -> Result<Option<Id>, StoreError> { panic!("default_home_team should not be queried") };

    // Invalid header on MemberAddPerson must NOT call default_home_team().
    let bad_add = OrgCommand::MemberAddPerson(MemberAddPersonParams {
        command_id: "bad".into(),
        member_id: id(0x31).to_string(),
        name: "Ada".into(),
        contact: None,
        handle: None,
        home_team_id: None,
    });
    assert_eq!(
        parse_command(&actor, &bad_add, no_read)
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );

    // Omitted handle and home_team_id fall back cleanly.
    let fallback_add = OrgCommand::MemberAddPerson(MemberAddPersonParams {
        command_id: cmd_id.clone(),
        member_id: id(0x31).to_string(),
        name: "Ada Lovelace".into(),
        contact: Some("ada@example.test".into()),
        handle: None,
        home_team_id: None,
    });
    match parse_command(&actor, &fallback_add, || Ok(Some(id(0x11))))
        .unwrap()
        .unwrap()
    {
        WireOp::AddPerson(req) => {
            assert_eq!(req.command.handle, "ada-lovelace");
            assert_eq!(req.command.home_team, id(0x11));
            assert_eq!(req.command.contact.as_deref(), Some("ada@example.test"));
        }
        other => panic!("unexpected op: {other:?}"),
    }

    // MemberUpdatePerson refuses contact + clear_contact together, maps Keep/Set/Clear.
    let both = OrgCommand::MemberUpdatePerson(MemberUpdatePersonParams {
        command_id: cmd_id.clone(),
        member_id: id(0x31).to_string(),
        expected_version: 1,
        name: None,
        contact: Some("a@b".into()),
        clear_contact: true,
    });
    assert_eq!(
        parse_command(&actor, &both, no_read)
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    let clear = OrgCommand::MemberUpdatePerson(MemberUpdatePersonParams {
        command_id: cmd_id.clone(),
        member_id: id(0x31).to_string(),
        expected_version: 2,
        name: Some("Ada".into()),
        contact: None,
        clear_contact: true,
    });
    match parse_command(&actor, &clear, no_read).unwrap().unwrap() {
        WireOp::UpdatePerson(req) => {
            assert_eq!(req.command.name.as_deref(), Some("Ada"));
            assert_eq!(req.command.contact, Edit::Clear);
        }
        other => panic!("unexpected op: {other:?}"),
    }

    // TeamSetTitle and MemberSetReportsTo clear when None.
    let clear_title = OrgCommand::TeamSetTitle(TeamSetTitleParams {
        command_id: cmd_id.clone(),
        membership_id: id(0x31).to_string(),
        expected_version: 2,
        title_in_team: None,
    });
    match parse_command(&actor, &clear_title, no_read)
        .unwrap()
        .unwrap()
    {
        WireOp::UpdatePerson(req) => assert_eq!(req.command.title_in_team, Edit::Clear),
        other => panic!("unexpected op: {other:?}"),
    }
    let clear_mgr = OrgCommand::MemberSetReportsTo(MemberSetReportsToParams {
        command_id: cmd_id.clone(),
        member_id: id(0x31).to_string(),
        expected_version: 2,
        reports_to_member_id: None,
    });
    match parse_command(&actor, &clear_mgr, no_read).unwrap().unwrap() {
        WireOp::UpdatePerson(req) => assert_eq!(req.command.manager, Edit::Clear),
        other => panic!("unexpected op: {other:?}"),
    }

    // MemberMove refuses non-None title_in_team instead of silently dropping it.
    let move_with_title = OrgCommand::MemberMove(MemberMoveParams {
        command_id: cmd_id.clone(),
        membership_id: id(0x31).to_string(),
        expected_version: 2,
        new_membership_id: id(0x31).to_string(),
        to_team_id: id(0x22).to_string(),
        title_in_team: Some("Lead".into()),
    });
    assert_eq!(
        parse_command(&actor, &move_with_title, no_read)
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
}
