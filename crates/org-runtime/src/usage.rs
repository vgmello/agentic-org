//! Token usage and rate limits, normalized across runtimes.

use serde::{Deserialize, Serialize};

use crate::event::redact_opt;

/// Token usage, and its cost where the runtime estimates one.
///
/// Adapters normalize their runtime's reporting into it. Claude, for instance, reports usage
/// per message (repeated across a message's content blocks, and zero in the turn result of an
/// aborted turn), so its adapter sums the messages de-duplicated by message id and takes the
/// final output count from the closing stream delta.
///
/// ## Rules
/// - **Usage comes before `TurnEnded`.** A session emits all of a turn's usage before that
///   turn's [`crate::SessionEvent::TurnEnded`], so the office can close the run's numbers
///   when the turn ends.
/// - **Tokens and cost have separate scopes** ([`token_scope`](Self::token_scope),
///   [`cost_scope`](Self::cost_scope)): a runtime may count tokens per message but cost as a
///   running total (Claude's `total_cost_usd`).
/// - **No running totals that survive the process.** [`UsageScope::Cumulative`] means "since
///   this process started", and the office takes differences. A runtime whose totals run
///   across a resume (Antigravity's per-conversation counts) is converted by its adapter into
///   [`UsageScope::TurnDelta`]s from a baseline:
///   - for a **new conversation** the baseline is 0, so the first turn counts in full;
///   - for a **resume** it's the total the runtime reports when the conversation loads,
///     before any turn. If the runtime reports none then, the adapter can't tell the first
///     turn's share: it takes that turn's total as the baseline, so **the first turn after
///     such a resume isn't counted**, and says so with a [`crate::SessionEvent::Notice`]
///     warning.
///
/// It's `#[non_exhaustive]` so it can grow: build one with [`Usage::new`], then set fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Usage {
    /// The model the usage is for, where the runtime says.
    pub model: Option<String>,
    /// Input tokens, not counting cache reads.
    pub input: u64,
    /// Output tokens, reasoning included.
    pub output: u64,
    /// Input tokens read from the prompt cache.
    pub cache_read: u64,
    /// Input tokens written to the prompt cache.
    pub cache_write: u64,
    /// Of `output`, the reasoning tokens, where the runtime splits them out.
    pub reasoning: u64,
    /// The cost in US dollars, where the runtime gives one. See
    /// [`cost_basis`](Self::cost_basis).
    pub cost_usd: Option<f64>,
    /// What the cost means.
    pub cost_basis: CostBasis,
    /// Whether the token counts are a delta or a running total since the process started.
    pub token_scope: UsageScope,
    /// Whether the cost is a delta or a running total since the process started.
    pub cost_scope: UsageScope,
}

impl Usage {
    /// No tokens and no cost yet, with these scopes; set the counts after.
    pub fn new(token_scope: UsageScope, cost_scope: UsageScope) -> Self {
        Usage {
            model: None,
            input: 0,
            output: 0,
            cache_read: 0,
            cache_write: 0,
            reasoning: 0,
            cost_usd: None,
            cost_basis: CostBasis::None,
            token_scope,
            cost_scope,
        }
    }

    pub(crate) fn redact(&mut self, f: &mut dyn FnMut(&mut String)) {
        redact_opt(&mut self.model, f);
    }
}

/// What a cost figure means. Unknown values decode as [`CostBasis::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CostBasis {
    /// The runtime's estimate at list prices; not what the account is billed.
    ListPriceEstimate,
    /// The runtime gives no cost; the office prices the tokens itself, if at all.
    None,
    /// Not known to this build.
    #[serde(other)]
    Unknown,
}

/// Whether usage numbers are a delta or a running total (see [`Usage`]'s rules). Unknown
/// values decode as [`UsageScope::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum UsageScope {
    /// Added to what came before: one turn's (or one message's) usage.
    TurnDelta,
    /// The running total since **this process** started: the office takes the difference
    /// from the previous total of the same process. Never a total that survives a resume.
    Cumulative,
    /// Not known to this build.
    #[serde(other)]
    Unknown,
}

/// One rate-limit window's state, as the runtime reports it in-stream or a quota probe reads.
///
/// It's `#[non_exhaustive]` so it can grow: build one with [`RateLimit::new`], then set the
/// optional fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RateLimit {
    /// Which window.
    pub window: Window,
    /// How much of it is used, from 0.0 to 1.0, where the runtime says.
    pub used_fraction: Option<f64>,
    /// When it resets: RFC 3339 in UTC (e.g. `2026-11-08T05:00:00Z`), converted by the
    /// adapter from whatever the runtime sends.
    pub resets_at: Option<String>,
    /// Whether requests still go through.
    pub status: LimitStatus,
}

impl RateLimit {
    /// A window's state, with no usage fraction or reset time yet.
    pub fn new(window: Window, status: LimitStatus) -> Self {
        RateLimit {
            window,
            used_fraction: None,
            resets_at: None,
            status,
        }
    }

    pub(crate) fn redact(&mut self, f: &mut dyn FnMut(&mut String)) {
        let RateLimit {
            window,
            used_fraction: _,
            resets_at,
            status: _,
        } = self;
        match window {
            Window::Other { id, mins: _ } => f(id),
            Window::FiveHour | Window::SevenDay | Window::Unknown => {}
        }
        redact_opt(resets_at, f);
    }
}

/// A rate-limit window. Unknown kinds decode as [`Window::Unknown`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Window {
    /// The rolling five-hour window.
    FiveHour,
    /// The rolling seven-day window.
    SevenDay,
    /// Another window the runtime names.
    Other {
        /// The runtime's name for it.
        id: String,
        /// Its length in minutes, where the runtime says.
        mins: Option<u64>,
    },
    /// Not known to this build.
    #[serde(other)]
    Unknown,
}

/// Whether a rate-limit window still lets requests through. Unknown values decode as
/// [`LimitStatus::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum LimitStatus {
    /// Fine.
    Ok,
    /// Close to the limit.
    Warning,
    /// Over the limit: requests are refused.
    Rejected,
    /// Not known to this build.
    #[serde(other)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn usage_and_rate_limit_round_trip() {
        let usage = Usage {
            model: None,
            input: 10,
            output: 20,
            cache_read: 30,
            cache_write: 40,
            reasoning: 5,
            cost_usd: None,
            cost_basis: CostBasis::None,
            token_scope: UsageScope::TurnDelta,
            cost_scope: UsageScope::Cumulative,
        };
        let json = serde_json::to_value(&usage).unwrap();
        assert_eq!(json["cost_basis"], json!("none"));
        assert_eq!(json["token_scope"], json!("turn_delta"));
        assert_eq!(json["cost_scope"], json!("cumulative"));
        let mut built = Usage::new(UsageScope::TurnDelta, UsageScope::Cumulative);
        built.input = 10;
        built.output = 20;
        built.cache_read = 30;
        built.cache_write = 40;
        built.reasoning = 5;
        assert_eq!(built, usage);
        assert_eq!(serde_json::from_value::<Usage>(json).unwrap(), usage);

        for window in [
            Window::FiveHour,
            Window::SevenDay,
            Window::Other {
                id: "x".into(),
                mins: None,
            },
        ] {
            let limit = RateLimit {
                window,
                used_fraction: Some(0.9),
                resets_at: None,
                status: LimitStatus::Rejected,
            };
            let json = serde_json::to_string(&limit).unwrap();
            assert_eq!(serde_json::from_str::<RateLimit>(&json).unwrap(), limit);
        }
        assert_eq!(
            serde_json::to_value(Window::FiveHour).unwrap(),
            json!({"type": "five_hour"})
        );
    }

    #[test]
    fn unknown_values_decode_as_unknown() {
        assert_eq!(
            serde_json::from_str::<CostBasis>(r#""billed""#).unwrap(),
            CostBasis::Unknown
        );
        assert_eq!(
            serde_json::from_str::<UsageScope>(r#""per_message""#).unwrap(),
            UsageScope::Unknown
        );
        assert_eq!(
            serde_json::from_str::<LimitStatus>(r#""throttled""#).unwrap(),
            LimitStatus::Unknown
        );
        assert_eq!(
            serde_json::from_value::<Window>(json!({"type": "one_day", "mins": 1440})).unwrap(),
            Window::Unknown
        );
        let limit: RateLimit = serde_json::from_value(json!({
            "window": {"type": "monthly"}, "used_fraction": null, "resets_at": null,
            "status": "soft_blocked"
        }))
        .unwrap();
        assert_eq!(limit.window, Window::Unknown);
        assert_eq!(limit.status, LimitStatus::Unknown);
    }
}
