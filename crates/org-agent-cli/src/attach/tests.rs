//! `session attach`: every line answered in one JSON envelope, and nothing sent for a line it
//! cannot answer.
use std::io::Cursor;

use super::*;
use crate::transport::{OfficeTransport, TransportOffice};

struct StubTransport {
    calls: Vec<(String, Value)>,
}

impl OfficeTransport for StubTransport {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, org_command::Refusal> {
        self.calls.push((method.to_owned(), params));
        match method {
            "chat.inbox" => Ok(serde_json::json!({ "items": [] })),
            "status.set" => Ok(serde_json::json!({ "status": "building" })),
            _ => Err(org_command::Refusal::new(
                ErrorCode::UnknownMethod,
                "stub unknown",
                "check",
            )),
        }
    }
}

#[test]
fn attach_answers_every_line_and_continues_after_bad_json() {
    let stdin = concat!(
        r#"{"command":"msg inbox","input":{},"options":{"limit":5}}"#,
        "\n",
        "not json at all\n",
        r#"{"command":"status set","input":{"text":"building"}}"#,
        "\n"
    );
    let mut out = Vec::new();
    let mut office = TransportOffice(StubTransport { calls: Vec::new() });
    let exit = run_attach_loop("agent-os", Cursor::new(stdin), &mut out, &mut office);
    assert_eq!(exit, 0);
    let lines: Vec<&str> = std::str::from_utf8(&out).unwrap().lines().collect();
    assert_eq!(lines.len(), 3, "{lines:?}");
    let l0: Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(l0["ok"], true);
    let l1: Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(l1["ok"], false);
    assert_eq!(l1["error"]["code"], "invalid_request");
    let l2: Value = serde_json::from_str(lines[2]).unwrap();
    assert_eq!(l2["ok"], true);
    assert_eq!(l2["data"]["status"], "building");
    assert_eq!(office.0.calls.len(), 2);
}

#[test]
fn attach_rejects_non_object_options_and_wraps_help_as_single_line_json() {
    let stdin = concat!(
        r#"{"command":"msg inbox","options":["not-an-object"]}"#,
        "\n",
        r#"{"command":"msg inbox","options":{"help":true}}"#,
        "\n",
        r#"{"command":"status set","input":{"text":"still alive"}}"#,
        "\n"
    );
    let mut out = Vec::new();
    let mut office = TransportOffice(StubTransport { calls: Vec::new() });
    let exit = run_attach_loop("agent-os", Cursor::new(stdin), &mut out, &mut office);
    assert_eq!(exit, 0);
    let lines: Vec<&str> = std::str::from_utf8(&out).unwrap().lines().collect();
    assert_eq!(lines.len(), 3, "{lines:?}");
    let l0: Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(l0["ok"], false);
    assert_eq!(l0["error"]["code"], "invalid_request");
    let l1: Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(l1["ok"], false);
    assert_eq!(l1["error"]["code"], "invalid_request");
    let l2: Value = serde_json::from_str(lines[2]).unwrap();
    assert_eq!(l2["ok"], true);
    assert_eq!(l2["data"]["status"], "building");

    let parsed =
        parse_line_args(r#"{"command":"task note add","input":{"task":"T-A1","text":"plan"}}"#)
            .unwrap();
    assert_eq!(parsed[0], "task-note");
    assert_eq!(parsed[1], "add");
}

#[test]
fn a_line_that_cannot_be_read_answers_a_schema_valid_envelope() {
    let stdin = "not json\n{\"command\":\"\u{9b}31m\"}\n";
    let mut out = Vec::new();
    let mut office = TransportOffice(StubTransport { calls: Vec::new() });
    assert_eq!(
        run_attach_loop("agent-os", Cursor::new(stdin), &mut out, &mut office),
        0
    );
    let text = String::from_utf8(out).unwrap();
    assert!(!text.contains('\u{9b}'), "{text}");
    let schema = org_command::envelope_schema();
    let envelopes: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(envelopes.len(), 2, "{text}");
    for envelope in &envelopes {
        org_command::testkit::validate_schema(&schema, envelope).expect("schema");
        assert_eq!(envelope["data"], Value::Null, "{envelope}");
        assert_eq!(envelope["meta"], serde_json::json!({}), "{envelope}");
    }
    // A line that is not JSON gets the line hint; a command it names that does not exist
    // gets the parser's own hint, and the C1 control in it arrives as an escape.
    assert_eq!(envelopes[0]["error"]["hint"], LINE_HINT);
    assert!(
        text.lines().nth(1).unwrap().contains("\\u009b31m"),
        "{text}"
    );
    assert_eq!(
        envelopes[1]["error"]["message"]
            .as_str()
            .map(|m| m.contains('\u{9b}')),
        Some(true)
    );
}

struct FailingWriter(io::ErrorKind);

impl Write for FailingWriter {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::from(self.0))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_closed_pipe_ends_the_loop_with_0_and_another_write_error_with_1() {
    let stdin = "{\"command\":\"msg inbox\"}\n{\"command\":\"msg inbox\"}\n";
    for (kind, code) in [(io::ErrorKind::BrokenPipe, 0), (io::ErrorKind::Other, 1)] {
        let mut office = TransportOffice(StubTransport { calls: Vec::new() });
        let exit = run_attach_loop(
            "agent-os",
            Cursor::new(stdin),
            FailingWriter(kind),
            &mut office,
        );
        assert_eq!(exit, code, "{kind:?}");
        assert_eq!(
            office.0.calls.len(),
            1,
            "{kind:?}: stopped after the failed write"
        );
    }
}

/// A transport that records every call, to prove a refused line sends nothing.
fn recording() -> TransportOffice<StubTransport> {
    TransportOffice(StubTransport { calls: Vec::new() })
}

fn attach_lines(stdin: &str, office: &mut TransportOffice<StubTransport>) -> Vec<Value> {
    let mut out = Vec::new();
    assert_eq!(
        run_attach_loop("agent-os", Cursor::new(stdin.to_owned()), &mut out, office),
        0
    );
    let schema = org_command::envelope_schema();
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|line| {
            let envelope: Value = serde_json::from_str(line).unwrap();
            org_command::testkit::validate_schema(&schema, &envelope).expect("schema");
            envelope
        })
        .collect()
}

#[test]
fn a_line_attach_cannot_answer_in_json_is_refused_before_any_write() {
    let refused = [
        r#"{"command":"status set","input":{"text":"x"},"options":{"format":"yaml"}}"#,
        r#"{"command":"status set","input":{"text":"x"},"options":{"format":"compact"}}"#,
        r#"{"command":"status set","input":{"text":"x"},"options":{"--format":"yaml"}}"#,
        r#"{"command":"msg send","input":{"to":"owner"},"options":{"stdin":true}}"#,
        r#"{"command":"status set","input":{"text":"x"},"options":{"help":true}}"#,
        r#"{"command":"status set","input":{"text":"x"},"inputs":{"text":"y"}}"#,
        r#"{"command":"status set","input":{"text":"x"},"id":7}"#,
    ];
    for line in refused {
        let mut office = recording();
        let envelopes = attach_lines(&format!("{line}\n"), &mut office);
        assert_eq!(envelopes.len(), 1, "{line}");
        assert_eq!(envelopes[0]["ok"], false, "{line}");
        assert_eq!(envelopes[0]["error"]["code"], "invalid_request", "{line}");
        assert!(office.0.calls.is_empty(), "{line}: a write was sent");
    }
    // JSON, named or by default, still runs.
    let mut office = recording();
    let envelopes = attach_lines(
        "{\"command\":\"status set\",\"input\":{\"text\":\"x\"},\"options\":{\"format\":\"json\"}}\n",
        &mut office,
    );
    assert_eq!(envelopes[0]["ok"], true);
    assert_eq!(office.0.calls.len(), 1);
}

#[test]
fn attach_sources_never_read_standard_input() {
    let mut sources = AttachSources::default();
    let error = sources.stdin(16).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Unsupported);
}

#[test]
fn shorthands_mean_the_same_command_in_attach() {
    for (line, words) in [
        (r#"{"command":"who"}"#, vec!["directory", "list"]),
        (r#"{"command":"boot"}"#, vec!["session", "boot"]),
        (
            r#"{"command":"instructions overview"}"#,
            vec!["session", "boot"],
        ),
        (r#"{"command":"task note list"}"#, vec!["task-note", "list"]),
    ] {
        let args = parse_line_args(line).unwrap();
        assert_eq!(&args[..words.len()], words.as_slice(), "{line}");
    }
}

#[test]
fn session_attach_with_a_flag_is_a_usage_refusal() {
    let words = |line: &str| -> Vec<String> { line.split(' ').map(str::to_owned).collect() };
    assert!(usage_refusal(&words("session attach")).is_none());
    let refusal = usage_refusal(&words("session attach --limit 5")).expect("refused");
    assert_eq!(refusal.code, ErrorCode::InvalidRequest);
    assert!(refusal.message.contains("--limit 5"), "{}", refusal.message);
    assert!(refusal.hint.contains("--help"), "{}", refusal.hint);
}
