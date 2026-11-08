//! The session's contract and its edge cases: the port's conformance testkit, driven by
//! recorded lines, and the paths the spike's transcripts don't cover (timeouts, stderr,
//! idle interrupts, exits mid-turn, malformed input).

mod common;

use std::time::{Duration, Instant};

use common::*;
use org_runtime::testkit::{self, Script, ScriptApproval};
use org_runtime::{
    AgentRuntime, ApprovalId, Control, ControlError, Decision, ExitInfo, Output, PermissionMode,
    Session, SessionEvent, StartFailure, TurnOutcome,
};
use serde_json::{json, Value};

/// A recorded line of `stream-json-multi-turn.jsonl` (1-based).
fn multi_turn_line(n: usize) -> Vec<u8> {
    bytes(&fixture("stream-json-multi-turn.jsonl")[n - 1].msg)
}

/// The session's first request is always `org-1`: initialize.
fn opening() -> Vec<u8> {
    bytes(&recorded_initialize_reply("org-1"))
}

fn script() -> Script {
    Script {
        opening: vec![opening()],
        // The turn's system/init; then its reply and result end it.
        turn: vec![multi_turn_line(6)],
        turn_end: vec![multi_turn_line(7), multi_turn_line(8), multi_turn_line(9)],
        stderr: vec![
            b"Warning: something harmless".to_vec(),
            b"Not logged in. Run claude auth login to authenticate.".to_vec(),
        ],
        // The recorded request, and Claude withdrawing it on interrupt.
        approval: Some(ScriptApproval {
            request: bytes(&fixture("approval-no-answer.jsonl")[7].msg),
            id: ApprovalId::new(uuid(8)),
            cancel: Some(bytes(&fixture("approval-no-answer.jsonl")[9].msg)),
        }),
    }
}

#[test]
fn conforms_to_the_session_contract() {
    assert_eq!(
        fixture("approval-no-answer.jsonl")[9].msg["type"],
        "control_cancel_request"
    );
    testkit::check_all(
        &mut || session("zephyr", PermissionMode::Ask),
        &script(),
        &org_runtime_claude::ClaudeRuntime::new().capabilities(),
        Instant::now(),
    );
}

/// A session that's Ready.
fn ready() -> (Box<dyn Session>, Instant) {
    let now = Instant::now();
    let mut s = session("zephyr", PermissionMode::Ask);
    s.start(now);
    let out = s.on_line(&opening(), now);
    assert!(matches!(events(&out)[0], SessionEvent::Ready { .. }));
    (s, now)
}

fn send(s: &mut dyn Session, now: Instant) {
    s.control(
        Control::Send {
            text: "go".into(),
            urgent: false,
        },
        now,
    )
    .unwrap();
}

fn line(s: &mut dyn Session, v: Value, now: Instant) -> Vec<SessionEvent> {
    events(&s.on_line(&bytes(&v), now))
}

#[test]
fn opening_times_out() {
    let now = Instant::now();
    let mut s = session("zephyr", PermissionMode::Ask);
    let out = s.start(now);
    assert_eq!(
        writes(&out),
        vec![json!({"type": "control_request", "request_id": "org-1",
            "request": {"subtype": "initialize"}})]
    );
    assert_eq!(s.next_deadline(), Some(now + Duration::from_secs(30)));
    assert_eq!(
        s.control(
            Control::Send {
                text: "x".into(),
                urgent: false
            },
            now
        ),
        Err(ControlError::NotReady)
    );
    assert!(s.on_tick(now + Duration::from_secs(29)).is_empty());
    let out = s.on_tick(now + Duration::from_secs(30));
    assert_eq!(
        out,
        vec![
            Output::Event(SessionEvent::StartFailed {
                reason: StartFailure::ProtocolTimeout
            }),
            Output::CloseStdin
        ]
    );
    assert_eq!(s.next_deadline(), None);
    assert_eq!(
        s.control(Control::Interrupt, now),
        Err(ControlError::Closed)
    );
    // A late reply changes nothing.
    assert!(s.on_line(&opening(), now).is_empty());
}

#[test]
fn start_failures_on_stderr() {
    let now = Instant::now();
    for (text, expected) in [
        (
            "No conversation found with session ID: 0b8e2a",
            StartFailure::ResumeFailed {
                detail: "No conversation found with session ID: 0b8e2a".into(),
            },
        ),
        (
            "Not logged in. Run claude auth login to authenticate.",
            StartFailure::AuthRequired,
        ),
        (
            "Invalid API key · Please run /login",
            StartFailure::AuthRequired,
        ),
    ] {
        let mut s = session("zephyr", PermissionMode::Ask);
        s.start(now);
        assert!(s.on_stderr_line(b"some unrelated warning", now).is_empty());
        let out = s.on_stderr_line(text.as_bytes(), now);
        assert_eq!(
            out,
            vec![
                Output::Event(SessionEvent::StartFailed { reason: expected }),
                Output::CloseStdin
            ],
            "{text}"
        );
    }
    // Once Ready, stderr is the office's log's business.
    let (mut s, now) = ready();
    assert!(s.on_stderr_line(b"Not logged in", now).is_empty());
}

#[test]
fn initialize_refused_or_without_mode() {
    let now = Instant::now();
    let mut s = session("zephyr", PermissionMode::Ask);
    s.start(now);
    let ev = line(
        s.as_mut(),
        json!({"type": "control_response", "response": {"subtype": "error",
            "request_id": "org-1", "error": "bad"}}),
        now,
    );
    assert_eq!(
        ev,
        vec![SessionEvent::StartFailed {
            reason: StartFailure::Other {
                detail: "bad".into()
            }
        }]
    );

    let mut s = session("zephyr", PermissionMode::Ask);
    s.start(now);
    let mut reply = recorded_initialize_reply("org-1");
    reply["response"]["response"]
        .as_object_mut()
        .unwrap()
        .remove("current_permission_mode");
    let ev = line(s.as_mut(), reply, now);
    assert_eq!(
        ev,
        vec![SessionEvent::StartFailed {
            reason: StartFailure::ModeMismatch {
                requested: "default".into(),
                reported: String::new()
            }
        }]
    );
}

#[test]
fn idle_interrupt_is_only_acknowledged() {
    let (mut s, now) = ready();
    let out = s.control(Control::Interrupt, now).unwrap();
    assert_eq!(
        writes(&out),
        vec![json!({"type": "control_request", "request_id": "org-2",
            "request": {"subtype": "interrupt"}})]
    );
    assert_eq!(s.next_deadline(), Some(now + Duration::from_secs(10)));
    let ack = json!({"type": "control_response", "response": {"subtype": "success",
        "request_id": "org-2", "response": {"still_queued": []}}});
    assert!(s.on_line(&bytes(&ack), now).is_empty());
    assert_eq!(s.next_deadline(), None);
    // Still idle: a normal send goes through.
    send(s.as_mut(), now);
}

#[test]
fn unacknowledged_interrupt_is_reported() {
    let (mut s, now) = ready();
    send(s.as_mut(), now);
    s.control(Control::Interrupt, now).unwrap();
    assert!(s.on_tick(now + Duration::from_secs(9)).is_empty());
    let ev = events(&s.on_tick(now + Duration::from_secs(10)));
    assert!(
        matches!(ev[..], [SessionEvent::ProtocolError { .. }]),
        "{ev:?}"
    );
    assert_eq!(s.next_deadline(), None);
}

#[test]
fn a_late_interrupt_ack_is_not_an_error() {
    let (mut s, now) = ready();
    s.control(Control::Interrupt, now).unwrap();
    let late = now + Duration::from_secs(10);
    assert!(matches!(
        events(&s.on_tick(late))[..],
        [SessionEvent::ProtocolError { .. }]
    ));
    let ack = json!({"type": "control_response", "response": {"subtype": "success",
        "request_id": "org-2", "response": {"still_queued": []}}});
    assert!(s.on_line(&bytes(&ack), late).is_empty());
    // A second one is a stray.
    assert!(matches!(
        events(&s.on_line(&bytes(&ack), late))[..],
        [SessionEvent::ProtocolError { .. }]
    ));
}

#[test]
fn a_numeric_request_id_is_answered_as_sent() {
    let (mut s, now) = ready();
    send(s.as_mut(), now);
    let ev = line(
        s.as_mut(),
        json!({"type": "control_request", "request_id": 42, "request": {
            "subtype": "can_use_tool", "tool_name": "Bash", "input": {"command": "ls"}}}),
        now,
    );
    let id = match &ev[1] {
        SessionEvent::ApprovalRequested(a) => a.id.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(id.as_str(), "42");
    let out = s
        .control(
            Control::Answer {
                request_id: id,
                decision: Decision::AllowOnce,
            },
            now,
        )
        .unwrap();
    assert_eq!(writes(&out)[0]["response"]["request_id"], json!(42));

    // A numeric id is withdrawn by its number too.
    let ev = line(
        s.as_mut(),
        json!({"type": "control_request", "request_id": 43, "request": {
            "subtype": "can_use_tool", "tool_name": "Bash", "input": {"command": "ls"}}}),
        now,
    );
    assert!(matches!(ev[..], [SessionEvent::ApprovalRequested(_)]));
    let ev = line(
        s.as_mut(),
        json!({"type": "control_cancel_request", "request_id": 43}),
        now,
    );
    assert_eq!(
        ev,
        vec![SessionEvent::ApprovalCancelled {
            id: ApprovalId::new("43")
        }]
    );
}

#[test]
fn a_duplicate_pending_request_id_is_declined() {
    let (mut s, now) = ready();
    send(s.as_mut(), now);
    let request = |id: Value, command: &str| {
        json!({"type": "control_request", "request_id": id, "request": {
            "subtype": "can_use_tool", "tool_name": "Bash", "input": {"command": command}}})
    };
    line(s.as_mut(), request(json!("42"), "ls"), now);
    let out = s.on_line(&bytes(&request(json!(42), "rm -rf x")), now);
    let ev = events(&out);
    assert!(
        matches!(ev[..], [SessionEvent::ProtocolError { .. }]),
        "{ev:?}"
    );
    assert_eq!(writes(&out)[0]["response"]["subtype"], json!("error"));
    assert_eq!(writes(&out)[0]["response"]["request_id"], json!(42));
    // The answer goes to the first request, as raised.
    let out = s
        .control(
            Control::Answer {
                request_id: ApprovalId::new("42"),
                decision: Decision::AllowOnce,
            },
            now,
        )
        .unwrap();
    assert_eq!(writes(&out)[0]["response"]["request_id"], json!("42"));
}

#[test]
fn file_writes_outside_the_writable_roots_escalate() {
    // The file tools run outside Claude's sandbox: a write outside the writable roots is
    // flagged as a sandbox bypass, which the office always denies.
    let (mut s, now) = ready();
    send(s.as_mut(), now);
    let mut escalations = vec![];
    for (i, path) in [
        format!("{WORKSPACE}/src/main.rs"),
        format!("{TMPDIR}/scratch.txt"),
        format!("{HOME}/memory/MEMORY.md"),
        "/etc/hosts".to_owned(),
        format!("{WORKSPACE}/../t2/x"),
    ]
    .iter()
    .enumerate()
    {
        let ev = line(
            s.as_mut(),
            json!({"type": "control_request", "request_id": format!("w{i}"), "request": {
                "subtype": "can_use_tool", "tool_name": "Write",
                "input": {"file_path": path, "content": "x"}}}),
            now,
        );
        let a = ev
            .iter()
            .find_map(|e| match e {
                SessionEvent::ApprovalRequested(a) => Some(a.clone()),
                _ => None,
            })
            .unwrap();
        escalations.push(a.escalation);
    }
    use org_runtime::Escalation::{None as Plain, SandboxBypass};
    assert_eq!(
        escalations,
        [Plain, Plain, SandboxBypass, SandboxBypass, SandboxBypass]
    );
}

#[test]
fn close_works_before_ready() {
    let now = Instant::now();
    let mut s = session("zephyr", PermissionMode::Ask);
    s.start(now);
    assert_eq!(s.control(Control::Close, now), Ok(vec![Output::CloseStdin]));
    assert_eq!(s.control(Control::Close, now), Err(ControlError::Closed));
    let ev = events(&s.on_exit(EXIT_0, now));
    assert!(matches!(
        ev[..],
        [SessionEvent::StartFailed { .. }, SessionEvent::Ended { .. }]
    ));
}

#[test]
fn busy_and_urgent_input() {
    let (mut s, now) = ready();
    // Urgent input while idle is just input.
    s.control(
        Control::Send {
            text: "now".into(),
            urgent: true,
        },
        now,
    )
    .unwrap();
    for (urgent, err) in [
        (false, ControlError::Busy),
        (true, ControlError::NeedsInterrupt),
    ] {
        assert_eq!(
            s.control(
                Control::Send {
                    text: "more".into(),
                    urgent
                },
                now
            ),
            Err(err)
        );
    }
}

#[test]
fn approvals_answered_and_refused() {
    let (mut s, now) = ready();
    assert_eq!(
        s.control(
            Control::Answer {
                request_id: ApprovalId::new("nope"),
                decision: Decision::AllowOnce
            },
            now
        ),
        Err(ControlError::UnknownApproval)
    );
    send(s.as_mut(), now);
    let request = |id: &str, tool: &str, input: Value| {
        json!({"type": "control_request", "request_id": id, "request": {
            "subtype": "can_use_tool", "tool_name": tool, "input": input,
            "description": "trust me", "tool_use_id": format!("toolu_{id}")}})
    };
    let ev = line(
        s.as_mut(),
        request(
            "r1",
            "Edit",
            json!({"file_path": "/w/a", "old_string": "a", "new_string": "b"}),
        ),
        now,
    );
    // Content before an init still opens the turn, solicited.
    assert_eq!(
        ev[0],
        SessionEvent::TurnStarted {
            turn_id: None,
            solicited: true
        }
    );
    match &ev[1] {
        SessionEvent::ApprovalRequested(a) => {
            assert_eq!(a.kind, org_runtime::ApprovalKind::FileChange);
            assert_eq!(a.justification_untrusted.as_deref(), Some("trust me"));
        }
        other => panic!("{other:?}"),
    }
    line(
        s.as_mut(),
        request("r2", "Bash", json!({"command": "ls"})),
        now,
    );
    let out = s
        .control(
            Control::Answer {
                request_id: ApprovalId::new("r1"),
                decision: Decision::Deny {
                    message: None,
                    stop_turn: true,
                },
            },
            now,
        )
        .unwrap();
    assert_eq!(
        writes(&out),
        vec![
            json!({"type": "control_response", "response": {"subtype": "success",
            "request_id": "r1", "response": {"behavior": "deny",
            "message": "The office denied this action.", "interrupt": true}}})
        ]
    );
    assert_eq!(
        s.control(
            Control::Answer {
                request_id: ApprovalId::new("r1"),
                decision: Decision::AllowOnce
            },
            now
        ),
        Err(ControlError::UnknownApproval)
    );
    // r2 is still pending when the process exits: it's withdrawn, then the turn is cut short.
    let out = s.on_exit(
        ExitInfo {
            code: None,
            signal: Some(9),
        },
        now,
    );
    let ev = events(&out);
    assert_eq!(
        ev,
        vec![
            SessionEvent::ApprovalCancelled {
                id: ApprovalId::new("r2")
            },
            SessionEvent::TurnEnded {
                outcome: TurnOutcome::Failed {
                    reason: "Claude exited during the turn".into()
                },
                errors: vec![]
            },
            SessionEvent::Ended {
                exit: ExitInfo {
                    code: None,
                    signal: Some(9)
                }
            }
        ]
    );
}

#[test]
fn a_turn_cut_short_still_reports_its_usage() {
    let (mut s, now) = ready();
    send(s.as_mut(), now);
    s.on_line(&multi_turn_line(6), now);
    s.on_line(&multi_turn_line(7), now);
    let ev = events(&s.on_stdout_closed(now));
    assert!(matches!(ev[0], SessionEvent::Usage(ref u) if u.output == 5 && u.input == 2));
    assert_eq!(
        ev[1],
        SessionEvent::TurnEnded {
            outcome: TurnOutcome::Failed {
                reason: "Claude's output ended during the turn".into()
            },
            errors: vec![]
        }
    );
    // The exit owes nothing more than Ended.
    assert_eq!(
        events(&s.on_exit(EXIT_0, now)),
        vec![SessionEvent::Ended { exit: EXIT_0 }]
    );
}

#[test]
fn background_work_starts_an_unsolicited_turn() {
    let (mut s, now) = ready();
    send(s.as_mut(), now);
    for n in [6, 7, 8, 9] {
        s.on_line(&multi_turn_line(n), now);
    }
    // A new init after the result, with nothing sent.
    let ev = events(&s.on_line(&multi_turn_line(6), now));
    assert_eq!(
        ev,
        vec![SessionEvent::TurnStarted {
            turn_id: None,
            solicited: false
        }]
    );
    assert_eq!(
        s.control(
            Control::Send {
                text: "x".into(),
                urgent: false
            },
            now
        ),
        Err(ControlError::Busy)
    );
    let ev = events(&s.on_line(&multi_turn_line(9), now));
    assert!(matches!(ev.last(), Some(SessionEvent::TurnEnded { .. })));
}

#[test]
fn a_permission_mode_change_is_reported() {
    let (mut s, now) = ready();
    send(s.as_mut(), now);
    let mut init: Value = serde_json::from_slice(&multi_turn_line(6)).unwrap();
    init["permissionMode"] = json!("acceptEdits");
    let ev = line(s.as_mut(), init, now);
    assert!(ev.contains(&SessionEvent::ModeChanged {
        mode: Some("acceptEdits".into())
    }));
    // Reported once; the same mode on the next turn is no change.
    for n in [7, 8, 9] {
        s.on_line(&multi_turn_line(n), now);
    }
    send(s.as_mut(), now);
    let mut init: Value = serde_json::from_slice(&multi_turn_line(6)).unwrap();
    init["permissionMode"] = json!("acceptEdits");
    let ev = line(s.as_mut(), init, now);
    assert!(!ev
        .iter()
        .any(|e| matches!(e, SessionEvent::ModeChanged { .. })));
}

#[test]
fn malformed_and_unknown_input() {
    let (mut s, now) = ready();
    let secret = b"{\"type\": \"assistant\", \"message\": {\"content\": [ SECRET-TOKEN-123";
    let ev = events(&s.on_line(secret, now));
    match &ev[..] {
        [SessionEvent::ProtocolError { detail }] => assert!(!detail.contains("SECRET")),
        other => panic!("{other:?}"),
    }
    for (input, expected) in [
        (json!([1, 2]), "protocol_error"),
        (json!({"no": "type"}), "protocol_error"),
        (json!({"type": "keep_alive"}), "unknown"),
        (
            json!({"type": "system", "subtype": "hook_started"}),
            "unknown",
        ),
        (json!({"type": "assistant"}), "protocol_error"),
        (json!({"type": "rate_limit_event"}), "protocol_error"),
        (
            json!({"type": "control_response", "response": {"subtype": "success", "request_id": "x"}}),
            "protocol_error",
        ),
    ] {
        let ev = line(s.as_mut(), input.clone(), now);
        let kinds: Vec<String> = ev
            .iter()
            .map(|e| {
                serde_json::to_value(e).unwrap()["type"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        assert_eq!(kinds, vec![expected], "{input}");
    }
    assert_eq!(
        line(s.as_mut(), json!({"type": "mystery"}), now),
        vec![SessionEvent::Unknown {
            type_name: "mystery".into()
        }]
    );
    assert_eq!(
        events(&s.on_truncated(5_000_000, now)),
        vec![SessionEvent::Truncated { len: 5_000_000 }]
    );
}

#[test]
fn requests_that_need_content_back_are_declined() {
    let (mut s, now) = ready();
    let out = s.on_line(
        &bytes(&json!({"type": "control_request", "request_id": "h1",
            "request": {"subtype": "elicitation", "message": "Your password?"}})),
        now,
    );
    assert_eq!(
        writes(&out),
        vec![
            json!({"type": "control_response", "response": {"subtype": "error",
            "request_id": "h1", "error": "the office doesn't support this request"}})
        ]
    );
    assert_eq!(
        events(&out),
        vec![SessionEvent::Unknown {
            type_name: "control_request/elicitation".into()
        }]
    );
}

#[test]
fn close_withdraws_and_stops_raising_approvals() {
    let (mut s, now) = ready();
    send(s.as_mut(), now);
    let out = s.control(Control::Close, now).unwrap();
    assert_eq!(out, vec![Output::CloseStdin]);
    assert_eq!(
        s.control(Control::Interrupt, now),
        Err(ControlError::Closed)
    );
    // Claude denies it itself when it reads EOF; nothing to raise, nothing to write.
    let out = s.on_line(
        &bytes(
            &json!({"type": "control_request", "request_id": "r9", "request": {
            "subtype": "can_use_tool", "tool_name": "Bash", "input": {"command": "ls"}}}),
        ),
        now,
    );
    assert!(writes(&out).is_empty());
    assert!(!events(&out)
        .iter()
        .any(|e| matches!(e, SessionEvent::ApprovalRequested(_))));
}
