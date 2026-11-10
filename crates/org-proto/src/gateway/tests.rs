use super::*;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

fn parse<T: DeserializeOwned>(value: &Value) -> Result<T, GatewayError> {
    // Fixture credentials are fixed public test strings. Production credentials decode
    // directly from the HTTP buffer, never through this test-only Value path.
    decode_request(&serde_json::to_vec(value).unwrap())
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

fn check_typed_response<T: DeserializeOwned + Serialize>(case: &Value) {
    let response = parse::<T>(&case["wire"]);
    assert_eq!(
        response.is_ok(),
        case["expect"]["valid"] == true,
        "{}",
        case["name"]
    );
    if let Ok(response) = response {
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
            assert_eq!(
                response.is_final(),
                case["expect"]["final"] == true,
                "{}",
                case["name"]
            );
            let wire = serde_json::to_value(response).unwrap();
            assert_eq!(wire, case["expect"]["normalized"], "{}", case["name"]);
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
        26,
    );
    for case in vectors["cases"].as_array().unwrap() {
        if case["category"] == "cancel" {
            let response: Result<ReadResponse<CancelResult>, _> = parse(&case["wire"]);
            assert_eq!(
                response.is_ok(),
                case["expect"]["valid"] == true,
                "{}",
                case["name"]
            );
            if let Ok(response) = response {
                let (status, normalized) = match response {
                    ReadResponse::Result(result) => {
                        (result.http_status(), serde_json::to_value(result).unwrap())
                    }
                    ReadResponse::Error(error) => (
                        error.http_status(true),
                        serde_json::to_value(error).unwrap(),
                    ),
                };
                assert_eq!(json!(status), case["expect"]["http_status"]);
                assert_eq!(normalized, case["expect"]["normalized"]);
            }
            continue;
        }
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
fn typed_core_commands_and_cancellation_do_not_accept_another_operation_identity() {
    assert!(decode_request::<CancelRequest>(b"{}").is_ok());
    assert!(decode_request::<CancelRequest>(br#"{"operation_id":"a new id"}"#).is_err());
    let vectors: Value =
        serde_json::from_str(include_str!("../../test-vectors/v2/gateway-envelopes.json")).unwrap();
    let request = &vectors["cases"][0]["wire"];
    let typed: OrganizationCreateRequest = parse(request).unwrap();
    assert_eq!(typed.params.name, "Acme");
    let mut invalid = request.clone();
    invalid["params"] = json!({});
    assert!(parse::<OrganizationCreateRequest>(&invalid).is_err());
}

#[test]
fn shared_envelopes_have_one_native_id_unchanged_versions_and_explicit_scope() {
    let vectors = fixture(
        include_str!("../../test-vectors/v2/gateway-envelopes.json"),
        17,
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
    let vectors = fixture(include_str!("../../test-vectors/v2/gateway-views.json"), 17);
    for case in vectors["cases"].as_array().unwrap() {
        match case["category"].as_str().unwrap() {
            "version" => check_view::<Version>(case),
            "instance" => check_view::<InstanceView>(case),
            "office" => check_view::<OfficeView>(case),
            "organization" => check_view::<OrganizationView>(case),
            "capabilities" => {
                check_view::<Capabilities>(case);
                if let Some(expected) = case["expect"].get("available") {
                    let ReadResponse::Result(caps): ReadResponse<Capabilities> =
                        parse(&case["wire"]).unwrap()
                    else {
                        panic!("expected capabilities")
                    };
                    let known: Vec<_> = OPERATIONS.iter().map(|op| op.name).collect();
                    assert_eq!(
                        json!(caps
                            .operations
                            .iter()
                            .map(|op| op.is_available(&known))
                            .collect::<Vec<_>>()),
                        *expected
                    );
                }
            }
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
    assert_eq!(vectors["clock_cases"].as_array().unwrap().len(), 2);
    for case in vectors["clock_cases"].as_array().unwrap() {
        let server = case["server_time_ms"].as_i64().unwrap();
        let local = case["browser_time_ms"].as_i64().unwrap();
        let offset = case["expected_offset_ms"].as_i64().unwrap();
        assert_eq!(local + offset, server);
        assert_eq!(
            crate::ulid_time_ms(case["corrected_operation_id"].as_str().unwrap()),
            Some(server as u64)
        );
        assert_eq!(
            crate::ulid_time_ms(case["raw_local_operation_id"].as_str().unwrap()),
            Some(local as u64)
        );
        let refusal: GatewayError = parse(&case["raw_local_refusal"]).unwrap();
        assert_eq!(
            json!(refusal.http_status(true)),
            case["expect"]["raw_http_status"]
        );
    }
}

#[test]
fn shared_idempotency_examples_keep_native_identity_and_trusted_context_distinct() {
    let vectors = fixture(
        include_str!("../../test-vectors/v2/gateway-idempotency.json"),
        4,
    );
    assert_eq!(
        vectors["native_before_move2a_reuse_code"],
        "invalid_request"
    );
    for case in vectors["cases"].as_array().unwrap() {
        let first: CommandEnvelope<Value> = parse(&case["first"]["wire"]).unwrap();
        let second: CommandEnvelope<Value> = parse(&case["second"]["wire"]).unwrap();
        let first_params = first.native_params(OperationKind::Org).unwrap();
        let second_params = second.native_params(OperationKind::Org).unwrap();
        assert_eq!(first_params["command_id"], second_params["command_id"]);
        assert_eq!(first.operation_id, second.operation_id);
        assert_eq!(
            first.params == second.params,
            case["expect"]["same_params"] == true
        );
        assert_eq!(
            case["first"]["trusted_account"] == case["second"]["trusted_account"],
            case["expect"]["same_account"] == true
        );
        let response: MutationOutcome<Value> = parse(&case["response"]).unwrap();
        assert_eq!(
            json!(response.http_status(true)),
            case["expect"]["http_status"]
        );
        assert_eq!(serde_json::to_value(response).unwrap(), case["response"]);
        // The fixture states the later worker replay contract; this crate does not implement it.
        assert_eq!(
            case["expect"]["replay"] == true,
            case["expect"]["same_params"] == true && case["expect"]["same_account"] == true
        );
    }
}

#[test]
fn shared_stream_and_poll_frames_join_the_snapshot_resume_cursor() {
    let vectors = fixture(
        include_str!("../../test-vectors/v2/gateway-stream.json"),
        15,
    );
    for case in vectors["cases"].as_array().unwrap() {
        if case["category"] == "poll" {
            check_typed_response::<PollResponse>(case);
            continue;
        }
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
        assert_eq!(scope.known(), Some(&request.subscriptions[0].scope));
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
        decode_request(br#"{"login":"alex","password":"PUBLIC FIXTURE PASSWORD"}"#).unwrap();
    assert!(!format!("{params:?}").contains("PUBLIC FIXTURE PASSWORD"));
    for invalid_proof in [
        "AB".repeat(32),
        "gg".repeat(32),
        "00".repeat(31),
        "00".repeat(33),
    ] {
        assert!(SessionProof::parse(invalid_proof).is_err());
    }
    for raw in [
        r#"{"login":"alex","password":3141592653589793}"#,
        r#"{"login":"alex","password":-3141592653589793}"#,
    ] {
        let error = decode_request::<SignInParams>(raw.as_bytes()).unwrap_err();
        assert!(!format!("{error:?}").contains("314159"));
        let raw_error = serde_json::from_str::<SignInParams>(raw).unwrap_err();
        assert!(!raw_error.to_string().contains("314159"));
    }
}

#[test]
fn diagnostics_elide_business_payloads_in_requests_outcomes_and_events() {
    let envelopes: Value =
        serde_json::from_str(include_str!("../../test-vectors/v2/gateway-envelopes.json")).unwrap();
    let mut request = envelopes["cases"][0]["wire"].clone();
    request["params"]["name"] = json!("PRIVATE BUSINESS PAYLOAD");
    let request: CommandEnvelope<Value> = parse(&request).unwrap();
    assert!(!format!("{request:?}").contains("PRIVATE BUSINESS PAYLOAD"));
    let stream: Value =
        serde_json::from_str(include_str!("../../test-vectors/v2/gateway-stream.json")).unwrap();
    let mut change = stream["cases"][1]["wire"].clone();
    change["data"] = json!({"body":"PRIVATE BUSINESS PAYLOAD"});
    let change: EventFrame = parse(&change).unwrap();
    assert!(!format!("{change:?}").contains("PRIVATE BUSINESS PAYLOAD"));
    let result: MutationOutcome<Value> = MutationOutcome::Completed {
        operation_id: request.operation_id,
        result: json!({"body":"PRIVATE BUSINESS PAYLOAD"}),
    };
    assert!(!format!("{result:?}").contains("PRIVATE BUSINESS PAYLOAD"));
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

#[test]
fn shared_path_segments_are_refused_before_decoding_or_routing() {
    let vectors = fixture(include_str!("../../test-vectors/v2/gateway-paths.json"), 17);
    for case in vectors["cases"].as_array().unwrap() {
        let raw = case["raw_segment"].as_str().unwrap();
        let result = validate_raw_path_segment(raw);
        assert_eq!(
            result.is_ok(),
            case["expect"]["valid"] == true,
            "{}",
            case["name"]
        );
        if let Err(error) = result {
            assert_eq!(json!(error.code), case["expect"]["code"]);
            assert_eq!(
                json!(error.http_status(false)),
                case["expect"]["http_status"]
            );
        }
    }
}

#[test]
fn shared_resource_refusals_hide_existence_and_keep_attempts_distinct_from_lookups() {
    let vectors = fixture(
        include_str!("../../test-vectors/v2/gateway-resource-errors.json"),
        3,
    );
    for case in vectors["cases"].as_array().unwrap() {
        let access = match case["access"].as_str().unwrap() {
            "missing" => ResourceAccess::Missing,
            "hidden" => ResourceAccess::Hidden,
            "read_only" => ResourceAccess::ReadOnly,
            other => panic!("unknown access decision {other}"),
        };
        let error = GatewayError::for_resource_access(access);
        assert_eq!(
            json!(error.http_status(true)),
            case["expect"]["http_status"]
        );
        assert_eq!(
            serde_json::to_value(ReadResponse::<Value>::Error(error)).unwrap(),
            case["wire"]
        );
    }
    assert_eq!(
        vectors["cases"][0]["wire"], vectors["cases"][1]["wire"],
        "hidden and absent must be indistinguishable"
    );
    assert_eq!(vectors["attempts"].as_array().unwrap().len(), 2);
    for trace in vectors["attempts"].as_array().unwrap() {
        let submitted: MutationOutcome<Value> = parse(&trace["submit"]).unwrap();
        assert!(
            matches!(submitted, MutationOutcome::Rejected { ref error, .. } if error.code == GatewayErrorCode::NotFound)
        );
        let looked_up: ReadResponse<OperationLookup<Value>> = parse(&trace["lookup"]).unwrap();
        let resolution = match looked_up {
            ReadResponse::Error(error) if error.code == GatewayErrorCode::NotFound => "unresolved",
            ReadResponse::Result(OperationLookup::Outcome {
                outcome: MutationOutcome::Rejected { error, .. },
                ..
            }) if error.code == GatewayErrorCode::NotFound => "rejected",
            _ => panic!("invalid shared not_found trace"),
        };
        assert_eq!(
            resolution,
            trace["expect"]["lookup_if_submit_lost"].as_str().unwrap()
        );
        // These traces specify the later ledger/client contract, not a storage implementation.
        assert_eq!(trace["expect"]["keep_draft"], true);
        assert_eq!(trace["expect"]["auto_resubmit"], false);
        assert_eq!(trace["expect"]["new_user_attempt"], "new_id");
    }
}

#[test]
fn shared_create_and_restart_projections_match_lookup_bytes_and_reject_private_fields() {
    let vectors = fixture(
        include_str!("../../test-vectors/v2/gateway-mutation-results.json"),
        44,
    );
    for case in vectors["cases"].as_array().unwrap() {
        match (
            case["operation"].as_str().unwrap(),
            case["response_kind"].as_str().unwrap(),
        ) {
            ("organization.create", "mutation") => {
                check_typed_response::<OrganizationCreateOutcome>(case)
            }
            ("organization.create", "lookup") => {
                check_typed_response::<ReadResponse<OrganizationCreateLookup>>(case)
            }
            ("office.restart", "mutation") => check_typed_response::<OfficeRestartOutcome>(case),
            ("office.restart", "lookup") => {
                check_typed_response::<ReadResponse<OfficeRestartLookup>>(case)
            }
            _ => panic!("unknown public projection case"),
        }
    }
    for pair in vectors["pairs"].as_array().unwrap() {
        let (mutation, lookup) = if pair["operation"] == "organization.create" {
            (
                serde_json::to_vec(&parse::<OrganizationCreateOutcome>(&pair["mutation"]).unwrap())
                    .unwrap(),
                serde_json::to_vec(
                    &parse::<ReadResponse<OrganizationCreateLookup>>(&pair["lookup"]).unwrap(),
                )
                .unwrap(),
            )
        } else {
            (
                serde_json::to_vec(&parse::<OfficeRestartOutcome>(&pair["mutation"]).unwrap())
                    .unwrap(),
                serde_json::to_vec(
                    &parse::<ReadResponse<OfficeRestartLookup>>(&pair["lookup"]).unwrap(),
                )
                .unwrap(),
            )
        };
        // This checks actual serialized bytes, not only Value equality (which sorts keys).
        let result = if pair["operation"] == "organization.create" {
            serde_json::to_vec(
                &serde_json::from_value::<OrganizationView>(pair["mutation"]["result"].clone())
                    .unwrap(),
            )
            .unwrap()
        } else {
            b"{}".to_vec()
        };
        let mut needle = b"\"result\":".to_vec();
        needle.extend(result);
        assert!(mutation.windows(needle.len()).any(|part| part == needle));
        assert!(lookup.windows(needle.len()).any(|part| part == needle));
    }
}

#[test]
fn shared_lookup_query_fields_are_strict_and_bound_to_the_session_instance() {
    let vectors = fixture(
        include_str!("../../test-vectors/v2/gateway-lookup-query.json"),
        18,
    );
    for case in vectors["cases"].as_array().unwrap() {
        let kind: OperationKind = serde_json::from_value(case["kind"].clone()).unwrap();
        let fields: Vec<(String, String)> = serde_json::from_value(case["fields"].clone()).unwrap();
        let query = LookupQuery::from_fields(fields);
        assert_eq!(
            query.is_ok(),
            case["expect"]["parse_valid"] == true,
            "{}: query parsing must be tested separately from instance binding",
            case["name"]
        );
        let checked = query
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|q| q.scope(kind, case["session_instance"].as_str().unwrap()));
        assert_eq!(
            checked.is_ok(),
            case["expect"]["valid"] == true,
            "{}",
            case["name"]
        );
        match checked {
            Ok(scope) => {
                assert_eq!(
                    serde_json::to_value(scope).unwrap(),
                    case["expect"]["scope"]
                );
                assert_eq!(
                    serde_json::to_value(query.unwrap()).unwrap(),
                    case["expect"]["normalized"]
                );
            }
            Err(error) => assert_eq!(
                json!(error.http_status(true)),
                case["expect"]["http_status"]
            ),
        }
    }
    let visibility = &vectors["visibility"];
    let fields: Vec<(String, String)> =
        serde_json::from_value(visibility["fields"].clone()).unwrap();
    assert!(LookupQuery::from_fields(fields)
        .unwrap()
        .scope(OperationKind::Org, "01M72N8MG00000000000000001")
        .is_ok());
    for (access, expected) in [
        (ResourceAccess::Hidden, &visibility["hidden"]),
        (ResourceAccess::Missing, &visibility["missing"]),
    ] {
        let error = GatewayError::for_resource_access(access);
        assert_eq!(json!(error.http_status(true)), visibility["http_status"]);
        assert_eq!(
            serde_json::to_value(ReadResponse::<serde_json::Value>::Error(error)).unwrap(),
            *expected
        );
    }
    assert_eq!(visibility["hidden"], visibility["missing"]);
}
