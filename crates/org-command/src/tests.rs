//! The one-line envelopes a caller outside the table prints.

use serde_json::{json, Value};

use super::{failure_envelope_json, success_envelope_json, Refusal};
use org_proto::ErrorCode;

#[test]
fn the_success_envelope_is_the_four_keys_in_order_with_sorted_data() {
    let line = success_envelope_json(&json!({"zeta": 1, "alpha": [true, null], "mid": "x"}));
    assert_eq!(
        line,
        r#"{"ok":true,"data":{"alpha":[true,null],"mid":"x","zeta":1},"meta":{"status":"completed"},"error":null}"#
    );
}

#[test]
fn the_success_envelope_escapes_every_character_a_terminal_or_reader_would_act_on() {
    // DEL, a C1 control, the line and paragraph separators, a right-to-left override and a
    // left-to-right isolate, plus the C0 controls JSON itself escapes.
    let hostile = "a\u{7f}b\u{85}c\u{2028}d\u{2029}e\u{202e}f\u{2066}g\nh\u{1b}[31m";
    let line = success_envelope_json(&json!({ "name": hostile, hostile: "key" }));
    assert!(!line.contains('\n'), "{line}");
    for raw in [
        '\u{7f}', '\u{85}', '\u{2028}', '\u{2029}', '\u{202e}', '\u{2066}', '\u{1b}',
    ] {
        assert!(!line.contains(raw), "{raw:?} is printed raw: {line}");
    }
    for escaped in [
        "\\u007f", "\\u0085", "\\u2028", "\\u2029", "\\u202e", "\\u2066", "\\u001b", "\\n",
    ] {
        assert!(line.contains(escaped), "{escaped} missing: {line}");
    }
    // Escaping never changes the value a JSON reader gets back.
    let parsed: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(parsed["data"]["name"], hostile);
    assert_eq!(parsed["data"][hostile], "key");
}

#[test]
fn success_and_failure_envelopes_escape_alike() {
    let text = "x\u{202e}y\u{2028}z";
    let success = success_envelope_json(&json!({ "message": text }));
    let failure = failure_envelope_json(
        &Refusal::new(ErrorCode::InvalidRequest, text, "hint"),
        false,
    );
    assert!(success.contains("\"x\\u202ey\\u2028z\""), "{success}");
    assert!(failure.contains("\"x\\u202ey\\u2028z\""), "{failure}");
}
