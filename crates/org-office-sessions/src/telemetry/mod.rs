//! In-memory bounded per-turn and per-session efficiency telemetry (`TASK-136`).
//!
//! Tracks wall latency, normalized token counts (including cached reads and thought/reasoning
//! tokens), computed cache-hit ratios, live/peak child RSS bytes, and sandbox/policy denial
//! counts in bounded ring buffers per agent (zero unbounded memory growth, strictly secret-free).

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use org_agent_runtime::{Usage, UsageScope};
use org_proto::telemetry::TurnTelemetrySample;

use crate::credentials::now_ms;
use crate::redact::redact_text;

pub(crate) mod denials;
#[cfg(test)]
mod tests;

pub(crate) use denials::is_sandbox_denial_text;

/// Maximum recent turns retained in memory per tracked agent.
pub const MAX_RECENT_TURNS_PER_AGENT: usize = 64;

/// Maximum distinct agents tracked in memory per office session manager.
pub const MAX_TRACKED_AGENTS: usize = 256;

/// Maximum per-run cumulative usage watermarks kept per agent.
const MAX_RUN_WATERMARKS_PER_AGENT: usize = 8;

/// Maximum length of redacted metadata identifiers (`runtime_id`, `model`, `effort`).
const MAX_META_LEN: usize = 128;

/// Maximum deduplication digests tracked per active turn (strictly bounded, zero raw text).
const MAX_DENIAL_DEDUP_KEYS: usize = 64;

fn denial_digest(tag: &str, raw: &str, normalize_case: bool) -> [u8; 16] {
    let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
    ctx.update(tag.as_bytes());
    ctx.update(&[0u8]);
    let trimmed = raw.trim();
    if normalize_case {
        let lower = trimmed.to_ascii_lowercase();
        ctx.update(lower.as_bytes());
    } else {
        ctx.update(trimmed.as_bytes());
    }
    let digest = ctx.finish();
    let mut out = [0u8; 16];
    out.copy_from_slice(&digest.as_ref()[..16]);
    out
}

/// Computes `cached_read_tokens / (input_tokens + cached_read_tokens)` when the denominator
/// is positive, or `None` when no input tokens were observed.
///
/// Per `org-agent-runtime::Usage`, `input_tokens` excludes `cached_read_tokens`, so their sum
/// is total input context tokens and the ratio is strictly in `[0.0, 1.0]`.
pub fn compute_cache_hit_ratio(input_tokens: u64, cached_read_tokens: u64) -> Option<f64> {
    let denom = u128::from(input_tokens).saturating_add(u128::from(cached_read_tokens));
    if denom == 0 {
        return None;
    }
    Some((cached_read_tokens as f64) / (denom as f64))
}

/// Computes the nearest-rank percentile (`1..=100`, e.g. `50` or `95`) of a non-empty
/// ascending-sorted slice of latency samples in milliseconds.
pub fn percentile_u64(sorted: &[u64], pct: u8) -> Option<u64> {
    if sorted.is_empty() {
        return None;
    }
    let clamped_pct = usize::from(pct.clamp(1, 100));
    let rank = sorted
        .len()
        .saturating_mul(clamped_pct)
        .div_ceil(100)
        .saturating_sub(1)
        .min(sorted.len() - 1);
    Some(sorted[rank])
}

/// Computes integer mean of latency samples in milliseconds using `u128` accumulation.
pub fn average_u64(values: &[u64]) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let sum = values.iter().fold(0u128, |acc, &v| acc + u128::from(v));
    u64::try_from(sum / (values.len() as u128)).ok()
}

/// Sanitizes and redacts short non-secret metadata labels (`runtime_id`, `model`, `effort`)
/// through `org-redact` so accidental token-shaped strings can never leak into telemetry.
fn sanitize_meta(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() {
        return None;
    }
    let redacted = redact_text(trimmed, &[]);
    let clean: String = redacted
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_META_LEN)
        .collect();
    (!clean.is_empty()).then_some(clean)
}

#[derive(Debug, Clone)]
struct InFlightTurn {
    started_at: Instant,
    input_tokens: u64,
    output_tokens: u64,
    cached_read_tokens: u64,
    thought_tokens: u64,
    sandbox_denial_count: u64,
    model: Option<String>,
    effort: Option<String>,
    denied_tool_keys: HashSet<[u8; 16]>,
    unkeyed_auto_denies: HashMap<[u8; 16], u64>,
    seen_denial_texts: HashSet<[u8; 16]>,
}

impl InFlightTurn {
    fn note_denial_text(&mut self, text: Option<&str>) {
        let Some(raw) = text.map(str::trim).filter(|s| !s.is_empty()) else {
            return;
        };
        if self.seen_denial_texts.len() < MAX_DENIAL_DEDUP_KEYS {
            self.seen_denial_texts
                .insert(denial_digest("txt", raw, true));
        }
    }
}

#[derive(Debug, Clone, Default)]
struct RunWatermark {
    tokens: [u64; 4],
    order: u64,
}

#[derive(Debug, Clone, Default)]
struct AgentTelemetryRecord {
    recent_turns: VecDeque<TurnTelemetrySample>,
    total_turn_count: u64,
    input_tokens: u64,
    output_tokens: u64,
    cached_read_tokens: u64,
    thought_tokens: u64,
    sandbox_denial_count: u64,
    last_rss_bytes: Option<u64>,
    peak_rss_bytes: Option<u64>,
    runtime_id: Option<String>,
    active_model: Option<String>,
    active_effort: Option<String>,
    active_turn: Option<InFlightTurn>,
    run_watermarks: HashMap<String, RunWatermark>,
    watermark_seq: u64,
    touched_seq: u64,
}

impl AgentTelemetryRecord {
    fn normalize_tokens(&mut self, run_id: &str, usage: &Usage) -> (u64, u64, u64, u64) {
        match usage.token_scope {
            UsageScope::Cumulative => {
                self.watermark_seq = self.watermark_seq.saturating_add(1);
                let order = self.watermark_seq;
                if !self.run_watermarks.contains_key(run_id)
                    && self.run_watermarks.len() >= MAX_RUN_WATERMARKS_PER_AGENT
                {
                    if let Some(oldest_key) = self
                        .run_watermarks
                        .iter()
                        .min_by_key(|(_, w)| w.order)
                        .map(|(k, _)| k.clone())
                    {
                        self.run_watermarks.remove(&oldest_key);
                    }
                }
                let wm = self.run_watermarks.entry(run_id.to_owned()).or_default();
                let delta = (
                    usage.input.saturating_sub(wm.tokens[0]),
                    usage.output.saturating_sub(wm.tokens[1]),
                    usage.cache_read.saturating_sub(wm.tokens[2]),
                    usage.reasoning.saturating_sub(wm.tokens[3]),
                );
                wm.tokens[0] = wm.tokens[0].max(usage.input);
                wm.tokens[1] = wm.tokens[1].max(usage.output);
                wm.tokens[2] = wm.tokens[2].max(usage.cache_read);
                wm.tokens[3] = wm.tokens[3].max(usage.reasoning);
                wm.order = order;
                delta
            }
            _ => (usage.input, usage.output, usage.cache_read, usage.reasoning),
        }
    }

    fn note_rss(&mut self, rss_bytes: u64) {
        self.last_rss_bytes = Some(rss_bytes);
        self.peak_rss_bytes = Some(
            self.peak_rss_bytes
                .map_or(rss_bytes, |prev| prev.max(rss_bytes)),
        );
    }

    fn to_snapshot(&self) -> AgentTelemetrySnapshot {
        AgentTelemetrySnapshot {
            recent_turns: self.recent_turns.iter().cloned().collect(),
            total_turn_count: self.total_turn_count,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cached_read_tokens: self.cached_read_tokens,
            thought_tokens: self.thought_tokens,
            sandbox_denial_count: self.sandbox_denial_count,
            last_rss_bytes: self.last_rss_bytes,
            peak_rss_bytes: self.peak_rss_bytes,
            runtime_id: self.runtime_id.clone(),
            active_model: self.active_model.clone(),
            active_effort: self.active_effort.clone(),
        }
    }
}

/// Immutable read-only copy of one agent's bounded telemetry for unlocked aggregation.
#[derive(Debug, Clone, Default)]
pub struct AgentTelemetrySnapshot {
    pub recent_turns: Vec<TurnTelemetrySample>,
    pub total_turn_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_read_tokens: u64,
    pub thought_tokens: u64,
    pub sandbox_denial_count: u64,
    pub last_rss_bytes: Option<u64>,
    pub peak_rss_bytes: Option<u64>,
    pub runtime_id: Option<String>,
    pub active_model: Option<String>,
    pub active_effort: Option<String>,
}

#[derive(Default)]
struct TelemetryState {
    agents: HashMap<String, AgentTelemetryRecord>,
    touch_seq: u64,
    office_peak_rss_bytes: u64,
}

impl TelemetryState {
    fn entry_mut(&mut self, emp_id: &str) -> &mut AgentTelemetryRecord {
        self.touch_seq = self.touch_seq.saturating_add(1);
        let seq = self.touch_seq;
        if !self.agents.contains_key(emp_id) && self.agents.len() >= MAX_TRACKED_AGENTS {
            let evict = self
                .agents
                .iter()
                .filter(|(_, rec)| rec.active_turn.is_none())
                .min_by_key(|(_, rec)| rec.touched_seq)
                .or_else(|| self.agents.iter().min_by_key(|(_, rec)| rec.touched_seq))
                .map(|(k, _)| k.clone());
            if let Some(key) = evict {
                self.agents.remove(&key);
            }
        }
        let rec = self.agents.entry(emp_id.to_owned()).or_default();
        rec.touched_seq = seq;
        rec
    }
}

/// Thread-safe, strictly bounded in-memory telemetry collector owned by `SessionManager`.
///
/// Every method acquires an internal mutex briefly for pure in-memory updates and never
/// performs I/O, SQLite queries, or OS process sampling while holding the lock.
#[derive(Default)]
pub struct TelemetryCollector {
    state: Mutex<TelemetryState>,
}

fn lock_state(m: &Mutex<TelemetryState>) -> MutexGuard<'_, TelemetryState> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl TelemetryCollector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_session_started(
        &self,
        emp_id: &str,
        runtime_id: Option<&str>,
        model: Option<&str>,
        effort: Option<&str>,
    ) {
        let mut st = lock_state(&self.state);
        let rec = st.entry_mut(emp_id);
        // Defensively clear any orphaned in-flight turn so a new session can never inherit
        // stale latency, tokens, or denial counters from a dead run (`TASK-136`).
        rec.active_turn = None;
        rec.runtime_id = sanitize_meta(runtime_id);
        rec.active_model = sanitize_meta(model);
        rec.active_effort = sanitize_meta(effort);
    }

    pub fn record_turn_started(
        &self,
        emp_id: &str,
        model: Option<&str>,
        effort: Option<&str>,
        now: Instant,
    ) {
        let mut st = lock_state(&self.state);
        let rec = st.entry_mut(emp_id);
        let clean_model = sanitize_meta(model).or_else(|| rec.active_model.clone());
        let clean_effort = sanitize_meta(effort).or_else(|| rec.active_effort.clone());
        if clean_model.is_some() {
            rec.active_model = clean_model.clone();
        }
        if clean_effort.is_some() {
            rec.active_effort = clean_effort.clone();
        }
        if rec.active_turn.is_none() {
            rec.active_turn = Some(InFlightTurn {
                started_at: now,
                input_tokens: 0,
                output_tokens: 0,
                cached_read_tokens: 0,
                thought_tokens: 0,
                sandbox_denial_count: 0,
                model: clean_model,
                effort: clean_effort,
                denied_tool_keys: HashSet::new(),
                unkeyed_auto_denies: HashMap::new(),
                seen_denial_texts: HashSet::new(),
            });
        }
    }

    pub fn record_usage(&self, emp_id: &str, run_id: &str, usage: &Usage) {
        let mut st = lock_state(&self.state);
        let rec = st.entry_mut(emp_id);
        let (in_tok, out_tok, cache_read, thought) = rec.normalize_tokens(run_id, usage);
        rec.input_tokens = rec.input_tokens.saturating_add(in_tok);
        rec.output_tokens = rec.output_tokens.saturating_add(out_tok);
        rec.cached_read_tokens = rec.cached_read_tokens.saturating_add(cache_read);
        rec.thought_tokens = rec.thought_tokens.saturating_add(thought);
        let usage_model = sanitize_meta(usage.model.as_deref());
        if usage_model.is_some() {
            rec.active_model = usage_model.clone();
        }
        if let Some(turn) = rec.active_turn.as_mut() {
            turn.input_tokens = turn.input_tokens.saturating_add(in_tok);
            turn.output_tokens = turn.output_tokens.saturating_add(out_tok);
            turn.cached_read_tokens = turn.cached_read_tokens.saturating_add(cache_read);
            turn.thought_tokens = turn.thought_tokens.saturating_add(thought);
            if usage_model.is_some() {
                turn.model = usage_model;
            }
        }
    }

    pub fn record_sandbox_denial(&self, emp_id: &str, count: u64) {
        if count == 0 {
            return;
        }
        let mut st = lock_state(&self.state);
        let rec = st.entry_mut(emp_id);
        rec.sandbox_denial_count = rec.sandbox_denial_count.saturating_add(count);
        if let Some(turn) = rec.active_turn.as_mut() {
            turn.sandbox_denial_count = turn.sandbox_denial_count.saturating_add(count);
        }
    }

    /// Records an Office `AutoDeny` for one tool call, deduplicated by `tool_use_id` so a later
    /// runtime `ActionDenied` or matching notice/error does not double-count the same denial.
    pub fn record_auto_deny(
        &self,
        emp_id: &str,
        tool: &str,
        tool_use_id: Option<&str>,
        reason: Option<&str>,
    ) {
        let mut st = lock_state(&self.state);
        let rec = st.entry_mut(emp_id);
        if let Some(turn) = rec.active_turn.as_mut() {
            turn.note_denial_text(reason);
            if let Some(tid) = tool_use_id.map(str::trim).filter(|s| !s.is_empty()) {
                let key = denial_digest("tu", tid, false);
                if turn.denied_tool_keys.contains(&key) {
                    return;
                }
                if turn.denied_tool_keys.len() < MAX_DENIAL_DEDUP_KEYS {
                    turn.denied_tool_keys.insert(key);
                }
            } else {
                let tkey = denial_digest("tool", tool, true);
                if turn.unkeyed_auto_denies.contains_key(&tkey)
                    || turn.unkeyed_auto_denies.len() < MAX_DENIAL_DEDUP_KEYS
                {
                    let slot = turn.unkeyed_auto_denies.entry(tkey).or_insert(0);
                    *slot = slot.saturating_add(1);
                }
            }
            turn.sandbox_denial_count = turn.sandbox_denial_count.saturating_add(1);
        }
        rec.sandbox_denial_count = rec.sandbox_denial_count.saturating_add(1);
    }

    /// Records a runtime `ActionDenied` event, suppressing duplicates already counted via Office
    /// `AutoDeny` for the same `tool_use_id` (or unmatched unkeyed auto-deny for the same tool).
    pub fn record_action_denied(
        &self,
        emp_id: &str,
        tool: &str,
        tool_use_id: Option<&str>,
        message: Option<&str>,
    ) {
        let mut st = lock_state(&self.state);
        let rec = st.entry_mut(emp_id);
        if let Some(turn) = rec.active_turn.as_mut() {
            turn.note_denial_text(message);
            if let Some(tid) = tool_use_id.map(str::trim).filter(|s| !s.is_empty()) {
                let key = denial_digest("tu", tid, false);
                if turn.denied_tool_keys.contains(&key) {
                    return;
                }
                if turn.denied_tool_keys.len() < MAX_DENIAL_DEDUP_KEYS {
                    turn.denied_tool_keys.insert(key);
                }
            } else {
                let tkey = denial_digest("tool", tool, true);
                if let Some(pending) = turn.unkeyed_auto_denies.get_mut(&tkey) {
                    if *pending > 0 {
                        *pending -= 1;
                        return;
                    }
                }
            }
            turn.sandbox_denial_count = turn.sandbox_denial_count.saturating_add(1);
        }
        rec.sandbox_denial_count = rec.sandbox_denial_count.saturating_add(1);
    }

    /// Records a sandbox/policy denial surfaced in a runtime `Notice`, deduplicating repeated
    /// normalized messages within the active turn without retaining raw text.
    pub fn record_denial_notice(&self, emp_id: &str, text: &str) {
        if !is_sandbox_denial_text(text) {
            return;
        }
        let key = denial_digest("txt", text, true);
        let mut st = lock_state(&self.state);
        let rec = st.entry_mut(emp_id);
        if let Some(turn) = rec.active_turn.as_mut() {
            if turn.seen_denial_texts.contains(&key) {
                return;
            }
            if turn.seen_denial_texts.len() < MAX_DENIAL_DEDUP_KEYS {
                turn.seen_denial_texts.insert(key);
            }
            turn.sandbox_denial_count = turn.sandbox_denial_count.saturating_add(1);
        }
        rec.sandbox_denial_count = rec.sandbox_denial_count.saturating_add(1);
    }

    /// Records fallback sandbox/policy denials from `TurnEnded.errors` only when no structured
    /// denial or notice already accounted for denials during the active turn.
    pub fn record_turn_end_denial_errors(&self, emp_id: &str, errors: &[String]) {
        let mut st = lock_state(&self.state);
        let rec = st.entry_mut(emp_id);
        if rec
            .active_turn
            .as_ref()
            .is_some_and(|t| t.sandbox_denial_count > 0)
        {
            return;
        }
        let mut added = 0u64;
        let mut local_seen = HashSet::new();
        for err in errors {
            if !is_sandbox_denial_text(err) {
                continue;
            }
            let key = denial_digest("txt", err, true);
            if !local_seen.insert(key) {
                continue;
            }
            if let Some(turn) = rec.active_turn.as_mut() {
                if turn.seen_denial_texts.contains(&key) {
                    continue;
                }
                if turn.seen_denial_texts.len() < MAX_DENIAL_DEDUP_KEYS {
                    turn.seen_denial_texts.insert(key);
                }
                turn.sandbox_denial_count = turn.sandbox_denial_count.saturating_add(1);
            }
            added = added.saturating_add(1);
        }
        rec.sandbox_denial_count = rec.sandbox_denial_count.saturating_add(added);
    }

    pub fn record_rss_sample(&self, emp_id: &str, rss_bytes: u64) {
        if rss_bytes == 0 {
            return;
        }
        let mut st = lock_state(&self.state);
        let rec = st.entry_mut(emp_id);
        rec.note_rss(rss_bytes);
    }

    pub fn record_office_rss_total(&self, total_rss_bytes: u64) {
        let mut st = lock_state(&self.state);
        st.office_peak_rss_bytes = st.office_peak_rss_bytes.max(total_rss_bytes);
    }

    pub fn record_turn_finished(
        &self,
        emp_id: &str,
        succeeded: bool,
        rss_bytes: Option<u64>,
        now: Instant,
        completed_at_ms: u64,
    ) {
        let mut st = lock_state(&self.state);
        let rec = st.entry_mut(emp_id);
        let current_rss = rss_bytes.filter(|&b| b > 0);
        if let Some(bytes) = current_rss {
            rec.note_rss(bytes);
        }
        let in_flight = rec.active_turn.take();
        let (wall_latency_ms, in_tok, out_tok, cache_read, thought, denials, model, effort) =
            match in_flight {
                Some(t) => (
                    now.saturating_duration_since(t.started_at)
                        .as_millis()
                        .min(u128::from(u64::MAX)) as u64,
                    t.input_tokens,
                    t.output_tokens,
                    t.cached_read_tokens,
                    t.thought_tokens,
                    t.sandbox_denial_count,
                    t.model.or_else(|| rec.active_model.clone()),
                    t.effort.or_else(|| rec.active_effort.clone()),
                ),
                None => (
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    rec.active_model.clone(),
                    rec.active_effort.clone(),
                ),
            };
        let sample = TurnTelemetrySample {
            wall_latency_ms,
            input_tokens: in_tok,
            output_tokens: out_tok,
            cached_read_tokens: cache_read,
            thought_tokens: thought,
            cache_hit_ratio: compute_cache_hit_ratio(in_tok, cache_read),
            rss_bytes: current_rss,
            sandbox_denial_count: denials,
            model,
            effort,
            succeeded,
            completed_at_ms,
        };
        if rec.recent_turns.len() >= MAX_RECENT_TURNS_PER_AGENT {
            rec.recent_turns.pop_front();
        }
        rec.recent_turns.push_back(sample);
        rec.total_turn_count = rec.total_turn_count.saturating_add(1);
    }

    pub fn remove_agent(&self, emp_id: &str) {
        lock_state(&self.state).agents.remove(emp_id);
    }

    pub fn snapshot_agent(&self, emp_id: &str) -> Option<AgentTelemetrySnapshot> {
        lock_state(&self.state)
            .agents
            .get(emp_id)
            .map(AgentTelemetryRecord::to_snapshot)
    }

    pub fn snapshot_all(&self) -> (u64, HashMap<String, AgentTelemetrySnapshot>) {
        let st = lock_state(&self.state);
        let map = st
            .agents
            .iter()
            .map(|(k, v)| (k.clone(), v.to_snapshot()))
            .collect();
        (st.office_peak_rss_bytes, map)
    }
}

/// Lightweight pump-owned handle recording turn latency, tokens, RSS, and sandbox denials
/// without ever holding coarse locks across OS process inspection (`TASK-136`).
#[derive(Clone)]
pub(crate) struct PumpTelemetry {
    pub collector: Arc<TelemetryCollector>,
    pub sample_rss: Arc<dyn Fn(u32) -> Option<u64> + Send + Sync>,
    pub child_pid: Option<u32>,
    pub active_model: Option<String>,
    pub active_effort: Option<String>,
}

impl PumpTelemetry {
    pub(crate) fn on_turn_started(&self, emp_id: &str) {
        self.collector.record_turn_started(
            emp_id,
            self.active_model.as_deref(),
            self.active_effort.as_deref(),
            Instant::now(),
        );
    }

    pub(crate) fn on_usage(&self, emp_id: &str, run_id: &str, usage: &Usage) {
        self.collector.record_usage(emp_id, run_id, usage);
    }

    pub(crate) fn on_auto_deny(
        &self,
        emp_id: &str,
        tool: &str,
        tool_use_id: Option<&str>,
        reason: Option<&str>,
    ) {
        self.collector
            .record_auto_deny(emp_id, tool, tool_use_id, reason);
    }

    pub(crate) fn on_action_denied(
        &self,
        emp_id: &str,
        tool: &str,
        tool_use_id: Option<&str>,
        message: Option<&str>,
    ) {
        self.collector
            .record_action_denied(emp_id, tool, tool_use_id, message);
    }

    pub(crate) fn on_notice(&self, emp_id: &str, text: &str) {
        self.collector.record_denial_notice(emp_id, text);
    }

    /// Samples child process-tree RSS unlocked and finalizes the completed turn telemetry record.
    pub(crate) fn on_turn_ended(&self, emp_id: &str, succeeded: bool, errors: &[String]) {
        self.collector.record_turn_end_denial_errors(emp_id, errors);
        let rss_bytes = self.child_pid.and_then(|pid| (self.sample_rss)(pid));
        self.collector
            .record_turn_finished(emp_id, succeeded, rss_bytes, Instant::now(), now_ms());
    }
}
