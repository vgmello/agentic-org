//! The port's operator-session rule (`validate_operator_shell`, `TASK-80.1`): the fixture
//! operator spec passes, every malformed variant is refused, and an ordinary spec is no
//! operator at all.

use org_agent_runtime::testkit::{malformed_operator_specs, operator_spec};
use org_agent_runtime::{validate_operator_shell, PlanError, SessionSpec};

#[test]
fn the_fixture_operator_spec_is_one() {
    assert_eq!(validate_operator_shell(&operator_spec("codex")), Ok(true));
}

#[test]
fn an_ordinary_read_only_spec_is_no_operator() {
    let spec = SessionSpec {
        operator_shell_prefixes: Vec::new(),
        ..operator_spec("codex")
    };
    assert_eq!(validate_operator_shell(&spec), Ok(false));
}

#[test]
fn every_malformed_operator_spec_is_refused() {
    for (what, spec) in malformed_operator_specs("claude") {
        match validate_operator_shell(&spec) {
            Err(PlanError::InvalidSpec(reason)) => {
                assert!(reason.starts_with("operator session: "), "{what}: {reason}");
            }
            other => panic!("{what}: expected a refusal, got {other:?}"),
        }
    }
}

#[test]
fn the_operator_field_round_trips_and_is_absent_when_empty() {
    let spec = operator_spec("agy");
    let json = serde_json::to_string(&spec).unwrap();
    assert!(
        json.contains("\"operator_shell_prefixes\":[\"agent-os\"]"),
        "{json}"
    );
    let back: SessionSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(back, spec);
    let plain = SessionSpec {
        operator_shell_prefixes: Vec::new(),
        ..spec.clone()
    };
    let plain_json = serde_json::to_string(&plain).unwrap();
    assert!(
        !plain_json.contains("operator_shell_prefixes"),
        "{plain_json}"
    );
    assert_ne!(plain.spec_hash(), spec.spec_hash());
}
