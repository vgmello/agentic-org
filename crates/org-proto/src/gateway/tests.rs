use super::*;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

fn parse<T: DeserializeOwned>(value: &Value) -> Result<T, GatewayError> {
    // Fixture credentials are fixed public test strings. Production credentials decode
    // directly from the HTTP buffer, never through this test-only Value path.
    decode(&serde_json::to_vec(value).unwrap())
}

fn fixture(text: &str, expected_cases: usize) -> Value {
    let value: Value = serde_json::from_str(text).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["cases"].as_array().unwrap().len(), expected_cases);
    value
}

fn check_view<T: DeserializeOwned + Serialize>(case: &Value) {
    let result: Result<ReadResponse<T>, _> = parse(&case["wire"]);
    assert_eq!(
        result.is_ok(),
        case["expect"]["valid"] == true,
        "{}: {result:?}",
        case["name"]
    );
    if let Ok(response) = result {
        assert_eq!(
            serde_json::to_value(response).unwrap(),
            case["expect"]["normalized"],
            "{}",
            case["name"]
        );
    }
}

#[test]
fn public_version_has_only_constant_protocol_metadata() {
    assert_eq!(
        serde_json::to_value(Version::default()).unwrap(),
        json!({"api_version":1,"contract_revision":1})
    );
}

#[test]
fn shared_mutation_outcomes_preserve_identity_and_http_meaning() {
    let vectors = fixture(
        include_str!("../../test-vectors/v2/gateway-outcomes.json"),
        35,
    );
    for case in vectors["cases"].as_array().unwrap() {
        let result: Result<MutationOutcome<Value>, _> = parse(&case["wire"]);
        assert_eq!(
            result.is_ok(),
            case["expect"]["valid"] == true,
            "{}: {result:?}",
            case["name"]
        );
        if let Ok(response) = result {
            if let Some(native) = case.get("native_error") {
                let native: crate::ApiError = serde_json::from_value(native.clone()).unwrap();
                assert_eq!(
                    serde_json::to_value(GatewayError::from(native)).unwrap(),
                    case["wire"]["error"]
                );
            }
            assert_eq!(
                json!(response.http_status(case["authenticated"] == true)),
                case["expect"]["http_status"],
                "{}",
                case["name"]
            );
            let wire = serde_json::to_value(response).unwrap();
            assert_eq!(
                wire["outcome"], case["expect"]["variant"],
                "{}",
                case["name"]
            );
            assert_eq!(
                wire["operation_id"], case["wire"]["operation_id"],
                "an error must not replace the ID"
            );
        }
    }
}

#[test]
fn shared_lookups_distinguish_final_abandonment_from_unresolved_and_expired() {
    let vectors = fixture(
        include_str!("../../test-vectors/v2/gateway-lookups.json"),
        17,
    );
    for case in vectors["cases"].as_array().unwrap() {
        let response: Result<ReadResponse<OperationLookup<Value>>, _> = parse(&case["wire"]);
        assert_eq!(
            response.is_ok(),
            case["expect"]["valid"] == true,
            "{}: {response:?}",
            case["name"]
        );
        match response {
            Ok(ReadResponse::Result(record)) => {
                assert_eq!(
                    record.is_final(),
                    case["expect"]["final"] == true,
                    "{}",
                    case["name"]
                );
                assert_eq!(
                    serde_json::to_value(record).unwrap(),
                    case["expect"]["normalized"],
                    "{}",
                    case["name"]
                );
            }
            Ok(ReadResponse::Error(error)) => {
                assert_eq!(
                    json!(error.http_status(true)),
                    case["expect"]["http_status"]
                );
                let resolution = match error.code {
                    GatewayErrorCode::NotFound => "unresolved",
                    GatewayErrorCode::Expired => "expired",
                    other => panic!("unexpected lookup error {other:?}"),
                };
                assert_eq!(resolution, case["expect"]["resolution"].as_str().unwrap());
            }
            Err(_) => {}
        }
    }
}

#[test]
fn shared_envelopes_have_one_native_id_unchanged_versions_and_explicit_scope() {
    let vectors = fixture(
        include_str!("../../test-vectors/v2/gateway-envelopes.json"),
        15,
    );
    for case in vectors["cases"].as_array().unwrap() {
        let result: Result<CommandEnvelope<Value>, _> = parse(&case["wire"]);
        assert_eq!(
            result.is_ok(),
            case["expect"]["valid"] == true,
            "{}: {result:?}",
            case["name"]
        );
        if let Ok(envelope) = result {
            let target: Scope = parse(&case["route_scope"]).unwrap();
            assert_eq!(
                envelope.scope.check_target(&target).is_ok(),
                case["expect"]["target_matches"] == true,
                "{}",
                case["name"]
            );
            if case["expect"].get("native_params").is_some() {
                let kind: OperationKind = serde_json::from_value(case["kind"].clone()).unwrap();
                assert_eq!(
                    envelope.native_params(kind).unwrap(),
                    case["expect"]["native_params"],
                    "{}",
                    case["name"]
                );
            }
        }
    }
}

#[test]
fn shared_views_are_typed_additive_and_do_not_expose_private_endpoints() {
    let vectors = fixture(include_str!("../../test-vectors/v2/gateway-views.json"), 16);
    for case in vectors["cases"].as_array().unwrap() {
        match case["category"].as_str().unwrap() {
            "version" => check_view::<Version>(case),
            "instance" => check_view::<InstanceView>(case),
            "office" => check_view::<OfficeView>(case),
            "organization" => check_view::<OrganizationView>(case),
            "capabilities" => check_view::<Capabilities>(case),
            "snapshot" => check_view::<SnapshotResponse>(case),
            other => panic!("unknown shared view category {other}"),
        }
    }
    let office_case = &vectors["cases"][2]["wire"]["result"];
    let mut internal = office_case.clone();
    internal["endpoint"] = json!("/private/test-only/office.sock");
    let internal: crate::OfficeInfo = serde_json::from_value(internal).unwrap();
    assert_eq!(
        serde_json::to_value(OfficeView::from(internal)).unwrap(),
        *office_case
    );

    let caps: ReadResponse<Capabilities> = parse(&vectors["cases"][4]["wire"]).unwrap();
    let ReadResponse::Result(caps) = caps else {
        panic!("expected capabilities")
    };
    let known: Vec<_> = OPERATIONS.iter().map(|op| op.name).collect();
    assert!(caps.operations[0].is_available(&known));
    assert!(!caps.operations[1].is_available(&known));
    assert!(
        !caps.operations[2].is_available(&known),
        "an unknown advertised name is unavailable"
    );
}

#[test]
fn shared_session_shapes_keep_proof_out_of_reads_and_preserve_server_clock() {
    let vectors = fixture(
        include_str!("../../test-vectors/v2/gateway-session.json"),
        8,
    );
    assert_eq!(vectors["proof_header"], SESSION_PROOF_HEADER);
    for case in vectors["cases"].as_array().unwrap() {
        let valid = match case["category"].as_str().unwrap() {
            "sign_in" => parse::<SignInParams>(&case["wire"]).is_ok(),
            "established" => parse::<ReadResponse<SessionEstablished>>(&case["wire"]).is_ok(),
            "session" => parse::<ReadResponse<SessionView>>(&case["wire"]).is_ok(),
            other => panic!("unknown shared session category {other}"),
        };
        assert_eq!(valid, case["expect"]["valid"] == true, "{}", case["name"]);
    }
    let authenticated: ReadResponse<SessionEstablished> =
        parse(&vectors["cases"][1]["wire"]).unwrap();
    let ReadResponse::Result(authenticated) = authenticated else {
        panic!("expected sign-in")
    };
    let view = serde_json::to_value(authenticated.view()).unwrap();
    assert_eq!(view, vectors["cases"][2]["wire"]["result"]);
    for forbidden in ["proof", "session", "grant", "channel_id"] {
        assert!(view.get(forbidden).is_none());
    }
    let clock = &vectors["clock"];
    let server = authenticated.server_time_ms.get();
    assert_eq!(server, clock["server_time_ms"].as_u64().unwrap());
    let browser = clock["browser_time_ms"].as_u64().unwrap();
    assert_eq!(
        server - browser,
        clock["expected_offset_ms"].as_u64().unwrap()
    );
    assert_eq!(
        crate::ulid_time_ms(clock["operation_id"].as_str().unwrap()),
        Some(server)
    );
}

#[test]
fn shared_stream_and_poll_frames_join_the_snapshot_resume_cursor() {
    let vectors = fixture(
        include_str!("../../test-vectors/v2/gateway-stream.json"),
        10,
    );
    for case in vectors["cases"].as_array().unwrap() {
        let response: Result<EventFrame, _> = parse(&case["wire"]);
        assert_eq!(
            response.is_ok(),
            case["expect"]["valid"] == true,
            "{}: {response:?}",
            case["name"]
        );
        if let Ok(frame) = response {
            assert_eq!(
                serde_json::to_value(frame).unwrap(),
                case["expect"]["normalized"],
                "{}",
                case["name"]
            );
        }
    }
    let poll: PollResponse = parse(&vectors["poll"]).unwrap();
    assert_eq!(serde_json::to_value(poll).unwrap(), vectors["poll"]);
    let trace = &vectors["trace"];
    let snapshot: SnapshotResponse = parse(&trace["snapshot"]).unwrap();
    let request: StreamRequest = parse(&trace["subscribe"]).unwrap();
    assert_eq!(
        request.subscriptions[0].cursor.as_ref(),
        Some(&snapshot.resume_cursor)
    );
    let mut previous = snapshot.snapshot.position.seq;
    let mut cursor = snapshot.resume_cursor;
    let mut positions = Vec::new();
    for wire in trace["updates"].as_array().unwrap() {
        let EventFrame::Change {
            scope,
            feed,
            cursor: next,
            data,
            ..
        } = parse(wire).unwrap()
        else {
            panic!("expected change")
        };
        assert_eq!(scope, request.subscriptions[0].scope);
        assert_eq!(feed, Feed::Organization);
        assert_eq!(data["position"]["epoch"], snapshot.snapshot.position.epoch);
        let seq = data["position"]["seq"].as_u64().unwrap();
        assert_eq!(
            seq,
            previous + 1,
            "shared trace must have no gap or duplicate"
        );
        previous = seq;
        cursor = next;
        positions.push(seq);
    }
    assert_eq!(json!(positions), trace["expect"]["positions"]);
    assert_eq!(
        cursor.as_str(),
        trace["expect"]["last_cursor"].as_str().unwrap()
    );
    let reconnect: StreamRequest = parse(&trace["reconnect"]).unwrap();
    assert_eq!(reconnect.subscriptions[0].cursor.as_ref(), Some(&cursor));
    assert!(matches!(
        parse::<EventFrame>(&trace["resync"]).unwrap(),
        EventFrame::Resync {
            reason: ResyncReason::SnapshotRequired,
            ..
        }
    ));
}

#[test]
fn credential_diagnostics_never_echo_mistyped_secrets() {
    let proof = "0123456789abcdef".repeat(4);
    let proof = SessionProof::parse(proof.clone()).unwrap();
    assert_eq!(format!("{proof:?}"), "SessionProof(<redacted>)");
    let params: SignInParams =
        decode(br#"{"login":"alex","password":"PUBLIC FIXTURE PASSWORD"}"#).unwrap();
    assert!(!format!("{params:?}").contains("PUBLIC FIXTURE PASSWORD"));
    for invalid_proof in ["AB".repeat(32), "gg".repeat(32), "00".repeat(31), "00".repeat(33)] {
        assert!(SessionProof::parse(invalid_proof).is_err());
    }
    for raw in [
        r#"{"login":"alex","password":3141592653589793}"#,
        r#"{"login":"alex","password":-3141592653589793}"#,
    ] {
        let error = decode::<SignInParams>(raw.as_bytes()).unwrap_err();
        assert!(!format!("{error:?}").contains("314159"));
    }
}

#[test]
fn version_negotiation_and_clock_values_are_shared_browser_semantics() {
    let required = Version::default();
    assert!(required.check_required(required).is_ok());
    assert!(Version {
        contract_revision: 2,
        ..required
    }
    .check_required(required)
    .is_ok());
    for old_or_other in [
        Version {
            contract_revision: 0,
            ..required
        },
        Version {
            api_version: 2,
            ..required
        },
    ] {
        assert_eq!(
            old_or_other
                .check_required(required)
                .unwrap_err()
                .http_status(true),
            Some(426)
        );
    }
    assert_eq!(
        serde_json::from_str::<Milliseconds>("1.0").unwrap().get(),
        1
    );
    assert_eq!(
        Milliseconds::new(MAX_SAFE_INTEGER).unwrap().get(),
        MAX_SAFE_INTEGER
    );
    for raw in ["1.5", "-1", "9007199254740992", "\"1\""] {
        assert!(serde_json::from_str::<Milliseconds>(raw).is_err(), "{raw}");
    }
}
