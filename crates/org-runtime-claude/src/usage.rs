//! One turn's token usage, summed from the messages rather than read from `result`.
//!
//! Claude repeats a message's `usage` on each of its content blocks (one `assistant` event
//! per block), as a `message_start` snapshot whose `output_tokens` undercounts. The final
//! count arrives in the message's `message_delta` (`--include-partial-messages`). A turn
//! aborted mid-stream reports all zeros in `result.usage` (spike fact 15), so the session
//! sums the messages, de-duplicated by message id, with the final counts from
//! `message_delta` where it came. The cost is `result.model_usage_since_last_result`'s,
//! per model, where the result gives one (it's `{}` for an aborted stream).

use std::collections::BTreeMap;

use org_runtime::{CostBasis, Usage, UsageScope};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Default, PartialEq)]
struct MessageUsage {
    model: Option<String>,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
    reasoning: u64,
    /// The counts are `message_delta`'s final ones: no snapshot may change them.
    is_final: bool,
}

/// The usage of the messages seen so far in one turn.
#[derive(Debug, Clone, Default)]
pub(crate) struct TurnUsage {
    /// By message id, in the order first seen.
    messages: Vec<(String, MessageUsage)>,
    /// The message each stream (main thread, or a subagent's `parent_tool_use_id`) is in.
    streaming: Vec<(Option<String>, String)>,
}

impl TurnUsage {
    /// An `assistant` event's `message`: its usage snapshot, the first time its id is seen.
    pub fn on_assistant(&mut self, message: &Value) {
        let Some(id) = message.get("id").and_then(Value::as_str) else {
            return;
        };
        let model = text(message, "model");
        let snapshot = message.get("usage");
        let entry = self.entry(id);
        if entry.model.is_none() {
            entry.model = model;
        }
        if let Some(usage) = snapshot {
            if !entry.is_final {
                apply(entry, usage);
            }
        }
    }

    /// A `stream_event` `message_start`: the stream is in this message now.
    pub fn on_message_start(&mut self, parent: Option<&str>, message: &Value) {
        let Some(id) = message.get("id").and_then(Value::as_str) else {
            return;
        };
        self.on_assistant(message);
        let parent = parent.map(str::to_owned);
        self.streaming.retain(|(p, _)| *p != parent);
        self.streaming.push((parent, id.to_owned()));
    }

    /// A `stream_event` `message_delta`: the message's final counts. `message_id` is the
    /// event's `api_message_id`, or else the stream's current message.
    pub fn on_message_delta(
        &mut self,
        parent: Option<&str>,
        message_id: Option<&str>,
        usage: &Value,
    ) {
        let id = match message_id {
            Some(id) => id.to_owned(),
            None => match self.streaming.iter().find(|(p, _)| p.as_deref() == parent) {
                Some((_, id)) => id.clone(),
                None => return,
            },
        };
        let entry = self.entry(&id);
        apply(entry, usage);
        if let Some(thinking) = usage
            .pointer("/output_tokens_details/thinking_tokens")
            .and_then(Value::as_u64)
        {
            entry.reasoning = thinking;
        }
        entry.is_final = true;
    }

    /// The turn's usage, one [`Usage`] per model (by model name), each a delta. `costs` is
    /// the result's `model_usage_since_last_result`, if the turn had a result.
    pub fn finish(self, costs: Option<&Map<String, Value>>) -> Vec<Usage> {
        let mut by_model: BTreeMap<Option<String>, MessageUsage> = BTreeMap::new();
        for (_, m) in self.messages {
            let total = by_model.entry(m.model.clone()).or_default();
            total.input += m.input;
            total.output += m.output;
            total.cache_read += m.cache_read;
            total.cache_write += m.cache_write;
            total.reasoning += m.reasoning;
        }
        by_model
            .into_iter()
            .map(|(model, t)| {
                let entry = model.as_deref().and_then(|m| costs.and_then(|c| c.get(m)));
                let cost_usd = entry
                    .and_then(|e| e.get("costUSD"))
                    .and_then(Value::as_f64)
                    .filter(|c| c.is_finite());
                let cost_basis = match (cost_usd, entry.and_then(|e| text(e, "costBasis"))) {
                    (None, _) => CostBasis::None,
                    (Some(_), Some(basis)) if basis == "list" => CostBasis::ListPriceEstimate,
                    // 2.1.311 always says "list"; anything else is a basis we don't know.
                    (Some(_), Some(_)) => CostBasis::Unknown,
                    (Some(_), None) => CostBasis::ListPriceEstimate,
                };
                Usage {
                    model,
                    input: t.input,
                    output: t.output,
                    cache_read: t.cache_read,
                    cache_write: t.cache_write,
                    reasoning: t.reasoning,
                    cost_usd,
                    cost_basis,
                    scope: UsageScope::TurnDelta,
                }
            })
            .collect()
    }

    fn entry(&mut self, id: &str) -> &mut MessageUsage {
        let index = match self.messages.iter().position(|(i, _)| i == id) {
            Some(index) => index,
            None => {
                self.messages.push((id.to_owned(), MessageUsage::default()));
                self.messages.len() - 1
            }
        };
        &mut self.messages[index].1
    }
}

/// Takes the counts a `usage` object has. Counts only grow within a message, so a snapshot
/// never lowers one.
fn apply(entry: &mut MessageUsage, usage: &Value) {
    let count = |key: &str| usage.get(key).and_then(Value::as_u64);
    for (field, key) in [
        (&mut entry.input, "input_tokens"),
        (&mut entry.output, "output_tokens"),
        (&mut entry.cache_read, "cache_read_input_tokens"),
        (&mut entry.cache_write, "cache_creation_input_tokens"),
    ] {
        if let Some(n) = count(key) {
            *field = (*field).max(n);
        }
    }
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn msg(id: &str, out: u64) -> Value {
        json!({"id": id, "model": "m1", "usage": {"input_tokens": 2, "output_tokens": out,
            "cache_read_input_tokens": 100, "cache_creation_input_tokens": 7}})
    }

    #[test]
    fn de_duplicates_blocks_and_takes_final_counts_from_the_delta() {
        let mut u = TurnUsage::default();
        u.on_message_start(None, &msg("a", 2));
        u.on_assistant(&msg("a", 2)); // thinking block
        u.on_assistant(&msg("a", 2)); // text block, same message
        u.on_message_delta(
            None,
            None,
            &json!({"input_tokens": 2, "output_tokens": 87,
                "output_tokens_details": {"thinking_tokens": 40}}),
        );
        u.on_assistant(&msg("a", 2)); // a late snapshot can't lower the final count
        u.on_assistant(&msg("b", 5)); // a second message, no stream
        let costs = json!({"m1": {"costUSD": 0.5, "costBasis": "list"}});
        let usage = u.finish(costs.as_object());
        assert_eq!(
            usage,
            vec![Usage {
                model: Some("m1".into()),
                input: 4,
                output: 92,
                cache_read: 200,
                cache_write: 14,
                reasoning: 40,
                cost_usd: Some(0.5),
                cost_basis: CostBasis::ListPriceEstimate,
                scope: UsageScope::TurnDelta,
            }]
        );
    }

    #[test]
    fn no_cost_without_a_result_entry() {
        let mut u = TurnUsage::default();
        u.on_assistant(&msg("a", 3));
        let usage = u.finish(Some(&Map::new()));
        assert_eq!(usage[0].cost_usd, None);
        assert_eq!(usage[0].cost_basis, CostBasis::None);
        assert!(TurnUsage::default().finish(None).is_empty());
    }

    #[test]
    fn per_model_and_per_stream() {
        let mut u = TurnUsage::default();
        u.on_message_start(None, &msg("main", 1));
        let mut sub = msg("sub", 1);
        sub["model"] = json!("m2");
        u.on_message_start(Some("toolu_1"), &sub);
        u.on_message_delta(Some("toolu_1"), None, &json!({"output_tokens": 9}));
        u.on_message_delta(None, None, &json!({"output_tokens": 4}));
        let usage = u.finish(None);
        assert_eq!(usage.len(), 2);
        assert_eq!(
            (usage[0].model.as_deref(), usage[0].output),
            (Some("m1"), 4)
        );
        assert_eq!(
            (usage[1].model.as_deref(), usage[1].output),
            (Some("m2"), 9)
        );
    }
}
