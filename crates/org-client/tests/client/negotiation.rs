//! Opt-in X-02 negotiation, with independent framed Unix peers.
use super::*;
use org_proto::ApiRange;

const INSTANCE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const OTHER: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
const TIMEOUT: Duration = Duration::from_secs(10);
const RANGE_ERROR: &str = "the server chose an API version outside the offered range";
const ID_ERROR: &str = "the server's instance id is missing or malformed";
const V1_ERROR: &str = "the instance cannot be verified with API version 1";

struct Probe;
impl Call for Probe {
    const METHOD: &'static str = "probe";
    type Params = Empty;
    type Result = Empty;
}

fn bounded(peer: &mut Peer) {
    peer.reader
        .get_mut()
        .set_read_timeout(Some(TIMEOUT))
        .unwrap();
}

fn result(version: u32, instance: Value) -> Value {
    json!({"api_version":version,"server_version":"fake","host":"fake","instance_id":instance})
}

fn assert_protocol(error: ClientError, expected: &str) {
    match error {
        ClientError::Protocol(message) => assert_eq!(message, expected),
        other => panic!("expected fixed protocol error, got {other:?}"),
    }
}

fn after_failure(client: &Client) {
    assert!(matches!(
        client.call_with_timeout::<Probe>(&Empty {}, TIMEOUT),
        Err(ClientError::Disconnected)
    ));
}

/// The peer stays open until the client closes. A second request is always a failure.
fn result_failure(
    offered: ApiRange,
    expected: &'static str,
    answer: Value,
    check: impl FnOnce(ClientError),
) {
    let server = fake(move |mut peer| {
        bounded(&mut peer);
        let request = peer.next_request().unwrap();
        peer.raw(json!({"id":request.id,"result":answer}));
        assert!(
            peer.next_request().is_none(),
            "failed handshake sent another request"
        );
    });
    let client = server.client();
    let error = client
        .hello_offering(offered, expected, TIMEOUT)
        .unwrap_err();
    check(error);
    after_failure(&client);
    drop(client);
    server.finish();
}

#[test]
fn negotiated_hello_sends_the_offer_and_leaves_a_valid_connection_usable() {
    let server = fake(|mut peer| {
        bounded(&mut peer);
        let request = peer.next_request().unwrap();
        assert_eq!(request.method, "hello");
        assert_eq!(request.params, json!({"api":{"min":1,"max":2}}));
        peer.raw(json!({"id":request.id,"result":result(2,json!(INSTANCE))}));
        let next = peer.next_request().unwrap();
        assert_eq!(next.method, Probe::METHOD);
        peer.reply::<Probe>(next.id, &Empty {});
    });
    let client = server.client();
    let hello = client
        .hello_offering(ApiRange { min: 1, max: 2 }, INSTANCE, TIMEOUT)
        .unwrap();
    assert_eq!(hello.api_version, 2);
    assert_eq!(hello.instance_id.as_deref(), Some(INSTANCE));
    client.call::<Probe>(&Empty {}).unwrap();
    drop(client);
    server.finish();
}

#[test]
fn arbitrary_well_formed_offers_accept_both_boundaries_without_a_global_clamp() {
    for (offered, chosen) in [
        (ApiRange { min: 2, max: 3 }, 2),
        (ApiRange { min: 2, max: 3 }, 3),
        (
            ApiRange {
                min: u32::MAX,
                max: u32::MAX,
            },
            u32::MAX,
        ),
    ] {
        let server = fake(move |mut peer| {
            bounded(&mut peer);
            let request = peer.next_request().unwrap();
            assert_eq!(
                request.params["api"],
                json!({"min":offered.min,"max":offered.max})
            );
            peer.raw(json!({"id":request.id,"result":result(chosen,json!(INSTANCE))}));
            assert!(peer.next_request().is_none());
        });
        let client = server.client();
        assert_eq!(
            client
                .hello_offering(offered, INSTANCE, TIMEOUT)
                .unwrap()
                .api_version,
            chosen
        );
        drop(client);
        server.finish();
    }
}

#[test]
fn chosen_version_below_the_offer_is_refused_with_other_checks_passing() {
    result_failure(
        ApiRange { min: 3, max: 4 },
        INSTANCE,
        result(2, json!(INSTANCE)),
        |e| assert_protocol(e, RANGE_ERROR),
    );
}

#[test]
fn chosen_version_above_the_offer_is_refused_with_other_checks_passing() {
    result_failure(
        ApiRange { min: 2, max: 2 },
        INSTANCE,
        result(3, json!(INSTANCE)),
        |e| assert_protocol(e, RANGE_ERROR),
    );
}

#[test]
fn version_one_is_unverifiable_even_with_a_matching_optional_id() {
    result_failure(
        ApiRange { min: 1, max: 2 },
        INSTANCE,
        result(1, json!(INSTANCE)),
        |e| assert_protocol(e, V1_ERROR),
    );
}

#[test]
fn version_two_requires_a_present_canonical_instance_id() {
    for instance in [
        Value::Null,
        json!(""),
        json!(INSTANCE.to_lowercase()),
        json!("81ARZ3NDEKTSV4RRFFQ69G5FAV"),
        json!("01ARZ3NDEKTSV4RRFFQ69G5FAU"),
        json!("SECRET-PEER-INSTANCE"),
    ] {
        result_failure(
            ApiRange { min: 2, max: 2 },
            INSTANCE,
            result(2, instance),
            |e| {
                let formatted = format!("{e:?} {e}");
                assert!(!formatted.contains("SECRET-PEER-INSTANCE"));
                assert_protocol(e, ID_ERROR);
            },
        );
    }
    let mut missing = result(2, json!(INSTANCE));
    missing.as_object_mut().unwrap().remove("instance_id");
    result_failure(ApiRange { min: 2, max: 2 }, INSTANCE, missing, |e| {
        assert_protocol(e, ID_ERROR)
    });
}

#[test]
fn a_valid_different_instance_has_its_own_error_and_closes() {
    result_failure(
        ApiRange { min: 2, max: 2 },
        INSTANCE,
        result(2, json!(OTHER)),
        |e| {
            assert!(matches!(e, ClientError::InstanceMismatch));
            assert_eq!(e.to_string(), "the server belongs to another instance");
            assert!(std::error::Error::source(&e).is_none());
        },
    );
}

#[test]
fn typed_result_decode_precedes_range_and_range_precedes_identity() {
    result_failure(
        ApiRange { min: 2, max: 2 },
        INSTANCE,
        result(3, json!(7)),
        |e| assert_protocol(e, "a result doesn't fit the contract"),
    );
    result_failure(
        ApiRange { min: 3, max: 4 },
        INSTANCE,
        result(2, json!(OTHER)),
        |e| assert_protocol(e, RANGE_ERROR),
    );
}

#[test]
fn malformed_typed_results_are_fixed_protocol_errors_and_close() {
    // The envelope is valid, so the generic reader stays open until hello_offering closes it.
    for answer in [
        json!(["SECRET-RESULT"]),
        json!({"api_version":"SECRET-RESULT"}),
        result(2, json!("SECRET-RESULT")),
    ] {
        result_failure(ApiRange { min: 2, max: 2 }, INSTANCE, answer, |e| {
            assert!(!format!("{e:?} {e}").contains("SECRET-RESULT"));
            assert!(matches!(e, ClientError::Protocol(_)));
        });
    }
}

#[test]
fn a_well_formed_server_refusal_keeps_its_code_and_message_but_closes() {
    let refusal = ApiError::new(
        ErrorCode::UnsupportedVersion,
        "the server refused this range",
    );
    let sent = refusal.clone();
    let server = fake(move |mut peer| {
        bounded(&mut peer);
        let request = peer.next_request().unwrap();
        peer.raw(json!({"id":request.id,"error":sent}));
        assert!(
            peer.next_request().is_none(),
            "API refusal left the client writable"
        );
    });
    let client = server.client();
    match client
        .hello_offering(ApiRange { min: 2, max: 2 }, INSTANCE, TIMEOUT)
        .unwrap_err()
    {
        ClientError::Api(actual) => assert_eq!(actual, refusal),
        other => panic!("API refusal was changed: {other:?}"),
    }
    after_failure(&client);
    drop(client);
    server.finish();
}

#[test]
fn a_transport_timeout_keeps_its_variant_and_closes_even_before_a_write() {
    let server = fake(|mut peer| {
        bounded(&mut peer);
        // A zero-deadline request may be abandoned before writing. Neither outcome may
        // be followed by another frame, and the peer deliberately never answers hello.
        if let Some(request) = peer.next_request() {
            assert_eq!(request.method, "hello");
            assert!(peer.next_request().is_none());
        }
    });
    let client = server.client();
    assert!(matches!(
        client.hello_offering(ApiRange { min: 2, max: 2 }, INSTANCE, Duration::ZERO),
        Err(ClientError::Timeout)
    ));
    after_failure(&client);
    drop(client);
    server.finish();
}

#[test]
fn a_transport_disconnect_keeps_its_variant() {
    let server = fake(|peer| {
        peer.stream.shutdown(std::net::Shutdown::Both).unwrap();
    });
    let client = server.client();
    assert!(matches!(
        client.hello_offering(ApiRange { min: 2, max: 2 }, INSTANCE, TIMEOUT),
        Err(ClientError::Disconnected)
    ));
    after_failure(&client);
    drop(client);
    server.finish();
}

#[test]
fn local_argument_errors_send_nothing_and_allow_a_valid_retry() {
    let server = fake(|mut peer| {
        bounded(&mut peer);
        let request = peer.next_request().unwrap();
        assert_eq!(
            request.params,
            json!({"api":{"min":2,"max":2}}),
            "an invalid argument reached the wire"
        );
        peer.raw(json!({"id":request.id,"result":result(2,json!(INSTANCE))}));
        assert!(peer.next_request().is_none());
    });
    let client = server.client();
    for range in [ApiRange { min: 0, max: 2 }, ApiRange { min: 3, max: 2 }] {
        assert_protocol(
            client.hello_offering(range, INSTANCE, TIMEOUT).unwrap_err(),
            "the offered API range is invalid",
        );
    }
    for invalid in [
        "",
        "bad",
        "01arz3ndektsv4rrffq69g5fav",
        "81ARZ3NDEKTSV4RRFFQ69G5FAV",
        "01ARZ3NDEKTSV4RRFFQ69G5FAU",
    ] {
        assert_protocol(
            client
                .hello_offering(ApiRange { min: 2, max: 2 }, invalid, TIMEOUT)
                .unwrap_err(),
            "the expected instance id is invalid",
        );
    }
    assert_eq!(
        client
            .hello_offering(ApiRange { min: 2, max: 2 }, INSTANCE, TIMEOUT)
            .unwrap()
            .api_version,
        2
    );
    drop(client);
    server.finish();
}

#[test]
fn repeated_hello_uses_the_new_offer_and_the_new_expected_instance() {
    for change_identity in [false, true] {
        let server = fake(|mut peer| {
            bounded(&mut peer);
            for _ in 0..2 {
                let request = peer.next_request().unwrap();
                peer.raw(json!({"id":request.id,"result":result(2,json!(INSTANCE))}));
            }
            assert!(peer.next_request().is_none());
        });
        let client = server.client();
        client
            .hello_offering(ApiRange { min: 2, max: 2 }, INSTANCE, TIMEOUT)
            .unwrap();
        let error = if change_identity {
            client.hello_offering(ApiRange { min: 2, max: 2 }, OTHER, TIMEOUT)
        } else {
            client.hello_offering(ApiRange { min: 3, max: 4 }, INSTANCE, TIMEOUT)
        }
        .unwrap_err();
        if change_identity {
            assert!(matches!(error, ClientError::InstanceMismatch));
        } else {
            assert_protocol(error, RANGE_ERROR);
        }
        after_failure(&client);
        drop(client);
        server.finish();
    }
}
