use std::io::Cursor;

use serde_json::json;

use crate::frame::{read_line, read_message, write_message, FrameError};
use crate::*;

fn org() -> Organization {
    Organization {
        id: "01M6WEQ98NHCE6ZVZWJGWQAJT3".into(),
        name: "Acme".into(),
        created_at: "2026-11-08T02:10:00.000Z".into(),
        offices: vec![hq()],
    }
}

fn hq() -> OfficeInfo {
    OfficeInfo {
        id: "01M6WGY1WJCC503RXF2NS0SH83".into(),
        organization_id: "01M6WEQ98NHCE6ZVZWJGWQAJT3".into(),
        name: "HQ".into(),
        role: OfficeRole::Hq,
        host: "This Mac".into(),
        endpoint: Some("/tmp/o1/run/3RXF2NS0SH83.sock".into()),
        status: OfficeStatus::Active,
    }
}

#[test]
fn request_wire_shape_and_typed_params() {
    let req = Request::new::<OrganizationCreate>(
        7,
        &OrganizationCreateParams {
            name: "Acme".into(),
        },
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&req).unwrap(),
        json!({"id": 7, "method": "organization.create", "params": {"name": "Acme"}})
    );
    assert!(req.is::<OrganizationCreate>());
    assert!(!req.is::<Hello>());
    assert_eq!(req.params::<OrganizationCreate>().unwrap().name, "Acme");
}

#[test]
fn missing_or_null_params_read_as_empty() {
    let req: Request = serde_json::from_str(r#"{"id":1,"method":"hello"}"#).unwrap();
    assert_eq!(req.params::<Hello>().unwrap(), Empty {});
    let req: Request = serde_json::from_str(r#"{"id":1,"method":"hello","params":null}"#).unwrap();
    assert_eq!(req.params::<Hello>().unwrap(), Empty {});
    assert_eq!(
        serde_json::to_value(Request::new::<Hello>(1, &Empty {}).unwrap()).unwrap()["params"],
        json!({})
    );
}

#[test]
fn wrong_params_shape_is_an_error_not_a_panic() {
    let req: Request =
        serde_json::from_str(r#"{"id":1,"method":"organization.create","params":{"name":5}}"#)
            .unwrap();
    assert!(req.params::<OrganizationCreate>().is_err());
    let req: Request = serde_json::from_str(r#"{"id":1,"method":"organization.create"}"#).unwrap();
    assert!(req.params::<OrganizationCreate>().is_err());
}

#[test]
fn unknown_method_still_parses() {
    let req: Request =
        serde_json::from_str(r#"{"id":3,"method":"org.frobnicate","params":{}}"#).unwrap();
    assert_eq!(req.method, "org.frobnicate");
}

#[test]
fn response_ok_and_error_wire_shapes() {
    let ok = Response::ok::<OrganizationCreate>(7, &org()).unwrap();
    let v = serde_json::to_value(&ok).unwrap();
    assert_eq!(v["id"], 7);
    assert_eq!(v["result"]["name"], "Acme");
    assert!(v.get("error").is_none());

    let err = Response::error(8, ApiError::new(ErrorCode::AlreadyExists, "taken"));
    assert_eq!(
        serde_json::to_value(&err).unwrap(),
        json!({"id": 8, "error": {"code": "already_exists", "message": "taken"}})
    );
}

#[test]
fn response_round_trip_into_typed_result() {
    let line =
        serde_json::to_string(&Response::ok::<OrganizationCreate>(7, &org()).unwrap()).unwrap();
    let back: Response = serde_json::from_str(&line).unwrap();
    assert_eq!(back.id, 7);
    assert_eq!(back.into_result::<OrganizationCreate>().unwrap(), org());

    let back: Response =
        serde_json::from_str(r#"{"id":8,"error":{"code":"invalid_name","message":"empty"}}"#)
            .unwrap();
    match back.into_result::<OrganizationCreate>() {
        Err(CallError::Api(e)) => assert_eq!(e.code, ErrorCode::InvalidName),
        other => panic!("expected an api error, got {other:?}"),
    }
}

#[test]
fn result_of_wrong_shape_is_a_decode_error() {
    let r: Response = serde_json::from_str(r#"{"id":1,"result":{"nope":true}}"#).unwrap();
    assert!(matches!(
        r.into_result::<Hello>(),
        Err(CallError::Decode(_))
    ));
}

#[test]
fn response_with_neither_result_nor_error_is_rejected() {
    assert!(serde_json::from_str::<Response>(r#"{"id":1}"#).is_err());
}

#[test]
fn unknown_error_code_and_unknown_fields_are_tolerated() {
    let r: Response = serde_json::from_str(
        r#"{"id":1,"error":{"code":"quota_exceeded","message":"x","hint":"y"},"trace":"z"}"#,
    )
    .unwrap();
    match r.outcome {
        Outcome::Error(e) => assert_eq!(e.code, ErrorCode::Unknown),
        other => panic!("expected an error, got {other:?}"),
    }
    let o: Organization = serde_json::from_value(json!({
        "id": "x", "name": "n", "created_at": "2026-11-08T02:10:00.000Z", "owner": "later"
    }))
    .unwrap();
    assert_eq!(o.name, "n");
}

#[test]
fn hello_and_list_wire_shapes() {
    let hello = HelloResult {
        api_version: API_VERSION,
        server_version: "0.0.0".into(),
        host: "This Mac".into(),
        office: None,
    };
    let v = serde_json::to_value(Response::ok::<Hello>(1, &hello).unwrap()).unwrap();
    assert_eq!(v["result"]["api_version"], 1);
    assert_eq!(v["result"]["host"], "This Mac");
    assert!(v["result"].get("office").is_none());

    let list = ListOrganizationsResult {
        organizations: vec![org()],
    };
    let v = serde_json::to_value(Response::ok::<OrganizationList>(2, &list).unwrap()).unwrap();
    assert_eq!(
        v["result"]["organizations"][0]["id"],
        "01M6WEQ98NHCE6ZVZWJGWQAJT3"
    );
}

#[test]
fn server_message_tells_responses_from_events() {
    let resp: ServerMessage = serde_json::from_str(r#"{"id":4,"result":{}}"#).unwrap();
    assert!(matches!(
        resp,
        ServerMessage::Response(Response { id: 4, .. })
    ));

    let ev = Event::new::<OrganizationCreated>(1, &org()).unwrap();
    let line = serde_json::to_string(&ev).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&line).unwrap()["event"],
        "organization.created"
    );
    match serde_json::from_str::<ServerMessage>(&line).unwrap() {
        ServerMessage::Event(e) => {
            assert_eq!(e.seq, 1);
            assert_eq!(e.data::<OrganizationCreated>().unwrap().unwrap(), org());
        }
        other => panic!("expected an event, got {other:?}"),
    }
}

#[test]
fn unknown_event_parses_and_typed_read_returns_none() {
    let m: ServerMessage =
        serde_json::from_str(r#"{"event":"agent.hired","seq":9,"data":{"x":1}}"#).unwrap();
    match m {
        ServerMessage::Event(e) => assert!(e.data::<OrganizationCreated>().is_none()),
        other => panic!("expected an event, got {other:?}"),
    }
}

#[test]
fn frames_round_trip_and_newlines_in_strings_are_escaped() {
    let mut buf = Vec::new();
    let params = OrganizationCreateParams {
        name: "two\nlines".into(),
    };
    write_message(
        &mut buf,
        &Request::new::<OrganizationCreate>(1, &params).unwrap(),
    )
    .unwrap();
    write_message(&mut buf, &Request::new::<Hello>(2, &Empty {}).unwrap()).unwrap();
    assert_eq!(buf.iter().filter(|b| **b == b'\n').count(), 2);

    let mut r = Cursor::new(buf);
    let a: Request = read_message(&mut r).unwrap().unwrap();
    assert_eq!(a.params::<OrganizationCreate>().unwrap().name, "two\nlines");
    let b: Request = read_message(&mut r).unwrap().unwrap();
    assert_eq!(b.id, 2);
    assert!(read_message::<Request>(&mut r).unwrap().is_none());
}

#[test]
fn frame_at_the_limit_is_accepted_and_one_byte_over_is_rejected() {
    let mut at_limit = vec![b'x'; MAX_FRAME_BYTES];
    at_limit.push(b'\n');
    assert_eq!(
        read_line(&mut Cursor::new(at_limit))
            .unwrap()
            .unwrap()
            .len(),
        MAX_FRAME_BYTES
    );

    let mut over = vec![b'x'; MAX_FRAME_BYTES + 1];
    over.push(b'\n');
    assert!(matches!(
        read_line(&mut Cursor::new(over)),
        Err(FrameError::TooLarge)
    ));
}

#[test]
fn truncated_and_invalid_frames_are_errors() {
    assert!(matches!(
        read_line(&mut Cursor::new(b"{\"id\":1".to_vec())),
        Err(FrameError::Truncated)
    ));
    assert!(matches!(
        read_message::<Request>(&mut Cursor::new(b"not json\n".to_vec())),
        Err(FrameError::Json(_))
    ));
    assert!(matches!(
        read_message::<Request>(&mut Cursor::new(
            b"{\"id\":1,\"method\":\"\xff\"}\n".to_vec()
        )),
        Err(FrameError::Json(_))
    ));
    assert!(read_line(&mut Cursor::new(Vec::new())).unwrap().is_none());
}

#[test]
fn response_with_both_result_and_error_is_rejected_in_either_order() {
    let a = r#"{"id":1,"result":{},"error":{"code":"invalid_name","message":"x"}}"#;
    let b = r#"{"id":1,"error":{"code":"invalid_name","message":"x"},"result":{}}"#;
    assert!(serde_json::from_str::<Response>(a).is_err());
    assert!(serde_json::from_str::<Response>(b).is_err());
    assert!(serde_json::from_str::<ServerMessage>(a).is_err());
    assert!(serde_json::from_str::<Response>(r#"{"id":1,"result":{},"result":{}}"#).is_err());
}

#[test]
fn array_or_scalar_params_are_rejected() {
    for params in [r#"["Acme"]"#, "[]", "5", r#""Acme""#] {
        let line = format!(r#"{{"id":1,"method":"organization.create","params":{params}}}"#);
        let req: Request = serde_json::from_str(&line).unwrap();
        assert!(
            req.params::<OrganizationCreate>().is_err(),
            "accepted {params}"
        );
    }
    let req: Request = serde_json::from_str(r#"{"id":1,"method":"hello","params":[]}"#).unwrap();
    assert!(req.params::<Hello>().is_err());
}

#[test]
fn event_carrying_an_id_is_still_an_event() {
    let m: ServerMessage =
        serde_json::from_str(r#"{"id":3,"event":"organization.created","seq":1,"data":{}}"#)
            .unwrap();
    assert!(matches!(m, ServerMessage::Event(_)));
}

#[test]
fn event_without_data_reads_as_null() {
    let m: ServerMessage = serde_json::from_str(r#"{"event":"ping","seq":2}"#).unwrap();
    match m {
        ServerMessage::Event(e) => assert_eq!(e.data, serde_json::Value::Null),
        other => panic!("expected an event, got {other:?}"),
    }
}

#[test]
fn unknown_server_message_shapes_are_kept_not_rejected() {
    // e.g. a newer server asking the client something: an id but no result or error
    let m: ServerMessage =
        serde_json::from_str(r#"{"id":5,"method":"msg.deliver","params":{}}"#).unwrap();
    assert!(matches!(m, ServerMessage::Unknown(_)));
    assert!(serde_json::from_str::<ServerMessage>("[1,2]").is_err());
    // a known shape that is malformed is still an error
    assert!(serde_json::from_str::<ServerMessage>(r#"{"event":"x"}"#).is_err());
    assert!(serde_json::from_str::<ServerMessage>(r#"{"id":"one","result":{}}"#).is_err());
}

#[test]
fn bad_json_line_keeps_the_stream_in_step() {
    let mut r = Cursor::new(b"not json\n{\"id\":2,\"method\":\"hello\"}\n".to_vec());
    let err = read_message::<Request>(&mut r).unwrap_err();
    assert!(!err.closes_stream());
    let next: Request = read_message(&mut r).unwrap().unwrap();
    assert_eq!(next.id, 2);
    assert!(FrameError::TooLarge.closes_stream());
    assert!(FrameError::Truncated.closes_stream());
}

#[test]
fn salvage_id_recovers_the_id_of_a_bad_request_or_response() {
    let bad_req = br#"{"id":42,"method":5}"#;
    assert!(serde_json::from_slice::<Request>(bad_req).is_err());
    assert_eq!(salvage_id(bad_req), Some(42));

    let bad_resp = br#"{"id":43,"error":"boom"}"#;
    assert!(serde_json::from_slice::<ServerMessage>(bad_resp).is_err());
    assert_eq!(salvage_id(bad_resp), Some(43));

    assert_eq!(salvage_id(b"not json"), None);
    assert_eq!(salvage_id(br#"{"method":"hello"}"#), None);
    assert_eq!(salvage_id(br#"{"id":"one"}"#), None);
    assert_eq!(salvage_id(br#"{"id":-1}"#), None);
    assert_eq!(UNREADABLE_REQUEST_ID, 0);
}

#[test]
fn error_responses_decode_through_server_message() {
    let m: ServerMessage =
        serde_json::from_str(r#"{"id":9,"error":{"code":"storage_error","message":"disk full"}}"#)
            .unwrap();
    match m {
        ServerMessage::Response(r) => match r.into_result::<OrganizationCreate>() {
            Err(CallError::Api(e)) => assert_eq!(e.code, ErrorCode::StorageError),
            other => panic!("expected an api error, got {other:?}"),
        },
        other => panic!("expected a response, got {other:?}"),
    }
    let m: ServerMessage =
        serde_json::from_str(r#"{"id":9,"error":{"code":"brand_new","message":"x"}}"#).unwrap();
    match m {
        ServerMessage::Response(Response {
            outcome: Outcome::Error(e),
            ..
        }) => {
            assert_eq!(e.code, ErrorCode::Unknown)
        }
        other => panic!("expected an error response, got {other:?}"),
    }
}

#[test]
fn oversize_outgoing_message_keeps_the_stream_open() {
    let huge = OrganizationCreateParams {
        name: "x".repeat(MAX_FRAME_BYTES),
    };
    let req = Request::new::<OrganizationCreate>(1, &huge).unwrap();
    let mut out = Vec::new();
    let err = write_message(&mut out, &req).unwrap_err();
    assert!(matches!(err, FrameError::TooLargeToSend));
    assert!(!err.closes_stream());
    assert!(out.is_empty());
}

#[test]
fn null_result_is_refused_when_building_a_response() {
    struct NullCall;
    impl Call for NullCall {
        const METHOD: &'static str = "test.null";
        type Params = Empty;
        type Result = Option<Empty>;
    }
    assert!(Response::ok::<NullCall>(1, &None).is_err());
    assert!(Response::ok::<NullCall>(1, &Some(Empty {})).is_ok());
}

#[test]
fn wire_names_are_pinned() {
    assert_eq!(Hello::METHOD, "hello");
    assert_eq!(OrganizationCreate::METHOD, "organization.create");
    assert_eq!(OrganizationList::METHOD, "organization.list");
    assert_eq!(Subscribe::METHOD, "subscribe");
    assert_eq!(OrganizationCreated::NAME, "organization.created");
    for (code, name) in [
        (ErrorCode::InvalidRequest, "invalid_request"),
        (ErrorCode::UnknownMethod, "unknown_method"),
        (ErrorCode::InvalidName, "invalid_name"),
        (ErrorCode::AlreadyExists, "already_exists"),
        (ErrorCode::StorageError, "storage_error"),
    ] {
        assert_eq!(serde_json::to_value(code).unwrap(), name);
        assert_eq!(
            serde_json::from_value::<ErrorCode>(name.into()).unwrap(),
            code
        );
    }
    assert_eq!(API_VERSION, 1);
    assert_eq!(MAX_FRAME_BYTES, 1_048_576);
    assert_eq!(ORGANIZATION_NAME_MAX_CHARS, 64);
}

#[test]
fn office_server_hello_carries_its_identity() {
    let hello = HelloResult {
        api_version: API_VERSION,
        server_version: "0.0.0".into(),
        host: "This Mac".into(),
        office: Some(OfficeHello {
            organization_name: "Acme".into(),
            office: hq(),
        }),
    };
    let v = serde_json::to_value(&hello).unwrap();
    assert_eq!(v["office"]["office"]["role"], "hq");
    assert_eq!(v["office"]["organization_name"], "Acme");
    let back: HelloResult = serde_json::from_value(v).unwrap();
    assert_eq!(back, hello);
}

#[test]
fn office_fields_default_and_tolerate_unknown_values() {
    let minimal = json!({"id": "o", "organization_id": "g", "name": "HQ", "host": "This Mac"});
    let o: OfficeInfo = serde_json::from_value(minimal).unwrap();
    assert_eq!(o.role, OfficeRole::Unknown);
    assert_eq!(o.status, OfficeStatus::Unknown);
    assert_eq!(o.endpoint, None);
    assert!(serde_json::to_value(&o).unwrap().get("endpoint").is_none());

    let newer = json!({"id": "o", "organization_id": "g", "name": "X", "host": "h",
        "role": "observer", "status": "migrating"});
    let o: OfficeInfo = serde_json::from_value(newer).unwrap();
    assert_eq!(o.role, OfficeRole::Unknown);
    assert_eq!(o.status, OfficeStatus::Unknown);

    for (status, name) in [
        (OfficeStatus::Starting, "starting"),
        (OfficeStatus::Pending, "pending"),
        (OfficeStatus::Offline, "offline"),
        (OfficeStatus::Unavailable, "unavailable"),
        (OfficeStatus::UpdateNeeded, "update_needed"),
    ] {
        assert_eq!(serde_json::to_value(status).unwrap(), name);
    }
    assert_eq!(
        serde_json::to_value(OfficeRole::Satellite).unwrap(),
        "satellite"
    );
}

#[test]
fn organization_without_offices_still_decodes() {
    let o: Organization = serde_json::from_value(json!({
        "id": "x", "name": "n", "created_at": "2026-11-08T02:10:00.000Z"
    }))
    .unwrap();
    assert!(o.offices.is_empty());
}

#[test]
fn office_list_wire_shape() {
    assert_eq!(OfficeList::METHOD, "office.list");
    let v = serde_json::to_value(
        Response::ok::<OfficeList>(
            3,
            &ListOfficesResult {
                offices: vec![hq()],
            },
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        v["result"]["offices"][0]["endpoint"],
        "/tmp/o1/run/3RXF2NS0SH83.sock"
    );
}

#[test]
fn trimming_is_rust_str_trim_unicode_white_space() {
    // U+0085 (NEL) is White_Space; U+FEFF (BOM) is not. JS trim() does the opposite.
    assert_eq!("\u{85}Acme\u{85}".trim(), "Acme");
    assert_eq!("\u{feff}Acme".trim(), "\u{feff}Acme");
}

#[test]
fn org_wire_names_are_pinned() {
    use crate::org::*;
    let names = [
        (OrgSnapshotCall::METHOD, "org.snapshot"),
        (TeamCreate::METHOD, "team.create"),
        (TeamRename::METHOD, "team.rename"),
        (TeamMove::METHOD, "team.move"),
        (TeamSetLead::METHOD, "team.set_lead"),
        (TeamDelete::METHOD, "team.delete"),
        (TeamAddMember::METHOD, "team.add_member"),
        (TeamRemoveMember::METHOD, "team.remove_member"),
        (TeamSetTitle::METHOD, "team.set_title"),
        (MemberAddPerson::METHOD, "member.add_person"),
        (MemberAddAgent::METHOD, "member.add_agent"),
        (MemberUpdateAgent::METHOD, "member.update_agent"),
        (MemberSetAgentStatus::METHOD, "member.set_agent_status"),
        (MemberMove::METHOD, "member.move"),
        (MemberUpdatePerson::METHOD, "member.update_person"),
        (MemberRemove::METHOD, "member.remove"),
        (MemberSetReportsTo::METHOD, "member.set_reports_to"),
        (ProjectAddTeam::METHOD, "project.add_team"),
        (ProjectRemoveTeam::METHOD, "project.remove_team"),
        (OrgChanged::NAME, "org.changed"),
        (OrgRejected::NAME, "org.rejected"),
    ];
    for (got, want) in names {
        assert_eq!(got, want);
    }
    for (code, name) in [
        (ErrorCode::Cycle, "cycle"),
        (ErrorCode::NotEmpty, "not_empty"),
        (ErrorCode::NotAMember, "not_a_member"),
        (ErrorCode::OwnerProtected, "owner_protected"),
        (ErrorCode::NotFound, "not_found"),
        (ErrorCode::Conflict, "conflict"),
        (ErrorCode::UnknownRecipient, "unknown_recipient"),
        (ErrorCode::SnapshotRequired, "snapshot_required"),
        (ErrorCode::Expired, "expired"),
        (ErrorCode::ClockSkew, "clock_skew"),
    ] {
        assert_eq!(serde_json::to_value(code).unwrap(), name);
        assert_eq!(
            serde_json::from_value::<ErrorCode>(name.into()).unwrap(),
            code
        );
    }
}

#[test]
fn positions_order_by_epoch_then_seq() {
    let a = Position { epoch: 1, seq: 900 };
    let b = Position { epoch: 2, seq: 1 };
    assert!(a < b);
    assert!(Position { epoch: 2, seq: 2 } > b);
}

#[test]
fn subscribe_accepts_empty_params_and_a_position() {
    let req: Request =
        serde_json::from_str(r#"{"id":1,"method":"subscribe","params":{}}"#).unwrap();
    assert_eq!(
        req.params::<Subscribe>().unwrap(),
        SubscribeParams::default()
    );
    let req: Request = serde_json::from_str(
        r#"{"id":1,"method":"subscribe","params":{"from_position":{"epoch":1,"seq":42}}}"#,
    )
    .unwrap();
    assert_eq!(
        req.params::<Subscribe>().unwrap().from_position,
        Some(Position { epoch: 1, seq: 42 })
    );
}

#[test]
fn member_owner_flag_and_kind_round_trip() {
    let owner = Member {
        id: "m1".into(),
        kind: MemberKind::Person,
        name: "Vitor".into(),
        contact: None,
        owner: true,
        office_id: None,
        agent: None,
        reports_to_member_id: None,
        version: 1,
    };
    let v = serde_json::to_value(&owner).unwrap();
    assert!(v.get("agent").is_none(), "a person has no agent profile");
    assert!(v.get("reports_to_member_id").is_none());
    assert_eq!(v["owner"], true);
    assert_eq!(v["kind"], "person");
    let agent: Member = serde_json::from_value(json!({
        "id": "a1", "kind": "agent", "name": "ada", "office_id": "o1",
        "reports_to_member_id": "p1", "version": 3
    }))
    .unwrap();
    assert_eq!(agent.reports_to_member_id.as_deref(), Some("p1"));
    assert!(!agent.owner);
    assert!(serde_json::to_value(&agent).unwrap().get("owner").is_none());
    let robot: Member =
        serde_json::from_value(json!({"id": "x", "kind": "robot", "name": "r", "version": 1}))
            .unwrap();
    assert_eq!(robot.kind, MemberKind::Unknown);
}

#[test]
fn snapshot_and_changes_round_trip_and_unknown_changes_are_tolerated() {
    let snap = OrgSnapshot {
        position: Position { epoch: 1, seq: 7 },
        teams: vec![Team {
            id: "t1".into(),
            name: "Product".into(),
            owner_team_id: None,
            lead_member_id: Some("a1".into()),
            version: 2,
        }],
        members: vec![],
        memberships: vec![Membership {
            id: "ms1".into(),
            team_id: "t1".into(),
            member_id: "a1".into(),
            title_in_team: None,
            version: 1,
        }],
        projects: vec![ProjectSummary {
            id: "p1".into(),
            name: "Checkout".into(),
        }],
        project_teams: vec![ProjectTeam {
            id: "pt1".into(),
            project_id: "p1".into(),
            team_id: "t1".into(),
            version: 1,
        }],
    };
    let v = serde_json::to_value(Response::ok::<OrgSnapshotCall>(1, &snap).unwrap()).unwrap();
    assert_eq!(v["result"]["teams"][0]["name"], "Product");
    assert!(v["result"]["teams"][0].get("labels").is_none());
    let back: Response = serde_json::from_value(v).unwrap();
    assert_eq!(back.into_result::<OrgSnapshotCall>().unwrap(), snap);

    let ev = Event::new::<OrgChanged>(
        3,
        &OrgChangedData {
            position: Position { epoch: 1, seq: 8 },
            command_id: "c42".into(),
            changes: vec![OrgChange::TeamDeleted {
                team_id: "t9".into(),
            }],
        },
    )
    .unwrap();
    let v = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["event"], "org.changed");
    assert_eq!(v["data"]["changes"][0]["type"], "team_deleted");
    assert_eq!(v["data"]["command_id"], "c42");

    let newer: OrgChangedData = serde_json::from_value(json!({
        "position": {"epoch": 1, "seq": 9},
        "command_id": "c43",
        "changes": [{"type": "budget_set", "amount": 5}]
    }))
    .unwrap();
    assert_eq!(newer.changes, vec![OrgChange::Unknown]);
}

#[test]
fn commands_carry_command_ids_and_expected_versions() {
    use crate::org::*;
    let req = Request::new::<TeamMove>(
        5,
        &TeamMoveParams {
            command_id: "c1".into(),
            team_id: "t2".into(),
            expected_version: 4,
            owner_team_id: None,
        },
    )
    .unwrap();
    let v = serde_json::to_value(&req).unwrap();
    assert_eq!(v["method"], "team.move");
    assert_eq!(v["params"]["command_id"], "c1");
    assert_eq!(v["params"]["expected_version"], 4);
    assert!(v["params"].get("owner_team_id").is_none());

    let missing: Request = serde_json::from_str(
        r#"{"id":1,"method":"team.rename","params":{"command_id":"c","team_id":"t","name":"x"}}"#,
    )
    .unwrap();
    assert!(missing.params::<TeamRename>().is_err());

    let ok = Response::ok::<TeamCreate>(
        1,
        &CommandAccepted {
            status: CommandStatus::Committed,
            position: Some(Position { epoch: 1, seq: 10 }),
        },
    )
    .unwrap();
    let v = serde_json::to_value(ok).unwrap();
    assert_eq!(v["result"]["status"], "committed");
    assert_eq!(v["result"]["position"]["seq"], 10);

    let queued = CommandAccepted {
        status: CommandStatus::Queued,
        position: None,
    };
    let v = serde_json::to_value(queued).unwrap();
    assert_eq!(v["status"], "queued");
    assert!(v.get("position").is_none());
    let newer: CommandAccepted = serde_json::from_value(json!({"status": "deferred"})).unwrap();
    assert_eq!(newer.status, CommandStatus::Unknown);

    let p: MemberUpdatePersonParams = serde_json::from_value(json!({
        "command_id": "c", "member_id": "m", "expected_version": 1, "clear_contact": true
    }))
    .unwrap();
    assert!(p.clear_contact && p.contact.is_none());
}

#[test]
fn rejections_reach_the_client_by_command_id() {
    let ev = Event::new::<OrgRejected>(
        4,
        &OrgRejectedData {
            position: Position { epoch: 1, seq: 11 },
            command_id: "c7".into(),
            error: ApiError::new(ErrorCode::Cycle, "a team can't own its own owner"),
        },
    )
    .unwrap();
    let v = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["event"], "org.rejected");
    assert_eq!(v["data"]["command_id"], "c7");
    assert_eq!(v["data"]["error"]["code"], "cycle");
    let parsed: Event = serde_json::from_value(v).unwrap();
    let back = parsed.data::<OrgRejected>().unwrap().unwrap();
    assert_eq!(back.error.code, ErrorCode::Cycle);
}

fn profile() -> AgentProfile {
    AgentProfile {
        role: "software engineer".into(),
        title: "Senior Backend Engineer".into(),
        focus: Some("storage".into()),
        runtime: AgentRuntime::ClaudeCode,
        home: Some("/Users/me/agents/river".into()),
        messaging_name: "river".into(),
        status: AgentStatus::Active,
    }
}

#[test]
fn agent_profile_wire_shape_is_pinned() {
    assert_eq!(
        serde_json::to_value(profile()).unwrap(),
        json!({
            "role": "software engineer",
            "title": "Senior Backend Engineer",
            "focus": "storage",
            "runtime": "claude_code",
            "home": "/Users/me/agents/river",
            "messaging_name": "river",
            "status": "active",
        })
    );
    // Optional fields are left out when empty, and read back as None.
    let bare = AgentProfile {
        focus: None,
        home: None,
        ..profile()
    };
    let v = serde_json::to_value(&bare).unwrap();
    assert!(v.get("focus").is_none() && v.get("home").is_none());
    assert_eq!(serde_json::from_value::<AgentProfile>(v).unwrap(), bare);
}

#[test]
fn agent_runtime_and_status_tolerate_values_from_a_newer_build() {
    for (wire, want) in [
        (json!("claude_code"), AgentRuntime::ClaudeCode),
        (json!("codex"), AgentRuntime::Codex),
        (json!("antigravity"), AgentRuntime::Antigravity),
        (json!({"other": "zed"}), AgentRuntime::Other("zed".into())),
        (json!("some_future_runtime"), AgentRuntime::Unknown),
    ] {
        assert_eq!(
            serde_json::from_value::<AgentRuntime>(wire.clone()).unwrap(),
            want,
            "{wire}"
        );
        if want != AgentRuntime::Unknown {
            assert_eq!(serde_json::to_value(&want).unwrap(), wire);
        }
    }
    assert_eq!(
        serde_json::from_value::<AgentStatus>(json!("paused")).unwrap(),
        AgentStatus::Paused
    );
    assert_eq!(
        serde_json::from_value::<AgentStatus>(json!("retired")).unwrap(),
        AgentStatus::Unknown
    );
}

#[test]
fn a_member_carries_its_agent_profile_in_one_record() {
    let agent = Member {
        id: "a1".into(),
        kind: MemberKind::Agent,
        name: "River".into(),
        contact: None,
        owner: false,
        office_id: Some("o1".into()),
        agent: Some(profile()),
        reports_to_member_id: None,
        version: 4,
    };
    let v = serde_json::to_value(&agent).unwrap();
    assert_eq!(v["agent"]["messaging_name"], "river");
    assert_eq!(serde_json::from_value::<Member>(v).unwrap(), agent);

    // An older record without a profile still reads.
    let old: Member = serde_json::from_value(
        json!({"id": "a2", "kind": "agent", "name": "x", "office_id": "o1", "version": 1}),
    )
    .unwrap();
    assert!(old.agent.is_none());

    // One entity, one version: the change carries the profile.
    let change = OrgChange::MemberUpserted {
        member: agent.clone(),
    };
    let back: OrgChange = serde_json::from_value(serde_json::to_value(&change).unwrap()).unwrap();
    assert_eq!(back, change);
}

#[test]
fn agent_commands_have_the_agreed_shapes() {
    use crate::org::*;
    let add = Request::new::<MemberAddAgent>(
        1,
        &MemberAddAgentParams {
            command_id: "01C".into(),
            member_id: "a1".into(),
            name: "River".into(),
            office_id: "o1".into(),
            profile: profile(),
        },
    )
    .unwrap();
    assert_eq!(add.method, "member.add_agent");
    assert_eq!(add.params["profile"]["runtime"], "claude_code");
    assert_eq!(add.params::<MemberAddAgent>().unwrap().office_id, "o1");

    // Update: only what is given is sent, so only that changes.
    let update = Request::new::<MemberUpdateAgent>(
        2,
        &MemberUpdateAgentParams {
            command_id: "01C".into(),
            member_id: "a1".into(),
            expected_version: 4,
            name: None,
            role: None,
            title: Some("Staff Engineer".into()),
            focus: None,
            clear_focus: true,
            runtime: Some(AgentRuntime::Other("zed".into())),
            home: None,
            clear_home: false,
            messaging_name: None,
        },
    )
    .unwrap();
    assert_eq!(
        update.params,
        json!({
            "command_id": "01C", "member_id": "a1", "expected_version": 4,
            "title": "Staff Engineer", "clear_focus": true, "runtime": {"other": "zed"},
        })
    );

    let status = Request::new::<MemberSetAgentStatus>(
        3,
        &MemberSetAgentStatusParams {
            command_id: "01C".into(),
            member_id: "a1".into(),
            expected_version: 5,
            status: AgentStatus::Paused,
        },
    )
    .unwrap();
    assert_eq!(status.params["status"], "paused");

    let moved = Request::new::<MemberMove>(
        4,
        &MemberMoveParams {
            command_id: "01C".into(),
            membership_id: "ms1".into(),
            expected_version: 2,
            new_membership_id: "ms2".into(),
            to_team_id: "t2".into(),
            title_in_team: None,
        },
    )
    .unwrap();
    assert!(moved.params.get("title_in_team").is_none());
    assert_eq!(
        moved.params::<MemberMove>().unwrap().new_membership_id,
        "ms2"
    );
}

#[test]
fn agent_command_params_are_pinned_field_by_field() {
    use crate::org::*;
    let add = Request::new::<MemberAddAgent>(
        1,
        &MemberAddAgentParams {
            command_id: "01C".into(),
            member_id: "a1".into(),
            name: "River".into(),
            office_id: "o1".into(),
            profile: profile(),
        },
    )
    .unwrap();
    assert_eq!(
        add.params,
        json!({
            "command_id": "01C", "member_id": "a1", "name": "River", "office_id": "o1",
            "profile": {
                "role": "software engineer", "title": "Senior Backend Engineer",
                "focus": "storage", "runtime": "claude_code",
                "home": "/Users/me/agents/river", "messaging_name": "river",
                "status": "active",
            },
        })
    );
    let status = Request::new::<MemberSetAgentStatus>(
        2,
        &MemberSetAgentStatusParams {
            command_id: "01C".into(),
            member_id: "a1".into(),
            expected_version: 5,
            status: AgentStatus::Paused,
        },
    )
    .unwrap();
    assert_eq!(
        status.params,
        json!({"command_id": "01C", "member_id": "a1", "expected_version": 5, "status": "paused"})
    );
    let moved = Request::new::<MemberMove>(
        3,
        &MemberMoveParams {
            command_id: "01C".into(),
            membership_id: "ms1".into(),
            expected_version: 2,
            new_membership_id: "ms2".into(),
            to_team_id: "t2".into(),
            title_in_team: None,
        },
    )
    .unwrap();
    assert_eq!(
        moved.params,
        json!({
            "command_id": "01C", "membership_id": "ms1", "expected_version": 2,
            "new_membership_id": "ms2", "to_team_id": "t2",
        })
    );
    // Nothing given but the ids: no nulls, no clear flags.
    let bare = Request::new::<MemberUpdateAgent>(
        4,
        &MemberUpdateAgentParams {
            command_id: "01C".into(),
            member_id: "a1".into(),
            expected_version: 4,
            name: None,
            role: None,
            title: None,
            focus: None,
            clear_focus: false,
            runtime: None,
            home: None,
            clear_home: false,
            messaging_name: None,
        },
    )
    .unwrap();
    assert_eq!(
        bare.params,
        json!({"command_id": "01C", "member_id": "a1", "expected_version": 4})
    );
}

#[test]
fn a_runtime_from_a_newer_build_never_stops_a_member_a_change_or_a_snapshot_decoding() {
    // Shapes an older build has never seen, in string and object form.
    for runtime in [
        json!("some_future_runtime"),
        json!({"future": "x"}),
        json!({"future": {"host": "h", "port": 1}}),
        json!({"claude_code": "x"}),
        json!({"other": 5}),
        json!({}),
        json!(null),
        json!([1, 2]),
        json!(7),
    ] {
        let member = json!({
            "id": "a1", "kind": "agent", "name": "x", "office_id": "o1", "version": 1,
            "agent": {
                "role": "r", "title": "t", "runtime": runtime,
                "messaging_name": "x", "status": "active",
            },
        });
        let decoded: Member = serde_json::from_value(member.clone())
            .unwrap_or_else(|e| panic!("runtime {runtime} broke the member: {e}"));
        assert_eq!(
            decoded.agent.as_ref().unwrap().runtime,
            AgentRuntime::Unknown,
            "{runtime}"
        );

        let change: OrgChange =
            serde_json::from_value(json!({"type": "member_upserted", "member": member}))
                .unwrap_or_else(|e| panic!("runtime {runtime} broke the change: {e}"));
        assert!(matches!(change, OrgChange::MemberUpserted { .. }));

        let snapshot: OrgSnapshot = serde_json::from_value(json!({
            "position": {"epoch": 1, "seq": 1},
            "teams": [], "members": [member], "memberships": [],
        }))
        .unwrap_or_else(|e| panic!("runtime {runtime} broke the snapshot: {e}"));
        assert_eq!(snapshot.members.len(), 1);
    }
    // Extra keys beside a named `other` do not hide it.
    assert_eq!(
        serde_json::from_value::<AgentRuntime>(json!({"other": "zed", "x": 1})).unwrap(),
        AgentRuntime::Other("zed".into())
    );
}

#[test]
fn a_missing_runtime_or_status_reads_as_unknown_not_as_an_error() {
    let profile: AgentProfile = serde_json::from_value(json!({
        "role": "r", "title": "t", "messaging_name": "x",
    }))
    .unwrap();
    assert_eq!(profile.runtime, AgentRuntime::Unknown);
    assert_eq!(profile.status, AgentStatus::Unknown);
}

#[test]
fn agent_limits_are_pinned() {
    assert_eq!(AGENT_MESSAGING_NAME_MAX_CHARS, 64);
    assert_eq!(AGENT_TEXT_MAX_CHARS, 200);
    assert_eq!(AGENT_HOME_MAX_BYTES, 1024);
}

#[test]
fn the_three_changes_of_a_member_move_that_leaves_a_led_team_have_these_wire_shapes() {
    // What the contract says HQ emits when the member led the team it leaves: three changes in
    // one event. This pins the shapes of those changes only; the move itself is HQ's, and is
    // tested where it is implemented.
    let changed = OrgChangedData {
        position: Position { epoch: 1, seq: 9 },
        command_id: "01C".into(),
        changes: vec![
            OrgChange::MembershipRemoved {
                membership_id: "ms1".into(),
            },
            OrgChange::MembershipUpserted {
                membership: Membership {
                    id: "ms2".into(),
                    team_id: "t2".into(),
                    member_id: "a1".into(),
                    title_in_team: None,
                    version: 1,
                },
            },
            OrgChange::TeamUpserted {
                team: Team {
                    id: "t1".into(),
                    name: "Platform".into(),
                    owner_team_id: None,
                    lead_member_id: None,
                    version: 3,
                },
            },
        ],
    };
    assert_eq!(
        serde_json::to_value(&changed).unwrap(),
        json!({
            "position": {"epoch": 1, "seq": 9},
            "command_id": "01C",
            "changes": [
                {"type": "membership_removed", "membership_id": "ms1"},
                {"type": "membership_upserted", "membership": {
                    "id": "ms2", "team_id": "t2", "member_id": "a1", "version": 1}},
                {"type": "team_upserted", "team": {
                    "id": "t1", "name": "Platform", "version": 3}},
            ],
        })
    );
    let event = Event::new::<OrgChanged>(1, &changed).unwrap();
    assert_eq!(event.data::<OrgChanged>().unwrap().unwrap(), changed);
}

#[test]
fn unknown_serializes_as_unknown_and_odd_runtimes_round_trip() {
    assert_eq!(
        serde_json::to_value(AgentRuntime::Unknown).unwrap(),
        json!("unknown")
    );
    assert_eq!(
        serde_json::to_value(AgentStatus::Unknown).unwrap(),
        json!("unknown")
    );
    // A decoded Unknown sent back stays Unknown (HQ refuses it; the client never invents one).
    let back: AgentRuntime =
        serde_json::from_value(serde_json::to_value(AgentRuntime::Unknown).unwrap()).unwrap();
    assert_eq!(back, AgentRuntime::Unknown);
    for name in ["", "zed", "a\"b\n\0\u{1F600}", "Codex"] {
        let runtime = AgentRuntime::Other(name.into());
        let back: AgentRuntime =
            serde_json::from_value(serde_json::to_value(&runtime).unwrap()).unwrap();
        assert_eq!(back, runtime, "{name:?}");
    }
}

#[test]
fn an_agent_status_in_any_unknown_shape_decodes_as_unknown() {
    for status in [
        json!(null),
        json!(5),
        json!([1]),
        json!({"future": 1}),
        json!({"paused": {"until": "x"}}),
        json!("retired"),
    ] {
        let profile: AgentProfile = serde_json::from_value(json!({
            "role": "r", "title": "t", "runtime": "codex", "messaging_name": "x",
            "status": status,
        }))
        .unwrap_or_else(|e| panic!("status {status} broke the profile: {e}"));
        assert_eq!(profile.status, AgentStatus::Unknown, "{status}");
    }
    let known: AgentStatus = serde_json::from_value(json!("paused")).unwrap();
    assert_eq!(known, AgentStatus::Paused);
}
