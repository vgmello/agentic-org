use std::io::Cursor;

use serde::Serialize;
use serde_json::json;

use crate::frame::{read_line, read_message, write_message, FrameError};
use crate::*;

/// A call of this test file's own: one string in, an organization out. The wire and
/// framing rules under test do not depend on which method carries them.
struct Named;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
struct NamedParams {
    name: String,
}

impl Call for Named {
    const METHOD: &'static str = "test.named";
    type Params = NamedParams;
    type Result = Organization;
}

#[test]
fn shared_office_wire_payloads_survive_desktop_removal() {
    let shapes: serde_json::Value =
        serde_json::from_str(include_str!("../test-vectors/office-wire-shapes.json")).unwrap();
    fn roundtrip<T: serde::de::DeserializeOwned + Serialize>(value: &serde_json::Value) {
        let decoded: T = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), *value);
    }
    roundtrip::<ApiError>(&shapes["api_error"]);
    roundtrip::<OfficeInfo>(&shapes["office"]);
    roundtrip::<Organization>(&shapes["organization"]);
    roundtrip::<OrgSnapshot>(&shapes["snapshot"]);
    roundtrip::<CommandAccepted>(&shapes["command_accepted"]);
    assert_eq!(
        serde_json::to_value(ApiError::new(ErrorCode::AlreadyExists, "Taken.")).unwrap(),
        shapes["api_error"]
    );
}

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
    let req = Request::new::<Named>(
        7,
        &NamedParams {
            name: "Acme".into(),
        },
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&req).unwrap(),
        json!({"id": 7, "method": "test.named", "params": {"name": "Acme"}})
    );
    assert!(req.is::<Named>());
    assert!(!req.is::<Hello>());
    assert_eq!(req.params::<Named>().unwrap().name, "Acme");
}

#[test]
fn missing_or_null_params_read_as_empty() {
    let req: Request = serde_json::from_str(r#"{"id":1,"method":"hello"}"#).unwrap();
    assert_eq!(req.params::<Hello>().unwrap(), HelloParams::default());
    let req: Request = serde_json::from_str(r#"{"id":1,"method":"hello","params":null}"#).unwrap();
    assert_eq!(req.params::<Hello>().unwrap(), HelloParams::default());
    assert_eq!(
        serde_json::to_value(Request::new::<Hello>(1, &HelloParams::default()).unwrap()).unwrap()
            ["params"],
        json!({})
    );
}

#[test]
fn wrong_params_shape_is_an_error_not_a_panic() {
    let req: Request =
        serde_json::from_str(r#"{"id":1,"method":"test.named","params":{"name":5}}"#).unwrap();
    assert!(req.params::<Named>().is_err());
    let req: Request = serde_json::from_str(r#"{"id":1,"method":"test.named"}"#).unwrap();
    assert!(req.params::<Named>().is_err());
}

#[test]
fn unknown_method_still_parses() {
    let req: Request =
        serde_json::from_str(r#"{"id":3,"method":"org.frobnicate","params":{}}"#).unwrap();
    assert_eq!(req.method, "org.frobnicate");
}

#[test]
fn response_ok_and_error_wire_shapes() {
    let ok = Response::ok::<Named>(7, &org()).unwrap();
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
    let line = serde_json::to_string(&Response::ok::<Named>(7, &org()).unwrap()).unwrap();
    let back: Response = serde_json::from_str(&line).unwrap();
    assert_eq!(back.id, 7);
    assert_eq!(back.into_result::<Named>().unwrap(), org());

    let back: Response =
        serde_json::from_str(r#"{"id":8,"error":{"code":"invalid_name","message":"empty"}}"#)
            .unwrap();
    match back.into_result::<Named>() {
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
    let params = NamedParams {
        name: "two\nlines".into(),
    };
    write_message(&mut buf, &Request::new::<Named>(1, &params).unwrap()).unwrap();
    write_message(
        &mut buf,
        &Request::new::<Hello>(2, &HelloParams::default()).unwrap(),
    )
    .unwrap();
    assert_eq!(buf.iter().filter(|b| **b == b'\n').count(), 2);

    let mut r = Cursor::new(buf);
    let a: Request = read_message(&mut r).unwrap().unwrap();
    assert_eq!(a.params::<Named>().unwrap().name, "two\nlines");
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
        let line = format!(r#"{{"id":1,"method":"test.named","params":{params}}}"#);
        let req: Request = serde_json::from_str(&line).unwrap();
        assert!(req.params::<Named>().is_err(), "accepted {params}");
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
        ServerMessage::Response(r) => match r.into_result::<Named>() {
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
    let huge = NamedParams {
        name: "x".repeat(MAX_FRAME_BYTES),
    };
    let req = Request::new::<Named>(1, &huge).unwrap();
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
            instance_id: None,
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
            instance_id: None,
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

    // A fatal reason (until `office.restart`), with no restart pending.
    let fatal = OfficeInfo {
        status: OfficeStatus::Unavailable,
        status_reason: Some(OfficeStatusReason::RestartsExhausted),
        next_restart_at: None,
        ..hq()
    };
    let v = serde_json::to_value(&fatal).unwrap();
    assert_eq!(v["status_reason"], "restarts_exhausted");
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
    // The retired `key_missing` (an office without its key now starts) is no longer a reason.
    assert_eq!(
        serde_json::from_value::<OfficeStatusReason>("key_missing".into()).unwrap(),
        OfficeStatusReason::Unknown
    );

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
        handle: None,
        home_team_id: None,
        role_id: None,
        title_in_team: None,
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
        "role_id": "0193a5c0-0000-7000-8000-000000000040",
        "reports_to_member_id": "p1", "version": 3
    }))
    .unwrap();
    assert_eq!(agent.reports_to_member_id.as_deref(), Some("p1"));
    assert_eq!(
        agent.role_id.as_deref(),
        Some("0193a5c0-0000-7000-8000-000000000040")
    );
    assert!(v.get("role_id").is_none());
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
        projects: vec![ProjectSummary::new("p1", "Checkout")],
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
            actor: None,
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
            .into_result::<OfficeRestart>()
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
    let params = json!({"office_secrets_key": hex});
    let size = serde_json::to_string(&params).unwrap().len();

    let req: Request =
        serde_json::from_value(json!({"id": 7, "method": "office.restart", "params": params}))
            .unwrap();
    let debug = format!("{req:?}");
    assert!(!debug.contains(&hex[..8]), "{debug}");
    assert_eq!(
        debug,
        format!(r#"Request {{ id: 7, method: "office.restart", params: <elided {size} bytes> }}"#)
    );
    assert!(!format!("{req:#?}").contains(&hex[..8]));

    let resp = Response {
        id: 7,
        outcome: Outcome::Result(params.clone()),
        waiting_messages: None,
    };
    let wire_none = serde_json::to_string(&resp).unwrap();
    assert!(!wire_none.contains("waiting_messages"), "{wire_none}");
    let hinted = resp.clone().with_waiting_messages(2);
    let wire_hint = serde_json::to_string(&hinted).unwrap();
    assert!(wire_hint.contains(r#""waiting_messages":2"#), "{wire_hint}");
    let decoded: Response = serde_json::from_str(&wire_hint).unwrap();
    assert_eq!(decoded.waiting_messages, Some(2));
    let decoded_zero: Response =
        serde_json::from_str(r#"{"id":7,"result":{},"waiting_messages":0}"#).unwrap();
    assert_eq!(decoded_zero.waiting_messages, None);
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
        handle: None,
        home_team_id: None,
        role_id: None,
        title_in_team: None,
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
        member: Box::new(agent.clone()),
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
            "teams": [], "members": [member],
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
fn the_changes_of_a_member_move_that_leaves_a_led_team_have_these_wire_shapes() {
    // What the contract says HQ emits when the member led the team it leaves: the moved member
    // (updated home_team_id) and the team whose lead was cleared.
    let changed = OrgChangedData {
        position: Position { epoch: 1, seq: 9 },
        command_id: "01C".into(),
        changes: vec![
            OrgChange::MemberUpserted {
                member: Box::new(Member {
                    id: "a1".into(),
                    kind: MemberKind::Agent,
                    name: "Ada".into(),
                    handle: Some("ada".into()),
                    home_team_id: Some("t2".into()),
                    role_id: None,
                    title_in_team: None,
                    contact: None,
                    owner: false,
                    office_id: None,
                    agent: None,
                    reports_to_member_id: None,
                    created_at: None,
                    version: 2,
                }),
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
        actor: None,
    };
    assert_eq!(
        serde_json::to_value(&changed).unwrap(),
        json!({
            "position": {"epoch": 1, "seq": 9},
            "command_id": "01C",
            "changes": [
                {"type": "member_upserted", "member": {
                    "id": "a1", "kind": "agent", "name": "Ada", "handle": "ada",
                    "home_team_id": "t2", "version": 2}},
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
        handle: None,
        home_team_id: None,
        role_id: None,
        title_in_team: None,
        contact: None,
        owner: false,
        office_id: Some("o1".into()),
        agent: Some(offboarded),
        reports_to_member_id: None,
        created_at: None,
        version: 6,
    };
    let change = OrgChange::MemberUpserted {
        member: Box::new(member),
    };
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
        handle: None,
        home_team_id: None,
        role_id: None,
        title_in_team: None,
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
        "01900000-0000-7000-8000-000000000001",
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
        "01900000-0000-4000-8000-000000000001",
        "01900000-0000-7000-8000-00000000000",
        "01900000000070008000000000000001",
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
        "Refs/Heads/task/W-12",
        "HEADS/Merged/x",
        "refs/heads/Merged/x",
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
        "refs/heads/heads/task/x",
        "heads/heads/task/x",
        "refs/tags/merged/task/W-1",
        "",
        "é/task/",
    ] {
        assert!(!has_reserved_branch_prefix(fine), "{fine:?}");
    }
}

// ---- The project and area commands and the shared error codes --------------------------------

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

#[test]
fn project_and_area_wire_names_and_limits_are_pinned() {
    use crate::org::*;
    let names = [
        (ProjectCreate::METHOD, "project.create"),
        (ProjectUpdate::METHOD, "project.update"),
        (ProjectArchive::METHOD, "project.archive"),
    ];
    for (got, want) in names {
        assert_eq!(got, want);
    }
    let limits = [
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
            .into_result::<Named>()
        {
            Err(CallError::Api(e)) => assert_eq!(e.code, code),
            other => panic!("expected an api error, got {other:?}"),
        }
    }
    // A code from a later phase reads as Unknown, the way a build older than
    // `checklist_incomplete` reads that one.
    let later: Response =
        serde_json::from_str(r#"{"id":3,"error":{"code":"not_owner","message":"x"}}"#).unwrap();
    match later.into_result::<Named>() {
        Err(CallError::Api(e)) => assert_eq!(e.code, ErrorCode::Unknown),
        other => panic!("expected an api error, got {other:?}"),
    }
}

/// The phase-1 work feed is retired: `subscribe` still resumes the organization feed, but a
/// `from_work_position` is refused while the params decode, whatever its value, `null` included,
/// and the refusal never quotes the value. It is never written either.
#[test]
fn subscribe_refuses_the_retired_work_feed_position() {
    fn copy<T: Copy>() {}
    copy::<SubscribeParams>();

    let org = SubscribeParams {
        from_position: Some(Position { epoch: 1, seq: 42 }),
        ..SubscribeParams::default()
    };
    pin_params::<Subscribe>(&org, json!({"from_position": {"epoch": 1, "seq": 42}}));
    // Live events only, as before.
    pin_params::<Subscribe>(&SubscribeParams::default(), json!({}));

    for value in [
        json!({"incarnation": 7, "seq": 41}),
        json!(null),
        json!(41),
        json!("41"),
    ] {
        let request = Request {
            id: 1,
            method: Subscribe::METHOD.into(),
            principal: None,
            params: json!({"from_position": {"epoch": 1, "seq": 42}, "from_work_position": value}),
        };
        let error = request.params::<Subscribe>().unwrap_err().to_string();
        assert!(
            error.contains(crate::retired::FROM_WORK_POSITION_RETIRED),
            "{value}: {error}"
        );
        assert!(!error.contains("41"), "{value}: {error}");
    }
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
fn an_org_snapshot_reads_projects_and_areas_round_trip() {
    let old: OrgSnapshot = serde_json::from_value(json!({
        "position": {"epoch": 1, "seq": 7}, "teams": [], "members": [],
        "projects": [{"id": "p1", "name": "Checkout"}],
        "project_teams": []
    }))
    .unwrap();
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
    let tombstone_value = serde_json::to_value(&tombstone).unwrap();
    assert_eq!(tombstone_value["deleted"], true);
    assert_eq!(tombstone_value["replaced_by"], "ar1");
    assert_eq!(
        serde_json::from_value::<Area>(tombstone_value).unwrap(),
        tombstone
    );
    let v = serde_json::to_value(&old).unwrap();
    assert_eq!(serde_json::from_value::<OrgSnapshot>(v).unwrap(), old);
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
        actor: None,
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
            slug: None,
            owner_team_id: None,
            create_team_id: None,
            parent_team_id: None,
            contributing_team_ids: vec![],
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
            slug: None,
            owner_team_id: None,
            create_team_id: None,
            parent_team_id: None,
            contributing_team_ids: vec![],
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
    // Every one answers CommandAccepted, as T1's commands do.
    let ok = Response::ok::<ProjectCreate>(
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
        capability::OFFICE_RESTART.into(),
        capability::OFFICE_STATUS_CHANGED.into(),
        "some.future_call".into(),
    ]);
    let v = serde_json::to_value(&host).unwrap();
    assert_eq!(v["capabilities"][0], "office.restart");
    let back: HelloResult = serde_json::from_value(v).unwrap();
    assert_eq!(back, host);
    assert!(back.supports("office.restart"));
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
