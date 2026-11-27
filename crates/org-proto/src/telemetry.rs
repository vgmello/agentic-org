//! Secret-free per-turn and per-session efficiency telemetry wire contracts (`TASK-136`).

use serde::{Deserialize, Serialize};

use crate::sessions::SessionState;
use crate::Call;

/// `office.metrics`: owner/operator-only live efficiency telemetry snapshot for one office.
pub struct OfficeMetrics;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfficeMetricsParams {}

impl Call for OfficeMetrics {
    const METHOD: &'static str = "office.metrics";
    type Params = OfficeMetricsParams;
    type Result = OfficeMetricsResult;
}

/// One bounded, strictly secret-free turn telemetry sample.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnTelemetrySample {
    pub wall_latency_ms: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_read_tokens: u64,
    pub thought_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_hit_ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rss_bytes: Option<u64>,
    pub sandbox_denial_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    pub succeeded: bool,
    pub completed_at_ms: u64,
}

/// Per-agent live and recent efficiency telemetry returned by `office.metrics`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentMetricsView {
    pub employee_id: String,
    pub handle: String,
    pub name: String,
    pub state: SessionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_effort: Option<String>,
    pub recent_turn_count: u64,
    pub total_turn_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p50_wall_latency_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p95_wall_latency_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avg_wall_latency_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_wall_latency_ms: Option<u64>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_read_tokens: u64,
    pub thought_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_hit_ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_rss_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_rss_bytes: Option<u64>,
    pub sandbox_denial_count: u64,
}

/// Office-wide aggregate efficiency summary returned by `office.metrics`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OfficeAggregateMetrics {
    pub active_sessions: u64,
    pub idle_sessions: u64,
    pub recent_turn_count: u64,
    pub total_turn_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p50_wall_latency_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p95_wall_latency_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avg_wall_latency_ms: Option<u64>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_read_tokens: u64,
    pub thought_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_hit_ratio: Option<f64>,
    pub current_rss_bytes: u64,
    pub peak_rss_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rss_budget_bytes: Option<u64>,
    pub sandbox_denial_count: u64,
    pub hibernated_total: u64,
    pub rss_evicted_total: u64,
}

/// Response payload for `office.metrics`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OfficeMetricsResult {
    pub office_id: String,
    pub org_id: String,
    pub collected_at: String,
    pub aggregate: OfficeAggregateMetrics,
    pub agents: Vec<AgentMetricsView>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Request, Response};

    #[test]
    fn office_metrics_round_trips_and_rejects_unknown_params() {
        let req = Request::new::<OfficeMetrics>(7, &OfficeMetricsParams {}).unwrap();
        assert_eq!(req.method, "office.metrics");
        let parsed = req.params::<OfficeMetrics>().unwrap();
        assert_eq!(parsed, OfficeMetricsParams {});

        let bad = Request {
            id: 8,
            method: OfficeMetrics::METHOD.to_owned(),
            params: serde_json::json!({ "unexpected": true }),
            principal: None,
        };
        assert!(bad.params::<OfficeMetrics>().is_err());

        let result = OfficeMetricsResult {
            office_id: "off-1".into(),
            org_id: "org-1".into(),
            collected_at: "2026-11-27T20:00:00Z".into(),
            aggregate: OfficeAggregateMetrics {
                active_sessions: 1,
                idle_sessions: 2,
                recent_turn_count: 3,
                total_turn_count: 5,
                p50_wall_latency_ms: Some(1200),
                p95_wall_latency_ms: Some(2500),
                avg_wall_latency_ms: Some(1500),
                input_tokens: 1000,
                output_tokens: 400,
                cached_read_tokens: 3000,
                thought_tokens: 150,
                cache_hit_ratio: Some(0.75),
                current_rss_bytes: 64 * 1024 * 1024,
                peak_rss_bytes: 96 * 1024 * 1024,
                rss_budget_bytes: Some(512 * 1024 * 1024),
                sandbox_denial_count: 1,
                hibernated_total: 2,
                rss_evicted_total: 0,
            },
            agents: vec![AgentMetricsView {
                employee_id: "emp-1".into(),
                handle: "ada".into(),
                name: "Ada".into(),
                state: SessionState::Running,
                runtime_id: Some("claude".into()),
                active_model: Some("claude-sonnet-4-6".into()),
                active_effort: Some("high".into()),
                recent_turn_count: 3,
                total_turn_count: 5,
                p50_wall_latency_ms: Some(1200),
                p95_wall_latency_ms: Some(2500),
                avg_wall_latency_ms: Some(1500),
                last_wall_latency_ms: Some(1100),
                input_tokens: 1000,
                output_tokens: 400,
                cached_read_tokens: 3000,
                thought_tokens: 150,
                cache_hit_ratio: Some(0.75),
                current_rss_bytes: Some(64 * 1024 * 1024),
                peak_rss_bytes: Some(96 * 1024 * 1024),
                sandbox_denial_count: 1,
            }],
        };
        let resp = Response::ok::<OfficeMetrics>(7, &result).unwrap();
        let decoded = resp.into_result::<OfficeMetrics>().unwrap();
        assert_eq!(decoded, result);
    }
}
