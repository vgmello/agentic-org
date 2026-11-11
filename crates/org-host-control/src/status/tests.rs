use super::*;
use org_proto::{Outcome, Request, Response};
use serde_json::{json, Value};

#[test]
fn status_vector_uses_counts_and_instance_identity_without_office_ids() {
    let vector: Value =
        serde_json::from_str(include_str!("../../test-vectors/instance-status.json")).unwrap();
    let request: Request = serde_json::from_value(vector["request"].clone()).unwrap();
    assert!(request.is::<InstanceStatus>());
    request.params::<InstanceStatus>().unwrap();
    let response: Response = serde_json::from_value(vector["response"].clone()).unwrap();
    let result = response.into_result::<InstanceStatus>().unwrap();
    assert_eq!(result.mode, InstanceMode::Service);
    assert_eq!(result.gateway.state, GatewayState::NotConfigured);
    assert_eq!(
        result.offices,
        OfficeCounts {
            total: 3,
            active: 1,
            failed: 1
        }
    );
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        vector["response"]["result"]
    );
    assert!(matches!(
        Response::ok::<InstanceStatus>(7, &result).unwrap().outcome,
        Outcome::Result(_)
    ));
}
#[test]
fn status_params_are_an_empty_object_with_only_envelope_null_normalization() {
    for value in [json!({}), Value::Null] {
        let request: Request =
            serde_json::from_value(json!({"id":1,"method":"instance.status","params":value}))
                .unwrap();
        request.params::<InstanceStatus>().unwrap();
    }
    let absent: Request =
        serde_json::from_value(json!({"id":1,"method":"instance.status"})).unwrap();
    absent.params::<InstanceStatus>().unwrap();
    for value in [
        json!([]),
        json!([1]),
        json!(""),
        json!(0),
        json!(false),
        json!({"session":"x"}),
        json!({"extra":null}),
    ] {
        let request: Request =
            serde_json::from_value(json!({"id":1,"method":"instance.status","params":value}))
                .unwrap();
        assert!(request.params::<InstanceStatus>().is_err());
    }
}
#[test]
fn unknown_response_states_are_readable_but_never_sent() {
    let mut result: Value =
        serde_json::from_str::<Value>(include_str!("../../test-vectors/instance-status.json"))
            .unwrap()["response"]["result"]
            .clone();
    result["mode"] = json!("future_mode");
    result["gateway"]["state"] = json!("future_state");
    let typed: InstanceStatusResult = serde_json::from_value(result).unwrap();
    assert_eq!(typed.mode, InstanceMode::Unknown);
    assert_eq!(typed.gateway.state, GatewayState::Unknown);
    assert!(serde_json::to_value(typed.mode).is_err());
    assert!(serde_json::to_value(typed.gateway).is_err());
}

#[test]
fn every_office_status_has_an_explicit_diagnostic_bucket() {
    use org_proto::OfficeStatus;
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../test-vectors/office-status-buckets.json"
    ))
    .unwrap();
    assert_eq!(rows.len(), 7);
    let mut active = 0;
    let mut failed = 0;
    for row in rows {
        let status: OfficeStatus = serde_json::from_value(row["status"].clone()).unwrap();
        assert_eq!(
            status == OfficeStatus::Active,
            row["active"].as_bool().unwrap()
        );
        assert_eq!(
            status.requires_operator_action(),
            row["failed"].as_bool().unwrap()
        );
        active += u64::from(status == OfficeStatus::Active);
        failed += u64::from(status.requires_operator_action());
    }
    assert_eq!((active, failed), (1, 3));
    let future: OfficeStatus = serde_json::from_value(json!("future_status")).unwrap();
    assert!(future.requires_operator_action());
}

#[test]
fn every_known_mode_and_gateway_state_has_its_frozen_spelling() {
    for (mode, wire) in [
        (InstanceMode::Bootstrap, "bootstrap"),
        (InstanceMode::Service, "service"),
    ] {
        assert_eq!(serde_json::to_value(mode).unwrap(), json!(wire));
        assert_eq!(
            serde_json::from_value::<InstanceMode>(json!(wire)).unwrap(),
            mode
        );
    }
    for (state, wire) in [
        (GatewayState::NotConfigured, "not_configured"),
        (GatewayState::Starting, "starting"),
        (GatewayState::Running, "running"),
        (GatewayState::Failed, "failed"),
    ] {
        assert_eq!(serde_json::to_value(state).unwrap(), json!(wire));
        assert_eq!(
            serde_json::from_value::<GatewayState>(json!(wire)).unwrap(),
            state
        );
    }
}
