//! The spike's recorded transcripts (Claude Code 2.1.311), replayed through the real
//! session: the writes it makes must be the lines the spike's harness sent, and its events
//! must be the expected ones (`tests/expected/*.txt`, plus the assertions here).

mod common;

use common::*;
use org_runtime::{
    ApprovalKind, Control, ControlError, Decision, Output, PermissionMode, SessionEvent, SessionId,
    StartFailure, TurnOutcome, Usage, Window,
};
use serde_json::{json, Value};

/// Replays `name` with an agent called `agent` and checks the expected-output file.
fn replay(name: &str, agent: &str, before: &[(usize, Control)]) -> Replay {
    let mut r = Replay::new(session_for(name, agent, PermissionMode::Ask)).injecting(agent);
    r.run(name, before);
    r.finish(EXIT_0);
    r.check_expected(name.trim_end_matches(".jsonl"));
    r
}

fn turn_outcomes(events: &[SessionEvent]) -> Vec<TurnOutcome> {
    events
        .iter()
        .filter_map(|e| match e {
            SessionEvent::TurnEnded { outcome, .. } => Some(outcome.clone()),
            _ => None,
        })
        .collect()
}

fn usages(events: &[SessionEvent]) -> Vec<Usage> {
    events
        .iter()
        .filter_map(|e| match e {
            SessionEvent::Usage(u) => Some(u.clone()),
            _ => None,
        })
        .collect()
}

fn kinds(events: &[SessionEvent]) -> Vec<String> {
    events
        .iter()
        .map(|e| {
            serde_json::to_value(e).unwrap()["type"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect()
}

#[test]
fn multi_turn_session() {
    let r = replay("stream-json-multi-turn.jsonl", "zephyr", &[]);
    let ev = r.events();
    assert_eq!(
        kinds(&ev),
        [
            "ready",
            // turn 1
            "session_started",
            "turn_started",
            "assistant_text",
            "rate_limit",
            "rate_limit",
            "usage",
            "turn_ended",
            // turn 2: the Read tool
            "turn_started",
            "tool_started",
            "tool_finished",
            "assistant_text",
            "usage",
            "turn_ended",
            "ended",
        ]
    );
    match &ev[0] {
        SessionEvent::Ready {
            runtime_version,
            pid,
            agents,
            permission_mode,
        } => {
            assert_eq!(runtime_version.as_deref(), Some("2.1.311"));
            assert!(pid.is_some());
            assert!(agents.contains(&"general-purpose".to_owned()));
            assert!(agents.contains(&"zephyr".to_owned()));
            assert_eq!(permission_mode.as_deref(), Some("default"));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        ev[1],
        SessionEvent::SessionStarted {
            runtime_session_id: SessionId::try_from(uuid(2).as_str()).unwrap()
        }
    );
    assert_eq!(
        ev[2],
        SessionEvent::TurnStarted {
            turn_id: None,
            solicited: true
        }
    );
    assert_eq!(
        ev[3],
        SessionEvent::AssistantText {
            message_id: Some("msg_011Cgq4aA1HaDhf6bgC9Uk2n".into()),
            text: "ONE".into()
        }
    );
    // The rate-limit windows read. Their values aren't asserted: the fixture's are
    // scrubbed (and due to be replaced by synthetic ones).
    match (&ev[4], &ev[5]) {
        (SessionEvent::RateLimit(a), SessionEvent::RateLimit(b)) => {
            assert_eq!(a.window, Window::FiveHour);
            assert_eq!(b.window, Window::SevenDay);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        turn_outcomes(&ev),
        [TurnOutcome::Completed, TurnOutcome::Completed]
    );
    match &ev[9] {
        SessionEvent::ToolStarted { name, input, .. } => {
            assert_eq!(name, "Read");
            assert!(input["file_path"].as_str().unwrap().ends_with("note.txt"));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        ev[10],
        SessionEvent::ToolFinished {
            tool_use_id: "toolu_01V1nJ57CLAUdL2DW15kCweW".into(),
            is_error: false
        }
    );
    // Every write is a recorded line: initialize and the two user lines.
    assert_eq!(r.writes().len(), 3);
}

#[test]
fn queued_input_waits_for_the_turn() {
    // The harness wrote two lines back to back; the session refuses the second as Busy, and
    // the office sends it once the first turn ended.
    let r = replay("stream-json-queued.jsonl", "zephyr", &[]);
    assert!(r.log.iter().any(|l| l == "# refused: Busy"));
    let ev = r.events();
    assert_eq!(
        turn_outcomes(&ev),
        [TurnOutcome::Completed, TurnOutcome::Completed]
    );
    let solicited: Vec<bool> = ev
        .iter()
        .filter_map(|e| match e {
            SessionEvent::TurnStarted { solicited, .. } => Some(*solicited),
            _ => None,
        })
        .collect();
    assert_eq!(solicited, [true, true]);
}

#[test]
fn inline_agent_is_checked_and_ready() {
    let r = replay("agent-inline-json.jsonl", "zephyr", &[]);
    match &r.events()[0] {
        SessionEvent::Ready { agents, .. } => assert!(agents.contains(&"zephyr".into())),
        other => panic!("{other:?}"),
    }
    replay("agent-file.jsonl", "zephyr", &[]);
}

#[test]
fn unknown_agent_fails_the_start() {
    // `--agent zephyr` with no definition: Claude runs its default agent silently, but
    // `initialize` doesn't list zephyr, so the session refuses to go on.
    let mut r = Replay::new(session("zephyr", PermissionMode::Ask));
    r.run("agent-unknown.jsonl", &[]);
    let ev = r.events();
    match &ev[0] {
        SessionEvent::StartFailed {
            reason:
                StartFailure::AgentMismatch {
                    requested,
                    available,
                },
        } => {
            assert_eq!(requested, "zephyr");
            assert!(available.contains(&"general-purpose".to_owned()));
            assert!(!available.contains(&"zephyr".to_owned()));
        }
        other => panic!("{other:?}"),
    }
    assert!(r.outputs.contains(&Output::CloseStdin));
    // Nothing after it: the Send is refused, the turn's lines are ignored.
    assert!(r.log.iter().any(|l| l == "# refused: Closed"));
    assert_eq!(ev.len(), 1, "{ev:?}");
    r.finish(EXIT_0);
    assert_eq!(r.events().len(), 2);
    r.check_expected("agent-unknown");
}

#[test]
fn a_different_permission_mode_fails_the_start() {
    let mut r = Replay::new(session("zephyr", PermissionMode::AcceptEdits)).injecting("zephyr");
    r.run("stream-json-multi-turn.jsonl", &[]);
    assert_eq!(
        r.events()[0],
        SessionEvent::StartFailed {
            reason: StartFailure::ModeMismatch {
                requested: "acceptEdits".into(),
                reported: "default".into()
            }
        }
    );
    assert!(r.outputs.contains(&Output::CloseStdin));
}

#[test]
fn approval_allowed() {
    let r = replay("approval-allow.jsonl", "zephyr", &[]);
    let ev = r.events();
    let req = ev
        .iter()
        .find_map(|e| match e {
            SessionEvent::ApprovalRequested(a) => Some(a.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(req.id.as_str(), uuid(6));
    assert_eq!(req.kind, ApprovalKind::Command);
    assert_eq!(req.tool, "Bash");
    assert_eq!(req.display_name.as_deref(), Some("Bash"));
    assert_eq!(
        req.input,
        json!({"command": "echo approved > allow.txt", "description": "Write approved to allow.txt"})
    );
    assert_eq!(
        req.justification_untrusted.as_deref(),
        Some("Write approved to allow.txt")
    );
    assert_eq!(req.blocked_path.as_deref(), Some("<tmp>/s2/ws/allow.txt"));
    assert_eq!(
        req.tool_use_id.as_deref(),
        Some("toolu_01KFJzeqWUczVQzaFnPZTpzB")
    );
    // The answer is exactly the recorded `{"behavior":"allow"}` (checked by the replay).
    assert!(r
        .writes()
        .iter()
        .any(|w| w["response"]["response"] == json!({"behavior": "allow"})));
    assert_eq!(turn_outcomes(&ev), [TurnOutcome::Completed]);
    assert!(!ev
        .iter()
        .any(|e| matches!(e, SessionEvent::ActionDenied { .. })));
}

#[test]
fn approval_denied_with_a_message() {
    let r = replay("approval-deny-message.jsonl", "zephyr", &[]);
    let ev = r.events();
    assert_eq!(turn_outcomes(&ev), [TurnOutcome::Completed]);
    // The deny went through the bridge: the result's permission_denials don't repeat it.
    assert!(!ev
        .iter()
        .any(|e| matches!(e, SessionEvent::ActionDenied { .. })));
    assert!(ev.contains(&SessionEvent::ToolFinished {
        tool_use_id: "toolu_01JFSTMTjdBMMcJmsw55tEpt".into(),
        is_error: true
    }));
}

#[test]
fn approval_denied_and_turn_stopped() {
    let r = replay("approval-deny-interrupt.jsonl", "zephyr", &[]);
    assert_eq!(
        turn_outcomes(&r.events()),
        [TurnOutcome::Interrupted, TurnOutcome::Completed]
    );
}

#[test]
fn unanswered_approval_withdrawn_on_interrupt() {
    let r = replay("approval-no-answer.jsonl", "zephyr", &[]);
    let ev = r.events();
    let id = org_runtime::ApprovalId::new(uuid(8));
    let requested = ev
        .iter()
        .position(|e| matches!(e, SessionEvent::ApprovalRequested(a) if a.id == id))
        .unwrap();
    let cancelled = ev
        .iter()
        .position(|e| *e == SessionEvent::ApprovalCancelled { id: id.clone() })
        .unwrap();
    assert!(requested < cancelled);
    assert_eq!(
        ev.iter()
            .filter(|e| matches!(e, SessionEvent::ApprovalCancelled { .. }))
            .count(),
        1
    );
    assert_eq!(turn_outcomes(&ev), [TurnOutcome::Interrupted]);
    assert!(!ev
        .iter()
        .any(|e| matches!(e, SessionEvent::ActionDenied { .. })));
}

#[test]
fn denied_without_a_bridge_is_reported_once() {
    let r = replay("approval-no-prompt-tool-flag.jsonl", "zephyr", &[]);
    let denied: Vec<_> = r
        .events()
        .into_iter()
        .filter(|e| matches!(e, SessionEvent::ActionDenied { .. }))
        .collect();
    assert_eq!(denied.len(), 1, "{denied:?}");
    match &denied[0] {
        SessionEvent::ActionDenied {
            tool,
            tool_use_id,
            message,
        } => {
            assert_eq!(tool, "Bash");
            assert_eq!(
                tool_use_id.as_deref(),
                Some("toolu_01YafNxDn11VeV76KnMbyCpY")
            );
            assert!(message
                .as_deref()
                .unwrap()
                .starts_with("Output redirection"));
        }
        _ => unreachable!(),
    }
}

#[test]
fn close_with_an_approval_pending() {
    // The harness closed stdin while the request was open: the office's Close withdraws
    // it (it can't be answered any more), and Claude finishes the turn.
    let r = replay(
        "approval-pending-stdin-closed.jsonl",
        "zephyr",
        &[(5, Control::Close)],
    );
    let ev = r.events();
    let cancelled = ev
        .iter()
        .position(|e| matches!(e, SessionEvent::ApprovalCancelled { .. }))
        .unwrap();
    assert!(r.outputs.contains(&Output::CloseStdin));
    assert_eq!(turn_outcomes(&ev), [TurnOutcome::Completed]);
    let ended = ev
        .iter()
        .position(|e| matches!(e, SessionEvent::TurnEnded { .. }))
        .unwrap();
    assert!(cancelled < ended);
}

#[test]
fn answers_decision_cannot_express_are_never_sent() {
    // The spike's rewrite, invalid deny and error-response answers can't be expressed: a
    // Decision never rewrites input, and a deny always carries a message. Replayed without
    // them, the session still ends the turn and withdraws the request it never answered.
    for name in [
        "approval-allow-rewrite.jsonl",
        "approval-deny-invalid.jsonl",
        "approval-error-response.jsonl",
    ] {
        let r = replay(name, "zephyr", &[]);
        assert_eq!(r.inexpressible.len(), 1, "{name}");
        let ev = r.events();
        assert!(
            ev.iter()
                .any(|e| matches!(e, SessionEvent::ApprovalCancelled { .. })),
            "{name}"
        );
        assert!(!r
            .writes()
            .iter()
            .any(|w| w.to_string().contains("updatedInput")));
    }
}

#[test]
fn interrupt_during_streaming() {
    let r = replay("interrupt-during-text.jsonl", "zephyr", &[]);
    let ev = r.events();
    assert!(ev.contains(&SessionEvent::TextDelta {
        text: "The l".into()
    }));
    assert!(ev.contains(&SessionEvent::Thinking));
    assert!(ev.contains(&SessionEvent::AssistantText {
        message_id: Some("msg_011Cgq53YnFAs9GNXE4snWuo".into()),
        text: "The l".into()
    }));
    assert_eq!(turn_outcomes(&ev), [TurnOutcome::Interrupted]);
    // The result reports zeros (spike fact 15); the messages don't, and nothing is lost.
    let result = fixture("interrupt-during-text.jsonl")
        .into_iter()
        .find(|r| r.msg["type"] == "result")
        .unwrap()
        .msg;
    assert_eq!(result["usage"]["input_tokens"], 0);
    assert_eq!(result["usage"]["cache_read_input_tokens"], 0);
    let u = usages(&ev);
    assert_eq!(u.len(), 1);
    assert_eq!(
        (u[0].input, u[0].output, u[0].cache_read, u[0].cache_write),
        (2, 2, 19430, 57)
    );
    assert_eq!(u[0].model.as_deref(), Some("claude-haiku-5-5"));
    // `model_usage_since_last_result` is `{}`: no cost to report.
    assert_eq!(u[0].cost_usd, None);
    // The usage comes before the turn's end.
    let usage_at = ev.iter().position(|e| matches!(e, SessionEvent::Usage(_)));
    let end_at = ev
        .iter()
        .position(|e| matches!(e, SessionEvent::TurnEnded { .. }));
    assert!(usage_at < end_at);
    // The interrupt was acknowledged: no deadline left.
    assert_eq!(r.session.next_deadline(), None);
}

#[test]
fn interrupt_during_a_tool() {
    let r = replay("interrupt-during-tool.jsonl", "zephyr", &[]);
    let ev = r.events();
    assert_eq!(turn_outcomes(&ev), [TurnOutcome::Interrupted]);
    assert!(ev.contains(&SessionEvent::ToolFinished {
        tool_use_id: "toolu_01MTDpnFzwfkK57FCAAGMEGN".into(),
        is_error: true
    }));
    // The final output count is message_delta's (87), not the snapshot's (16).
    let u = usages(&ev);
    assert_eq!((u[0].input, u[0].output, u[0].cache_read), (2, 87, 19217));
    assert_eq!(u[0].cost_usd, Some(0.00023587000000000003));
}

#[test]
fn resumed_session() {
    let mut c = ctx();
    let id = SessionId::try_from(uuid(1).as_str()).unwrap();
    c.resume = Some(id.clone());
    c.new_session_id = None;
    let (plan, session) = plan(&spec("zephyr", PermissionMode::Ask), &c);
    assert_eq!(plan.runtime_session_id, Some(id.clone()));
    let mut r = Replay::new(session);
    r.run("resume-R1-spec-v2-snapshot-on.jsonl", &[]);
    r.finish(EXIT_0);
    r.check_expected("resume-R1-spec-v2-snapshot-on");
    let ev = r.events();
    assert!(ev.contains(&SessionEvent::SessionStarted {
        runtime_session_id: id
    }));
    // The launch's own id matched: no warning.
    assert!(!ev.iter().any(|e| matches!(e, SessionEvent::Notice { .. })));
}

#[test]
fn every_fixture_replays_cleanly_and_usage_matches_the_results() {
    let mut names: Vec<String> = std::fs::read_dir(fixtures_dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".jsonl"))
        .collect();
    names.sort();
    assert_eq!(names.len(), 39);
    for name in &names {
        let lines = fixture(name);
        // Every session is launched with the agent `zephyr`; runs that had no `--agents`
        // get it added to their recorded `initialize` reply.
        let mut r = Replay::new(session_for(name, "zephyr", PermissionMode::Ask));
        if name != "agent-unknown.jsonl" {
            r = r.injecting("zephyr");
        }
        // The spike's pending-stdin-closed run closed stdin after the request.
        let before: &[(usize, Control)] = if name == "approval-pending-stdin-closed.jsonl" {
            &[(5, Control::Close)]
        } else {
            &[]
        };
        r.run(name, before);
        r.finish(EXIT_0);
        let ev = r.events();
        if name == "agent-unknown.jsonl" {
            continue;
        }
        for e in &ev {
            assert!(
                !matches!(
                    e,
                    SessionEvent::ProtocolError { .. }
                        | SessionEvent::Unknown { .. }
                        | SessionEvent::StartFailed { .. }
                        | SessionEvent::Notice { .. }
                ),
                "{name}: {e:?}"
            );
        }
        assert!(matches!(ev.last(), Some(SessionEvent::Ended { .. })));

        // Each recorded result against the usage the session summed for its turn.
        let results: Vec<&Value> = lines
            .iter()
            .filter(|r| r.msg["type"] == "result")
            .map(|r| &r.msg)
            .collect();
        let mut turn_usages: Vec<Vec<Usage>> = vec![];
        let mut current = vec![];
        for e in &ev {
            match e {
                SessionEvent::Usage(u) => current.push(u.clone()),
                SessionEvent::TurnEnded { .. } => turn_usages.push(std::mem::take(&mut current)),
                _ => {}
            }
        }
        assert_eq!(turn_usages.len(), results.len(), "{name}");
        // Which turns streamed partial messages (their output counts are final).
        let mut streamed = vec![];
        let mut saw_delta = false;
        for rec in &lines {
            if rec.msg["type"] == "stream_event" && rec.msg["event"]["type"] == "message_delta" {
                saw_delta = true;
            }
            if rec.msg["type"] == "result" {
                streamed.push(std::mem::take(&mut saw_delta));
            }
        }
        for ((result, usage), streamed) in results.iter().zip(&turn_usages).zip(streamed) {
            let reported = &result["usage"];
            if reported["input_tokens"] == 0 {
                continue; // an aborted stream: the result has nothing to compare with
            }
            let sum = |f: fn(&Usage) -> u64| usage.iter().map(f).sum::<u64>();
            let n = |k: &str| reported[k].as_u64().unwrap();
            assert_eq!(sum(|u| u.input), n("input_tokens"), "{name}");
            assert_eq!(
                sum(|u| u.cache_read),
                n("cache_read_input_tokens"),
                "{name}"
            );
            assert_eq!(
                sum(|u| u.cache_write),
                n("cache_creation_input_tokens"),
                "{name}"
            );
            if streamed {
                assert_eq!(sum(|u| u.output), n("output_tokens"), "{name}");
            } else {
                // Without partial messages only the snapshots' undercount is known.
                assert!(sum(|u| u.output) <= n("output_tokens"), "{name}");
            }
        }
    }
}

#[test]
fn scrubbed_session_id_is_a_protocol_error() {
    // The raw fixture's `<uuid-1>` isn't a valid SessionId: it must not be used.
    let mut s = session("zephyr", PermissionMode::Ask);
    let now = std::time::Instant::now();
    let init = writes(&s.start(now))[0].clone();
    s.on_line(
        &bytes(&recorded_initialize_reply(
            init["request_id"].as_str().unwrap(),
        )),
        now,
    );
    s.control(
        Control::Send {
            text: "hi".into(),
            urgent: false,
        },
        now,
    )
    .unwrap();
    let out = s.on_line(
        br#"{"type":"system","subtype":"init","session_id":"<uuid-1>","permissionMode":"default"}"#,
        now,
    );
    let ev = events(&out);
    assert!(
        matches!(ev[0], SessionEvent::ProtocolError { .. }),
        "{ev:?}"
    );
    assert!(!ev
        .iter()
        .any(|e| matches!(e, SessionEvent::SessionStarted { .. })));
    assert!(ev.contains(&SessionEvent::TurnStarted {
        turn_id: None,
        solicited: true
    }));
}

#[test]
fn deny_with_stop_turn_matches_the_recorded_shape() {
    // The recorded deny-interrupt answer, from a Decision.
    let recorded = fixture("approval-deny-interrupt.jsonl")
        .into_iter()
        .find(|r| r.to_claude && r.msg["type"] == "control_response")
        .unwrap()
        .msg;
    let mut r = Replay::new(session("zephyr", PermissionMode::Ask)).injecting("zephyr");
    r.run("approval-deny-interrupt.jsonl", &[]);
    assert!(r.writes().contains(&recorded));
    // A second answer to the same request is refused, and changes nothing.
    let again = r.session.control(
        Control::Answer {
            request_id: org_runtime::ApprovalId::new(uuid(4)),
            decision: Decision::AllowOnce,
        },
        r.now,
    );
    assert_eq!(again, Err(ControlError::UnknownApproval));
}
