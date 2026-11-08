//! How a `result` maps to a [`TurnOutcome`].
//!
//! From 2.1.311's schema (**\[binary\]**): `subtype` is `success` | `error_during_execution` |
//! `error_max_turns` | `error_max_budget_usd` | `error_max_structured_output_retries`;
//! `terminal_reason` is one of `completed`, `aborted_tools`, `aborted_streaming`,
//! `max_turns`, `budget_exhausted`, `prompt_too_long`, `model_error`, `api_error`, … ; an
//! API failure is `subtype: success` with `is_error: true`, `api_error_status` (HTTP) and
//! sometimes `api_error`. The spike recorded `success`/`completed` and
//! `error_during_execution` with `aborted_tools` and `aborted_streaming`; the rest of the
//! mapping follows the schema, unexercised.

use org_runtime::{LimitKind, TurnOutcome};
use serde_json::Value;

/// The outcome and error texts of a `result` message.
pub(crate) fn from_result(msg: &Value) -> (TurnOutcome, Vec<String>) {
    let text = |key: &str| msg.get(key).and_then(Value::as_str);
    let subtype = text("subtype");
    let terminal = text("terminal_reason");
    let is_error = msg.get("is_error").and_then(Value::as_bool) == Some(true);

    let mut errors: Vec<String> = msg
        .get("errors")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();

    let failed = |fallback: &str| TurnOutcome::Failed {
        reason: terminal.unwrap_or(fallback).to_owned(),
    };

    let outcome = match (subtype, terminal) {
        (_, Some("aborted_tools" | "aborted_streaming")) => TurnOutcome::Interrupted,
        (Some("error_max_turns"), _) | (_, Some("max_turns")) => TurnOutcome::Limit {
            kind: LimitKind::MaxTurns,
        },
        (Some("error_max_budget_usd"), _) | (_, Some("budget_exhausted")) => TurnOutcome::Limit {
            kind: LimitKind::Unknown,
        },
        (Some("error_during_execution"), _) => failed("error_during_execution"),
        (Some("error_max_structured_output_retries"), _) => {
            failed("error_max_structured_output_retries")
        }
        (Some("success"), _) if is_error => {
            // The error text is in `result` for an API failure.
            if let Some(t) = text("result").filter(|t| !t.is_empty()) {
                errors.push(t.to_owned());
            }
            match (
                msg.get("api_error_status").and_then(Value::as_i64),
                text("api_error"),
            ) {
                (_, Some("max_output_tokens")) => TurnOutcome::Limit {
                    kind: LimitKind::MaxTokens,
                },
                (Some(401), _) => TurnOutcome::SignedOut,
                (Some(429), _) => TurnOutcome::Quota,
                (_, Some(api_error)) => TurnOutcome::Failed {
                    reason: api_error.to_owned(),
                },
                _ => failed("api_error"),
            }
        }
        (Some("success"), _) => match text("stop_reason") {
            Some("refusal") => TurnOutcome::Refused,
            Some("max_tokens") => TurnOutcome::Limit {
                kind: LimitKind::MaxTokens,
            },
            _ => TurnOutcome::Completed,
        },
        _ => TurnOutcome::Unknown,
    };
    (outcome, errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn outcome(v: Value) -> TurnOutcome {
        from_result(&v).0
    }

    #[test]
    fn outcomes() {
        let ok = json!({"subtype": "success", "is_error": false, "terminal_reason": "completed",
            "stop_reason": "end_turn", "errors": null});
        assert_eq!(outcome(ok), TurnOutcome::Completed);
        for reason in ["aborted_tools", "aborted_streaming"] {
            let v = json!({"subtype": "error_during_execution", "is_error": true,
                "terminal_reason": reason, "errors": ["[ede_diagnostic] x"]});
            let (o, e) = from_result(&v);
            assert_eq!(o, TurnOutcome::Interrupted);
            assert_eq!(e, vec!["[ede_diagnostic] x".to_owned()]);
        }
        assert_eq!(
            outcome(json!({"subtype": "error_max_turns", "is_error": true})),
            TurnOutcome::Limit {
                kind: LimitKind::MaxTurns
            }
        );
        assert_eq!(
            outcome(json!({"subtype": "error_max_budget_usd", "is_error": true})),
            TurnOutcome::Limit {
                kind: LimitKind::Unknown
            }
        );
        assert_eq!(
            outcome(
                json!({"subtype": "error_during_execution", "terminal_reason": "prompt_too_long"})
            ),
            TurnOutcome::Failed {
                reason: "prompt_too_long".into()
            }
        );
        assert_eq!(
            outcome(json!({"subtype": "success", "stop_reason": "refusal", "is_error": false})),
            TurnOutcome::Refused
        );
        assert_eq!(
            outcome(json!({"subtype": "success", "stop_reason": "max_tokens", "is_error": false})),
            TurnOutcome::Limit {
                kind: LimitKind::MaxTokens
            }
        );
        let (o, e) = from_result(&json!({"subtype": "success", "is_error": true,
            "api_error_status": 429, "result": "API Error: 429 rate limited"}));
        assert_eq!(o, TurnOutcome::Quota);
        assert_eq!(e, vec!["API Error: 429 rate limited".to_owned()]);
        assert_eq!(
            outcome(json!({"subtype": "success", "is_error": true, "api_error_status": 401})),
            TurnOutcome::SignedOut
        );
        assert_eq!(
            outcome(
                json!({"subtype": "success", "is_error": true, "api_error_status": 500,
                "api_error": "model_error"})
            ),
            TurnOutcome::Failed {
                reason: "model_error".into()
            }
        );
        assert_eq!(
            outcome(json!({"subtype": "error_paused"})),
            TurnOutcome::Unknown
        );
        assert_eq!(outcome(json!({})), TurnOutcome::Unknown);
    }
}
