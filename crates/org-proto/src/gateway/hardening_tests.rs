use super::*;
use serde_json::Value;

#[test]
fn shared_outbound_lookup_encoding_guards() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../test-vectors/v2/gateway/encoding-guards.json"
    ))
    .unwrap();
    assert_eq!(fixture["schema_version"], 1);
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 1);
    for case in cases {
        assert_eq!(case["reader"], "Rust");
        let record = OperationLookup::<Value>::Outcome {
            kind: serde_json::from_value(case["kind"].clone()).unwrap(),
            state: None,
            outcome: MutationOutcome::Unknown {
                operation_id: OperationId::parse(case["operation_id"].as_str().unwrap().to_owned())
                    .unwrap(),
            },
        };
        assert_eq!(
            serde_json::to_value(record).is_ok(),
            case["expect"]["encode_valid"] == true
        );
    }
}

#[test]
fn create_in_progress_helper_matches_the_shared_conflict_wire_shape() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../test-vectors/v2/gateway-lookups.json")).unwrap();
    let case = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == "dedicated cancel in_progress")
        .unwrap();
    let operation_id = OperationId::parse(
        case["wire"]["error"]["operation_id"]
            .as_str()
            .unwrap()
            .to_owned(),
    )
    .unwrap();
    let error = GatewayError::create_in_progress(operation_id);
    assert_eq!(error.http_status(true), Some(409));
    let response = ReadResponse::<Value>::Error(error);
    assert_eq!(serde_json::to_value(response).unwrap(), case["wire"]);
}
