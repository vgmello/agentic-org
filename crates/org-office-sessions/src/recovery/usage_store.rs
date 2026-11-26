//! Durable usage capture: staging, FIFO overflow flush, watermarks, and rejected-usage
//! dead letters (`TASK-97`).

use super::*;

impl SessionStore {
    /// Atomically stages a raw usage observation (and any older overflow observations first, in
    /// strict FIFO order) into the single durable `usage-capture.v1.json` file that holds BOTH
    /// per-run cumulative watermarks AND normalized pending payloads (`TASK-97` / Final Gate
    /// Pass 13 #1/#2/#3).
    ///
    /// Because `watermarks` and `pending` are updated in ONE atomic rename + `full_sync` +
    /// ancestor-dir sync under `self.lock`, live capture and the background wake loop can never
    /// diverge, double-count on a partial rename/sync failure, or skip an older observation.
    pub fn stage_usage_observation(
        &self,
        employee_id: &str,
        entry: DeferredUsageEntry,
    ) -> io::Result<()> {
        let mut overflow = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let queue = overflow.entry(employee_id.to_owned()).or_default();
        if !queue
            .iter()
            .any(|existing| existing.run_id == entry.run_id && existing.seq == entry.seq)
        {
            queue.push_back(entry);
        }
        self.flush_overflow_locked(employee_id, &mut overflow)
    }

    /// Attempts to stage any in-memory overflow entries for `employee_id` in strict FIFO order
    /// into `usage-capture.v1.json`, stopping immediately on any persistence error.
    pub fn flush_overflow_usage(&self, employee_id: &str) -> io::Result<()> {
        let mut overflow = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        self.flush_overflow_locked(employee_id, &mut overflow)
    }

    /// Returns the number of unpersisted usage observations still buffered in memory for
    /// `employee_id` (non-zero only when disk staging failed and has not yet recovered).
    pub fn pending_usage_overflow_count(&self, employee_id: &str) -> usize {
        self.lock
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(employee_id)
            .map_or(0, std::collections::VecDeque::len)
    }

    pub(super) fn flush_overflow_locked(
        &self,
        employee_id: &str,
        overflow: &mut std::collections::BTreeMap<
            String,
            std::collections::VecDeque<DeferredUsageEntry>,
        >,
    ) -> io::Result<()> {
        let Some(queue) = overflow.get_mut(employee_id) else {
            return Ok(());
        };
        if queue.is_empty() {
            overflow.remove(employee_id);
            return Ok(());
        }
        let path = self.usage_capture_path(employee_id)?;
        let dir = self.employee_dir(employee_id)?;
        let mut state: UsageCaptureState = read_optional_json(&path)?.unwrap_or_default();
        while let Some(front) = queue.front() {
            let existing_max = state.watermarks.get(&front.run_id).map_or(0, |w| w.max_seq);
            if front.seq <= existing_max {
                // Rename may have succeeded on a prior attempt before `sync_dir` failed: ensure
                // ancestor dirs and file are durably synced before popping from overflow.
                ensure_owner_dirs_synced(&self.sessions_root, &dir)?;
                if path.is_file() {
                    let f = fs::File::open(&path)?;
                    org_platform::fs::full_sync(&f)?;
                }
                queue.pop_front();
                continue;
            }
            let next_order = state
                .watermarks
                .values()
                .map(|w| w.order)
                .max()
                .unwrap_or(0)
                .saturating_add(1);
            let mut next_state = state.clone();
            let wm = next_state
                .watermarks
                .entry(front.run_id.clone())
                .or_default();
            let params = normalize_usage_observation(employee_id, front, wm);
            wm.max_seq = wm.max_seq.max(front.seq);
            wm.order = next_order;
            if !next_state.pending.iter().any(|p| {
                p.producer_id == params.producer_id && p.source_event_id == params.source_event_id
            }) {
                next_state.pending.push(params);
            }
            ensure_owner_dirs_synced(&self.sessions_root, &dir)?;
            write_atomic_json_durable(&path, &next_state)?;
            state = next_state;
            queue.pop_front();
        }
        if queue.is_empty() {
            overflow.remove(employee_id);
        }
        Ok(())
    }

    pub fn read_pending_usage(
        &self,
        employee_id: &str,
    ) -> io::Result<Vec<org_proto::costs::CostUsageRecordParams>> {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let state: UsageCaptureState =
            read_optional_json(&self.usage_capture_path(employee_id)?)?.unwrap_or_default();
        Ok(state.pending)
    }

    pub fn remove_pending_usage_by_keys(
        &self,
        employee_id: &str,
        acked: &[(String, String)],
    ) -> io::Result<()> {
        if acked.is_empty() {
            return Ok(());
        }
        let overflow = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let path = self.usage_capture_path(employee_id)?;
        let Some(mut state): Option<UsageCaptureState> = read_optional_json(&path)? else {
            return Ok(());
        };
        state.pending.retain(|item| {
            !acked
                .iter()
                .any(|(p, e)| p == &item.producer_id && e == &item.source_event_id)
        });
        let mut protected_runs = self.live_and_queued_runs_locked(employee_id, &state.pending)?;
        if let Some(q) = overflow.get(employee_id) {
            for item in q {
                protected_runs.insert(item.run_id.clone());
            }
        }
        prune_watermarks(&mut state.watermarks, &protected_runs);
        let dir = self.employee_dir(employee_id)?;
        ensure_owner_dirs_synced(&self.sessions_root, &dir)?;
        write_atomic_json_durable(&path, &state)
    }

    pub(super) fn live_and_queued_runs_locked(
        &self,
        employee_id: &str,
        pending_remaining: &[org_proto::costs::CostUsageRecordParams],
    ) -> io::Result<std::collections::BTreeSet<String>> {
        let mut protected = std::collections::BTreeSet::new();
        if let Ok(Some(active)) =
            read_optional_json::<ActiveTurnRecord>(&self.active_turn_path(employee_id)?)
        {
            protected.insert(active.run_id);
        }
        if let Ok(Some(pending_start)) =
            read_optional_json::<PendingStartRecord>(&self.pending_start_path(employee_id)?)
        {
            protected.insert(pending_start.run_id);
        }
        for item in pending_remaining {
            if let Some(run_id) = item.run_id.as_deref() {
                protected.insert(run_id.to_owned());
            }
        }
        Ok(protected)
    }

    pub fn append_rejected_usage(
        &self,
        employee_id: &str,
        entry: &RejectedUsageEntry,
    ) -> io::Result<()> {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let path = self.rejected_usage_path(employee_id)?;
        let mut items: Vec<RejectedUsageEntry> = read_optional_json(&path)?.unwrap_or_default();
        if !items.iter().any(|existing| {
            existing.params.producer_id == entry.params.producer_id
                && existing.params.source_event_id == entry.params.source_event_id
        }) {
            items.push(entry.clone());
        }
        let dir = self.employee_dir(employee_id)?;
        ensure_owner_dirs_synced(&self.sessions_root, &dir)?;
        write_atomic_json_durable(&path, &items)
    }
}

/// Atomic single-file durable state for session usage capture (`usage-capture.v1.json`) pairing
/// per-run cumulative watermarks with the normalized pending submission queue (`Pass 13`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageCaptureState {
    #[serde(default)]
    pub watermarks: std::collections::BTreeMap<String, UsageWatermarkRecord>,
    #[serde(default)]
    pub pending: Vec<org_proto::costs::CostUsageRecordParams>,
}

/// Persisted per-run cumulative watermark stored atomically alongside pending usage records.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageWatermarkRecord {
    #[serde(default)]
    pub tokens: [u64; 5],
    #[serde(default)]
    pub cost_micro_usd: u64,
    #[serde(default)]
    pub max_seq: u64,
    #[serde(default)]
    pub order: u64,
}

/// Raw usage observation with receipt timestamp (`occurred_at`) captured once before staging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeferredUsageEntry {
    pub run_id: String,
    pub seq: u64,
    pub occurred_at: String,
    pub usage: org_agent_runtime::Usage,
}

/// Computes normalized incremental usage deltas from `wm`, advances `wm` in place, and builds
/// the immutable `CostUsageRecordParams` carrying the original receipt `occurred_at`.
pub(super) fn normalize_usage_observation(
    emp_id: &str,
    obs: &DeferredUsageEntry,
    wm: &mut UsageWatermarkRecord,
) -> org_proto::costs::CostUsageRecordParams {
    use org_agent_runtime::{CostBasis, UsageScope};
    use org_proto::costs::{
        AttributionBasis, CostUsageRecordParams, UsageActorKind, UsageScopeKind,
    };
    let usage = &obs.usage;
    let (in_tok, out_tok, cr_tok, cw_tok, reas_tok) = match usage.token_scope {
        UsageScope::Cumulative => {
            let d = (
                usage.input.saturating_sub(wm.tokens[0]),
                usage.output.saturating_sub(wm.tokens[1]),
                usage.cache_read.saturating_sub(wm.tokens[2]),
                usage.cache_write.saturating_sub(wm.tokens[3]),
                usage.reasoning.saturating_sub(wm.tokens[4]),
            );
            wm.tokens[0] = wm.tokens[0].max(usage.input);
            wm.tokens[1] = wm.tokens[1].max(usage.output);
            wm.tokens[2] = wm.tokens[2].max(usage.cache_read);
            wm.tokens[3] = wm.tokens[3].max(usage.cache_write);
            wm.tokens[4] = wm.tokens[4].max(usage.reasoning);
            d
        }
        _ => (
            usage.input,
            usage.output,
            usage.cache_read,
            usage.cache_write,
            usage.reasoning,
        ),
    };
    let raw_cost = match usage.cost_basis {
        CostBasis::ListPriceEstimate => usage
            .cost_usd
            .filter(|v| v.is_finite() && *v >= 0.0)
            .map(|v| (v * 1_000_000.0).round() as u64),
        _ => None,
    };
    let runtime_cost_micro_usd = match (raw_cost, usage.cost_scope) {
        (Some(total), UsageScope::Cumulative) => {
            let d = total.saturating_sub(wm.cost_micro_usd);
            wm.cost_micro_usd = wm.cost_micro_usd.max(total);
            Some(d)
        }
        (Some(delta), _) => Some(delta),
        (None, _) => None,
    };
    let meta = serde_json::json!({
        "token_scope": format!("{:?}", usage.token_scope),
        "cost_scope": format!("{:?}", usage.cost_scope),
        "cost_basis": format!("{:?}", usage.cost_basis),
    })
    .to_string();
    CostUsageRecordParams {
        id: None,
        producer_id: format!("session-{emp_id}"),
        source_event_id: format!("{}:{}", obs.run_id, obs.seq),
        run_id: Some(obs.run_id.clone()),
        actor_kind: UsageActorKind::Agent,
        employee_id: Some(emp_id.to_owned()),
        runtime: "agent_session".into(),
        provider: None,
        model: usage.model.clone(),
        occurred_at: obs.occurred_at.clone(),
        source_metadata_json: Some(meta),
        input_tokens: Some(in_tok),
        output_tokens: Some(out_tok),
        cache_read_tokens: Some(cr_tok),
        cache_write_tokens: Some(cw_tok),
        reasoning_tokens: Some(reas_tok),
        token_issue: None,
        runtime_cost_micro_usd,
        is_subscription: false,
        // `doc-87` / `doc-51` COST-3: unattributed session usage is NEVER defaulted to home team.
        scope_kind: UsageScopeKind::Organization,
        task_id: None,
        project_id: None,
        team_id: None,
        feature_id: None,
        milestone_id: None,
        release_id: None,
        task_type_snapshot: None,
        work_type: None,
        attribution_basis: AttributionBasis::Unattributed,
    }
}

pub(super) fn prune_watermarks(
    watermarks: &mut std::collections::BTreeMap<String, UsageWatermarkRecord>,
    protected_runs: &std::collections::BTreeSet<String>,
) {
    while watermarks.len() > 64 {
        let oldest = watermarks
            .iter()
            .filter(|(k, _)| !protected_runs.contains(*k))
            .min_by_key(|(_, v)| v.order)
            .map(|(k, _)| k.clone());
        if let Some(evict) = oldest {
            watermarks.remove(&evict);
        } else {
            break;
        }
    }
}

/// Dead-letter record for a usage event permanently rejected by accounting validation so later
/// valid events continue draining without loss of diagnostic evidence (`TASK-97` / Pass 8 #2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectedUsageEntry {
    pub params: org_proto::costs::CostUsageRecordParams,
    pub error_code: String,
    pub error_message: String,
    pub rejected_at: String,
}
