//! Unlocked office-wide and per-agent efficiency metrics aggregation (`office.metrics`, `TASK-136`).

use std::collections::{BTreeSet, HashMap};

use org_office::organization_family::records::{Employee, EmployeeKind, EmployeeStatus};
use org_proto::sessions::SessionState;
use org_proto::telemetry::{AgentMetricsView, OfficeAggregateMetrics, OfficeMetricsResult};
use org_proto::ApiError;

use crate::credentials::rfc3339_now;
use crate::redact::redact_text;
use crate::telemetry::{
    average_u64, compute_cache_hit_ratio, percentile_u64, AgentTelemetrySnapshot,
};

use super::SessionManager;

struct LiveSessionSummary {
    state: SessionState,
    runtime_id: Option<String>,
    active_model: Option<String>,
    active_effort: Option<String>,
}

impl SessionManager {
    /// Returns live per-office aggregate + per-agent efficiency telemetry (`office.metrics`).
    /// Owner-only: non-owners and agents fail closed with `NotAuthorized`.
    pub fn office_metrics_snapshot(
        &self,
        caller_id: &str,
    ) -> Result<OfficeMetricsResult, ApiError> {
        self.office_metrics_snapshot_inner(caller_id, false)
    }

    pub(crate) fn office_metrics_snapshot_inner(
        &self,
        caller_id: &str,
        hq_attested_owner: bool,
    ) -> Result<OfficeMetricsResult, ApiError> {
        self.authorize_owner_only_inner(caller_id, hq_attested_owner)?;

        // 1. Sample live RSS with zero coarse locks held.
        let rss_snap = self.office_session_rss_snapshot();
        let mut live_rss_by_emp: HashMap<String, u64> = HashMap::new();
        for entry in &rss_snap.per_session_rss {
            if entry.rss_bytes > 0 {
                self.telemetry
                    .record_rss_sample(&entry.employee_id, entry.rss_bytes);
            }
            live_rss_by_emp.insert(entry.employee_id.clone(), entry.rss_bytes);
        }
        self.telemetry
            .record_office_rss_total(rss_snap.total_rss_bytes);

        // 2. Brief lock on `registry` to snapshot live session states + active model/effort.
        let live_handles: HashMap<String, LiveSessionSummary> = {
            let reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
            reg.live
                .iter()
                .map(|(emp_id, handle)| {
                    let st = handle.state.lock().unwrap_or_else(|p| p.into_inner());
                    (
                        emp_id.clone(),
                        LiveSessionSummary {
                            state: st.state,
                            runtime_id: st.runtime_id.clone(),
                            active_model: handle.active_model.clone(),
                            active_effort: handle.active_effort.clone(),
                        },
                    )
                })
                .collect()
        };

        // 3. Brief lock on `telemetry` to clone bounded numeric/metadata summaries.
        let (office_peak_rss_bytes, telemetry_map) = self.telemetry.snapshot_all();

        // 4. Unlocked office metadata read for active agents (handles, names, default model/effort).
        let employees: HashMap<String, Employee> = self
            .office
            .list_employees()
            .unwrap_or_default()
            .into_iter()
            .filter(|e| e.kind == EmployeeKind::Agent)
            .map(|e| (e.id.as_str().to_owned(), e))
            .collect();

        let mut agent_ids: BTreeSet<String> = BTreeSet::new();
        for (id, emp) in &employees {
            if emp.status != EmployeeStatus::Inactive {
                agent_ids.insert(id.clone());
            }
        }
        agent_ids.extend(live_handles.keys().cloned());
        agent_ids.extend(telemetry_map.keys().cloned());

        let mut agents = Vec::with_capacity(agent_ids.len());
        let mut all_recent_latencies: Vec<u64> = Vec::new();
        let mut agg = OfficeAggregateMetrics {
            current_rss_bytes: rss_snap.total_rss_bytes,
            peak_rss_bytes: office_peak_rss_bytes.max(rss_snap.total_rss_bytes),
            rss_budget_bytes: rss_snap.budget_bytes,
            hibernated_total: rss_snap.hibernated_total,
            rss_evicted_total: rss_snap.rss_evicted_total,
            ..Default::default()
        };

        for emp_id in agent_ids {
            let live = live_handles.get(&emp_id);
            let tele = telemetry_map.get(&emp_id);
            let emp = employees.get(&emp_id);
            let live_rss = live_rss_by_emp.get(&emp_id).copied();
            let view = self.build_agent_metrics_view(
                &emp_id,
                emp,
                live,
                tele,
                live_rss,
                &mut all_recent_latencies,
            );

            match view.state {
                SessionState::Starting | SessionState::Running | SessionState::WaitingApproval => {
                    agg.active_sessions = agg.active_sessions.saturating_add(1);
                }
                SessionState::Idle if live.is_some() => {
                    agg.idle_sessions = agg.idle_sessions.saturating_add(1);
                }
                _ => {}
            }
            agg.recent_turn_count = agg.recent_turn_count.saturating_add(view.recent_turn_count);
            agg.total_turn_count = agg.total_turn_count.saturating_add(view.total_turn_count);
            agg.input_tokens = agg.input_tokens.saturating_add(view.input_tokens);
            agg.output_tokens = agg.output_tokens.saturating_add(view.output_tokens);
            agg.cached_read_tokens = agg
                .cached_read_tokens
                .saturating_add(view.cached_read_tokens);
            agg.thought_tokens = agg.thought_tokens.saturating_add(view.thought_tokens);
            agg.sandbox_denial_count = agg
                .sandbox_denial_count
                .saturating_add(view.sandbox_denial_count);
            agents.push(view);
        }

        agents.sort_by(|a, b| (&a.handle, &a.employee_id).cmp(&(&b.handle, &b.employee_id)));
        all_recent_latencies.sort_unstable();
        agg.p50_wall_latency_ms = percentile_u64(&all_recent_latencies, 50);
        agg.p95_wall_latency_ms = percentile_u64(&all_recent_latencies, 95);
        agg.avg_wall_latency_ms = average_u64(&all_recent_latencies);
        agg.cache_hit_ratio = compute_cache_hit_ratio(agg.input_tokens, agg.cached_read_tokens);

        let info = self.office.info();
        Ok(OfficeMetricsResult {
            office_id: info.office_id.clone(),
            org_id: info.org_id.clone(),
            collected_at: rfc3339_now(),
            aggregate: agg,
            agents,
        })
    }

    fn build_agent_metrics_view(
        &self,
        emp_id: &str,
        emp: Option<&Employee>,
        live: Option<&LiveSessionSummary>,
        tele: Option<&AgentTelemetrySnapshot>,
        live_rss: Option<u64>,
        office_latencies: &mut Vec<u64>,
    ) -> AgentMetricsView {
        let exec = self.office.load_agent_execution(emp_id).ok().flatten();
        let default_model = exec.as_ref().map(|c| c.model_id.clone());
        let default_effort = exec
            .as_ref()
            .and_then(|c| c.effort.map(|e| e.as_str().to_owned()));

        let mut latencies: Vec<u64> = tele
            .map(|t| t.recent_turns.iter().map(|s| s.wall_latency_ms).collect())
            .unwrap_or_default();
        let last_wall_latency_ms = latencies.last().copied();
        office_latencies.extend(latencies.iter().copied());
        latencies.sort_unstable();

        let input_tokens = tele.map_or(0, |t| t.input_tokens);
        let output_tokens = tele.map_or(0, |t| t.output_tokens);
        let cached_read_tokens = tele.map_or(0, |t| t.cached_read_tokens);
        let thought_tokens = tele.map_or(0, |t| t.thought_tokens);
        let sandbox_denial_count = tele.map_or(0, |t| t.sandbox_denial_count);
        let recent_turn_count = tele.map_or(0, |t| t.recent_turns.len() as u64);
        let total_turn_count = tele.map_or(0, |t| t.total_turn_count);

        let current_rss_bytes = live.and_then(|_| live_rss.filter(|&b| b > 0));
        let peak_rss_bytes = match (tele.and_then(|t| t.peak_rss_bytes), current_rss_bytes) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };

        let active_model = live
            .and_then(|l| l.active_model.clone())
            .or_else(|| tele.and_then(|t| t.active_model.clone()))
            .or(default_model)
            .map(|m| redact_text(&m, &[]));
        let active_effort = live
            .and_then(|l| l.active_effort.clone())
            .or_else(|| tele.and_then(|t| t.active_effort.clone()))
            .or(default_effort)
            .map(|e| redact_text(&e, &[]));
        let runtime_id = live
            .and_then(|l| l.runtime_id.clone())
            .or_else(|| tele.and_then(|t| t.runtime_id.clone()));

        let handle = emp
            .map(|e| redact_text(&e.handle, &[]))
            .unwrap_or_else(|| emp_id.to_owned());
        let name = emp
            .map(|e| redact_text(&e.name, &[]))
            .unwrap_or_else(|| emp_id.to_owned());

        AgentMetricsView {
            employee_id: emp_id.to_owned(),
            handle,
            name,
            state: live.map_or(SessionState::Idle, |l| l.state),
            runtime_id,
            active_model,
            active_effort,
            recent_turn_count,
            total_turn_count,
            p50_wall_latency_ms: percentile_u64(&latencies, 50),
            p95_wall_latency_ms: percentile_u64(&latencies, 95),
            avg_wall_latency_ms: average_u64(&latencies),
            last_wall_latency_ms,
            input_tokens,
            output_tokens,
            cached_read_tokens,
            thought_tokens,
            cache_hit_ratio: compute_cache_hit_ratio(input_tokens, cached_read_tokens),
            current_rss_bytes,
            peak_rss_bytes,
            sandbox_denial_count,
        }
    }
}
