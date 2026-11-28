//! Idle warm-session hibernation, per-office RSS budget LRU eviction, and build-concurrency limits
//! (`TASK-133`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::thread;
use std::time::{Duration, Instant};

use org_proto::sessions::SessionState;

use crate::credentials::rfc3339_now;

use super::environment::{clamp_build_concurrency_jobs, default_build_concurrency_jobs};
use super::{reap_live_handle, LiveSessionHandle, ReapMode, SessionManager, StartSlotGuard};

pub const DEFAULT_IDLE_HIBERNATION_TTL: Duration = Duration::from_secs(15 * 60);
pub const DEFAULT_RESOURCE_SWEEP_INTERVAL: Duration = Duration::from_secs(5);
const MIB_BYTES: u64 = 1024 * 1024;

/// Consumer-owned port sampling a supervised process tree's resident memory (RSS) in bytes.
pub trait ProcessMemorySampler: Send + Sync {
    fn process_tree_rss_bytes(&self, root_pid: u32) -> Option<u64>;
}

/// Default local-OS [`ProcessMemorySampler`] backed by `org_platform::process`.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsMemorySampler;

impl ProcessMemorySampler for OsMemorySampler {
    fn process_tree_rss_bytes(&self, root_pid: u32) -> Option<u64> {
        org_platform::process::process_tree_rss_bytes(root_pid)
    }
}

/// Per-office session resource limits: idle hibernation TTL, RSS ceiling, and build concurrency.
#[derive(Clone)]
pub struct ResourceLimitsConfig {
    pub idle_hibernation_ttl: Option<Duration>,
    pub max_office_rss_bytes: Option<u64>,
    pub build_concurrency_jobs: u32,
    pub sweep_interval: Duration,
    pub memory_sampler: Arc<dyn ProcessMemorySampler>,
}

impl Default for ResourceLimitsConfig {
    fn default() -> Self {
        Self {
            idle_hibernation_ttl: Some(DEFAULT_IDLE_HIBERNATION_TTL),
            max_office_rss_bytes: None,
            build_concurrency_jobs: default_build_concurrency_jobs(),
            sweep_interval: DEFAULT_RESOURCE_SWEEP_INTERVAL,
            memory_sampler: Arc::new(OsMemorySampler),
        }
    }
}

impl ResourceLimitsConfig {
    pub fn from_env() -> Self {
        Self::from_env_lookup(|name| std::env::var(name).ok())
    }

    pub fn from_env_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let mut cfg = Self::default();
        if let Some(v) = lookup("ORG_SESSION_IDLE_TTL_SECS").and_then(parse_limit) {
            cfg.idle_hibernation_ttl = v.map(Duration::from_secs);
        }
        if let Some(v) = lookup("ORG_OFFICE_MAX_RSS_MIB").and_then(parse_limit) {
            cfg.max_office_rss_bytes = v.map(|m| m.saturating_mul(MIB_BYTES));
        }
        if let Some(j) = lookup("ORG_AGENT_BUILD_JOBS").and_then(|s| s.trim().parse().ok()) {
            cfg.build_concurrency_jobs = clamp_build_concurrency_jobs(j);
        }
        cfg
    }
}

fn parse_limit(raw: String) -> Option<Option<u64>> {
    let t = raw.trim();
    if t == "0" || matches!(t.to_ascii_lowercase().as_str(), "none" | "off" | "disabled") {
        return Some(None);
    }
    t.parse::<u64>().ok().map(Some)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRssEntry {
    pub employee_id: String,
    pub pid: Option<u32>,
    pub rss_bytes: u64,
    pub is_idle: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfficeRssSnapshot {
    pub total_rss_bytes: u64,
    pub per_session_rss: Vec<SessionRssEntry>,
    pub budget_bytes: Option<u64>,
    pub hibernated_total: u64,
    pub rss_evicted_total: u64,
}

/// `(run_id, root_pid, last_used_at, idle_since)`
#[derive(Clone)]
struct LiveMeta(Option<String>, Option<u32>, Instant, Option<Instant>);

#[derive(Clone)]
struct SavedResume {
    runtime_session_id: String,
    runtime_id: Option<String>,
    spec_hash: String,
    stop_epoch: u64,
}

/// `(rss_entry, run_id, last_used_at, idle_since)`
struct Sample(SessionRssEntry, Option<String>, Instant, Option<Instant>);

#[derive(Clone, Copy)]
enum EvictGuard {
    IdleTtl { now: Instant, ttl: Duration },
    RssLru { snapshot_last_used_at: Instant },
}

#[derive(Default)]
pub(crate) struct HibernationState {
    live_meta: Mutex<HashMap<String, LiveMeta>>,
    hibernated: Mutex<HashMap<String, SavedResume>>,
    hibernated_count: AtomicU64,
    rss_evicted_count: AtomicU64,
    sweeper_started: AtomicBool,
    sweeper_stop: (Mutex<bool>, Condvar),
    sweeper_handle: Mutex<Option<thread::JoinHandle<()>>>,
}

impl Drop for HibernationState {
    fn drop(&mut self) {
        self.stop_and_join_sweeper();
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl HibernationState {
    pub(crate) fn record_spawned_pid(&self, emp: &str, run: &str, pid: u32, now: Instant) {
        let meta = LiveMeta(Some(run.into()), Some(pid), now, None);
        lock(&self.live_meta).insert(emp.into(), meta);
        lock(&self.hibernated).remove(emp);
    }

    pub(crate) fn note_activity(&self, emp: &str, now: Instant) {
        let mut g = lock(&self.live_meta);
        let e = g
            .entry(emp.into())
            .or_insert(LiveMeta(None, None, now, None));
        (e.2, e.3) = (now, None);
    }

    pub(crate) fn note_turn_idle(&self, emp: &str, now: Instant) {
        let mut g = lock(&self.live_meta);
        if let Some(e) = g.get_mut(emp) {
            e.2 = now;
            e.3 = Some(now);
        }
    }

    pub(crate) fn clear_live_tracking(&self, emp: &str) {
        lock(&self.live_meta).remove(emp);
    }

    pub(crate) fn live_pid(&self, emp: &str) -> Option<u32> {
        lock(&self.live_meta).get(emp).and_then(|meta| meta.1)
    }

    pub(crate) fn clear_all_hibernated(&self) {
        lock(&self.hibernated).clear();
    }

    pub(crate) fn stop_and_join_sweeper(&self) {
        {
            let mut stop = lock(&self.sweeper_stop.0);
            *stop = true;
            self.sweeper_stop.1.notify_all();
        }
        let cur = thread::current().id();
        if let Some(h) = lock(&self.sweeper_handle)
            .take()
            .filter(|h| h.thread().id() != cur)
        {
            let _ = h.join();
        }
    }

    /// Consumes a preserved resume ID if and only if runtime ID, `spec_hash`, and `stop_epoch`
    /// still match (never leaving a stale or corrupted entry pinned as a poison pill).
    pub(crate) fn matching_resume_id(
        &self,
        emp: &str,
        runtime_id: &str,
        spec_hash: &str,
        stop_epoch: u64,
    ) -> Option<String> {
        let saved = lock(&self.hibernated).remove(emp)?;
        let matches = saved.stop_epoch == stop_epoch
            && saved.runtime_id.as_deref() == Some(runtime_id)
            && saved.spec_hash == spec_hash;
        matches.then_some(saved.runtime_session_id)
    }

    #[cfg(test)]
    pub(crate) fn set_test_timing(&self, e: &str, r: &str, p: u32, at: (Instant, Option<Instant>)) {
        let meta = LiveMeta(Some(r.into()), Some(p), at.0, at.1);
        lock(&self.live_meta).insert(e.into(), meta);
    }

    #[cfg(test)]
    pub(crate) fn has_sweeper_thread(&self) -> bool {
        lock(&self.sweeper_handle).is_some()
    }

    #[cfg(test)]
    pub(crate) fn is_turn_idle(&self, emp: &str) -> bool {
        lock(&self.live_meta)
            .get(emp)
            .is_some_and(|meta| meta.3.is_some())
    }
}

impl SessionManager {
    pub fn hibernated_count(&self) -> u64 {
        self.hibernation.hibernated_count.load(Ordering::Relaxed)
    }

    pub fn rss_evicted_count(&self) -> u64 {
        self.hibernation.rss_evicted_count.load(Ordering::Relaxed)
    }

    /// Hibernates idle warm sessions exceeding `idle_hibernation_ttl`, preserving `runtime_session_id`.
    pub fn hibernate_idle_sessions(&self, now: Instant) -> Vec<String> {
        let synced = collect_live_sessions(self, now, false);
        let Some(ttl) = self.config.resource_limits.idle_hibernation_ttl else {
            return Vec::new();
        };
        let guard = EvictGuard::IdleTtl { now, ttl };
        let mut out = Vec::new();
        for s in synced {
            let due = s.0.is_idle && s.3.is_some_and(|t| now.saturating_duration_since(t) >= ttl);
            if due && hibernate_one_idle(self, &s.0.employee_id, s.1.as_deref(), guard, false) {
                out.push(s.0.employee_id);
            }
        }
        out
    }

    /// Samples live RSS unlocked and LRU-evicts idle warm sessions while above `max_office_rss_bytes`.
    pub fn enforce_rss_budget(&self) -> Vec<String> {
        self.enforce_rss_budget_at(Instant::now())
    }

    pub fn enforce_rss_budget_at(&self, now: Instant) -> Vec<String> {
        let Some(budget) = self.config.resource_limits.max_office_rss_bytes else {
            return Vec::new();
        };
        let sampled = collect_live_sessions(self, now, true);
        let mut total = sampled
            .iter()
            .fold(0u64, |a, s| a.saturating_add(s.0.rss_bytes));
        if total <= budget {
            return Vec::new();
        }
        let mut idle: Vec<_> = sampled.into_iter().filter(|s| s.0.is_idle).collect();
        idle.sort_by(|a, b| (a.2, &a.0.employee_id).cmp(&(b.2, &b.0.employee_id)));
        let mut evicted = Vec::new();
        for c in idle {
            if total <= budget {
                break;
            }
            let guard = EvictGuard::RssLru {
                snapshot_last_used_at: c.2,
            };
            if hibernate_one_idle(self, &c.0.employee_id, c.1.as_deref(), guard, true) {
                total = total.saturating_sub(c.0.rss_bytes);
                evicted.push(c.0.employee_id);
            }
        }
        evicted
    }

    pub fn office_session_rss_snapshot(&self) -> OfficeRssSnapshot {
        let sampled = collect_live_sessions(self, Instant::now(), true);
        let total_rss_bytes = sampled
            .iter()
            .fold(0u64, |a, s| a.saturating_add(s.0.rss_bytes));
        let mut per_session_rss: Vec<_> = sampled.into_iter().map(|s| s.0).collect();
        per_session_rss.sort_by(|a, b| a.employee_id.cmp(&b.employee_id));
        OfficeRssSnapshot {
            total_rss_bytes,
            per_session_rss,
            budget_bytes: self.config.resource_limits.max_office_rss_bytes,
            hibernated_total: self.hibernated_count(),
            rss_evicted_total: self.rss_evicted_count(),
        }
    }

    pub(crate) fn sweep_resource_limits(&self) {
        let now = Instant::now();
        let _ = self.hibernate_idle_sessions(now);
        let _ = self.enforce_rss_budget_at(now);
    }

    pub(crate) fn maybe_start_resource_sweeper(this: &Arc<Self>) {
        let lim = &this.config.resource_limits;
        let on = (lim.idle_hibernation_ttl.is_some() || lim.max_office_rss_bytes.is_some())
            && !lim.sweep_interval.is_zero();
        let flag = &this.hibernation.sweeper_started;
        if !on || flag.swap(true, Ordering::SeqCst) {
            return;
        }
        if lock(&this.registry).shutting_down {
            return flag.store(false, Ordering::SeqCst);
        }
        *lock(&this.hibernation.sweeper_stop.0) = false;
        let (weak, iv) = (Arc::downgrade(this), lim.sweep_interval);
        let spawned = thread::Builder::new()
            .name("org-session-reaper".into())
            .spawn(move || run_sweeper(weak, iv));
        match spawned {
            Ok(h) => *lock(&this.hibernation.sweeper_handle) = Some(h),
            Err(_) => flag.store(false, Ordering::SeqCst),
        }
    }
}

fn run_sweeper(weak: Weak<SessionManager>, interval: Duration) {
    while let Some(mgr) = weak.upgrade() {
        let (draining, shutting_down) = {
            let reg = lock(&mgr.registry);
            (reg.draining, reg.shutting_down)
        };
        if shutting_down {
            return;
        }
        let stopped = {
            let (mu, cv) = &mgr.hibernation.sweeper_stop;
            let guard = lock(mu);
            if *guard {
                true
            } else {
                let (guard, _) = cv
                    .wait_timeout(guard, interval)
                    .unwrap_or_else(|p| p.into_inner());
                *guard
            }
        };
        if stopped || lock(&mgr.registry).shutting_down {
            return;
        }
        if !draining && !lock(&mgr.registry).draining {
            mgr.sweep_resource_limits();
        }
    }
}

fn has_open_required_asks(mgr: &SessionManager, emp: &str) -> bool {
    lock(&mgr.open_required_asks)
        .get(emp)
        .is_some_and(|asks| !asks.is_empty())
}

fn state_is_truly_idle(
    permit_free: bool,
    open_asks: bool,
    st: &crate::pump::SessionRuntimeState,
) -> bool {
    permit_free
        && !open_asks
        && st.state == SessionState::Idle
        && st.queued_turns == 0
        && !st.recovery_required
        && st.pending_approvals.is_empty()
        && st.backgrounds.active_count() == 0
        && !st.backgrounds.has_pending_wakes()
}

fn is_handle_truly_idle(
    mgr: &SessionManager,
    emp: &str,
    h: &LiveSessionHandle,
) -> (bool, Option<String>) {
    let permit_free = lock(&h.active_permit).is_none();
    let open_asks = has_open_required_asks(mgr, emp);
    let st = lock(&h.state);
    (
        state_is_truly_idle(permit_free, open_asks, &st),
        st.run_id.clone(),
    )
}

fn collect_live_sessions(mgr: &SessionManager, now: Instant, with_rss: bool) -> Vec<Sample> {
    let observed = {
        let reg = lock(&mgr.registry);
        if reg.draining || reg.shutting_down {
            return Vec::new();
        }
        reg.live
            .iter()
            .map(|(id, h)| {
                let (idle, run) = is_handle_truly_idle(mgr, id, h);
                (id.clone(), idle && !reg.starting.contains(id), run)
            })
            .collect::<Vec<_>>()
    };
    let anchor = Instant::now().min(now);
    let mut meta = lock(&mgr.hibernation.live_meta);
    let mut sessions = observed
        .into_iter()
        .map(|(emp, is_idle, run_id)| {
            let e = meta.entry(emp.clone()).or_insert_with(|| {
                LiveMeta(run_id.clone(), None, anchor, is_idle.then_some(anchor))
            });
            if e.0 != run_id {
                *e = LiveMeta(run_id.clone(), None, anchor, None);
            }
            if is_idle {
                e.3.get_or_insert(anchor);
            } else {
                (e.2, e.3) = (now, None);
            }
            let entry = SessionRssEntry {
                employee_id: emp,
                pid: e.1,
                rss_bytes: 0,
                is_idle,
            };
            Sample(entry, run_id, e.2, e.3)
        })
        .collect::<Vec<_>>();
    drop(meta);
    if with_rss {
        sample_rss_unlocked(mgr, &mut sessions);
    }
    sessions
}

fn sample_rss_unlocked(mgr: &SessionManager, sessions: &mut [Sample]) {
    let sampler = mgr.config.resource_limits.memory_sampler.as_ref();
    for s in sessions {
        let pid = s.0.pid.or_else(|| {
            let rec = mgr.store.read_active_turn(&s.0.employee_id).ok()??;
            s.1.as_deref()
                .is_none_or(|id| rec.run_id == id)
                .then_some(rec.pid)?
        });
        s.0.pid = pid;
        s.0.rss_bytes = pid
            .and_then(|p| sampler.process_tree_rss_bytes(p))
            .unwrap_or(0);
    }
}

fn meta_still_eligible(
    mgr: &SessionManager,
    emp: &str,
    run_id: &Option<String>,
    guard: EvictGuard,
) -> bool {
    let meta = lock(&mgr.hibernation.live_meta);
    let Some(e) = meta.get(emp) else {
        return false;
    };
    if &e.0 != run_id {
        return false;
    }
    match guard {
        EvictGuard::IdleTtl { now, ttl } => {
            e.3.is_some_and(|idle_since| now.saturating_duration_since(idle_since) >= ttl)
        }
        EvictGuard::RssLru {
            snapshot_last_used_at,
        } => e.3.is_some() && e.2 <= snapshot_last_used_at,
    }
}

fn take_idle_candidate(
    mgr: &SessionManager,
    emp: &str,
    expected: Option<&str>,
    guard: EvictGuard,
) -> Option<(LiveSessionHandle, Option<SavedResume>)> {
    let mut reg = lock(&mgr.registry);
    if reg.draining || reg.shutting_down || reg.starting.contains(emp) {
        return None;
    }
    let handle = reg.live.get(emp)?;
    let permit_free = lock(&handle.active_permit).is_none();
    let open_asks = has_open_required_asks(mgr, emp);
    let st = lock(&handle.state);
    if !state_is_truly_idle(permit_free, open_asks, &st)
        || expected.is_some_and(|exp| st.run_id.as_deref() != Some(exp))
    {
        return None;
    }
    if !meta_still_eligible(mgr, emp, &st.run_id, guard) {
        return None;
    }
    let resume = handle.partition.is_none().then(|| SavedResume {
        runtime_session_id: st.runtime_session_id.clone().unwrap_or_default(),
        runtime_id: st.runtime_id.clone(),
        spec_hash: st.spec_hash.clone(),
        stop_epoch: reg.stop_epoch(emp),
    });
    drop(st);
    let removed = reg.live.remove(emp)?;
    reg.starting.insert(emp.to_owned());
    Some((removed, resume))
}

fn hibernate_one_idle(
    mgr: &SessionManager,
    emp: &str,
    run_id: Option<&str>,
    guard: EvictGuard,
    is_rss: bool,
) -> bool {
    let Some((handle, resume_tpl)) = take_idle_candidate(mgr, emp, run_id, guard) else {
        return false;
    };
    let _slot = StartSlotGuard::new(&mgr.registry, emp.to_owned());
    let (reason, ctr) = if is_rss {
        ("rss_budget_eviction", &mgr.hibernation.rss_evicted_count)
    } else {
        ("idle_hibernation", &mgr.hibernation.hibernated_count)
    };
    let mode = ReapMode::FailClosedRestore {
        conflict_message: format!(
            "could not verify teardown of hibernated session `{emp}`; resolve recovery first"
        ),
    };
    let Ok(reaped) = reap_live_handle(mgr, emp, handle, Some(reason), mode) else {
        return false;
    };
    let preserved = resume_tpl
        .map(|mut s| {
            if s.runtime_session_id.is_empty() {
                s.runtime_session_id = reaped.view.runtime_session_id.unwrap_or_default();
            }
            s
        })
        .filter(|s| !s.runtime_session_id.is_empty())
        .map(|s| lock(&mgr.hibernation.hibernated).insert(emp.into(), s))
        .is_some();
    ctr.fetch_add(1, Ordering::Relaxed);
    let tokens = mgr.bindings.active_tokens();
    let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
    let ev = serde_json::json!({
        "type": "session_hibernated",
        "reason": reason,
        "runtime_session_id_preserved": preserved,
        "at": rfc3339_now(),
    });
    let _ = mgr.store.append_history(emp, &ev, &refs);
    true
}
