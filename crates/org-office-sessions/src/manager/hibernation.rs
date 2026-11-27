//! Idle warm-session hibernation, per-office RSS budget LRU eviction, and build-concurrency limits
//! (`TASK-133`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
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
/// `(runtime_session_id, runtime_id, route_digest, workspace, stop_epoch)`
#[derive(Clone)]
struct SavedResume(String, Option<String>, String, Option<String>, u64);
/// `(rss_entry, run_id, last_used_at, idle_since)`
struct Sample(SessionRssEntry, Option<String>, Instant, Option<Instant>);
type ResumeKey<'a> = (&'a str, Option<&'a str>, &'a str, u64);

#[derive(Default)]
pub(crate) struct HibernationState {
    live_meta: Mutex<HashMap<String, LiveMeta>>,
    hibernated: Mutex<HashMap<String, SavedResume>>,
    hibernated_count: AtomicU64,
    rss_evicted_count: AtomicU64,
    sweeper_started: AtomicBool,
    sweeper_handle: Mutex<Option<thread::JoinHandle<()>>>,
}

impl Drop for HibernationState {
    fn drop(&mut self) {
        let cur = thread::current().id();
        if let Some(h) = lock(&self.sweeper_handle)
            .take()
            .filter(|h| h.thread().id() != cur)
        {
            let _ = h.join();
        }
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

    pub(crate) fn clear_live_tracking(&self, emp: &str) {
        lock(&self.live_meta).remove(emp);
    }

    pub(crate) fn matching_resume_id(&self, emp: &str, k: ResumeKey<'_>) -> Option<String> {
        let mut g = lock(&self.hibernated);
        let s = g.get(emp)?;
        if s.4 == k.3 && s.1.as_deref() == Some(k.0) && s.3.as_deref() == k.1 && s.2 == k.2 {
            return Some(s.0.clone());
        }
        g.remove(emp);
        None
    }

    #[cfg(test)]
    pub(crate) fn set_test_timing(&self, e: &str, r: &str, p: u32, at: (Instant, Option<Instant>)) {
        let meta = LiveMeta(Some(r.into()), Some(p), at.0, at.1);
        lock(&self.live_meta).insert(e.into(), meta);
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
        let mut out = Vec::new();
        for s in synced {
            let due = s.0.is_idle && s.3.is_some_and(|t| now.saturating_duration_since(t) >= ttl);
            if due && hibernate_one_idle(self, &s.0.employee_id, s.1.as_deref(), false) {
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
            if hibernate_one_idle(self, &c.0.employee_id, c.1.as_deref(), true) {
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
    let mut slept = Duration::ZERO;
    while let Some(mgr) = weak.upgrade() {
        let (draining, shutting_down) = {
            let reg = lock(&mgr.registry);
            (reg.draining, reg.shutting_down)
        };
        if shutting_down {
            return;
        }
        if slept >= interval {
            slept = Duration::ZERO;
            if !draining {
                mgr.sweep_resource_limits();
            }
        }
        drop(mgr);
        let step = Duration::from_millis(25).min(interval.saturating_sub(slept));
        thread::sleep(step);
        slept += step;
    }
}

fn is_handle_truly_idle(h: &LiveSessionHandle) -> (bool, Option<String>) {
    let permit_free = lock(&h.active_permit).is_none();
    let st = lock(&h.state);
    let idle = permit_free
        && st.state == SessionState::Idle
        && st.queued_turns == 0
        && !st.recovery_required
        && st.pending_approvals.is_empty()
        && st.backgrounds.active_count() == 0;
    (idle, st.run_id.clone())
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
                let (idle, run) = is_handle_truly_idle(h);
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

fn take_idle_candidate(
    mgr: &SessionManager,
    emp: &str,
    expected: Option<&str>,
) -> Option<(LiveSessionHandle, Option<SavedResume>)> {
    let mut reg = lock(&mgr.registry);
    if reg.draining || reg.shutting_down || reg.starting.contains(emp) {
        return None;
    }
    let handle = reg.live.get(emp)?;
    let (is_idle, run_id) = is_handle_truly_idle(handle);
    if !is_idle || expected.is_some_and(|exp| run_id.as_deref() != Some(exp)) {
        return None;
    }
    let st = lock(&handle.state);
    let resume = handle.partition.is_none().then(|| {
        SavedResume(
            st.runtime_session_id.clone().unwrap_or_default(),
            st.runtime_id.clone(),
            handle.route_digest.clone(),
            handle.workspace.clone(),
            reg.stop_epoch(emp),
        )
    });
    drop(st);
    let removed = reg.live.remove(emp)?;
    reg.starting.insert(emp.to_owned());
    Some((removed, resume))
}

fn hibernate_one_idle(mgr: &SessionManager, emp: &str, run_id: Option<&str>, is_rss: bool) -> bool {
    let Some((handle, resume_tpl)) = take_idle_candidate(mgr, emp, run_id) else {
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
            if s.0.is_empty() {
                s.0 = reaped.view.runtime_session_id.unwrap_or_default();
            }
            s
        })
        .filter(|s| !s.0.is_empty())
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
