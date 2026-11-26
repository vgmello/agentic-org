use serde_json::{json, Value};

use super::KNOWN_EVENT_TYPES;
use crate::{
    Activity, ApprovalId, ApprovalKind, ApprovalRequest, CostBasis, Escalation, ExitInfo,
    LimitKind, LimitStatus, NoticeKind, RateLimit, SessionEvent, SessionId, StartFailure,
    ToolCapability, TurnOutcome, Usage, UsageScope, Window,
};

/// One of every event, with every string field set, so tests cover them all.
pub(crate) fn every_event() -> Vec<SessionEvent> {
    let events = vec![
        SessionEvent::Ready {
            runtime_version: Some("s:version".into()),
            pid: Some(42),
            agents: vec!["s:agent".into()],
            permission_mode: Some("s:mode".into()),
        },
        SessionEvent::StartFailed {
            reason: StartFailure::AgentMismatch {
                requested: "s:requested".into(),
                available: vec!["s:available".into()],
            },
        },
        SessionEvent::StartFailed {
            reason: StartFailure::ModeMismatch {
                requested: "s:mode_requested".into(),
                reported: "s:mode_reported".into(),
            },
        },
        SessionEvent::StartFailed {
            reason: StartFailure::ResumeFailed {
                detail: "s:resume".into(),
            },
        },
        SessionEvent::StartFailed {
            reason: StartFailure::Other {
                detail: "s:other".into(),
            },
        },
        SessionEvent::StartFailed {
            reason: StartFailure::AuthRequired,
        },
        SessionEvent::StartFailed {
            reason: StartFailure::ProtocolTimeout,
        },
        SessionEvent::SessionStarted {
            runtime_session_id: SessionId::try_from("id:session").unwrap(),
        },
        SessionEvent::TurnStarted {
            turn_id: Some("id:turn".into()),
            solicited: false,
        },
        SessionEvent::Status(Activity::WaitingOnApproval),
        SessionEvent::TextDelta {
            text: "s:delta".into(),
        },
        SessionEvent::AssistantText {
            message_id: Some("id:message".into()),
            text: "s:text".into(),
        },
        SessionEvent::Thinking,
        SessionEvent::ModeChanged {
            mode: Some("s:new_mode".into()),
        },
        SessionEvent::ToolStarted {
            tool_use_id: "id:tool_use".into(),
            name: "s:tool".into(),
            capability: Some(ToolCapability::Shell),
            input: json!({
                "s:key": ["s:nested", {"s:deep": "s:deeper"}, 1, true, null],
                "type": "s:typed",
            }),
        },
        SessionEvent::ToolFinished {
            tool_use_id: "id:tool_use".into(),
            is_error: true,
        },
        SessionEvent::ApprovalRequested(ApprovalRequest {
            id: ApprovalId::new("id:approval"),
            kind: ApprovalKind::Command,
            escalation: Escalation::SandboxBypass,
            tool: "s:tool".into(),
            capability: Some(ToolCapability::Shell),
            display_name: Some("s:display".into()),
            input: json!({"command": "s:command", "s:k": {"s:k2": ["s:v"]}}),
            justification_untrusted: Some("s:why".into()),
            blocked_path: Some("s:path".into()),
            tool_use_id: Some("id:tool_use".into()),
        }),
        SessionEvent::ApprovalCancelled {
            id: ApprovalId::new("id:approval"),
        },
        SessionEvent::ActionDenied {
            tool: "s:tool".into(),
            tool_use_id: Some("id:tool_use".into()),
            message: Some("s:denied".into()),
        },
        SessionEvent::Usage(Usage {
            model: Some("s:model".into()),
            input: 1,
            output: 2,
            cache_read: 3,
            cache_write: 4,
            reasoning: 5,
            cost_usd: Some(0.25),
            cost_basis: CostBasis::ListPriceEstimate,
            token_scope: UsageScope::TurnDelta,
            cost_scope: UsageScope::Cumulative,
        }),
        SessionEvent::RateLimit(RateLimit {
            window: Window::Other {
                id: "s:window".into(),
                mins: Some(60),
            },
            used_fraction: Some(0.5),
            resets_at: Some("s:resets".into()),
            status: LimitStatus::Warning,
        }),
        SessionEvent::TurnEnded {
            outcome: TurnOutcome::Failed {
                reason: "s:reason".into(),
            },
            errors: vec!["s:error".into()],
        },
        SessionEvent::TurnEnded {
            outcome: TurnOutcome::Limit {
                kind: LimitKind::MaxTurns,
            },
            errors: vec![],
        },
        SessionEvent::TurnEnded {
            outcome: TurnOutcome::SignedOut,
            errors: vec![],
        },
        SessionEvent::MidTurnDelivered {
            delivery_id: "mid-1".into(),
        },
        SessionEvent::MidTurnDeliveryFailed {
            delivery_id: "mid-2".into(),
            reason: "s:mid_fail".into(),
        },
        SessionEvent::Notice {
            kind: NoticeKind::Retry,
            text: "s:notice".into(),
        },
        SessionEvent::ProtocolError {
            detail: "s:protocol".into(),
        },
        SessionEvent::Truncated { len: 5_000_000 },
        SessionEvent::Unknown {
            type_name: "s:type".into(),
        },
        SessionEvent::Ended {
            exit: ExitInfo {
                code: None,
                signal: Some(15),
            },
        },
    ];
    // Exhaustive: a new variant fails to compile here until it has a sample above.
    for e in &events {
        match e {
            SessionEvent::Ready { .. }
            | SessionEvent::StartFailed { .. }
            | SessionEvent::SessionStarted { .. }
            | SessionEvent::TurnStarted { .. }
            | SessionEvent::Status(_)
            | SessionEvent::TextDelta { .. }
            | SessionEvent::AssistantText { .. }
            | SessionEvent::Thinking
            | SessionEvent::ModeChanged { .. }
            | SessionEvent::ToolStarted { .. }
            | SessionEvent::ToolFinished { .. }
            | SessionEvent::ApprovalRequested(_)
            | SessionEvent::ApprovalCancelled { .. }
            | SessionEvent::ActionDenied { .. }
            | SessionEvent::Usage(_)
            | SessionEvent::RateLimit(_)
            | SessionEvent::TurnEnded { .. }
            | SessionEvent::MidTurnDelivered { .. }
            | SessionEvent::MidTurnDeliveryFailed { .. }
            | SessionEvent::Notice { .. }
            | SessionEvent::ProtocolError { .. }
            | SessionEvent::Truncated { .. }
            | SessionEvent::Unknown { .. }
            | SessionEvent::Ended { .. } => {}
        }
    }
    events
}

fn tag(e: &SessionEvent) -> String {
    serde_json::to_value(e).unwrap()["type"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn every_event_round_trips_through_serde() {
    let mut tags = std::collections::BTreeSet::new();
    for event in every_event() {
        let json = serde_json::to_string(&event).unwrap();
        let back: SessionEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, event, "{json}");
        tags.insert(tag(&event));
    }
    // Every variant's tag is a known one, and every known tag has a sample.
    let known: std::collections::BTreeSet<String> =
        KNOWN_EVENT_TYPES.iter().map(|s| s.to_string()).collect();
    assert_eq!(tags, known);
}

#[test]
fn event_wire_shapes() {
    let cases = [
        (SessionEvent::Thinking, json!({"type": "thinking"})),
        (
            SessionEvent::Ended {
                exit: ExitInfo {
                    code: Some(0),
                    signal: None,
                },
            },
            json!({"type": "ended", "data": {"exit": {"code": 0, "signal": null}}}),
        ),
        (
            SessionEvent::ApprovalRequested(ApprovalRequest {
                id: ApprovalId::new("7"),
                kind: ApprovalKind::FileChange,
                escalation: Escalation::None,
                tool: "Edit".into(),
                capability: None,
                display_name: None,
                input: json!({}),
                justification_untrusted: None,
                blocked_path: None,
                tool_use_id: None,
            }),
            json!({"type": "approval_requested", "data": {
                "id": "7", "kind": "file_change", "escalation": "none", "tool": "Edit",
                "display_name": null,
                "input": {}, "justification_untrusted": null, "blocked_path": null,
                "tool_use_id": null}}),
        ),
        (
            SessionEvent::Status(Activity::Busy),
            json!({"type": "status", "data": "busy"}),
        ),
        (
            SessionEvent::TextDelta { text: "hi".into() },
            json!({"type": "text_delta", "data": {"text": "hi"}}),
        ),
        (
            SessionEvent::TurnEnded {
                outcome: TurnOutcome::Limit {
                    kind: LimitKind::MaxTokens,
                },
                errors: vec![],
            },
            json!({"type": "turn_ended", "data": {
                "outcome": {"type": "limit", "kind": "max_tokens"}, "errors": []}}),
        ),
        (
            SessionEvent::ApprovalCancelled {
                id: ApprovalId::new("7"),
            },
            json!({"type": "approval_cancelled", "data": {"id": "7"}}),
        ),
    ];
    for (event, expected) in cases {
        assert_eq!(serde_json::to_value(&event).unwrap(), expected);
    }
}

#[test]
fn unknown_event_types_decode_as_unknown() {
    let e: SessionEvent =
        serde_json::from_value(json!({"type": "reasoning_summary", "data": {"n": 3}})).unwrap();
    assert_eq!(
        e,
        SessionEvent::Unknown {
            type_name: "reasoning_summary".into()
        }
    );
    let e: SessionEvent = serde_json::from_value(json!({"type": "new_unit"})).unwrap();
    assert_eq!(
        e,
        SessionEvent::Unknown {
            type_name: "new_unit".into()
        }
    );
    // A known type with a bad body is an error, not Unknown.
    assert!(serde_json::from_value::<SessionEvent>(
        json!({"type": "text_delta", "data": {"txt": 1}})
    )
    .is_err());
    assert!(serde_json::from_value::<SessionEvent>(json!({"data": {}})).is_err());
    assert!(serde_json::from_value::<SessionEvent>(json!("ended")).is_err());
}

#[test]
fn unknown_enum_values_decode_as_unknown() {
    let a: Activity = serde_json::from_str(r#""napping""#).unwrap();
    assert_eq!(a, Activity::Unknown);
    let k: LimitKind = serde_json::from_str(r#""max_budget""#).unwrap();
    assert_eq!(k, LimitKind::Unknown);
    let n: NoticeKind = serde_json::from_str(r#""info""#).unwrap();
    assert_eq!(n, NoticeKind::Unknown);
    let k: ApprovalKind = serde_json::from_str(r#""network""#).unwrap();
    assert_eq!(k, ApprovalKind::Unknown);
    let e: Escalation = serde_json::from_str(r#""root""#).unwrap();
    assert_eq!(e, Escalation::Unknown);
    let c: ToolCapability = serde_json::from_str(r#""network""#).unwrap();
    assert_eq!(c, ToolCapability::Unknown);
    let o: TurnOutcome = serde_json::from_value(json!({"type": "paused", "why": "x"})).unwrap();
    assert_eq!(o, TurnOutcome::Unknown);
    let o: TurnOutcome =
        serde_json::from_value(json!({"type": "limit", "kind": "max_budget"})).unwrap();
    assert_eq!(
        o,
        TurnOutcome::Limit {
            kind: LimitKind::Unknown
        }
    );
    let s: StartFailure =
        serde_json::from_value(json!({"type": "binary_missing", "path": "/x"})).unwrap();
    assert_eq!(s, StartFailure::Unknown);
    // Nested inside an event too.
    let e: SessionEvent =
        serde_json::from_value(json!({"type": "status", "data": "napping"})).unwrap();
    assert_eq!(e, SessionEvent::Status(Activity::Unknown));
}

#[test]
fn redact_visits_every_free_text_string() {
    for mut event in every_event() {
        let before = serde_json::to_value(&event).unwrap();
        let mut visited = Vec::new();
        event.redact(&mut |s| {
            visited.push(s.clone());
            s.insert_str(0, "R:");
        });
        let after = serde_json::to_value(&event).unwrap();
        // Every string in the serialized event that a sample marked as free text ("s:")
        // was visited and changed; every id ("id:") was left alone.
        let mut strings = Vec::new();
        collect_strings(&before, &mut strings);
        for s in strings.iter().filter(|s| s.starts_with("s:")) {
            assert!(visited.contains(s), "{s} not visited in {before}");
        }
        assert!(
            visited.iter().all(|s| !s.starts_with("id:")),
            "an id was visited: {visited:?}"
        );
        let mut redacted = Vec::new();
        collect_strings(&after, &mut redacted);
        assert!(
            redacted.iter().all(|s| !s.starts_with("s:")),
            "unredacted text left in {after}"
        );
    }
}

#[test]
fn redact_reaches_nested_values_and_keys() {
    let mut event = SessionEvent::ToolStarted {
        tool_use_id: "toolu_1".into(),
        name: "Bash".into(),
        capability: Some(ToolCapability::Shell),
        input: json!({
            "command": "echo TOKEN=abc",
            "env": {"TOKEN=abc": ["x", {"y": "TOKEN=abc"}]},
        }),
    };
    event.redact(&mut |s| *s = s.replace("TOKEN=abc", "TOKEN=***"));
    let json = serde_json::to_string(&event).unwrap();
    assert!(!json.contains("abc"), "{json}");
    assert!(json.contains("toolu_1"));
}

/// Every string in a JSON value, object keys included.
fn collect_strings(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Array(a) => a.iter().for_each(|x| collect_strings(x, out)),
        Value::Object(m) => {
            for (k, x) in m {
                out.push(k.clone());
                collect_strings(x, out);
            }
        }
        _ => {}
    }
}

#[test]
fn approval_id_is_opaque() {
    let id = ApprovalId::new("req-0001");
    // It's a plain string on the wire, and comes back unchanged.
    assert_eq!(serde_json::to_value(&id).unwrap(), json!("req-0001"));
    let back: ApprovalId = serde_json::from_value(json!("req-0001")).unwrap();
    assert_eq!(back, id);
    assert_eq!(back.as_str(), "req-0001");
    // Compared by value only; a numeric wire id is the adapter's to stringify.
    assert_ne!(ApprovalId::new("1"), ApprovalId::new("01"));
    assert!(serde_json::from_value::<ApprovalId>(json!(1)).is_err());
}
