//! `result` frames: how a turn ended, its usage, its denials and the conversation id.

use serde_json::Value;

use org_agent_runtime::{
    Activity, CostBasis, LimitKind, Output, SessionEvent, SessionId, TurnOutcome, Usage, UsageScope,
};

use super::classify::parse_claude_usage;
use super::{fail_staged_mid_turn, ClaudeSession, Phase};

impl ClaudeSession {
    pub(super) fn handle_result(&mut self, val: &Value) -> Vec<Output> {
        let mut out = Vec::new();
        if let Some(ev) = record_result_session_id(self, val) {
            out.push(ev);
        }
        collect_result_denials(val, &mut out);
        // Always emit Usage BEFORE TurnEnded (usage rule).
        if let Some(usage) = build_result_usage(self, val) {
            out.push(usage);
        }

        let (outcome, errors) = map_result_outcome(val);
        self.reset_turn_usage();
        // Pending approvals stay answerable: a background sub-agent's `can_use_tool` outlives
        // the main turn's `result` (ADR-0025), and Claude has not withdrawn it. Only the
        // process ending (stdout closed or exit) cancels them.
        match self.phase {
            Phase::Busy => {
                if let Some(fail) =
                    fail_staged_mid_turn(self, "turn ended before next tool boundary")
                {
                    out.push(fail);
                }
                self.phase = Phase::Idle;
                self.turn_open = false;
                out.push(Output::Event(SessionEvent::TurnEnded { outcome, errors }));
                let status = if self.pending_approvals.is_empty() {
                    Activity::Idle
                } else {
                    Activity::WaitingOnApproval
                };
                out.push(Output::Event(SessionEvent::Status(status)));
            }
            Phase::Closing => {
                if let Some(fail) =
                    fail_staged_mid_turn(self, "turn ended before next tool boundary")
                {
                    out.push(fail);
                }
                out.extend(self.end_closed_turn(Some((outcome, errors))));
            }
            Phase::Opening | Phase::Idle | Phase::Failed | Phase::Ended => {}
        }
        out
    }
}

fn record_result_session_id(s: &mut ClaudeSession, val: &Value) -> Option<Output> {
    let raw_sid = val.get("session_id").and_then(Value::as_str)?;
    let sid = SessionId::try_from(raw_sid).ok()?;
    if s.emitted_session_id.as_ref() == Some(&sid) {
        return None;
    }
    s.emitted_session_id = Some(sid.clone());
    Some(Output::Event(SessionEvent::SessionStarted {
        runtime_session_id: sid,
    }))
}

fn build_result_usage(s: &ClaudeSession, val: &Value) -> Option<Output> {
    let msg_totals = s
        .sum_turn_usage()
        .or_else(|| val.get("usage").map(parse_claude_usage));
    let cost_usd = val.get("total_cost_usd").and_then(Value::as_f64);
    if msg_totals.is_none() && cost_usd.is_none() {
        return None;
    }
    let mut usage = Usage::new(UsageScope::TurnDelta, UsageScope::Cumulative);
    usage.model = s.turn_model.clone();
    if let Some(t) = msg_totals {
        usage.input = t.input;
        usage.output = t.output;
        usage.cache_read = t.cache_read;
        usage.cache_write = t.cache_write;
        usage.reasoning = t.reasoning;
    }
    if let Some(cost) = cost_usd {
        usage.cost_usd = Some(cost);
        usage.cost_basis = CostBasis::ListPriceEstimate;
    }
    Some(Output::Event(SessionEvent::Usage(usage)))
}

fn collect_result_denials(val: &Value, out: &mut Vec<Output>) {
    let Some(denials) = val.get("permission_denials").and_then(Value::as_array) else {
        return;
    };
    for denial in denials {
        let tool = denial
            .get("tool_name")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_owned();
        let tool_use_id = denial
            .get("tool_use_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let message = denial
            .get("message")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        out.push(Output::Event(SessionEvent::ActionDenied {
            tool,
            tool_use_id,
            message,
        }));
    }
}

fn map_result_outcome(val: &Value) -> (TurnOutcome, Vec<String>) {
    let subtype = val.get("subtype").and_then(Value::as_str).unwrap_or("");
    let is_error = val
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let terminal_reason = val
        .get("terminal_reason")
        .and_then(Value::as_str)
        .unwrap_or("");
    let stop_reason = val.get("stop_reason").and_then(Value::as_str).unwrap_or("");
    let result_text = val.get("result").and_then(Value::as_str).unwrap_or("");

    let mut errors = Vec::new();
    if let Some(arr) = val.get("errors").and_then(Value::as_array) {
        for item in arr {
            if let Some(s) = item.as_str() {
                errors.push(s.to_owned());
            }
        }
    }

    if matches!(terminal_reason, "aborted_tools" | "aborted_streaming") {
        return (TurnOutcome::Interrupted, errors);
    }
    if result_text.contains("Not logged in") || result_text.contains("Please run /login") {
        return (TurnOutcome::SignedOut, errors);
    }
    if stop_reason == "refusal" {
        return (TurnOutcome::Refused, errors);
    }
    if subtype == "error_max_turns" {
        return (
            TurnOutcome::Limit {
                kind: LimitKind::MaxTurns,
            },
            errors,
        );
    }
    if stop_reason == "max_tokens" {
        return (
            TurnOutcome::Limit {
                kind: LimitKind::MaxTokens,
            },
            errors,
        );
    }
    if subtype == "error_max_budget_usd" {
        return (TurnOutcome::Quota, errors);
    }
    if is_error || subtype.starts_with("error") {
        let reason = if !result_text.is_empty() {
            result_text.to_owned()
        } else if !errors.is_empty() {
            errors.join("; ")
        } else {
            format!("Claude turn failed ({subtype})")
        };
        return (TurnOutcome::Failed { reason }, errors);
    }
    (TurnOutcome::Completed, errors)
}
