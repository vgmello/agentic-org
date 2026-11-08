use std::io::Cursor;

use serde::Serialize;
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
        status_reason: None,
        next_restart_at: None,
    }
}

fn offline_hq() -> OfficeInfo {
    OfficeInfo {
        status: OfficeStatus::Offline,
        status_reason: Some(OfficeStatusReason::Crashed),
        next_restart_at: Some("2026-11-08T02:10:04.000Z".into()),
        endpoint: None,
        ..hq()
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
    let hello = HelloResult::host("0.0.0", "This Mac");
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
fn hello_constructors_fill_every_field() {
    let identity = OfficeHello {
        organization_name: "Acme".into(),
        office: hq(),
    };
    assert_eq!(
        HelloResult::host("1.2.3", "This Mac"),
        HelloResult {
            api_version: API_VERSION,
            server_version: "1.2.3".into(),
            host: "This Mac".into(),
            office: None,
            capabilities: vec![],
        }
    );
    assert_eq!(
        HelloResult::office("1.2.3", "This Mac", identity.clone())
            .with_capabilities(vec!["x.y".into()]),
        HelloResult {
            api_version: API_VERSION,
            server_version: "1.2.3".into(),
            host: "This Mac".into(),
            office: Some(identity),
            capabilities: vec!["x.y".into()],
        }
    );
}

#[test]
fn office_server_hello_carries_its_identity() {
    let hello = HelloResult::office(
        "0.0.0",
        "This Mac",
        OfficeHello {
            organization_name: "Acme".into(),
            office: hq(),
        },
    );
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
fn office_status_reason_and_next_restart_round_trip_and_are_omitted_when_absent() {
    // Without them: neither key is on the wire, and the record reads back the same.
    let v = serde_json::to_value(hq()).unwrap();
    assert!(v.get("status_reason").is_none(), "{v}");
    assert!(v.get("next_restart_at").is_none(), "{v}");
    assert_eq!(serde_json::from_value::<OfficeInfo>(v).unwrap(), hq());

    // With them.
    let v = serde_json::to_value(offline_hq()).unwrap();
    assert_eq!(
        v,
        json!({"id": "01M6WGY1WJCC503RXF2NS0SH83",
            "organization_id": "01M6WEQ98NHCE6ZVZWJGWQAJT3", "name": "HQ", "role": "hq",
            "host": "This Mac", "status": "offline", "status_reason": "crashed",
            "next_restart_at": "2026-11-08T02:10:04.000Z"})
    );
    assert_eq!(
        serde_json::from_value::<OfficeInfo>(v).unwrap(),
        offline_hq()
    );

    // A fatal reason (until the key is restored and `office.restart` retries), with no
    // restart pending.
    let fatal = OfficeInfo {
        status: OfficeStatus::Unavailable,
        status_reason: Some(OfficeStatusReason::KeyMissing),
        next_restart_at: None,
        ..hq()
    };
    let v = serde_json::to_value(&fatal).unwrap();
    assert_eq!(v["status_reason"], "key_missing");
    assert!(v.get("next_restart_at").is_none(), "{v}");
    assert_eq!(serde_json::from_value::<OfficeInfo>(v).unwrap(), fatal);
}

#[test]
fn an_office_from_an_older_server_without_the_new_fields_still_decodes() {
    let old = json!({"id": "01M6WGY1WJCC503RXF2NS0SH83",
        "organization_id": "01M6WEQ98NHCE6ZVZWJGWQAJT3", "name": "HQ", "role": "hq",
        "host": "This Mac", "endpoint": "/tmp/o1/run/3RXF2NS0SH83.sock", "status": "active"});
    let o: OfficeInfo = serde_json::from_value(old).unwrap();
    assert_eq!(o, hq());
    assert_eq!(o.status_reason, None);
    assert_eq!(o.next_restart_at, None);

    // Explicit nulls read as absent too.
    let nulls = json!({"id": "o", "organization_id": "g", "name": "HQ", "host": "h",
        "status": "offline", "status_reason": null, "next_restart_at": null});
    let o: OfficeInfo = serde_json::from_value(nulls).unwrap();
    assert_eq!(o.status_reason, None);
    assert_eq!(o.next_restart_at, None);
}

#[test]
fn office_status_reasons_are_pinned_and_unknown_ones_decode_as_unknown() {
    for (reason, name) in [
        (OfficeStatusReason::KeyMissing, "key_missing"),
        (OfficeStatusReason::InUse, "in_use"),
        (OfficeStatusReason::StorageError, "storage_error"),
        (OfficeStatusReason::InvalidRequest, "invalid_request"),
        (
            OfficeStatusReason::UnsupportedVersion,
            "unsupported_version",
        ),
        (OfficeStatusReason::Crashed, "crashed"),
        (OfficeStatusReason::StartTimeout, "start_timeout"),
        (OfficeStatusReason::RestartsExhausted, "restarts_exhausted"),
        (OfficeStatusReason::StoppedByOwner, "stopped_by_owner"),
        (OfficeStatusReason::Unknown, "unknown"),
    ] {
        assert_eq!(serde_json::to_value(reason).unwrap(), name);
        assert_eq!(
            serde_json::from_value::<OfficeStatusReason>(name.into()).unwrap(),
            reason
        );
    }
    assert_eq!(OfficeStatusReason::default(), OfficeStatusReason::Unknown);

    // A reason from a newer server never stops the office record decoding.
    let newer = json!({"id": "o", "organization_id": "g", "name": "HQ", "host": "h",
        "status": "unavailable", "status_reason": "quarantined"});
    let o: OfficeInfo = serde_json::from_value(newer).unwrap();
    assert_eq!(o.status, OfficeStatus::Unavailable);
    assert_eq!(o.status_reason, Some(OfficeStatusReason::Unknown));
}

#[test]
fn office_status_changed_event_wire_shape() {
    assert_eq!(OfficeStatusChanged::NAME, "office.status_changed");
    let data = OfficeStatusChangedData {
        office: offline_hq(),
    };
    let ev = Event::new::<OfficeStatusChanged>(7, &data).unwrap();
    let line = serde_json::to_string(&ev).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&line).unwrap(),
        json!({"event": "office.status_changed", "seq": 7, "data": {"office": {
            "id": "01M6WGY1WJCC503RXF2NS0SH83",
            "organization_id": "01M6WEQ98NHCE6ZVZWJGWQAJT3", "name": "HQ", "role": "hq",
            "host": "This Mac", "status": "offline", "status_reason": "crashed",
            "next_restart_at": "2026-11-08T02:10:04.000Z"}}})
    );
    match serde_json::from_str::<ServerMessage>(&line).unwrap() {
        ServerMessage::Event(e) => {
            assert_eq!(e.seq, 7);
            assert!(e.data::<OrganizationCreated>().is_none());
            assert_eq!(e.data::<OfficeStatusChanged>().unwrap().unwrap(), data);
        }
        other => panic!("expected an event, got {other:?}"),
    }
}

#[test]
fn office_restart_wire_shapes() {
    assert_eq!(OfficeRestart::METHOD, "office.restart");
    let req = Request::new::<OfficeRestart>(
        5,
        &OfficeRestartParams {
            office_id: "01M6WGY1WJCC503RXF2NS0SH83".into(),
        },
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&req).unwrap(),
        json!({"id": 5, "method": "office.restart",
            "params": {"office_id": "01M6WGY1WJCC503RXF2NS0SH83"}})
    );
    assert_eq!(
        req.params::<OfficeRestart>().unwrap().office_id,
        "01M6WGY1WJCC503RXF2NS0SH83"
    );
    let missing: Request =
        serde_json::from_str(r#"{"id":5,"method":"office.restart","params":{}}"#).unwrap();
    assert!(missing.params::<OfficeRestart>().is_err());

    let ok = serde_json::to_value(Response::ok::<OfficeRestart>(5, &Empty {}).unwrap()).unwrap();
    assert_eq!(ok, json!({"id": 5, "result": {}}));
    let back: Response = serde_json::from_value(ok).unwrap();
    assert_eq!(back.into_result::<OfficeRestart>().unwrap(), Empty {});

    for (code, name) in [
        (ErrorCode::NotAuthorized, "not_authorized"),
        (ErrorCode::NotFound, "not_found"),
        (ErrorCode::Conflict, "conflict"),
        (ErrorCode::Unavailable, "unavailable"),
        (ErrorCode::NotReady, "not_ready"),
    ] {
        let line = serde_json::to_string(&Response::error(5, ApiError::new(code, "x"))).unwrap();
        assert_eq!(
            line,
            format!(r#"{{"id":5,"error":{{"code":"{name}","message":"x"}}}}"#)
        );
        match serde_json::from_str::<Response>(&line)
            .unwrap()
            .into_result::<OfficeRestart>()
        {
            Err(CallError::Api(e)) => assert_eq!(e.code, code),
            other => panic!("expected an api error, got {other:?}"),
        }
    }
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
        (MemberOffboard::METHOD, "member.offboard"),
        (MemberSetRunMode::METHOD, "member.set_run_mode"),
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
        created_at: None,
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
        projects: vec![ProjectSummary::new("p1", "Checkout")],
        project_teams: vec![ProjectTeam {
            id: "pt1".into(),
            project_id: "p1".into(),
            team_id: "t1".into(),
            version: 1,
        }],
        areas: vec![],
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

#[test]
fn office_app_token_wire_shapes() {
    assert_eq!(OfficeAppToken::METHOD, "office.app_token");
    let req = Request::new::<OfficeAppToken>(
        4,
        &OfficeAppTokenParams {
            office_id: "01M6WGY1WJCC503RXF2NS0SH83".into(),
        },
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&req).unwrap(),
        json!({"id": 4, "method": "office.app_token",
            "params": {"office_id": "01M6WGY1WJCC503RXF2NS0SH83"}})
    );
    assert_eq!(
        req.params::<OfficeAppToken>().unwrap().office_id,
        "01M6WGY1WJCC503RXF2NS0SH83"
    );
    let missing: Request =
        serde_json::from_str(r#"{"id":4,"method":"office.app_token","params":{}}"#).unwrap();
    assert!(missing.params::<OfficeAppToken>().is_err());

    let hex = "ab".repeat(32);
    let result = OfficeAppTokenResult {
        app_token: AppToken::from_bytes([0xab; 32]),
    };
    let v = serde_json::to_value(Response::ok::<OfficeAppToken>(4, &result).unwrap()).unwrap();
    assert_eq!(v, json!({"id": 4, "result": {"app_token": hex}}));
    let back: Response = serde_json::from_value(v).unwrap();
    let back = back.into_result::<OfficeAppToken>().unwrap();
    assert_eq!(back, result);
    // Debug of the result, as a log line would show it, has no token.
    let debug = format!("{back:?}");
    assert!(!debug.contains("abab"), "{debug}");
    assert!(debug.contains("AppToken(<redacted>)"), "{debug}");

    // A full-length token in uppercase is refused, and the error doesn't repeat it.
    let upper = "AB".repeat(32);
    let bad: Response =
        serde_json::from_value(json!({"id": 4, "result": {"app_token": upper}})).unwrap();
    match bad.into_result::<OfficeAppToken>() {
        Err(CallError::Decode(e)) => assert!(!e.to_string().contains(&upper[..8]), "{e}"),
        other => panic!("expected a decode error, got {other:?}"),
    }
}

#[test]
fn not_authorized_unavailable_and_not_ready_codes_round_trip() {
    for (code, name) in [
        (ErrorCode::NotAuthorized, "not_authorized"),
        (ErrorCode::Unavailable, "unavailable"),
        (ErrorCode::NotReady, "not_ready"),
    ] {
        assert_eq!(serde_json::to_value(code).unwrap(), name);
        assert_eq!(
            serde_json::from_value::<ErrorCode>(name.into()).unwrap(),
            code
        );
        let line = serde_json::to_string(&Response::error(9, ApiError::new(code, "x"))).unwrap();
        assert_eq!(
            line,
            format!(r#"{{"id":9,"error":{{"code":"{name}","message":"x"}}}}"#)
        );
        match serde_json::from_str::<Response>(&line)
            .unwrap()
            .into_result::<OfficeAppToken>()
        {
            Err(CallError::Api(e)) => assert_eq!(e.code, code),
            other => panic!("expected an api error, got {other:?}"),
        }
    }
}

#[test]
fn envelope_debug_elides_payloads_that_may_hold_secrets() {
    let hex = "c0ffee".repeat(10) + "abcd";
    assert_eq!(hex.len(), 64);
    let params = json!({"app_token": hex});
    let size = serde_json::to_string(&params).unwrap().len();

    let req: Request =
        serde_json::from_value(json!({"id": 7, "method": "office.app_token", "params": params}))
            .unwrap();
    let debug = format!("{req:?}");
    assert!(!debug.contains(&hex[..8]), "{debug}");
    assert_eq!(
        debug,
        format!(
            r#"Request {{ id: 7, method: "office.app_token", params: <elided {size} bytes> }}"#
        )
    );
    assert!(!format!("{req:#?}").contains(&hex[..8]));

    let resp = Response {
        id: 7,
        outcome: Outcome::Result(params.clone()),
    };
    let debug = format!("{resp:?}");
    assert!(!debug.contains(&hex[..8]), "{debug}");
    assert!(debug.contains("id: 7"), "{debug}");
    let msg = ServerMessage::Response(resp);
    assert!(!format!("{msg:?}").contains(&hex[..8]));

    // An error is shown: its message is for people and holds no secrets.
    let err = Response::error(8, ApiError::new(ErrorCode::NotAuthorized, "not the app"));
    assert!(format!("{err:?}").contains("not the app"));

    let ev = Event {
        event: "office.token_rotated".into(),
        seq: 2,
        data: params.clone(),
    };
    let debug = format!("{:?}", ServerMessage::Event(ev));
    assert!(!debug.contains(&hex[..8]), "{debug}");
    assert!(debug.contains("office.token_rotated") && debug.contains("seq: 2"));

    let unknown = ServerMessage::Unknown(json!({"id": 9, "secret": hex}));
    let debug = format!("{unknown:?}");
    assert!(!debug.contains(&hex[..8]), "{debug}");
    assert!(debug.starts_with("Unknown(<elided "), "{debug}");
}

fn profile() -> AgentProfile {
    AgentProfile {
        role: "software engineer".into(),
        title: "Senior Backend Engineer".into(),
        focus: Some("storage".into()),
        runtime: AgentRuntime::ClaudeCode,
        model: None,
        run_mode: None,
        schedule: None,
        home: Some("/Users/me/agents/river".into()),
        messaging_name: "river".into(),
        status: AgentStatus::Active,
        offboarded_at: None,
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
        created_at: None,
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
            model: None,
            clear_model: false,
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
            model: None,
            clear_model: false,
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
    assert_eq!(OWNER_HANDLE, "owner");
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

#[test]
fn an_offboarded_agent_is_a_status_and_a_timestamp_on_the_same_member_record() {
    use crate::org::*;
    // The wire form of an offboarded agent.
    let offboarded = AgentProfile {
        status: AgentStatus::Offboarded,
        offboarded_at: Some("2026-11-08T07:00:00.000Z".into()),
        ..profile()
    };
    let v = serde_json::to_value(&offboarded).unwrap();
    assert_eq!(v["status"], "offboarded");
    assert_eq!(v["offboarded_at"], "2026-11-08T07:00:00.000Z");
    assert_eq!(
        serde_json::from_value::<AgentProfile>(v).unwrap(),
        offboarded
    );
    // A working agent has no offboarded_at on the wire, and an older record without one reads.
    assert!(serde_json::to_value(profile())
        .unwrap()
        .get("offboarded_at")
        .is_none());
    let old: AgentProfile = serde_json::from_value(json!({
        "role": "r", "title": "t", "runtime": "codex", "messaging_name": "x", "status": "active",
    }))
    .unwrap();
    assert_eq!(old.offboarded_at, None);

    // The member keeps its id, name and handle: offboarding changes the status, so it travels
    // as an ordinary member_upserted.
    let member = Member {
        id: "a1".into(),
        kind: MemberKind::Agent,
        name: "River".into(),
        contact: None,
        owner: false,
        office_id: Some("o1".into()),
        agent: Some(offboarded),
        reports_to_member_id: None,
        created_at: None,
        version: 6,
    };
    let change = OrgChange::MemberUpserted { member };
    let back: OrgChange = serde_json::from_value(serde_json::to_value(&change).unwrap()).unwrap();
    assert_eq!(back, change);

    // This build reads the word. (How an older build reads a word it does not know is the
    // general mechanism tested by `an_agent_status_in_any_unknown_shape_decodes_as_unknown`.)
    assert_eq!(
        serde_json::from_value::<AgentStatus>(json!("offboarded")).unwrap(),
        AgentStatus::Offboarded
    );
}

#[test]
fn member_offboard_params_are_pinned() {
    use crate::org::*;
    let request = Request::new::<MemberOffboard>(
        1,
        &MemberOffboardParams {
            command_id: "01C".into(),
            member_id: "a1".into(),
            expected_version: 5,
        },
    )
    .unwrap();
    assert_eq!(request.method, "member.offboard");
    assert_eq!(
        request.params,
        json!({"command_id": "01C", "member_id": "a1", "expected_version": 5})
    );
    assert_eq!(
        request.params::<MemberOffboard>().unwrap().expected_version,
        5
    );
}

#[test]
fn run_mode_and_schedule_wire_shapes_are_pinned() {
    let scheduled = AgentProfile {
        model: Some("claude-sonnet-5-5".into()),
        run_mode: Some(RunMode::Scheduled),
        schedule: Some(Schedule {
            spec: "weekdays at 9".into(),
            time_zone: "Europe/Lisbon".into(),
        }),
        ..profile()
    };
    let v = serde_json::to_value(&scheduled).unwrap();
    assert_eq!(v["model"], "claude-sonnet-5-5");
    assert_eq!(v["run_mode"], "scheduled");
    assert_eq!(
        v["schedule"],
        json!({"spec": "weekdays at 9", "time_zone": "Europe/Lisbon"})
    );
    assert_eq!(
        serde_json::from_value::<AgentProfile>(v).unwrap(),
        scheduled
    );
    // Not set: nothing on the wire, and an older record reads as None.
    let v = serde_json::to_value(profile()).unwrap();
    for key in ["model", "run_mode", "schedule", "offboarded_at"] {
        assert!(v.get(key).is_none(), "{key} was sent for a plain agent");
    }
    for (wire, want) in [
        ("always_on", RunMode::AlwaysOn),
        ("on_call", RunMode::OnCall),
        ("scheduled", RunMode::Scheduled),
    ] {
        assert_eq!(serde_json::to_value(want).unwrap(), json!(wire));
        assert_eq!(
            serde_json::from_value::<RunMode>(json!(wire)).unwrap(),
            want
        );
    }
    assert_eq!(
        serde_json::to_value(RunMode::Unknown).unwrap(),
        json!("unknown")
    );
}

#[test]
fn a_run_mode_in_any_unknown_shape_decodes_as_unknown_and_never_breaks_the_profile() {
    for run_mode in [
        json!(null),
        json!(5),
        json!([1]),
        json!({"future": 1}),
        json!("on_demand"),
    ] {
        let profile: AgentProfile = serde_json::from_value(json!({
            "role": "r", "title": "t", "runtime": "codex", "messaging_name": "x",
            "status": "active", "run_mode": run_mode,
        }))
        .unwrap_or_else(|e| panic!("run_mode {run_mode} broke the profile: {e}"));
        // `null` reads as "not set"; everything else we cannot read is Unknown.
        let want = if run_mode.is_null() {
            None
        } else {
            Some(RunMode::Unknown)
        };
        assert_eq!(profile.run_mode, want, "{run_mode}");
    }
}

#[test]
fn set_run_mode_and_the_new_update_fields_are_pinned() {
    use crate::org::*;
    let set = Request::new::<MemberSetRunMode>(
        1,
        &MemberSetRunModeParams {
            command_id: "01C".into(),
            member_id: "a1".into(),
            expected_version: 3,
            run_mode: Some(RunMode::Scheduled),
            schedule: Some(Schedule {
                spec: "daily at 6".into(),
                time_zone: "UTC".into(),
            }),
        },
    )
    .unwrap();
    assert_eq!(
        set.params,
        json!({
            "command_id": "01C", "member_id": "a1", "expected_version": 3,
            "run_mode": "scheduled", "schedule": {"spec": "daily at 6", "time_zone": "UTC"},
        })
    );
    let always = Request::new::<MemberSetRunMode>(
        2,
        &MemberSetRunModeParams {
            command_id: "01C".into(),
            member_id: "a1".into(),
            expected_version: 4,
            run_mode: Some(RunMode::AlwaysOn),
            schedule: None,
        },
    )
    .unwrap();
    assert!(always.params.get("schedule").is_none());
    // Left out = "not office-run": a person starts the agent.
    let cleared = Request::new::<MemberSetRunMode>(
        3,
        &MemberSetRunModeParams {
            command_id: "01C".into(),
            member_id: "a1".into(),
            expected_version: 5,
            run_mode: None,
            schedule: None,
        },
    )
    .unwrap();
    assert_eq!(
        cleared.params,
        json!({"command_id": "01C", "member_id": "a1", "expected_version": 5})
    );
    assert_eq!(cleared.params::<MemberSetRunMode>().unwrap().run_mode, None);

    let update = Request::new::<MemberUpdateAgent>(
        3,
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
            model: Some("claude-opus-5-5".into()),
            clear_model: false,
            home: None,
            clear_home: false,
            messaging_name: None,
        },
    )
    .unwrap();
    assert_eq!(
        update.params,
        json!({"command_id": "01C", "member_id": "a1", "expected_version": 4,
               "model": "claude-opus-5-5"})
    );
}

#[test]
fn a_member_carries_when_hq_created_it_and_older_records_without_it_still_read() {
    let member = Member {
        id: "a1".into(),
        kind: MemberKind::Agent,
        name: "River".into(),
        contact: None,
        owner: false,
        office_id: Some("o1".into()),
        agent: Some(profile()),
        reports_to_member_id: None,
        created_at: Some("2026-11-08T06:00:00.000Z".into()),
        version: 1,
    };
    let v = serde_json::to_value(&member).unwrap();
    assert_eq!(v["created_at"], "2026-11-08T06:00:00.000Z");
    assert_eq!(serde_json::from_value::<Member>(v).unwrap(), member);
    let old: Member =
        serde_json::from_value(json!({"id": "a2", "kind": "agent", "name": "x", "version": 1}))
            .unwrap();
    assert_eq!(old.created_at, None);
    assert!(serde_json::to_value(&old)
        .unwrap()
        .get("created_at")
        .is_none());
}

#[test]
fn schedule_limits_are_pinned() {
    assert_eq!(AGENT_TIME_ZONE_MAX_CHARS, 64);
}

#[test]
fn a_schedule_missing_a_field_still_reads_so_a_newer_writer_cannot_break_older_readers() {
    let schedule: Schedule = serde_json::from_value(json!({"time_zone": "UTC"})).unwrap();
    assert_eq!(schedule.spec, "");
    let schedule: Schedule = serde_json::from_value(json!({"spec": "daily", "future": 1})).unwrap();
    assert_eq!(schedule.time_zone, "");
    // Inside a whole member too.
    let profile: AgentProfile = serde_json::from_value(json!({
        "role": "r", "title": "t", "runtime": "codex", "messaging_name": "x",
        "status": "active", "run_mode": "scheduled", "schedule": {},
    }))
    .unwrap();
    assert_eq!(
        profile.schedule,
        Some(Schedule {
            spec: String::new(),
            time_zone: String::new()
        })
    );
}

#[test]
fn reserved_handle_words_and_address_shapes() {
    assert_eq!(OFFICE_HANDLE, "office");
    assert_eq!(RUN_HANDLE_PREFIX, "run:");
    // A `run:` prefix, any case, after trimming.
    for bad in ["run:1", "RUN:x", "Run:", " run:abc ", "rUn:0123456789"] {
        assert!(is_shaped_like_an_address(bad), "{bad}");
    }
    // A ULID: exactly 26 Crockford base32 characters, either case.
    for bad in [
        "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "01arz3ndektsv4rrffq69g5fav",
        " 01ARZ3NDEKTSV4RRFFQ69G5FAV ",
        // KELVIN SIGN lower-cases to ASCII `k`: handles are compared by their lower-cased key.
        "\u{212A}1ARZ3NDEKTSV4RRFFQ69G5FAV",
        // A lenient ULID decoder reads I, L, O and U too: any 26 letters and digits count.
        "01ARZ3NDEKTSV4RRFFQ69G5FAI",
        "01ARZ3NDEKTSV4RRFFQ69G5FAL",
        "01arz3ndektsv4rrffq69g5fao",
        "UUUUUUUUUUUUUUUUUUUUUUUUUU",
        "remyremyremyremyremyremyre",
    ] {
        assert!(is_shaped_like_an_address(bad), "{bad}");
    }
    // Not addresses: a run word without the colon, the wrong length, punctuation, or
    // non-ASCII where a prefix would end.
    for fine in [
        "run",
        "runner:1",
        "Ana",
        "pixel",
        "owner",
        "01ARZ3NDEKTSV4RRFFQ69G5FA",
        "01ARZ3NDEKTSV4RRFFQ69G5FAVV",
        "01ARZ3NDEKTSV4RRFFQ69G5F-V",
        "rün:1",
        "ab€c",
        "a€bc",
        "ru€:",
        "😀run:",
        "",
    ] {
        assert!(!is_shaped_like_an_address(fine), "{fine}");
    }
}

#[test]
fn client_made_ids_are_short_and_plain() {
    assert_eq!(ID_MAX_CHARS, 64);
    for good in [
        "a",
        "t1",
        "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "ms-ann_2",
        &"x".repeat(64),
    ] {
        assert!(is_valid_id(good), "{good}");
    }
    for bad in ["", " ", "a b", "a.b", "a/b", "é", "a\n", &"x".repeat(65)] {
        assert!(!is_valid_id(bad), "{bad:?}");
    }
}

#[test]
fn member_ids_are_26_letters_and_digits_and_no_handle_has_that_shape() {
    assert_eq!(MEMBER_ID_CHARS, 26);
    for good in [
        "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "01arz3ndektsv4rrffq69g5fav",
        "00000000000000000000000000",
    ] {
        assert!(is_valid_member_id(good), "{good}");
        assert!(
            is_shaped_like_an_address(good),
            "an id must never be a handle: {good}"
        );
    }
    for bad in [
        "",
        "remy",
        "ann",
        "01ARZ3NDEKTSV4RRFFQ69G5FA",
        "01ARZ3NDEKTSV4RRFFQ69G5FAVV",
        "01ARZ3NDEKTSV4RRFFQ69G5F-V",
        "01ARZ3NDEKTSV4RRFFQ69G5FAé",
        " 01ARZ3NDEKTSV4RRFFQ69G5FAV",
    ] {
        assert!(!is_valid_member_id(bad), "{bad:?}");
    }
}

#[test]
fn an_integration_branch_may_not_start_with_a_prefix_the_office_owns() {
    assert_eq!(
        RESERVED_BRANCH_PREFIXES,
        ["task/", "docs/", "merged/", "archive/"]
    );
    for reserved in [
        "task/W-12-login",
        "docs/adr-0004",
        "merged/task/W-12-login",
        "archive/task/W-9",
        "task/",
        "refs/heads/task/W-12",
        "heads/task/W-12",
        "Task/W-12",
        "DOCS/x",
    ] {
        assert!(has_reserved_branch_prefix(reserved), "{reserved:?}");
    }
    for fine in [
        "main",
        "v1",
        "develop",
        "release/2026-11",
        "tasks/x",
        "task",
        "my-task/x",
        "feature/task/x",
        "refs/heads/main",
        "refs/task/x",
        "",
        "é/task/",
    ] {
        assert!(!has_reserved_branch_prefix(fine), "{fine:?}");
    }
}

// ---- Work items (org_proto::work) and the project and area commands --------------------------

/// Pins a call's params: built through `Request::new` they are exactly `want`, and the line
/// reads back as the same params.
fn pin_params<C: Call>(params: &C::Params, want: serde_json::Value)
where
    C::Params: PartialEq + std::fmt::Debug,
{
    let request = Request::new::<C>(1, params).unwrap();
    assert_eq!(request.method, C::METHOD);
    assert_eq!(request.params, want, "{}", C::METHOD);
    let line = serde_json::to_string(&request).unwrap();
    let back: Request = serde_json::from_str(&line).unwrap();
    assert_eq!(&back.params::<C>().unwrap(), params, "{}", C::METHOD);
}

/// Every value has its wire name both ways, `Unknown` is written as "unknown", and anything
/// else, in any shape, reads as `Unknown`.
fn pin_enum<T>(pairs: &[(T, &str)], unknown: T)
where
    T: Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug + Copy,
{
    for &(value, name) in pairs {
        assert_eq!(serde_json::to_value(value).unwrap(), name);
        assert_eq!(serde_json::from_value::<T>(name.into()).unwrap(), value);
    }
    assert_eq!(serde_json::to_value(unknown).unwrap(), "unknown");
    for other in [
        json!("from_a_newer_build"),
        json!("TODO"),
        json!(7),
        json!(true),
        json!({"todo": {}}),
        json!(["todo"]),
    ] {
        assert_eq!(serde_json::from_value::<T>(other).unwrap(), unknown);
    }
}

fn task_summary() -> WorkItemSummary {
    WorkItemSummary {
        id: "01JC0000000000000000Q3K9F2X".into(),
        kind: ItemKind::Task,
        project_id: Some("p1".into()),
        number: Some(12),
        short_id: "3k9f2x".into(),
        title: "Add the checkout button".into(),
        status: ItemStatus::InProgress,
        priority: Some(Priority::High),
        assignee_id: Some("a1".into()),
        created_by: "owner1".into(),
        created_at: "2026-11-08T02:10:00.000Z".into(),
        updated_at: "2026-11-08T03:00:00.000Z".into(),
        team_id: Some("t1".into()),
        checklist: ChecklistCounts {
            total: 2,
            done: 1,
            required_left: 1,
        },
        owner_office_id: "hq".into(),
        generation: 1,
        version: 4,
        ..Default::default()
    }
}

fn wpos(seq: u64) -> WorkPosition {
    WorkPosition {
        incarnation: 7,
        seq,
    }
}

#[test]
fn work_wire_names_are_pinned() {
    use crate::org::*;
    use crate::work::*;
    let names = [
        (WorkSnapshot::METHOD, "work.snapshot"),
        (WorkGet::METHOD, "work.get"),
        (WorkList::METHOD, "work.list"),
        (WorkTimes::METHOD, "work.times"),
        (WorkActivity::METHOD, "work.activity"),
        (TaskCreate::METHOD, "task.create"),
        (TaskUpdate::METHOD, "task.update"),
        (TaskAssign::METHOD, "task.assign"),
        (TaskSetStatus::METHOD, "task.set_status"),
        (WorkSetCurrent::METHOD, "work.set_current"),
        (WorkComment::METHOD, "work.comment"),
        (WorkWatch::METHOD, "work.watch"),
        (WorkLink::METHOD, "work.link"),
        (WorkUnlink::METHOD, "work.unlink"),
        (WorkUndo::METHOD, "work.undo"),
        (WorkDelete::METHOD, "work.delete"),
        (ChecklistAdd::METHOD, "checklist.add"),
        (ChecklistEdit::METHOD, "checklist.edit"),
        (ChecklistCheck::METHOD, "checklist.check"),
        (ChecklistMove::METHOD, "checklist.move"),
        (ChecklistRemove::METHOD, "checklist.remove"),
        (ChecklistMakeTask::METHOD, "checklist.make_task"),
        (ChecklistUnlinkTask::METHOD, "checklist.unlink_task"),
        (BugReport::METHOD, "bug.report"),
        (BugUpdate::METHOD, "bug.update"),
        (BugTriage::METHOD, "bug.triage"),
        (BugAssign::METHOD, "bug.assign"),
        (BugMarkFixed::METHOD, "bug.mark_fixed"),
        (BugVerify::METHOD, "bug.verify"),
        (BugClose::METHOD, "bug.close"),
        (BugReopen::METHOD, "bug.reopen"),
        (WorkChanged::NAME, "work.changed"),
        (ProjectCreate::METHOD, "project.create"),
        (ProjectUpdate::METHOD, "project.update"),
        (ProjectArchive::METHOD, "project.archive"),
        (AreaCreate::METHOD, "area.create"),
        (AreaRename::METHOD, "area.rename"),
        (AreaSetTeam::METHOD, "area.set_team"),
        (AreaSetLead::METHOD, "area.set_lead"),
        (AreaDelete::METHOD, "area.delete"),
    ];
    for (got, want) in names {
        assert_eq!(got, want);
    }
    let limits = [
        (WORK_TITLE_MAX_CHARS, 120),
        (WORK_TEXT_MAX_CHARS, 10_000),
        (WORK_DESCRIPTION_MAX_CHARS, 20_000),
        (WORK_REQUEST_TEXT_MAX_CHARS, 20_000),
        (WORK_REASON_MAX_CHARS, 1_000),
        (WORK_BLOCKED_REASON_MAX_CHARS, 200),
        (WORK_DOD_MAX_CHARS, 1_000),
        (WORK_ENVIRONMENT_MAX_CHARS, 2_000),
        (WORK_STEP_TEXT_MAX_CHARS, 200),
        (WORK_MAX_STEPS, 100),
        (WORK_MAX_LINKS, 100),
        (WORK_MAX_DRAFT_TASKS, 50),
        (WORK_PAGE_MAX as usize, 500),
        (WORK_PARAMS_MAX_BYTES, 786_432),
        (WORK_RESPONSE_MAX_BYTES, 786_432),
        (WORK_CHANGED_MAX_BYTES, 524_288),
        (WORK_GET_MAX_LISTED, 200),
        (AREA_NAME_MAX_CHARS, 40),
        (PROJECT_KEY_MAX_CHARS, 10),
        (PROJECT_DESCRIPTION_MAX_CHARS, 1_000),
    ];
    for (got, want) in limits {
        assert_eq!(got, want);
    }
    assert_eq!(
        RESERVED_PROJECT_KEYS,
        &["req", "feat", "adr", "eval", "debt", "lim", "exc"]
    );
    // A work write is too large well before it fills a frame.
    const { assert!(WORK_PARAMS_MAX_BYTES < MAX_FRAME_BYTES) };
}

#[test]
fn the_new_error_codes_round_trip_and_reach_the_caller() {
    for (code, name) in [
        (ErrorCode::NotBound, "not_bound"),
        (ErrorCode::TooLarge, "too_large"),
        (ErrorCode::ChecklistIncomplete, "checklist_incomplete"),
    ] {
        assert_eq!(serde_json::to_value(code).unwrap(), name);
        assert_eq!(
            serde_json::from_value::<ErrorCode>(name.into()).unwrap(),
            code
        );
        let line = serde_json::to_string(&Response::error(3, ApiError::new(code, "x"))).unwrap();
        match serde_json::from_str::<Response>(&line)
            .unwrap()
            .into_result::<crate::work::TaskSetStatus>()
        {
            Err(CallError::Api(e)) => assert_eq!(e.code, code),
            other => panic!("expected an api error, got {other:?}"),
        }
    }
    // A code from a later phase reads as Unknown, the way a build older than
    // `checklist_incomplete` reads that one.
    let later: Response =
        serde_json::from_str(r#"{"id":3,"error":{"code":"not_owner","message":"x"}}"#).unwrap();
    match later.into_result::<crate::work::TaskSetStatus>() {
        Err(CallError::Api(e)) => assert_eq!(e.code, ErrorCode::Unknown),
        other => panic!("expected an api error, got {other:?}"),
    }
}

#[test]
fn work_enums_have_their_wire_names_and_read_anything_else_as_unknown() {
    pin_enum(
        &[
            (WorkStatus::Committed, "committed"),
            (WorkStatus::Queued, "queued"),
        ],
        WorkStatus::Unknown,
    );
    pin_enum(
        &[
            (ItemKind::Task, "task"),
            (ItemKind::Bug, "bug"),
            (ItemKind::Request, "request"),
        ],
        ItemKind::Unknown,
    );
    pin_enum(
        &[
            (ItemStatus::Todo, "todo"),
            (ItemStatus::InProgress, "in_progress"),
            (ItemStatus::Blocked, "blocked"),
            (ItemStatus::Done, "done"),
            (ItemStatus::New, "new"),
            (ItemStatus::Triaged, "triaged"),
            (ItemStatus::Fixed, "fixed"),
            (ItemStatus::Closed, "closed"),
            (ItemStatus::Routed, "routed"),
            (ItemStatus::Scoping, "scoping"),
            (ItemStatus::Scoped, "scoped"),
            (ItemStatus::Approved, "approved"),
            (ItemStatus::Declined, "declined"),
            (ItemStatus::Merged, "merged"),
        ],
        ItemStatus::Unknown,
    );
    pin_enum(
        &[
            (Resolution::Completed, "completed"),
            (Resolution::Cancelled, "cancelled"),
            (Resolution::Verified, "verified"),
            (Resolution::Duplicate, "duplicate"),
            (Resolution::WontFix, "wont_fix"),
            (Resolution::CantReproduce, "cant_reproduce"),
        ],
        Resolution::Unknown,
    );
    pin_enum(
        &[
            (Priority::Urgent, "urgent"),
            (Priority::High, "high"),
            (Priority::Normal, "normal"),
            (Priority::Low, "low"),
        ],
        Priority::Unknown,
    );
    pin_enum(
        &[
            (Severity::Critical, "critical"),
            (Severity::Major, "major"),
            (Severity::Minor, "minor"),
            (Severity::Trivial, "trivial"),
        ],
        Severity::Unknown,
    );
    pin_enum(
        &[
            (VerifyOutcome::Fixed, "fixed"),
            (VerifyOutcome::StillHappens, "still_happens"),
        ],
        VerifyOutcome::Unknown,
    );
    pin_enum(
        &[
            (RequestKind::Feature, "feature"),
            (RequestKind::Change, "change"),
            (RequestKind::Technical, "technical"),
            (RequestKind::Question, "question"),
        ],
        RequestKind::Unknown,
    );
    pin_enum(
        &[
            (RoutedBy::Person, "person"),
            (RoutedBy::Rule, "rule"),
            (RoutedBy::Copilot, "copilot"),
        ],
        RoutedBy::Unknown,
    );
    pin_enum(
        &[
            (LinkKind::Fixes, "fixes"),
            (LinkKind::PartOf, "part_of"),
            (LinkKind::FromRequest, "from_request"),
            (LinkKind::ReportedFrom, "reported_from"),
            (LinkKind::RelatesTo, "relates_to"),
            (LinkKind::PaysBack, "pays_back"),
        ],
        LinkKind::Unknown,
    );
    pin_enum(
        &[
            (FeatureStatus::Proposed, "proposed"),
            (FeatureStatus::Planned, "planned"),
            (FeatureStatus::InProgress, "in_progress"),
            (FeatureStatus::Delivered, "delivered"),
            (FeatureStatus::Retired, "retired"),
        ],
        FeatureStatus::Unknown,
    );
    pin_enum(
        &[
            (CommentKind::Note, "note"),
            (CommentKind::Question, "question"),
            (CommentKind::Answer, "answer"),
        ],
        CommentKind::Unknown,
    );
    pin_enum(
        &[(RepoHost::Local, "local"), (RepoHost::Github, "github")],
        RepoHost::Unknown,
    );
    // A write naming a status this server doesn't know still reads, as Unknown, so the server
    // can answer `invalid_request` instead of failing to read the call.
    let request: Request = serde_json::from_value(json!({
        "id": 1, "method": "task.set_status",
        "params": {"change_id": "c", "item_id": "i", "expected_version": 2, "status": "parked"}
    }))
    .unwrap();
    assert_eq!(
        request
            .params::<crate::work::TaskSetStatus>()
            .unwrap()
            .status,
        ItemStatus::Unknown
    );
}

#[test]
fn a_work_item_summary_has_this_wire_shape_and_leaves_out_what_is_empty() {
    let summary = task_summary();
    let want = json!({
        "id": "01JC0000000000000000Q3K9F2X",
        "kind": "task",
        "project_id": "p1",
        "number": 12,
        "short_id": "3k9f2x",
        "title": "Add the checkout button",
        "status": "in_progress",
        "priority": "high",
        "assignee_id": "a1",
        "created_by": "owner1",
        "created_at": "2026-11-08T02:10:00.000Z",
        "updated_at": "2026-11-08T03:00:00.000Z",
        "team_id": "t1",
        "checklist": {"total": 2, "done": 1, "required_left": 1},
        "owner_office_id": "hq",
        "generation": 1,
        "version": 4
    });
    assert_eq!(serde_json::to_value(&summary).unwrap(), want);
    assert_eq!(
        serde_json::from_value::<WorkItemSummary>(want).unwrap(),
        summary
    );

    // A bug from a later phase, with values and fields this build doesn't know: it still reads.
    let bug: WorkItemSummary = serde_json::from_value(json!({
        "id": "b1", "kind": "bug", "project_id": "p1", "number": 7, "short_id": "b1",
        "title": "Crash", "status": "needs_info", "resolution": "obsolete",
        "priority": "p0", "created_by": "a1",
        "created_at": "2026-11-08T02:10:00.000Z", "updated_at": "2026-11-08T02:10:00.000Z",
        "area_id": "ar1", "severity": "catastrophic", "severity_confirmed": true,
        "reopen_count": 2, "duplicate_of": "b0", "fixed_by": ["t9"],
        "owner_office_id": "hq", "generation": 1, "version": 3, "deleted": true,
        "sla": {"due": "tomorrow"}
    }))
    .unwrap();
    assert_eq!(bug.kind, ItemKind::Bug);
    assert_eq!(bug.status, ItemStatus::Unknown);
    assert_eq!(bug.resolution, Some(Resolution::Unknown));
    assert_eq!(bug.priority, Some(Priority::Unknown));
    assert_eq!(bug.severity, Some(Severity::Unknown));
    assert!(bug.severity_confirmed && bug.deleted);
    assert_eq!(bug.reopen_count, 2);
    assert_eq!(bug.fixed_by, vec!["t9".to_string()]);
    assert_eq!(bug.checklist, ChecklistCounts::default());
    // A newer writer leaving out a count never breaks an older reader.
    let counts: ChecklistCounts = serde_json::from_value(json!({"total": 3})).unwrap();
    assert_eq!((counts.total, counts.done, counts.required_left), (3, 0, 0));

    // An item HQ hasn't numbered yet (phase 2) has no number on the wire.
    let pending = WorkItemSummary {
        number: None,
        ..task_summary()
    };
    assert!(serde_json::to_value(&pending)
        .unwrap()
        .get("number")
        .is_none());
}

#[test]
fn work_get_returns_the_whole_item_and_unknown_links_never_break_it() {
    use crate::work::*;
    let item = WorkItem {
        summary: task_summary(),
        description: Some("Put it **under** the cart.".into()),
        definition_of_done: Some("The button pays.".into()),
        checklist: vec![
            ChecklistStep {
                id: "s1".into(),
                text: "Design".into(),
                required: true,
                done: true,
                checked_by: Some("a1".into()),
                checked_at: Some("2026-11-08T02:30:00.000Z".into()),
                version: 2,
                ..Default::default()
            },
            ChecklistStep {
                id: "s2".into(),
                text: "Tests".into(),
                required: true,
                task_id: Some("t13".into()),
                version: 2,
                ..Default::default()
            },
        ],
        links: vec![Link {
            id: "l1".into(),
            from_item_id: task_summary().id,
            kind: LinkKind::RelatesTo,
            to: LinkTarget::Doc {
                project_id: "p1".into(),
                doc_key: "ADR-0004".into(),
            },
            created_by: "owner1".into(),
            created_at: "2026-11-08T02:11:00.000Z".into(),
            target_deleted: false,
        }],
        backlinks: vec![Link {
            id: "l2".into(),
            from_item_id: "t13".into(),
            kind: LinkKind::PartOf,
            to: LinkTarget::Item {
                item_id: task_summary().id,
            },
            created_by: "a1".into(),
            created_at: "2026-11-08T02:40:00.000Z".into(),
            target_deleted: false,
        }],
        watchers: vec!["owner1".into(), "a1".into()],
        time: TimeFigures {
            lead_ms: 3_000_000,
            cycle_ms: Some(1_200_000),
            working_ms: None,
            waiting_on_owner_ms: 0,
            blocked_ms: 600_000,
            idle_ms: None,
            rest_ms: 2_400_000,
            open: true,
            rounds: 1,
            as_of: "2026-11-08T03:00:00.000Z".into(),
        },
        ..Default::default()
    };
    let response = Response::ok::<WorkGet>(2, &WorkGetResult { item: item.clone() }).unwrap();
    let v = serde_json::to_value(&response).unwrap();
    let wire = &v["result"]["item"];
    assert_eq!(wire["summary"]["number"], 12);
    assert_eq!(
        wire["links"][0]["to"],
        json!({"type": "doc", "project_id": "p1", "doc_key": "ADR-0004"})
    );
    assert_eq!(wire["links"][0]["kind"], "relates_to");
    assert!(wire["links"][0].get("target_deleted").is_none());
    assert_eq!(
        wire["backlinks"][0]["to"],
        json!({"type": "item", "item_id": "01JC0000000000000000Q3K9F2X"})
    );
    assert_eq!(wire["checklist"][1]["task_id"], "t13");
    assert_eq!(wire["checklist"][1]["done"], false);
    assert!(wire["time"].get("working_ms").is_none(), "not tracked");
    assert!(wire.get("outcome").is_none() && wire.get("shortcut").is_none());
    let back: Response = serde_json::from_value(v).unwrap();
    assert_eq!(back.into_result::<WorkGet>().unwrap().item, item);

    // A link kind and a target kind from a newer build read as Unknown; the item still reads.
    let newer: WorkGetResult = serde_json::from_value(json!({"item": {
        "summary": serde_json::to_value(task_summary()).unwrap(),
        "links": [{
            "id": "l9", "from_item_id": "x", "kind": "blocks",
            "to": {"type": "commit", "sha": "abc"},
            "created_by": "a1", "created_at": "2026-11-08T02:40:00.000Z"
        }],
        "time": {"lead_ms": 5, "as_of": "2026-11-08T03:00:00.000Z"}
    }}))
    .unwrap();
    assert_eq!(newer.item.links[0].kind, LinkKind::Unknown);
    assert_eq!(newer.item.links[0].to, LinkTarget::Unknown);
    assert!(newer.item.checklist.is_empty() && newer.item.watchers.is_empty());
    assert_eq!(newer.item.time.rounds, 0);

    // The later phases' detail reads now, so they add calls, not fields.
    let request: WorkItem = serde_json::from_value(json!({
        "summary": {
            "id": "r1", "kind": "request", "short_id": "r1", "title": "Dark mode",
            "status": "scoped", "created_by": "p2", "created_at": "2026-11-08T02:10:00.000Z",
            "updated_at": "2026-11-08T02:10:00.000Z", "request_kind": "feature",
            "owner_office_id": "hq", "generation": 1, "version": 5
        },
        "text": "Please add dark mode.", "routed_by": "copilot",
        "outcome": {
            "feature": {"type": "new", "feature_id": "f1"},
            "docs": [{"project_id": "p1", "doc_key": "docs/designs/dark.md"}],
            "draft_tasks": [{"draft_id": "d1", "title": "Theme tokens", "priority": "low"}],
            "estimate": {"effort": "a week", "cost_usd_cents_max": 5000}
        },
        "shortcut": {"type": "debt", "ids": ["DEBT-0012"]},
        "time": {"lead_ms": 0, "as_of": "2026-11-08T03:00:00.000Z"}
    }))
    .unwrap();
    assert_eq!(request.summary.request_kind, Some(RequestKind::Feature));
    assert_eq!(request.routed_by, Some(RoutedBy::Copilot));
    let outcome = request.outcome.unwrap();
    assert_eq!(
        outcome.feature,
        Some(FeatureOutcome::New {
            feature_id: "f1".into()
        })
    );
    assert_eq!(outcome.draft_tasks[0].priority, Some(Priority::Low));
    assert_eq!(outcome.estimate.unwrap().cost_usd_cents_max, Some(5000));
    assert_eq!(
        request.shortcut,
        Some(Shortcut::Debt {
            ids: vec!["DEBT-0012".into()]
        })
    );
    assert_eq!(
        serde_json::to_value(Shortcut::None).unwrap(),
        json!({"type": "none"})
    );
    assert_eq!(
        serde_json::from_value::<Shortcut>(json!({"type": "skipped_tests"})).unwrap(),
        Shortcut::Unknown
    );
    assert_eq!(
        serde_json::from_value::<FeatureOutcome>(json!({"type": "split", "into": []})).unwrap(),
        FeatureOutcome::Unknown
    );
}

#[test]
fn status_changed_names_a_duplicates_original_only_when_there_is_one() {
    use crate::work::*;
    // A duplicate close carries the original's id in `duplicate_of`; `reason` stays free text.
    let dup = WorkEventKind::StatusChanged {
        from: ItemStatus::New,
        to: ItemStatus::Closed,
        resolution: Some(Resolution::Duplicate),
        reason: None,
        anyway: None,
        duplicate_of: Some("b1".into()),
    };
    assert_eq!(
        serde_json::to_value(&dup).unwrap(),
        json!({"type": "status_changed", "from": "new", "to": "closed",
               "resolution": "duplicate", "duplicate_of": "b1"})
    );
    let back: WorkEventKind = serde_json::from_value(serde_json::to_value(&dup).unwrap()).unwrap();
    assert_eq!(back, dup);
    // Any other status change leaves it out.
    let wont_fix = WorkEventKind::StatusChanged {
        from: ItemStatus::Triaged,
        to: ItemStatus::Closed,
        resolution: Some(Resolution::WontFix),
        reason: Some("By design".into()),
        anyway: None,
        duplicate_of: None,
    };
    let wire = serde_json::to_value(&wont_fix).unwrap();
    assert_eq!(
        wire,
        json!({"type": "status_changed", "from": "triaged", "to": "closed",
               "resolution": "wont_fix", "reason": "By design"})
    );
    // An older writer's event (no `duplicate_of`) still reads, as absent.
    let old: WorkEventKind = serde_json::from_value(wire).unwrap();
    assert_eq!(old, wont_fix);
    let explicit_null: WorkEventKind = serde_json::from_value(json!({"type": "status_changed",
        "from": "triaged", "to": "closed", "resolution": "wont_fix", "reason": "By design",
        "duplicate_of": null}))
    .unwrap();
    assert_eq!(explicit_null, wont_fix);
}

#[test]
fn work_events_round_trip_and_unknown_kinds_never_break_the_activity() {
    use crate::work::*;
    let step = ChecklistStep {
        id: "s1".into(),
        text: "Design".into(),
        required: true,
        version: 1,
        ..Default::default()
    };
    let kinds = vec![
        WorkEventKind::Created,
        WorkEventKind::FieldsChanged {
            fields: vec![FieldChange {
                field: "title".into(),
                from: Some(json!("Old")),
                to: Some(json!("New")),
            }],
        },
        WorkEventKind::StatusChanged {
            from: ItemStatus::InProgress,
            to: ItemStatus::Done,
            resolution: Some(Resolution::Completed),
            reason: None,
            anyway: Some(DoneAnyway {
                reason: "Shipped behind a flag".into(),
                steps: vec!["s2".into()],
            }),
            duplicate_of: None,
        },
        WorkEventKind::Assigned {
            from: None,
            to: Some("a1".into()),
        },
        WorkEventKind::Triaged {
            area_id: "ar1".into(),
            severity: Severity::Major,
            priority: Priority::High,
            assignee_id: Some("a1".into()),
        },
        WorkEventKind::Verified {
            outcome: VerifyOutcome::StillHappens,
            by_reporter: true,
            note: Some("Happens on Safari too".into()),
        },
        WorkEventKind::Reopened { note: None },
        WorkEventKind::Linked {
            link: Link {
                id: "l1".into(),
                from_item_id: "t1".into(),
                kind: LinkKind::RelatesTo,
                to: LinkTarget::Item {
                    item_id: "t2".into(),
                },
                created_by: "a1".into(),
                created_at: "2026-11-08T02:11:00.000Z".into(),
                target_deleted: false,
            },
        },
        WorkEventKind::Unlinked {
            link_id: "l1".into(),
        },
        WorkEventKind::Comment {
            comment_id: "c1".into(),
            body: "Which colour?".into(),
            comment_kind: CommentKind::Question,
            asks: Some("owner1".into()),
            answers: None,
            watch: true,
        },
        WorkEventKind::StepsAdded {
            steps: vec![step],
            after_step_id: None,
        },
        WorkEventKind::StepEdited {
            step_id: "s1".into(),
            text: Some("Design it".into()),
            required: None,
        },
        WorkEventKind::StepChecked {
            step_id: "s1".into(),
            done: true,
        },
        WorkEventKind::StepMoved {
            step_id: "s1".into(),
            after_step_id: Some("s2".into()),
        },
        WorkEventKind::StepRemoved {
            step_id: "s1".into(),
        },
        WorkEventKind::StepMadeTask {
            step_id: "s2".into(),
            task_id: "t3".into(),
        },
        WorkEventKind::StepTaskUnlinked {
            step_id: "s2".into(),
            task_id: "t3".into(),
        },
        WorkEventKind::WaitingStarted {
            on_member_id: "owner1".into(),
            reason: WaitReason::Question {
                comment_id: "c1".into(),
            },
        },
        WorkEventKind::WaitingEnded {
            on_member_id: "owner1".into(),
            reason: WaitReason::Question {
                comment_id: "c1".into(),
            },
        },
        WorkEventKind::Watched {
            member_id: "a1".into(),
            watching: true,
        },
        WorkEventKind::Undone { event_version: 6 },
        WorkEventKind::Deleted,
    ];
    let events: Vec<WorkEvent> = kinds
        .into_iter()
        .enumerate()
        .map(|(i, change)| WorkEvent {
            target_id: "t1".into(),
            version: i as u64 + 1,
            at: "2026-11-08T02:10:00.000Z".into(),
            actor_id: Some("a1".into()),
            auto: false,
            cause: None,
            org_position: Position { epoch: 1, seq: 40 },
            change,
        })
        .collect();
    let result = WorkActivityResult {
        events: events.clone(),
        next_after_version: Some(19),
    };
    let v = serde_json::to_value(Response::ok::<WorkActivity>(1, &result).unwrap()).unwrap();
    let types: Vec<&str> = v["result"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["change"]["type"].as_str().unwrap())
        .collect();
    assert_eq!(
        types,
        [
            "created",
            "fields_changed",
            "status_changed",
            "assigned",
            "triaged",
            "verified",
            "reopened",
            "linked",
            "unlinked",
            "comment",
            "steps_added",
            "step_edited",
            "step_checked",
            "step_moved",
            "step_removed",
            "step_made_task",
            "step_task_unlinked",
            "waiting_started",
            "waiting_ended",
            "watched",
            "undone",
            "deleted",
        ]
    );
    assert_eq!(
        v["result"]["events"][2]["change"],
        json!({
            "type": "status_changed", "from": "in_progress", "to": "done",
            "resolution": "completed",
            "anyway": {"reason": "Shipped behind a flag", "steps": ["s2"]}
        })
    );
    assert_eq!(
        v["result"]["events"][4]["change"],
        json!({
            "type": "triaged", "area_id": "ar1", "severity": "major", "priority": "high",
            "assignee_id": "a1"
        })
    );
    assert_eq!(
        v["result"]["events"][5]["change"],
        json!({
            "type": "verified", "outcome": "still_happens", "by_reporter": true,
            "note": "Happens on Safari too"
        })
    );
    assert_eq!(
        v["result"]["events"][6]["change"],
        json!({"type": "reopened"})
    );
    assert_eq!(
        v["result"]["events"][9]["change"],
        json!({
            "type": "comment", "comment_id": "c1", "body": "Which colour?",
            "comment_kind": "question", "asks": "owner1", "watch": true
        })
    );
    assert_eq!(
        v["result"]["events"][17]["change"],
        json!({
            "type": "waiting_started", "on_member_id": "owner1",
            "reason": {"type": "question", "comment_id": "c1"}
        })
    );
    assert_eq!(
        v["result"]["events"][18]["change"],
        json!({
            "type": "waiting_ended", "on_member_id": "owner1",
            "reason": {"type": "question", "comment_id": "c1"}
        })
    );
    assert!(v["result"]["events"][0].get("auto").is_none());
    let back: Response = serde_json::from_value(v).unwrap();
    assert_eq!(back.into_result::<WorkActivity>().unwrap(), result);

    // An automatic change has no actor and names its cause.
    let auto = WorkEvent {
        target_id: "t1".into(),
        version: 9,
        at: "2026-11-08T02:10:00.000Z".into(),
        actor_id: None,
        auto: true,
        cause: Some(EventRef {
            target_id: "t3".into(),
            version: 4,
        }),
        org_position: Position { epoch: 1, seq: 40 },
        change: WorkEventKind::StepChecked {
            step_id: "s2".into(),
            done: true,
        },
    };
    let v = serde_json::to_value(&auto).unwrap();
    assert_eq!(v["auto"], true);
    assert!(v.get("actor_id").is_none());
    assert_eq!(v["cause"], json!({"target_id": "t3", "version": 4}));

    // Kinds from later phases read as Unknown, and the rest of the page still reads.
    let newer: WorkActivityResult = serde_json::from_value(json!({"events": [
        {"target_id": "b1", "version": 2, "at": "2026-11-08T02:10:00.000Z", "actor_id": "a1",
         "org_position": {"epoch": 1, "seq": 1},
         "change": {"type": "debt_status_changed", "doc_key": "DEBT-0012", "to": "paid"}},
        {"target_id": "b1", "version": 3, "at": "2026-11-08T02:10:00.000Z",
         "org_position": {"epoch": 1, "seq": 1},
         "change": {"type": "step_checked", "step_id": "s1", "done": false, "by_rule": "x"}}
    ]}))
    .unwrap();
    assert_eq!(newer.events[0].change, WorkEventKind::Unknown);
    assert_eq!(
        newer.events[1].change,
        WorkEventKind::StepChecked {
            step_id: "s1".into(),
            done: false
        }
    );
    assert!(newer.next_after_version.is_none());
}

#[test]
fn wait_reasons_have_their_wire_shapes_and_an_unknown_reason_never_breaks_the_event() {
    use crate::work::*;
    let reasons = [
        (
            WaitReason::Question {
                comment_id: "c1".into(),
            },
            json!({"type": "question", "comment_id": "c1"}),
        ),
        (
            WaitReason::Message {
                message_id: "m1".into(),
            },
            json!({"type": "message", "message_id": "m1"}),
        ),
        (WaitReason::Triage, json!({"type": "triage"})),
        (
            WaitReason::Decision {
                doc_key: "ADR-0004".into(),
            },
            json!({"type": "decision", "doc_key": "ADR-0004"}),
        ),
    ];
    for (reason, wire) in reasons {
        assert_eq!(serde_json::to_value(&reason).unwrap(), wire);
        assert_eq!(serde_json::from_value::<WaitReason>(wire).unwrap(), reason);
    }
    // A reason from a later build reads as Unknown, in the event too.
    assert_eq!(
        serde_json::from_value::<WaitReason>(json!({"type": "review", "pr": 7})).unwrap(),
        WaitReason::Unknown
    );

    // A bookkeeping event: no actor, not automatic, caused by the item event that opened it.
    let event: WorkEvent = serde_json::from_value(json!({
        "target_id": "t1", "version": 5, "at": "2026-11-08T02:10:00.000Z",
        "cause": {"target_id": "t1", "version": 4},
        "org_position": {"epoch": 1, "seq": 40},
        "change": {"type": "waiting_started", "on_member_id": "owner1",
                   "reason": {"type": "review", "pr": 7}}
    }))
    .unwrap();
    assert_eq!(event.actor_id, None);
    assert!(!event.auto);
    assert_eq!(
        event.cause,
        Some(EventRef {
            target_id: "t1".into(),
            version: 4
        })
    );
    assert_eq!(
        event.change,
        WorkEventKind::WaitingStarted {
            on_member_id: "owner1".into(),
            reason: WaitReason::Unknown,
        }
    );
    let ended: WorkEventKind = serde_json::from_value(json!({
        "type": "waiting_ended", "on_member_id": "owner1", "reason": {"type": "triage"}
    }))
    .unwrap();
    assert_eq!(
        ended,
        WorkEventKind::WaitingEnded {
            on_member_id: "owner1".into(),
            reason: WaitReason::Triage,
        }
    );
    // The reason is required: the pre-rev-2 shape (`reference`, no `reason`) doesn't decode
    // as a waiting event.
    assert!(serde_json::from_value::<WorkEventKind>(json!({
        "type": "waiting_started", "on_member_id": "owner1", "reference": "c1"
    }))
    .is_err());
}

#[test]
fn work_changed_carries_its_position_and_parts_and_tolerates_unknown_changes() {
    let data = WorkChangedData {
        position: wpos(42),
        change_id: Some("01CHANGE".into()),
        part: 1,
        parts: 2,
        changes: vec![
            WorkChange::ItemUpserted {
                item: task_summary(),
            },
            WorkChange::EventAdded {
                event: WorkEvent {
                    target_id: task_summary().id,
                    version: 4,
                    at: "2026-11-08T03:00:00.000Z".into(),
                    actor_id: Some("a1".into()),
                    auto: false,
                    cause: None,
                    org_position: Position { epoch: 1, seq: 40 },
                    change: WorkEventKind::Created,
                },
            },
            WorkChange::ItemNumbered {
                item_id: "t9".into(),
                number: 13,
            },
        ],
    };
    let ev = Event::new::<WorkChanged>(5, &data).unwrap();
    let v = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["event"], "work.changed");
    assert_eq!(v["data"]["position"], json!({"incarnation": 7, "seq": 42}));
    assert_eq!(v["data"]["change_id"], "01CHANGE");
    assert_eq!(
        (v["data"]["part"].clone(), v["data"]["parts"].clone()),
        (json!(1), json!(2))
    );
    assert_eq!(v["data"]["changes"][0]["type"], "item_upserted");
    assert_eq!(v["data"]["changes"][0]["item"]["short_id"], "3k9f2x");
    assert_eq!(v["data"]["changes"][1]["type"], "event_added");
    assert_eq!(
        v["data"]["changes"][2],
        json!({"type": "item_numbered", "item_id": "t9", "number": 13})
    );
    let parsed: Event = serde_json::from_value(v).unwrap();
    assert_eq!(parsed.data::<WorkChanged>().unwrap().unwrap(), data);

    // A cascade has no change id; a change kind from a later phase reads as Unknown.
    let newer: WorkChangedData = serde_json::from_value(json!({
        "position": {"incarnation": 7, "seq": 43}, "part": 1, "parts": 1,
        "changes": [
            {"type": "feature_upserted", "feature": {"id": "f1"}},
            {"type": "item_numbered", "item_id": "t9", "number": 14}
        ]
    }))
    .unwrap();
    assert!(newer.change_id.is_none());
    assert_eq!(newer.changes[0], WorkChange::Unknown);
    assert_eq!(
        newer.changes[1],
        WorkChange::ItemNumbered {
            item_id: "t9".into(),
            number: 14
        }
    );
}

#[test]
fn every_work_write_starts_with_a_change_id_and_answers_work_accepted() {
    use crate::work::*;
    pin_params::<TaskCreate>(
        &TaskCreateParams {
            change_id: "01C".into(),
            item_id: "t1".into(),
            project_id: "p1".into(),
            title: "Add the checkout button".into(),
            description: None,
            definition_of_done: Some("It pays.".into()),
            assignee_id: Some("a1".into()),
            team_id: None,
            priority: Some(Priority::High),
            feature_id: None,
            fixes: vec![],
            checklist: vec![NewStep {
                step_id: "s1".into(),
                text: "Design".into(),
                required: true,
            }],
        },
        json!({
            "change_id": "01C", "item_id": "t1", "project_id": "p1",
            "title": "Add the checkout button", "definition_of_done": "It pays.",
            "assignee_id": "a1", "priority": "high",
            "checklist": [{"step_id": "s1", "text": "Design", "required": true}]
        }),
    );
    pin_params::<TaskCreate>(
        &TaskCreateParams {
            change_id: "01C".into(),
            item_id: "t2".into(),
            project_id: "p1".into(),
            title: "Fix it".into(),
            description: None,
            definition_of_done: None,
            assignee_id: None,
            team_id: None,
            priority: None,
            feature_id: Some("f1".into()),
            fixes: vec![NewFixLink {
                link_id: "l1".into(),
                bug_id: "b1".into(),
            }],
            checklist: vec![],
        },
        json!({
            "change_id": "01C", "item_id": "t2", "project_id": "p1", "title": "Fix it",
            "feature_id": "f1", "fixes": [{"link_id": "l1", "bug_id": "b1"}]
        }),
    );
    pin_params::<TaskUpdate>(
        &TaskUpdateParams {
            change_id: "01C".into(),
            item_id: "t1".into(),
            expected_version: 3,
            title: Some("New".into()),
            description: None,
            clear_description: true,
            definition_of_done: None,
            clear_definition_of_done: false,
            priority: None,
            team_id: None,
            clear_team: true,
            feature_id: None,
            clear_feature: false,
        },
        json!({
            "change_id": "01C", "item_id": "t1", "expected_version": 3, "title": "New",
            "clear_description": true, "clear_team": true
        }),
    );
    pin_params::<TaskAssign>(
        &TaskAssignParams {
            change_id: "01C".into(),
            item_id: "t1".into(),
            expected_version: 3,
            assignee_id: None,
        },
        json!({"change_id": "01C", "item_id": "t1", "expected_version": 3}),
    );
    pin_params::<TaskSetStatus>(
        &TaskSetStatusParams {
            change_id: "01C".into(),
            item_id: "t1".into(),
            expected_version: 5,
            status: ItemStatus::Done,
            blocked_reason: None,
            resolution: Some(Resolution::Completed),
            anyway_reason: Some("Shipped behind a flag".into()),
            shortcut: None,
        },
        json!({
            "change_id": "01C", "item_id": "t1", "expected_version": 5, "status": "done",
            "resolution": "completed", "anyway_reason": "Shipped behind a flag"
        }),
    );
    pin_params::<TaskSetStatus>(
        &TaskSetStatusParams {
            change_id: "01C".into(),
            item_id: "t1".into(),
            expected_version: 5,
            status: ItemStatus::Blocked,
            blocked_reason: Some("Waiting for the API key".into()),
            resolution: None,
            anyway_reason: None,
            shortcut: None,
        },
        json!({
            "change_id": "01C", "item_id": "t1", "expected_version": 5, "status": "blocked",
            "blocked_reason": "Waiting for the API key"
        }),
    );
    pin_params::<WorkComment>(
        &WorkCommentParams {
            change_id: "01C".into(),
            item_id: "t1".into(),
            comment_id: "c1".into(),
            body: "Which colour?".into(),
            comment_kind: Some(CommentKind::Question),
            asks: Some("owner1".into()),
            answers: None,
            watch: true,
        },
        json!({
            "change_id": "01C", "item_id": "t1", "comment_id": "c1", "body": "Which colour?",
            "comment_kind": "question", "asks": "owner1", "watch": true
        }),
    );
    pin_params::<WorkWatch>(
        &WorkWatchParams {
            change_id: "01C".into(),
            item_id: "t1".into(),
            watching: false,
        },
        json!({"change_id": "01C", "item_id": "t1", "watching": false}),
    );
    pin_params::<WorkLink>(
        &WorkLinkParams {
            change_id: "01C".into(),
            link_id: "l1".into(),
            from_item_id: "t1".into(),
            kind: LinkKind::RelatesTo,
            to: LinkTarget::Doc {
                project_id: "p1".into(),
                doc_key: "ADR-0004".into(),
            },
        },
        json!({
            "change_id": "01C", "link_id": "l1", "from_item_id": "t1", "kind": "relates_to",
            "to": {"type": "doc", "project_id": "p1", "doc_key": "ADR-0004"}
        }),
    );
    pin_params::<WorkUnlink>(
        &WorkUnlinkParams {
            change_id: "01C".into(),
            link_id: "l1".into(),
        },
        json!({"change_id": "01C", "link_id": "l1"}),
    );
    pin_params::<WorkUndo>(
        &WorkUndoParams {
            change_id: "01C".into(),
            target_id: "b1".into(),
            event_version: 6,
        },
        json!({"change_id": "01C", "target_id": "b1", "event_version": 6}),
    );
    pin_params::<WorkDelete>(
        &WorkDeleteParams {
            change_id: "01C".into(),
            item_id: "t1".into(),
            expected_version: 8,
        },
        json!({"change_id": "01C", "item_id": "t1", "expected_version": 8}),
    );
    pin_params::<ChecklistAdd>(
        &ChecklistAddParams {
            change_id: "01C".into(),
            item_id: "t1".into(),
            steps: vec![NewStep {
                step_id: "s3".into(),
                text: "Docs".into(),
                required: false,
            }],
            after_step_id: Some("s1".into()),
        },
        json!({
            "change_id": "01C", "item_id": "t1", "after_step_id": "s1",
            "steps": [{"step_id": "s3", "text": "Docs", "required": false}]
        }),
    );
    pin_params::<ChecklistEdit>(
        &ChecklistEditParams {
            change_id: "01C".into(),
            step_id: "s1".into(),
            expected_version: 2,
            text: None,
            required: Some(false),
        },
        json!({"change_id": "01C", "step_id": "s1", "expected_version": 2, "required": false}),
    );
    pin_params::<ChecklistCheck>(
        &ChecklistCheckParams {
            change_id: "01C".into(),
            step_id: "s1".into(),
            done: true,
        },
        json!({"change_id": "01C", "step_id": "s1", "done": true}),
    );
    pin_params::<ChecklistMove>(
        &ChecklistMoveParams {
            change_id: "01C".into(),
            step_id: "s1".into(),
            after_step_id: None,
        },
        json!({"change_id": "01C", "step_id": "s1"}),
    );
    pin_params::<ChecklistRemove>(
        &ChecklistRemoveParams {
            change_id: "01C".into(),
            step_id: "s1".into(),
            expected_version: 3,
        },
        json!({"change_id": "01C", "step_id": "s1", "expected_version": 3}),
    );
    pin_params::<ChecklistMakeTask>(
        &ChecklistMakeTaskParams {
            change_id: "01C".into(),
            step_id: "s2".into(),
            task_id: "t3".into(),
            link_id: "l3".into(),
            title: None,
        },
        json!({"change_id": "01C", "step_id": "s2", "task_id": "t3", "link_id": "l3"}),
    );
    pin_params::<ChecklistUnlinkTask>(
        &ChecklistUnlinkTaskParams {
            change_id: "01C".into(),
            step_id: "s2".into(),
        },
        json!({"change_id": "01C", "step_id": "s2"}),
    );

    // Bugs (phase 2).
    pin_params::<BugReport>(
        &BugReportParams {
            change_id: "01C".into(),
            item_id: "b1".into(),
            project_id: "p1".into(),
            title: "Coupon total is off".into(),
            what_happened: "The total ignores the coupon.".into(),
            severity: Severity::Major,
            area_id: None,
            expected: Some("The coupon is taken off.".into()),
            steps: None,
            environment: None,
            from: Some(BugReportedFrom {
                link_id: "l1".into(),
                to: LinkTarget::Item {
                    item_id: "t1".into(),
                },
            }),
            affects_feature_id: None,
            checklist: vec![],
        },
        json!({
            "change_id": "01C", "item_id": "b1", "project_id": "p1",
            "title": "Coupon total is off", "what_happened": "The total ignores the coupon.",
            "severity": "major", "expected": "The coupon is taken off.",
            "from": {"link_id": "l1", "to": {"type": "item", "item_id": "t1"}}
        }),
    );
    pin_params::<BugUpdate>(
        &BugUpdateParams {
            change_id: "01C".into(),
            item_id: "b1".into(),
            expected_version: 2,
            title: None,
            what_happened: None,
            expected: None,
            clear_expected: true,
            steps: Some("1. Add a coupon".into()),
            clear_steps: false,
            environment: None,
            clear_environment: false,
            severity: Some(Severity::Critical),
            priority: None,
            area_id: Some("ar2".into()),
            affects_feature_id: None,
            clear_affects_feature: false,
        },
        json!({
            "change_id": "01C", "item_id": "b1", "expected_version": 2,
            "clear_expected": true, "steps": "1. Add a coupon", "severity": "critical",
            "area_id": "ar2"
        }),
    );
    pin_params::<BugTriage>(
        &BugTriageParams {
            change_id: "01C".into(),
            item_id: "b1".into(),
            expected_version: 2,
            area_id: "ar1".into(),
            severity: Severity::Minor,
            priority: Priority::Low,
            assignee_id: Some("a1".into()),
        },
        json!({
            "change_id": "01C", "item_id": "b1", "expected_version": 2, "area_id": "ar1",
            "severity": "minor", "priority": "low", "assignee_id": "a1"
        }),
    );
    pin_params::<BugAssign>(
        &BugAssignParams {
            change_id: "01C".into(),
            item_id: "b1".into(),
            expected_version: 3,
            assignee_id: None,
        },
        json!({"change_id": "01C", "item_id": "b1", "expected_version": 3}),
    );
    pin_params::<BugMarkFixed>(
        &BugMarkFixedParams {
            change_id: "01C".into(),
            item_id: "b1".into(),
            expected_version: 4,
            note: "Rounded the total".into(),
            anyway_reason: Some("Docs follow".into()),
        },
        json!({
            "change_id": "01C", "item_id": "b1", "expected_version": 4,
            "note": "Rounded the total", "anyway_reason": "Docs follow"
        }),
    );
    pin_params::<BugVerify>(
        &BugVerifyParams {
            change_id: "01C".into(),
            item_id: "b1".into(),
            expected_version: 5,
            outcome: VerifyOutcome::StillHappens,
            note: Some("Still off by a cent".into()),
        },
        json!({
            "change_id": "01C", "item_id": "b1", "expected_version": 5,
            "outcome": "still_happens", "note": "Still off by a cent"
        }),
    );
    pin_params::<BugClose>(
        &BugCloseParams {
            change_id: "01C".into(),
            item_id: "b1".into(),
            expected_version: 5,
            duplicate_of: Some("b0".into()),
            resolution: None,
            reason: None,
        },
        json!({"change_id": "01C", "item_id": "b1", "expected_version": 5, "duplicate_of": "b0"}),
    );
    pin_params::<BugClose>(
        &BugCloseParams {
            change_id: "01C".into(),
            item_id: "b1".into(),
            expected_version: 5,
            duplicate_of: None,
            resolution: Some(Resolution::WontFix),
            reason: Some("By design".into()),
        },
        json!({
            "change_id": "01C", "item_id": "b1", "expected_version": 5,
            "resolution": "wont_fix", "reason": "By design"
        }),
    );
    pin_params::<BugReopen>(
        &BugReopenParams {
            change_id: "01C".into(),
            item_id: "b1".into(),
            expected_version: 6,
            note: "It came back".into(),
        },
        json!({"change_id": "01C", "item_id": "b1", "expected_version": 6, "note": "It came back"}),
    );
    // A report without a severity, and a triage without an area, are not valid params.
    for (method, params) in [
        (
            "bug.report",
            json!({"change_id": "c", "item_id": "b", "project_id": "p", "title": "t",
                   "what_happened": "w"}),
        ),
        (
            "bug.triage",
            json!({"change_id": "c", "item_id": "b", "expected_version": 1,
                   "severity": "minor", "priority": "low"}),
        ),
    ] {
        let request: Request =
            serde_json::from_value(json!({"id": 1, "method": method, "params": params})).unwrap();
        let read = match method {
            "bug.report" => request.params::<BugReport>().map(|_| ()),
            _ => request.params::<BugTriage>().map(|_| ()),
        };
        assert!(read.is_err(), "{method}");
    }

    // The change id is required, and so is an expected_version where the call has one.
    for (method, params) in [
        (
            "task.assign",
            json!({"item_id": "t1", "expected_version": 1}),
        ),
        ("checklist.check", json!({"step_id": "s1", "done": true})),
        (
            "task.update",
            json!({"change_id": "c", "item_id": "t1", "title": "x"}),
        ),
    ] {
        let request: Request =
            serde_json::from_value(json!({"id": 1, "method": method, "params": params})).unwrap();
        let read = match method {
            "task.assign" => request.params::<TaskAssign>().map(|_| ()),
            "checklist.check" => request.params::<ChecklistCheck>().map(|_| ()),
            _ => request.params::<TaskUpdate>().map(|_| ()),
        };
        assert!(read.is_err(), "{method}");
    }

    // The answer.
    let accepted = WorkAccepted {
        status: WorkStatus::Committed,
        position: Some(wpos(42)),
        items: vec![task_summary()],
        features: vec![],
        docs: vec![],
    };
    let v = serde_json::to_value(Response::ok::<TaskCreate>(1, &accepted).unwrap()).unwrap();
    assert_eq!(v["result"]["status"], "committed");
    assert_eq!(
        v["result"]["position"],
        json!({"incarnation": 7, "seq": 42})
    );
    assert_eq!(v["result"]["items"][0]["number"], 12);
    assert!(v["result"].get("features").is_none() && v["result"].get("docs").is_none());
    let back: Response = serde_json::from_value(v).unwrap();
    assert_eq!(back.into_result::<TaskCreate>().unwrap(), accepted);

    let queued: WorkAccepted = serde_json::from_value(json!({"status": "queued"})).unwrap();
    assert_eq!(queued.status, WorkStatus::Queued);
    assert!(queued.position.is_none() && queued.items.is_empty());
    let newer: WorkAccepted = serde_json::from_value(json!({
        "status": "forwarded", "items": [],
        "docs": [{"project_id": "p1", "path": "docs/features/7-x.md", "branch": "docs/x", "commit": "abc"}]
    }))
    .unwrap();
    assert_eq!(newer.status, WorkStatus::Unknown);
    assert_eq!(newer.docs[0].branch, "docs/x");
}

#[test]
fn work_reads_have_the_agreed_shapes() {
    // Every filter is optional: `{}` (and no params at all) reads.
    for method in ["work.snapshot", "work.list"] {
        for request in [
            json!({"id": 1, "method": method, "params": {}}),
            json!({"id": 1, "method": method}),
        ] {
            let request: Request = serde_json::from_value(request).unwrap();
            let ok = match method {
                "work.snapshot" => request.params::<WorkSnapshot>().unwrap() == Default::default(),
                _ => request.params::<WorkList>().unwrap() == Default::default(),
            };
            assert!(ok, "{method}");
        }
    }
    pin_params::<WorkSnapshot>(
        &WorkSnapshotParams {
            project_id: Some("p1".into()),
            closed_since: Some("2026-11-01T00:00:00.000Z".into()),
            all_closed: false,
            cursor: Some("k1".into()),
            limit: Some(200),
        },
        json!({
            "project_id": "p1", "closed_since": "2026-11-01T00:00:00.000Z", "cursor": "k1",
            "limit": 200
        }),
    );
    pin_params::<WorkGet>(
        &WorkGetParams {
            item: "chk-T-12".into(),
        },
        json!({"item": "chk-T-12"}),
    );
    pin_params::<WorkList>(
        &WorkListParams {
            project_id: Some("p1".into()),
            kinds: vec![ItemKind::Bug],
            statuses: vec![ItemStatus::New, ItemStatus::Triaged],
            assignee: Some("none".into()),
            linked_to: Some("b1".into()),
            link_kind: Some(LinkKind::Fixes),
            query: Some("checkout crash".into()),
            include_closed: true,
            ..Default::default()
        },
        json!({
            "project_id": "p1", "kinds": ["bug"], "statuses": ["new", "triaged"],
            "assignee": "none", "linked_to": "b1", "link_kind": "fixes",
            "query": "checkout crash", "include_closed": true
        }),
    );
    pin_params::<WorkTimes>(
        &WorkTimesParams {
            item_ids: vec!["t1".into(), "t2".into()],
        },
        json!({"item_ids": ["t1", "t2"]}),
    );
    pin_params::<WorkActivity>(
        &WorkActivityParams {
            target_id: "t1".into(),
            after_version: Some(3),
            limit: None,
        },
        json!({"target_id": "t1", "after_version": 3}),
    );

    let page = WorkSnapshotResult {
        position: wpos(41),
        items: vec![task_summary()],
        features: vec![],
        next_cursor: Some("k2".into()),
    };
    let v = serde_json::to_value(Response::ok::<WorkSnapshot>(1, &page).unwrap()).unwrap();
    assert_eq!(v["result"]["position"]["seq"], 41);
    assert_eq!(v["result"]["next_cursor"], "k2");
    assert!(v["result"].get("features").is_none());
    let back: Response = serde_json::from_value(v).unwrap();
    assert_eq!(back.into_result::<WorkSnapshot>().unwrap(), page);

    // Phase 3's features ride along in the same result.
    let with_features: WorkSnapshotResult = serde_json::from_value(json!({
        "position": {"incarnation": 7, "seq": 41}, "items": [],
        "features": [{
            "id": "f1", "project_id": "p1", "number": 7, "path": "docs/features/7-dark.md",
            "title": "Dark mode", "status": "sunset", "version": 2
        }]
    }))
    .unwrap();
    assert_eq!(with_features.features[0].status, FeatureStatus::Unknown);
    assert_eq!(with_features.features[0].tasks_total, 0);

    let times = WorkTimesResult {
        times: vec![ItemTime {
            item_id: "t1".into(),
            time: TimeFigures {
                lead_ms: 10,
                as_of: "2026-11-08T03:00:00.000Z".into(),
                ..Default::default()
            },
        }],
    };
    let v = serde_json::to_value(&times).unwrap();
    assert_eq!(
        v,
        json!({"times": [{"item_id": "t1", "time": {
            "lead_ms": 10, "waiting_on_owner_ms": 0, "blocked_ms": 0, "rest_ms": 0,
            "open": false, "rounds": 0, "as_of": "2026-11-08T03:00:00.000Z"
        }}]})
    );
    assert_eq!(serde_json::from_value::<WorkTimesResult>(v).unwrap(), times);

    let list: WorkListResult = serde_json::from_value(json!({"items": []})).unwrap();
    assert!(list.items.is_empty() && list.next_cursor.is_none());
}

#[test]
fn work_set_current_takes_no_change_id_and_answers_empty() {
    use crate::work::*;
    pin_params::<WorkSetCurrent>(
        &WorkSetCurrentParams {
            item: Some("chk-T-12".into()),
        },
        json!({"item": "chk-T-12"}),
    );
    pin_params::<WorkSetCurrent>(&WorkSetCurrentParams::default(), json!({}));
    let v = serde_json::to_value(Response::ok::<WorkSetCurrent>(1, &Empty {}).unwrap()).unwrap();
    assert_eq!(v["result"], json!({}));
}

#[test]
fn subscribe_can_resume_the_work_feed_too() {
    fn copy<T: Copy>() {}
    copy::<SubscribeParams>();
    copy::<WorkPosition>();

    let request: Request = serde_json::from_str(
        r#"{"id":1,"method":"subscribe","params":{"from_work_position":{"incarnation":7,"seq":41}}}"#,
    )
    .unwrap();
    let params = request.params::<Subscribe>().unwrap();
    assert_eq!(params.from_work_position, Some(wpos(41)));
    assert!(params.from_position.is_none());

    let both = SubscribeParams {
        from_position: Some(Position { epoch: 1, seq: 42 }),
        from_work_position: Some(wpos(41)),
    };
    pin_params::<Subscribe>(
        &both,
        json!({
            "from_position": {"epoch": 1, "seq": 42},
            "from_work_position": {"incarnation": 7, "seq": 41}
        }),
    );
    // Live events only, as before.
    pin_params::<Subscribe>(&SubscribeParams::default(), json!({}));
}

#[test]
fn the_project_record_replaces_project_summary_and_old_records_still_read() {
    // A record from before: `{ id, name }`.
    let old: ProjectSummary =
        serde_json::from_value(json!({"id": "p1", "name": "Checkout"})).unwrap();
    assert_eq!(old, Project::new("p1", "Checkout"));
    assert_eq!(old.key, "");
    assert!(old.repo.is_none() && !old.archived && old.created_at.is_none());
    assert_eq!(old.version, 0);

    let mut project = Project::new("p1", "Checkout");
    project.key = "chk".into();
    project.description = Some("The checkout flow.".into());
    project.repo = Some(ProjectRepo {
        host: RepoHost::Local,
        default_branch: "main".into(),
        path: Some("/Users/me/Org/repos/p1.git".into()),
        github: None,
    });
    project.integration_branch = Some("v1".into());
    project.created_at = Some("2026-11-08T02:00:00.000Z".into());
    project.version = 3;
    let want = json!({
        "id": "p1", "key": "chk", "name": "Checkout", "description": "The checkout flow.",
        "repo": {"host": "local", "default_branch": "main", "path": "/Users/me/Org/repos/p1.git"},
        "integration_branch": "v1", "created_at": "2026-11-08T02:00:00.000Z", "version": 3
    });
    assert_eq!(serde_json::to_value(&project).unwrap(), want);
    assert_eq!(serde_json::from_value::<Project>(want).unwrap(), project);

    // A GitHub repository, and a host this build doesn't know.
    let github: ProjectRepo = serde_json::from_value(json!({
        "host": "github", "default_branch": "main",
        "github": {"owner": "acme", "name": "checkout", "repo_id": 123456789, "private": true}
    }))
    .unwrap();
    assert_eq!(github.host, RepoHost::Github);
    assert_eq!(github.github.as_ref().unwrap().repo_id, 123_456_789);
    let gitlab: ProjectRepo =
        serde_json::from_value(json!({"host": {"gitlab": "x"}, "default_branch": "main"})).unwrap();
    assert_eq!(gitlab.host, RepoHost::Unknown);

    let archived: Project =
        serde_json::from_value(json!({"id": "p2", "name": "Old", "archived": true})).unwrap();
    assert!(archived.archived);
}

#[test]
fn an_org_snapshot_without_areas_still_reads_and_areas_round_trip() {
    let old: OrgSnapshot = serde_json::from_value(json!({
        "position": {"epoch": 1, "seq": 7}, "teams": [], "members": [], "memberships": [],
        "projects": [{"id": "p1", "name": "Checkout"}],
        "project_teams": []
    }))
    .unwrap();
    assert!(old.areas.is_empty());
    assert_eq!(old.projects, vec![Project::new("p1", "Checkout")]);

    let general = Area {
        id: "ar1".into(),
        project_id: "p1".into(),
        name: "General".into(),
        team_id: Some("t1".into()),
        lead_member_id: None,
        general: true,
        deleted: false,
        replaced_by: None,
        version: 1,
    };
    let tombstone = Area {
        id: "ar2".into(),
        project_id: "p1".into(),
        name: "Payments".into(),
        team_id: None,
        lead_member_id: None,
        general: false,
        deleted: true,
        replaced_by: Some("ar1".into()),
        version: 4,
    };
    assert_eq!(
        serde_json::to_value(&general).unwrap(),
        json!({"id": "ar1", "project_id": "p1", "name": "General", "team_id": "t1",
               "general": true, "version": 1})
    );
    let snapshot = OrgSnapshot {
        areas: vec![general, tombstone],
        ..old
    };
    let v = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(v["areas"][1]["deleted"], true);
    assert_eq!(v["areas"][1]["replaced_by"], "ar1");
    assert_eq!(serde_json::from_value::<OrgSnapshot>(v).unwrap(), snapshot);
}

#[test]
fn project_and_area_changes_reach_the_client_in_org_changed() {
    let mut project = Project::new("p1", "Checkout");
    project.key = "chk".into();
    project.version = 1;
    let data = OrgChangedData {
        position: Position { epoch: 1, seq: 9 },
        command_id: "c1".into(),
        changes: vec![
            OrgChange::ProjectUpserted {
                project: project.clone(),
            },
            OrgChange::AreaUpserted {
                area: Area {
                    id: "ar1".into(),
                    project_id: "p1".into(),
                    name: "General".into(),
                    team_id: None,
                    lead_member_id: None,
                    general: true,
                    deleted: false,
                    replaced_by: None,
                    version: 1,
                },
            },
        ],
    };
    let ev = Event::new::<OrgChanged>(6, &data).unwrap();
    let v = serde_json::to_value(&ev).unwrap();
    assert_eq!(
        v["data"]["changes"][0],
        json!({"type": "project_upserted", "project": {
            "id": "p1", "key": "chk", "name": "Checkout", "version": 1
        }})
    );
    assert_eq!(v["data"]["changes"][1]["type"], "area_upserted");
    let parsed: Event = serde_json::from_value(v).unwrap();
    assert_eq!(parsed.data::<OrgChanged>().unwrap().unwrap(), data);
}

#[test]
fn project_and_area_commands_are_pinned() {
    use crate::org::*;
    pin_params::<ProjectCreate>(
        &ProjectCreateParams {
            command_id: "01C".into(),
            project_id: "p1".into(),
            name: "Checkout".into(),
            key: Some("chk".into()),
            description: None,
            teams: vec![ProjectCreateTeam {
                project_team_id: "pt1".into(),
                team_id: "t1".into(),
            }],
            general_area_id: "ar1".into(),
            general_area_name: None,
            repo: Some(RepoSpec::NewLocal {}),
            integration_branch: None,
        },
        json!({
            "command_id": "01C", "project_id": "p1", "name": "Checkout", "key": "chk",
            "teams": [{"project_team_id": "pt1", "team_id": "t1"}],
            "general_area_id": "ar1", "repo": {"new_local": {}}
        }),
    );
    pin_params::<ProjectCreate>(
        &ProjectCreateParams {
            command_id: "01C".into(),
            project_id: "p2".into(),
            name: "Billing".into(),
            key: None,
            description: Some("Invoices.".into()),
            teams: vec![],
            general_area_id: "ar9".into(),
            general_area_name: Some("Core".into()),
            repo: Some(RepoSpec::ExistingLocal {
                path: "/Users/me/src/billing".into(),
            }),
            integration_branch: Some("v1".into()),
        },
        json!({
            "command_id": "01C", "project_id": "p2", "name": "Billing",
            "description": "Invoices.", "general_area_id": "ar9", "general_area_name": "Core",
            "repo": {"existing_local": {"path": "/Users/me/src/billing"}},
            "integration_branch": "v1"
        }),
    );
    // A repository kind this server doesn't know can't be read: the command is refused.
    let request: Request = serde_json::from_value(json!({
        "id": 1, "method": "project.create", "params": {
            "command_id": "c", "project_id": "p", "name": "n", "general_area_id": "a",
            "repo": {"new_github": {"owner": "acme"}}
        }
    }))
    .unwrap();
    assert!(request.params::<ProjectCreate>().is_err());

    pin_params::<ProjectUpdate>(
        &ProjectUpdateParams {
            command_id: "01C".into(),
            project_id: "p1".into(),
            expected_version: 2,
            name: None,
            description: None,
            clear_description: true,
            integration_branch: Some("main".into()),
            clear_integration_branch: false,
        },
        json!({
            "command_id": "01C", "project_id": "p1", "expected_version": 2,
            "clear_description": true, "integration_branch": "main"
        }),
    );
    pin_params::<ProjectArchive>(
        &ProjectArchiveParams {
            command_id: "01C".into(),
            project_id: "p1".into(),
            expected_version: 3,
            archived: true,
        },
        json!({"command_id": "01C", "project_id": "p1", "expected_version": 3, "archived": true}),
    );
    pin_params::<ProjectRemoveTeam>(
        &ProjectRemoveTeamParams {
            command_id: "01C".into(),
            project_team_id: "pt1".into(),
            expected_version: 1,
            areas_to_team_id: Some("t2".into()),
        },
        json!({
            "command_id": "01C", "project_team_id": "pt1", "expected_version": 1,
            "areas_to_team_id": "t2"
        }),
    );
    // The command as it was before still reads, and is written the same way.
    pin_params::<ProjectRemoveTeam>(
        &ProjectRemoveTeamParams {
            command_id: "01C".into(),
            project_team_id: "pt1".into(),
            expected_version: 1,
            areas_to_team_id: None,
        },
        json!({"command_id": "01C", "project_team_id": "pt1", "expected_version": 1}),
    );
    pin_params::<AreaCreate>(
        &AreaCreateParams {
            command_id: "01C".into(),
            area_id: "ar2".into(),
            project_id: "p1".into(),
            name: "Payments".into(),
            team_id: Some("t1".into()),
            lead_member_id: Some("a1".into()),
        },
        json!({
            "command_id": "01C", "area_id": "ar2", "project_id": "p1", "name": "Payments",
            "team_id": "t1", "lead_member_id": "a1"
        }),
    );
    pin_params::<AreaRename>(
        &AreaRenameParams {
            command_id: "01C".into(),
            area_id: "ar1".into(),
            expected_version: 1,
            name: "Core".into(),
        },
        json!({"command_id": "01C", "area_id": "ar1", "expected_version": 1, "name": "Core"}),
    );
    pin_params::<AreaSetTeam>(
        &AreaSetTeamParams {
            command_id: "01C".into(),
            area_id: "ar1".into(),
            expected_version: 2,
            team_id: None,
        },
        json!({"command_id": "01C", "area_id": "ar1", "expected_version": 2}),
    );
    pin_params::<AreaSetLead>(
        &AreaSetLeadParams {
            command_id: "01C".into(),
            area_id: "ar1".into(),
            expected_version: 2,
            lead_member_id: Some("a2".into()),
        },
        json!({
            "command_id": "01C", "area_id": "ar1", "expected_version": 2,
            "lead_member_id": "a2"
        }),
    );
    pin_params::<AreaDelete>(
        &AreaDeleteParams {
            command_id: "01C".into(),
            area_id: "ar2".into(),
            expected_version: 3,
            move_open_to_area_id: "ar1".into(),
        },
        json!({
            "command_id": "01C", "area_id": "ar2", "expected_version": 3,
            "move_open_to_area_id": "ar1"
        }),
    );
    // Every one answers CommandAccepted, as T1's commands do.
    let ok = Response::ok::<AreaCreate>(
        1,
        &CommandAccepted {
            status: CommandStatus::Committed,
            position: Some(Position { epoch: 1, seq: 12 }),
        },
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(ok).unwrap()["result"]["status"],
        "committed"
    );
}

#[test]
fn hello_capabilities_are_additive_and_checked_by_name() {
    // Empty: absent on the wire, so older clients see exactly the old shape.
    let plain = HelloResult::host("0.0.0", "This Mac");
    let v = serde_json::to_value(&plain).unwrap();
    assert!(v.get("capabilities").is_none());

    // A server older than the field: it parses, with no capabilities.
    let old: HelloResult = serde_json::from_value(json!({
        "api_version": 1, "server_version": "0.0.0", "host": "This Mac"
    }))
    .unwrap();
    assert!(old.capabilities.is_empty());
    assert!(!old.supports(capability::OFFICE_RESTART));

    // Listed names round-trip; unknown names are kept and simply never asked for.
    let host = plain.with_capabilities(vec![
        capability::OFFICE_APP_TOKEN.into(),
        capability::OFFICE_RESTART.into(),
        capability::OFFICE_STATUS_CHANGED.into(),
        "some.future_call".into(),
    ]);
    let v = serde_json::to_value(&host).unwrap();
    assert_eq!(v["capabilities"][1], "office.restart");
    let back: HelloResult = serde_json::from_value(v).unwrap();
    assert_eq!(back, host);
    assert!(back.supports("office.app_token"));
    assert!(back.supports("office.status_changed"));
    assert!(!back.supports("office.restar"));

    // hello must always decode: a malformed advisory list never breaks the handshake.
    for bad in [json!(null), json!("office.restart"), json!({"a": 1})] {
        let h: HelloResult = serde_json::from_value(json!({
            "api_version": 1, "server_version": "0.0.0", "host": "This Mac", "capabilities": bad
        }))
        .unwrap();
        assert!(h.capabilities.is_empty());
    }
    let mixed: HelloResult = serde_json::from_value(json!({
        "api_version": 1, "server_version": "0.0.0", "host": "This Mac",
        "capabilities": [1, "office.restart", null, ["x"]]
    }))
    .unwrap();
    assert_eq!(mixed.capabilities, vec!["office.restart".to_owned()]);
}
