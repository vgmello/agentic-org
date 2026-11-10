use super::*;
use serde_json::{json, Value};
use std::collections::BTreeSet;

fn fixture(text: &str, count: usize) -> Value {
    let value: Value = serde_json::from_str(text).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["contract_revision"], 2);
    assert_eq!(value["cases"].as_array().unwrap().len(), count);
    value
}

fn operation(case: &Value) -> OrgOperation {
    serde_json::from_value(case["operation"].clone()).unwrap()
}

fn assert_native(command: &OrgMutation, expected: &Value) {
    let request = serde_json::to_value(command.native_request(42).unwrap()).unwrap();
    assert_eq!(request["id"], 42);
    assert_eq!(request["method"], expected["method"]);
    assert_eq!(request["params"], expected["params"]);
    assert_eq!(
        command.native_method(),
        expected["method"].as_str().unwrap()
    );
    assert_eq!(
        request["params"]["command_id"],
        command.operation_id().as_str()
    );
    assert!(request["params"].get("operation_id").is_none());
}

#[test]
fn all_fifteen_operations_map_to_their_typed_native_call_with_every_optional_field() {
    let vectors = fixture(
        include_str!("../../../test-vectors/v2/gateway/org-mutation-mappings.json"),
        16,
    );
    let scope: Scope = serde_json::from_value(vectors["route_scope"].clone()).unwrap();
    let mut seen = BTreeSet::new();
    for case in vectors["cases"].as_array().unwrap() {
        assert_eq!(case["reader"], "Rust+TS");
        let op = operation(case);
        seen.insert(op.name());
        let command = op
            .decode(&serde_json::to_vec(&case["envelope"]).unwrap(), &scope)
            .unwrap_or_else(|error| panic!("{}: {error:?}", case["name"]));
        assert_eq!(command.operation(), op);
        assert_eq!(command.scope(), &scope);
        assert_eq!(
            command.operation_id().as_str(),
            case["envelope"]["operation_id"]
        );
        assert_native(&command, &case["native"]);
        // Path is a TS URL-builder expectation, not a claim of Rust HTTP-router coverage.
        assert!(case["path"]
            .as_str()
            .unwrap()
            .starts_with("/api/v1/organizations/"));
    }
    assert_eq!(OrgOperation::ALL.len(), 15);
    assert_eq!(seen, OrgOperation::ALL.iter().map(|op| op.name()).collect());
    for excluded in [
        "member.offboard",
        "member.add_agent",
        "project.create",
        "team.rename ",
    ] {
        assert!(serde_json::from_value::<OrgOperation>(json!(excluded)).is_err());
    }
}

#[test]
fn shared_optional_fields_and_strict_decode_matrix() {
    let vectors = fixture(
        include_str!("../../../test-vectors/v2/gateway/org-mutation-inputs.json"),
        233,
    );
    let scope: Scope = serde_json::from_value(vectors["route_scope"].clone()).unwrap();
    let cases = vectors["cases"].as_array().unwrap();
    for case in cases {
        assert_eq!(case["reader"], "Rust");
        let result =
            operation(case).decode(&serde_json::to_vec(&case["envelope"]).unwrap(), &scope);
        assert_eq!(
            result.is_ok(),
            case["expect"]["valid"] == true,
            "{}",
            case["name"]
        );
        match result {
            Ok(command) => {
                assert_native(&command, &case["expect"]["native"]);
                if let Some(expected) = case.get("typed_clear_contact") {
                    let browser: MemberUpdatePersonParams =
                        serde_json::from_value(case["envelope"]["params"].clone()).unwrap();
                    let native: org::MemberUpdatePersonParams =
                        serde_json::from_value(command.native_request(42).unwrap().params).unwrap();
                    assert_eq!(browser.clear_contact, expected.as_bool().unwrap());
                    assert_eq!(native.clear_contact, expected.as_bool().unwrap());
                }
            }
            Err(error) => {
                assert_eq!(
                    json!(error.http_status(true)),
                    case["expect"]["http_status"]
                );
                assert_eq!(
                    serde_json::to_value(error).unwrap(),
                    case["expect"]["error"]
                );
            }
        }
    }
    assert_eq!(
        cases
            .iter()
            .filter(|case| case["group"] == "optional")
            .count(),
        22
    );
    assert_eq!(
        cases
            .iter()
            .filter(|case| case["group"] == "invalid_version")
            .count(),
        55
    );
    for op in OrgOperation::ALL {
        let selected = |group: &str| {
            cases
                .iter()
                .filter(|case| case["operation"] == op.name() && case["group"] == group)
                .count()
        };
        assert_eq!(selected("strict_params"), 8, "{}", op.name());
        assert_eq!(selected("strict_envelope"), 1, "{}", op.name());
        assert!(
            matches!(selected("invalid_version"), 0 | 5),
            "{}",
            op.name()
        );
    }
}

fn issue_name(issue: Option<OrgReplyIssue>) -> Option<&'static str> {
    issue.map(|issue| match issue {
        OrgReplyIssue::NoAnswer => "no_answer",
        OrgReplyIssue::Undecodable => "undecodable",
        OrgReplyIssue::CommittedWithoutPosition => "committed_without_position",
        OrgReplyIssue::Queued => "queued",
        OrgReplyIssue::UnknownStatus => "unknown_status",
        OrgReplyIssue::UnknownErrorCode => "unknown_error_code",
        OrgReplyIssue::UnexpectedRefusal => "unexpected_refusal",
    })
}

#[test]
fn shared_native_replies_preserve_commit_uncertainty_and_authority_boundaries() {
    let vectors = fixture(
        include_str!("../../../test-vectors/v2/gateway/org-mutation-replies.json"),
        27,
    );
    for case in vectors["cases"].as_array().unwrap() {
        assert_eq!(case["native_reader"], "Rust");
        let operation_id =
            OperationId::parse(case["operation_id"].as_str().unwrap().to_owned()).unwrap();
        let reply = (case["native_present"] == true).then_some(&case["native_reply"]);
        match classify_org_reply(operation_id, reply) {
            OrgReplyClassification::Outcome { outcome, issue } => {
                assert_eq!(case["reader"], "Rust+TS");
                assert_eq!(case["expect"]["classification"], "outcome");
                assert!(!matches!(outcome, MutationOutcome::Accepted { .. }));
                assert_eq!(
                    json!(outcome.http_status(true)),
                    case["expect"]["http_status"],
                    "{}",
                    case["name"]
                );
                assert_eq!(outcome.is_final(), case["expect"]["final"] == true);
                assert_eq!(
                    json!(issue_name(issue)),
                    case["expect"]["issue"],
                    "{}",
                    case["name"]
                );
                let wire = serde_json::to_value(outcome).unwrap();
                assert_eq!(wire, case["expect"]["wire"], "{}", case["name"]);
                assert!(!wire.to_string().contains("PRIVATE_NATIVE_MARKER"));
                // The same browser JSON must also pass the core inbound reader.
                let decoded: MutationOutcome<Empty> = serde_json::from_value(wire).unwrap();
                assert_eq!(
                    json!(decoded.http_status(true)),
                    case["expect"]["http_status"]
                );
            }
            OrgReplyClassification::NeedsAuthorizationRecheck => {
                assert_eq!(case["reader"], "Rust");
                assert_eq!(
                    case["expect"]["classification"],
                    "needs_authorization_recheck"
                );
                assert!(case["expect"].get("wire").is_none());
            }
        }
    }
}

#[test]
fn validated_mutation_debug_elides_business_values() {
    let scope = Scope::Organization {
        instance_id: "INSTANCE".into(),
        org_id: "ORG".into(),
    };
    let body = json!({
        "operation_id":"01M72N8MG00000000000000006",
        "scope":scope,
        "params":{"member_id":"member", "name":"PRIVATE_PERSON", "contact":"PRIVATE_CONTACT"},
    });
    let command = OrgOperation::MemberAddPerson
        .decode(&serde_json::to_vec(&body).unwrap(), &scope)
        .unwrap();
    let debug = format!("{command:?}");
    assert!(!debug.contains("PRIVATE_PERSON"));
    assert!(!debug.contains("PRIVATE_CONTACT"));
}
