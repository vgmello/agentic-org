use super::*;

const ID: &str = "01J00000000000000000000000";
const COMMAND: &str = r#"{"expected_revision":9223372036854775807,"assigned_team_id":null,"assignee_id":"employee 🦀"}"#;

#[test]
fn selected_shared_vectors_preserve_all_pairs_and_large_revisions() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../test-vectors/v2/selected-tasks-v1.json")).unwrap();
    for case in fixture["valid"].as_array().unwrap() {
        let browser = decode_http_assignment(case["http"].as_str().unwrap().as_bytes()).unwrap();
        let native = decode_native_assignment(case["native"].as_str().unwrap().as_bytes()).unwrap();
        assert_eq!(
            browser.params.expected_revision.0.get().to_string(),
            case["revision"].as_str().unwrap()
        );
        assert_eq!(
            serde_json::to_value(&browser.params.assigned_team_id).unwrap(),
            case["assigned_team_id"]
        );
        assert_eq!(
            serde_json::to_value(&browser.params.assignee_id).unwrap(),
            case["assignee_id"]
        );
        assert_eq!(browser.params.into_native(), native.params.command);
    }
    for case in fixture["invalid_http"].as_array().unwrap() {
        assert!(decode_http_assignment(case.as_str().unwrap().as_bytes()).is_err());
    }
    for case in fixture["invalid_native"].as_array().unwrap() {
        assert!(decode_native_assignment(case.as_str().unwrap().as_bytes()).is_err());
    }
}

fn native(command: &str) -> String {
    format!(
        r#"{{"id":1,"method":"task.set_assignment","params":{{"change_id":"{ID}","task_id":"opaque-task-id","command":{command}}}}}"#
    )
}

fn http(command: &str) -> String {
    format!(
        r#"{{"operation_id":"{ID}","scope":{{"kind":"office","instance_id":"instance-id","org_id":"org-id","office_id":"office-id"}},"params":{command}}}"#
    )
}

#[test]
fn native_and_browser_preserve_full_signed_revision_and_complete_pair() {
    let decoded = decode_native_assignment(native(COMMAND).as_bytes()).unwrap();
    assert_eq!(decoded.params.command.expected_revision.get(), i64::MAX);
    assert_eq!(decoded.params.command.assigned_team_id.0, None);
    assert_eq!(
        decoded.params.command.assignee_id.0.as_deref(),
        Some("employee 🦀")
    );
    let browser = COMMAND.replace("9223372036854775807", "\"9223372036854775807\"");
    let browser = decode_http_assignment(http(&browser).as_bytes()).unwrap();
    assert_eq!(browser.params.into_native(), decoded.params.command);
    assert!(decoded.principal.raw().is_none());
}

#[test]
fn native_duplicates_at_every_wrapper_depth_are_rejected_before_value_conversion() {
    let base = native(COMMAND);
    for text in [
        base.replace("\"id\":1", "\"id\":1,\"id\":1"),
        base.replace("\"method\":", "\"method\":\"task.assign\",\"method\":"),
        base.replace("\"params\":", "\"params\":{},\"params\":"),
        base.replace("\"task_id\":", "\"task_id\":\"other\",\"task_id\":"),
        base.replace(
            "\"change_id\":",
            &format!("\"change_id\":\"{ID}\",\"change_id\":"),
        ),
        base.replace("\"command\":", "\"command\":{},\"command\":"),
        base.replace(
            "\"assigned_team_id\":null",
            "\"assigned_team_id\":null,\"assigned_team_id\":null",
        ),
        base.replace("\"assignee_id\":", "\"assignee_id\":null,\"assignee_id\":"),
        base.replace(
            "\"expected_revision\":",
            "\"expected_revision\":1,\"expected_revision\":",
        ),
        base.replace(
            "\"assigned_team_id\":null",
            "\"assigned_team_id\":null,\"assigned_team_\\u0069d\":null",
        ),
        base.replace("\"id\":1", "\"id\":1,\"principal\":null,\"principal\":null"),
    ] {
        assert!(decode_native_assignment(text.as_bytes()).is_err());
    }
}

#[test]
fn http_duplicates_arrays_missing_fields_and_unknown_keys_are_rejected() {
    let command = COMMAND.replace("9223372036854775807", "\"9223372036854775807\"");
    let base = http(&command);
    for text in [
        base.replace(
            "\"operation_id\":",
            &format!("\"operation_id\":\"{ID}\",\"operation_id\":"),
        ),
        base.replace("\"scope\":", "\"scope\":{},\"scope\":"),
        base.replace("\"params\":", "\"params\":{},\"params\":"),
        base.replace(
            "\"kind\":\"office\"",
            "\"kind\":\"office\",\"kind\":\"office\"",
        ),
        base.replace("\"org_id\":", "\"org_id\":\"other\",\"org_id\":"),
        base.replace("\"office_id\":", "\"office_id\":\"other\",\"office_id\":"),
        base.replace(
            "\"instance_id\":",
            "\"instance_id\":\"other\",\"instance_id\":",
        ),
        base.replace(
            "\"assigned_team_id\":null",
            "\"assigned_team_id\":null,\"assigned_team_id\":null",
        ),
        base.replace("\"assigned_team_id\":null,", ""),
        base.replace(",\"assignee_id\":\"employee 🦀\"", ""),
        base.replace(
            "\"expected_revision\":",
            "\"unknown\":0,\"expected_revision\":",
        ),
        http("[\"9223372036854775807\",null,\"employee 🦀\"]"),
        format!("[\"{ID}\",{{}},{{}}]"),
        http(&command).replace(
            "\"kind\":\"office\"",
            "\"kind\":\"office\",\"actor_id\":\"forged\"",
        ),
    ] {
        assert!(decode_http_assignment(text.as_bytes()).is_err());
    }
}

#[test]
fn revision_spelling_cannot_bypass_typed_retry_identity() {
    for spelling in [
        "0",
        "-0",
        "-1",
        "1.0",
        "1e0",
        "9223372036854775808",
        "\"1\"",
    ] {
        let command = COMMAND.replace("9223372036854775807", spelling);
        assert!(decode_native_assignment(native(&command).as_bytes()).is_err());
    }
    for spelling in [
        "0",
        "01",
        "-0",
        "-1",
        "+1",
        "1.0",
        "1e0",
        " 1",
        "1 ",
        "9223372036854775808",
    ] {
        let command = COMMAND.replace("9223372036854775807", &format!("\"{spelling}\""));
        assert!(decode_http_assignment(http(&command).as_bytes()).is_err());
    }
    assert!(decode_http_assignment(http(COMMAND).as_bytes()).is_err());
    let escaped = http(&COMMAND.replace("9223372036854775807", "\"9\\u003223372036854775807\""));
    assert_eq!(
        decode_http_assignment(escaped.as_bytes())
            .unwrap()
            .params
            .expected_revision
            .0
            .get(),
        i64::MAX
    );
}

#[test]
fn principal_null_stays_present_and_payload_debug_cannot_disclose_it() {
    let line = native(COMMAND).replace("\"id\":1", "\"id\":1,\"principal\":null");
    assert_eq!(
        decode_native_assignment(line.as_bytes())
            .unwrap()
            .principal
            .raw()
            .unwrap()
            .get(),
        "null"
    );
    assert_eq!(
        decode_http_assignment(&vec![b' '; crate::MAX_FRAME_BYTES + 1]).err(),
        Some(DecodeError)
    );
    assert_eq!(
        decode_native_assignment(&vec![b' '; crate::MAX_FRAME_BYTES + 1]).err(),
        Some(DecodeError)
    );
}

#[test]
fn raw_value_feature_leaves_known_legacy_value_number_outputs_unchanged() {
    for (input, expected) in [
        ("-0", "-0.0"),
        ("1.0", "1.0"),
        ("1E+000", "1.0"),
        ("18446744073709551615", "18446744073709551615"),
    ] {
        let value: serde_json::Value = serde_json::from_str(input).unwrap();
        assert_eq!(value.to_string(), expected);
    }
}
#[test]
fn selected_lookup_is_raw_object_only_and_rejects_duplicate_wrappers_and_params() {
    let valid = br#"{"id":1,"method":"operation.get","params":{"kind":"work","id":"01ARZ3NDEKTSV4RRFFQ69G5FAV"}}"#;
    assert_eq!(
        decode_native_lookup(valid).unwrap().operation_id.as_str(),
        "01ARZ3NDEKTSV4RRFFQ69G5FAV"
    );
    for bad in [
        r#"{"id":1,"id":2,"method":"operation.get","params":{"kind":"work","id":"01ARZ3NDEKTSV4RRFFQ69G5FAV"}}"#,
        r#"{"id":1,"method":"operation.get","params":{"kind":"work","k\u0069nd":"work","id":"01ARZ3NDEKTSV4RRFFQ69G5FAV"}}"#,
        r#"{"id":1,"method":"operation.get","params":["work","01ARZ3NDEKTSV4RRFFQ69G5FAV"]}"#,
        r#"[1,"operation.get",null,{"kind":"work","id":"01ARZ3NDEKTSV4RRFFQ69G5FAV"}]"#,
        r#"{"id":1,"method":"operation.get","params":{"kind":"org","id":"01ARZ3NDEKTSV4RRFFQ69G5FAV"}}"#,
    ] {
        assert!(decode_native_lookup(bad.as_bytes()).is_err());
    }
}
#[test]
fn assignment_result_and_lookup_keep_full_revisions_and_earlier_receipt_coverage() {
    let value = serde_json::json!({"status":"rejected","refusal":{"code":"conflict","message":"conflict","current":{"organization_id":"org","office_id":"office","task_id":"opaque","revision":9223372036854775807_i64,"assigned_team_id":null,"assignee_id":"employee"}}});
    let native: LookupOutcome<i64> = serde_json::from_value(value).unwrap();
    let browser = serde_json::to_value(native.into_browser().unwrap()).unwrap();
    assert_eq!(
        browser["refusal"]["current"]["revision"],
        "9223372036854775807"
    );
    let old = br#"{"status":"rejected","code":"conflict"}"#;
    let retained: AssignmentResult<i64> = serde_json::from_slice(old).unwrap();
    assert!(matches!(
        &retained,
        AssignmentResult::Rejected { current: None, .. }
    ));
    assert_eq!(serde_json::to_vec(&retained).unwrap(), old);
}
