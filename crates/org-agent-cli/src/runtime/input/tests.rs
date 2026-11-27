//! The input choke-point: one reader per concept, on a request and on its input alike.

use std::collections::BTreeMap;

use org_command::Request;
use org_proto::ErrorCode;
use serde_json::{json, Map, Value};

use super::*;

fn request(resource: &str, verb: &str, input: Value) -> Request {
    let Value::Object(input) = input else {
        panic!("input is an object");
    };
    Request {
        command: crate::registry::runtime_table()
            .find(resource, verb)
            .expect("a live row"),
        input,
        limit: None,
        cursor: None,
        dry_run: false,
        idempotency_key: Some("key-1".into()),
        if_match: None,
        durable: false,
        fields: None,
    }
}

#[test]
fn readers_read_a_request_and_its_input_alike() {
    let req = request(
        "task",
        "list",
        json!({"title": "x", "done": true, "labels": ["a", 1, "b"], "n": 3}),
    );
    let input: &Map<String, Value> = &req.input;
    assert_eq!(opt_str(&req, "title"), Some("x"));
    assert_eq!(opt_str(input, "title"), Some("x"));
    assert_eq!(opt_string(input, "title"), Some("x".to_owned()));
    assert_eq!(opt_str(input, "n"), None);
    assert_eq!(opt_bool(&req, "done"), Some(true));
    assert!(is_set(input, "done"));
    assert!(!is_set(input, "missing"));
    assert_eq!(
        opt_str_list(input, "labels"),
        Some(vec!["a".to_owned(), "b".to_owned()])
    );
    assert_eq!(opt_u64(input, "n").unwrap(), Some(3));
    assert_eq!(opt_usize(input, "n").unwrap(), Some(3));
}

#[test]
fn a_missing_required_field_names_its_flag() {
    let req = request("task", "list", json!({}));
    let refusal = require_str(&req, "max_results").unwrap_err();
    assert_eq!(refusal.code, ErrorCode::InvalidRequest);
    assert_eq!(refusal.message, "Missing required --max-results flag.");
    assert_eq!(refusal.hint, "Pass --max-results <value>.");
}

#[test]
fn whole_numbers_are_non_negative_and_null_is_not_given() {
    let input = json!({"neg": -1, "null": null, "text": "5", "big": u64::MAX, "frac": 1.5});
    let Value::Object(input) = input else {
        unreachable!()
    };
    assert_eq!(opt_u64(&input, "null").unwrap(), None);
    assert_eq!(opt_u64(&input, "absent").unwrap(), None);
    assert_eq!(opt_u64(&input, "big").unwrap(), Some(u64::MAX));
    for bad in ["neg", "text", "frac"] {
        let refusal = opt_u64(&input, bad).unwrap_err();
        assert_eq!(refusal.code, ErrorCode::InvalidRequest, "{bad}");
        assert!(refusal.message.contains(&format!("--{bad}")), "{bad}");
    }
}

#[test]
fn a_dry_run_of_a_write_without_one_is_refused_before_any_id_is_made() {
    let mut req = request("repo", "push", json!({}));
    req.dry_run = true;
    let refusal = command_id(&req, "git-command").unwrap_err();
    assert_eq!(refusal.code, ErrorCode::Unsupported);
    assert!(
        refusal.message.contains("`repo push`"),
        "{}",
        refusal.message
    );
    assert_eq!(
        hiring_command_id(&req).unwrap_err().code,
        ErrorCode::Unsupported
    );
}

#[test]
fn command_and_entity_ids_repeat_for_the_same_key_and_domain() {
    let req = request("task", "create", json!({"title": "t"}));
    let a = command_id(&req, "task-command").unwrap();
    assert_eq!(a, command_id(&req, "task-command").unwrap());
    assert_ne!(a, command_id(&req, "git-command").unwrap());
    assert_ne!(a, hiring_command_id(&req).unwrap());
    let given = request(
        "team",
        "create",
        json!({"name": "n", "team_id": "explicit"}),
    );
    assert_eq!(
        entity_id(&given, Some("team_id"), "team.create").unwrap(),
        "explicit"
    );
    let derived = entity_id(&req, None, "task.create").unwrap();
    assert!(org_proto::is_uuid_v7(&derived), "{derived}");

    let mut keyless = request("task", "create", json!({"title": "t"}));
    keyless.idempotency_key = None;
    assert_eq!(
        hiring_command_id(&keyless).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
}

#[test]
fn a_value_that_cannot_be_written_is_refused_not_a_panic() {
    let mut keyed = BTreeMap::new();
    keyed.insert((1, 2), "x");
    let refusal = encode(&keyed, "process.show").unwrap_err();
    assert_eq!(refusal.code, ErrorCode::Unavailable);
    assert!(
        refusal.message.contains("process.show"),
        "{}",
        refusal.message
    );
    assert_eq!(encode(&json!({"a": 1}), "m").unwrap(), json!({"a": 1}));
}

#[test]
fn durations_parse_with_units_and_refuse_nonsense() {
    assert_eq!(parse_ttl("90").unwrap(), 90);
    assert_eq!(parse_ttl("90s").unwrap(), 90);
    assert_eq!(parse_ttl("45m").unwrap(), 2_700);
    assert_eq!(parse_ttl("24h").unwrap(), 86_400);
    assert_eq!(parse_ttl("7D").unwrap(), 604_800);
    for bad in [
        "",
        "h",
        "0h",
        "-1h",
        "1w",
        "1.5h",
        "99999999999999999999d",
        "1h\u{e9}",
    ] {
        assert!(parse_ttl(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn answers_carry_the_key_ack_and_revision() {
    let req = request("task", "create", json!({"title": "t"}));
    let answer = write_answer(&req, json!({"id": "x"}), Some(4));
    assert_eq!(answer.operation_id.as_deref(), Some("key-1"));
    assert_eq!(answer.revision, Some(4));
    assert!(answer.ack.is_some());
    let committed = committed_answer(&req, json!({}));
    assert_eq!(committed.ack, Some(org_command::Ack::Committed));
    let revised = revised_answer(&req, json!({}), 9);
    assert_eq!(revised.revision, Some(9));
    let mut map = Map::new();
    insert_opt(&mut map, "a", Some("b"));
    insert_opt(&mut map, "c", None);
    assert_eq!(Value::Object(map), json!({"a": "b"}));
}
