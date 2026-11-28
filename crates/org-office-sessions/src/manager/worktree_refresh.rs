//! Task worktree auto-pre-prepare port (`TASK-135` AC #4) and live sandbox refresh for
//! mid-turn `worktree.prepare` (`TASK-160`).
//!
//! ## Security invariants (`doc-35` least-privilege)
//! 1. All deny globs and bare-cache writable roots flow through the single choke-point
//!    [`SessionManager::build_employee_preset_cfg`], so refreshed live policies and settings
//!    files never expose sibling agents' homes/worktrees or untargeted repositories.
//! 2. `LiveSessionHandle::sandbox_plan` is **never** mutated in place when a running session's
//!    worktree set changes: already-spawned runtime processes may cache launch-time OS sandbox
//!    mounts/profiles in memory, so keeping the launch-time `SandboxPlan` guarantees
//!    `SandboxRunner::is_plan_stale` remains `true` until the process actually recycles cleanly.
//! 3. Live settings artifacts (such as a runtime's planned JSON settings file) are rewritten
//!    exclusively via [`org_supervise::runtime::write_runtime_planned_file`] (`O_NOFOLLOW` +
//!    `O_EXCL` mode `0600` temp file + atomic `renameat`), closing symlink-escape attacks.
//! 4. Neither `registry` nor `handle.state` mutexes are ever held across disk, SQLite, or Git
//!    work: state is snapshotted briefly, refreshed unlocked, and re-validated by `run_id`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use org_agent_runtime::{AgentRuntime, FileLocation};
use org_office::organization_family::records::{Employee, PermissionPreset};
use org_proto::sessions::SessionState;

use crate::credentials::now_ms;
use crate::instructions::StandingInstructions;
use crate::preset::PresetConfig;
use crate::pump::SessionRuntimeState;
use crate::wake::{PendingWakeItem, WakeKind};

use super::environment::resolve_workspace;
use super::sandbox_runner::{SandboxPlan, SandboxPrepareContext};
use super::spec::{
    build_launch_context, build_session_spec, canonical_or_spec,
    effective_approval_writable_roots_from, ModelChoice,
};
use super::SessionManager;

/// Actionable guidance returned on `worktree.prepare` when the caller's running sandboxed
/// session launched before the newly prepared worktree existed (`TASK-160`).
pub const MID_TURN_SANDBOX_REFRESH_HINT: &str = "Sandbox grants were updated for the new worktree, but the active sandboxed session launched before this worktree existed. Finish this turn now so the office automatically recycles the sandbox and resumes your session with full git access to this worktree.";

/// Consumer-owned port for auto-pre-preparing repository-linked task worktrees before Turn 1
/// sandbox planning (`TASK-135` AC #4).
///
/// Implemented by `GitService` in `org-office-server` so `org-office-sessions` stays free of
/// `org-git` dependencies.
pub trait TaskWorktreePreparer: Send + Sync {
    /// Idempotently pre-prepares project-linked task worktrees owned by `employee_id` for
    /// `task_ref`. Must be non-destructive, strictly project/member-authorized, and safe on
    /// error (never panicking or leaking unauthorized repositories).
    fn preprepare_for_task(&self, employee_id: &str, task_ref: &str);
}

/// Metadata reported to [`SessionManager::on_worktree_prepared`] after `worktree.prepare`
/// creates or activates an isolated task worktree (`TASK-160`).
#[derive(Debug, Clone, Copy)]
pub struct PreparedWorktreeNotice<'a> {
    pub employee_id: &'a str,
    /// `true` when the workspace owner itself invoked `worktree.prepare` (typically mid-turn).
    pub triggered_by_self: bool,
    /// `true` when an idle stale target session should be recycled asynchronously (explicit
    /// `worktree.prepare` RPC). `false` during Turn-1 auto-pre-prepare right before the caller's
    /// synchronous start/drain path reaps and cold-starts the stale target itself.
    pub recycle_idle_target: bool,
    /// Canonical task ID (used for task-scoped budget holds and Turn-2 route/process context).
    pub task_id: &'a str,
    pub task_key: &'a str,
    pub repository_id: &'a str,
    pub admin_dir_name: &'a str,
    pub branch: &'a str,
    pub worktree_path: &'a str,
}

/// Outcome of refreshing live session sandbox state after a worktree is prepared (`TASK-160`).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WorktreeSandboxRefreshOutcome {
    pub sandbox_refresh_required: bool,
    pub sandbox_refresh_hint: Option<String>,
    pub live_state_refreshed: bool,
    pub settings_file_refreshed: bool,
    pub idle_session_recycled: bool,
    pub continuation_queued: bool,
}

struct LiveRefreshSnapshot {
    state: Arc<Mutex<SessionRuntimeState>>,
    run_id: String,
    session_id: String,
    runtime_id: Option<String>,
    workspace: Option<String>,
    preset: PermissionPreset,
    sandbox_enabled: bool,
    allow_host_auth: bool,
    session_state: SessionState,
    recovery_required: bool,
    permit_free: bool,
    sandbox_plan: SandboxPlan,
    partitioned: bool,
}

struct IdleRecycleCandidate {
    employee_id: String,
    run_id: String,
    sandbox_plan: SandboxPlan,
}

struct SettingsRewriteInput<'a> {
    adapter: &'a dyn AgentRuntime,
    employee: &'a Employee,
    home: &'a Path,
    workspace_real: &'a Path,
    workspace_spec: &'a org_agent_runtime::SpecPath,
    runtime_dir: &'a Path,
    tmpdir: &'a Path,
    attachments_root: &'a Path,
    session_id: &'a str,
    allow_host_auth: bool,
    preset_cfg: &'a PresetConfig,
    fresh_plan: &'a SandboxPlan,
}

impl SessionManager {
    /// Registers the office-local pre-preparer for repository-linked task worktrees (`TASK-135`).
    pub fn set_task_worktree_preparer(&self, preparer: Arc<dyn TaskWorktreePreparer>) {
        *self
            .task_worktree_preparer
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(preparer);
    }

    /// Best-effort, unlocked auto-pre-prepare of repository-linked task worktrees for
    /// `employee_id` before route admission and sandbox planning (`TASK-135` AC #4).
    pub(crate) fn preprepare_task_worktrees(&self, employee_id: &str, task_refs: &[&str]) {
        if task_refs.is_empty() {
            return;
        }
        let preparer = self
            .task_worktree_preparer
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let Some(preparer) = preparer else {
            return;
        };
        let mut seen = Vec::with_capacity(task_refs.len());
        for &task_ref in task_refs {
            let trimmed = task_ref.trim();
            if !trimmed.is_empty() && !seen.contains(&trimmed) {
                seen.push(trimmed);
                preparer.preprepare_for_task(employee_id, trimmed);
            }
        }
    }

    /// Refreshes sandbox state after `worktree.prepare` persists an active worktree (`TASK-160`).
    /// Recycles stale idle siblings, refreshes busy siblings' live approval policies, and handles
    /// the target session (recycling if idle and requested, or refreshing live policy + settings
    /// and queueing deferred `SandboxRefresh` if busy).
    pub fn on_worktree_prepared(
        &self,
        notice: &PreparedWorktreeNotice<'_>,
    ) -> WorktreeSandboxRefreshOutcome {
        self.recycle_stale_idle_siblings(notice.employee_id, !notice.recycle_idle_target);
        self.refresh_stale_busy_siblings(notice.employee_id);
        let Some(snap) = self.snapshot_live_for_refresh(notice.employee_id) else {
            return WorktreeSandboxRefreshOutcome::default();
        };
        if snap.recovery_required || is_terminal_session_state(snap.session_state) {
            return WorktreeSandboxRefreshOutcome::default();
        }
        let plan_stale =
            self.sandbox_runner
                .is_plan_stale(&self.office, notice.employee_id, &snap.sandbox_plan);
        if snap.session_state == SessionState::Idle && snap.permit_free {
            if !notice.recycle_idle_target {
                return WorktreeSandboxRefreshOutcome::default();
            }
            return self.handle_idle_worktree_refresh(notice.employee_id, &snap, plan_stale);
        }
        self.handle_busy_worktree_refresh(notice, &snap, plan_stale)
    }

    fn snapshot_live_for_refresh(&self, employee_id: &str) -> Option<LiveRefreshSnapshot> {
        let reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        let handle = reg.live.get(employee_id)?;
        Some(snapshot_handle_for_refresh(handle))
    }

    fn refresh_stale_busy_siblings(&self, exclude_employee_id: &str) {
        let busy_siblings = self.collect_busy_sibling_snapshots(exclude_employee_id);
        for (emp_id, snap) in busy_siblings {
            if !snap.sandbox_enabled
                || self
                    .sandbox_runner
                    .is_plan_stale(&self.office, &emp_id, &snap.sandbox_plan)
            {
                let _ = self.refresh_live_grants_and_settings(&emp_id, &snap);
            }
        }
    }

    fn collect_busy_sibling_snapshots(
        &self,
        exclude_employee_id: &str,
    ) -> Vec<(String, LiveRefreshSnapshot)> {
        let reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        if reg.draining || reg.shutting_down {
            return Vec::new();
        }
        reg.live
            .iter()
            .filter(|(emp_id, _)| emp_id.as_str() != exclude_employee_id)
            .filter_map(|(emp_id, handle)| {
                let snap = snapshot_handle_for_refresh(handle);
                let is_idle_free = snap.session_state == SessionState::Idle && snap.permit_free;
                if snap.recovery_required
                    || is_terminal_session_state(snap.session_state)
                    || is_idle_free
                {
                    return None;
                }
                Some((emp_id.clone(), snap))
            })
            .collect()
    }

    fn recycle_stale_idle_siblings(&self, exclude_employee_id: &str, synchronous: bool) {
        let candidates = self.collect_idle_recycle_candidates(exclude_employee_id);
        let mgr = (!synchronous)
            .then(|| self.weak_self().and_then(|w| w.upgrade()))
            .flatten();
        for candidate in candidates {
            if !self.sandbox_runner.is_plan_stale(
                &self.office,
                &candidate.employee_id,
                &candidate.sandbox_plan,
            ) {
                continue;
            }
            if synchronous {
                let _ = super::reap::recycle_stale_warm_handle_sync(
                    self,
                    &candidate.employee_id,
                    &candidate.run_id,
                    "sandbox_topology_changed",
                );
            } else if let Some(ref mgr) = mgr {
                let _ = super::reap::recycle_stale_warm_handle_in(
                    mgr,
                    &candidate.employee_id,
                    &candidate.run_id,
                    "sandbox_topology_changed",
                );
            }
        }
    }

    fn collect_idle_recycle_candidates(
        &self,
        exclude_employee_id: &str,
    ) -> Vec<IdleRecycleCandidate> {
        let reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        if reg.draining || reg.shutting_down {
            return Vec::new();
        }
        reg.live
            .iter()
            .filter(|(emp_id, _)| {
                emp_id.as_str() != exclude_employee_id && !reg.starting.contains(emp_id.as_str())
            })
            .filter_map(|(emp_id, handle)| idle_recycle_candidate(emp_id, handle))
            .collect()
    }

    fn handle_idle_worktree_refresh(
        &self,
        employee_id: &str,
        snap: &LiveRefreshSnapshot,
        plan_stale: bool,
    ) -> WorktreeSandboxRefreshOutcome {
        let mut outcome = WorktreeSandboxRefreshOutcome::default();
        if !plan_stale {
            return outcome;
        }
        if let Some(adapter) = self.resolve_live_adapter(snap.runtime_id.as_deref()) {
            let _ = self.prepare_fresh_sandbox_plan(employee_id, snap.sandbox_enabled, &*adapter);
        }
        if let Some(mgr) = self.weak_self().and_then(|w| w.upgrade()) {
            outcome.idle_session_recycled = super::reap::recycle_stale_warm_handle_in(
                &mgr,
                employee_id,
                &snap.run_id,
                "sandbox_topology_changed",
            );
        }
        outcome
    }

    fn handle_busy_worktree_refresh(
        &self,
        notice: &PreparedWorktreeNotice<'_>,
        snap: &LiveRefreshSnapshot,
        plan_stale: bool,
    ) -> WorktreeSandboxRefreshOutcome {
        let mut outcome = WorktreeSandboxRefreshOutcome::default();
        let (state_ok, settings_ok) =
            self.refresh_live_grants_and_settings(notice.employee_id, snap);
        outcome.live_state_refreshed = state_ok;
        outcome.settings_file_refreshed = settings_ok;

        if !snap.sandbox_enabled || !plan_stale {
            return outcome;
        }
        outcome.sandbox_refresh_required = true;
        outcome.sandbox_refresh_hint = Some(MID_TURN_SANDBOX_REFRESH_HINT.to_owned());
        if notice.triggered_by_self && !snap.partitioned {
            outcome.continuation_queued = self.enqueue_sandbox_refresh_wake(notice, &snap.run_id);
        }
        outcome
    }

    fn refresh_live_grants_and_settings(
        &self,
        employee_id: &str,
        snap: &LiveRefreshSnapshot,
    ) -> (bool, bool) {
        let Ok(Some(employee)) = self.office.load_employee(employee_id) else {
            return (false, false);
        };
        let home = self.office.agent_private_home_path(employee_id);
        let Ok(workspace_real) = resolve_workspace(&home, snap.workspace.as_deref()) else {
            return (false, false);
        };
        let Ok(workspace_spec) = canonical_or_spec(&workspace_real) else {
            return (false, false);
        };
        let adapter = self.resolve_live_adapter(snap.runtime_id.as_deref());
        let fresh_plan = adapter
            .as_deref()
            .and_then(|rt| self.prepare_fresh_sandbox_plan(employee_id, snap.sandbox_enabled, rt));
        let active_host_config_subdir = snap
            .allow_host_auth
            .then(|| {
                adapter
                    .as_deref()
                    .and_then(|rt| rt.host_profile().host_config_subdir)
            })
            .flatten();
        let effective_preset = PermissionPreset::effective_for(employee.builtin_key, snap.preset);
        let Ok(mut preset_cfg) = self.build_employee_preset_cfg_for_runtime(
            employee_id,
            effective_preset,
            snap.sandbox_enabled,
            &workspace_spec,
            true,
            active_host_config_subdir,
        ) else {
            return (false, false);
        };
        let Ok((runtime_dir, tmpdir)) = self.prepare_run_dirs(employee_id) else {
            return (false, false);
        };
        let attachments_root = crate::turn_attachments::attachments_root(&runtime_dir);
        Self::append_attachments_write_denies(&mut preset_cfg, &attachments_root);
        let live_state_refreshed =
            self.update_live_runtime_policy(employee_id, snap, &preset_cfg, &tmpdir);
        if !live_state_refreshed {
            return (false, false);
        }
        let settings_file_refreshed = match (adapter.as_deref(), fresh_plan.as_ref()) {
            (Some(rt), Some(plan)) => self.rewrite_runtime_sandbox_settings(
                employee_id,
                snap,
                &SettingsRewriteInput {
                    adapter: rt,
                    employee: &employee,
                    home: &home,
                    workspace_real: &workspace_real,
                    workspace_spec: &workspace_spec,
                    runtime_dir: &runtime_dir,
                    tmpdir: &tmpdir,
                    attachments_root: &attachments_root,
                    session_id: &snap.session_id,
                    allow_host_auth: snap.allow_host_auth,
                    preset_cfg: &preset_cfg,
                    fresh_plan: plan,
                },
            ),
            _ => false,
        };
        (live_state_refreshed, settings_file_refreshed)
    }

    fn is_live_snapshot_current(&self, employee_id: &str, snap: &LiveRefreshSnapshot) -> bool {
        let reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        let Some(handle) = reg.live.get(employee_id) else {
            return false;
        };
        if !Arc::ptr_eq(&handle.state, &snap.state) {
            return false;
        }
        let guard = handle.state.lock().unwrap_or_else(|p| p.into_inner());
        guard.run_id.as_deref() == Some(snap.run_id.as_str())
            && !guard.recovery_required
            && !is_terminal_session_state(guard.state)
    }

    fn update_live_runtime_policy(
        &self,
        employee_id: &str,
        snap: &LiveRefreshSnapshot,
        preset_cfg: &PresetConfig,
        tmpdir: &Path,
    ) -> bool {
        let new_roots = effective_approval_writable_roots_from(
            preset_cfg.preset,
            &preset_cfg.sandbox.writable_roots,
            tmpdir,
        );
        let reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        let Some(handle) = reg.live.get(employee_id) else {
            return false;
        };
        if !Arc::ptr_eq(&handle.state, &snap.state) {
            return false;
        }
        let mut guard = handle.state.lock().unwrap_or_else(|p| p.into_inner());
        if guard.run_id.as_deref() != Some(snap.run_id.as_str())
            || guard.recovery_required
            || is_terminal_session_state(guard.state)
        {
            return false;
        }
        guard.policy.deny = preset_cfg.policy.deny.clone();
        guard.policy.version = guard.policy.version.saturating_add(1);
        guard.writable_roots = new_roots;
        true
    }

    fn resolve_live_adapter(&self, runtime_id: Option<&str>) -> Option<Arc<dyn AgentRuntime>> {
        runtime_id.and_then(|id| self.config.runtimes.resolve(id).ok())
    }

    fn prepare_fresh_sandbox_plan(
        &self,
        employee_id: &str,
        sandbox_enabled: bool,
        adapter: &dyn AgentRuntime,
    ) -> Option<SandboxPlan> {
        self.sandbox_runner
            .prepare_launch(&SandboxPrepareContext {
                office: &self.office,
                office_socket: &self.config.office_socket,
                agent_os_state_roots: &self.config.agent_os_state_roots,
                employee_id,
                sandbox_enabled,
                host_profile: adapter.host_profile(),
            })
            .ok()
    }

    fn rewrite_runtime_sandbox_settings(
        &self,
        employee_id: &str,
        snap: &LiveRefreshSnapshot,
        input: &SettingsRewriteInput<'_>,
    ) -> bool {
        let host_profile = input.adapter.host_profile();
        let Some(settings_file) = host_profile.sandbox_settings_file else {
            return false;
        };
        let mut read_only_dirs = self.collect_session_read_only_dirs(
            input.adapter,
            input.home,
            input.workspace_real,
            input.preset_cfg.sandbox.enabled,
        );
        Self::append_attachments_read_only_dir(&mut read_only_dirs, input.attachments_root);
        let Ok(mut spec) = build_session_spec(
            input.adapter.id(),
            input.employee,
            ModelChoice::default(),
            input.workspace_spec.clone(),
            read_only_dirs,
            StandingInstructions::default(),
            input.preset_cfg,
        ) else {
            return false;
        };
        self.sandbox_runner
            .adapt_session_spec(input.fresh_plan, &mut spec);
        let binary = self
            .config
            .runtimes
            .resolve_binary(input.adapter, None)
            .unwrap_or_else(|_| PathBuf::from("/usr/bin/env"));
        let Ok(ctx) = build_launch_context(
            binary,
            input.runtime_dir,
            input.tmpdir,
            input.session_id,
            None,
            host_profile.host_config_subdir,
            input.allow_host_auth,
        ) else {
            return false;
        };
        let Ok((plan, _)) = input.adapter.plan_session(&spec, &ctx) else {
            return false;
        };
        let Some(file) = plan.files.into_iter().find(|f| {
            f.location == FileLocation::RuntimeDir && f.rel_path.as_str() == settings_file
        }) else {
            return false;
        };
        if !self.is_live_snapshot_current(employee_id, snap) {
            return false;
        }
        org_supervise::runtime::write_runtime_planned_file(
            Path::new(ctx.runtime_dir.as_str()),
            Path::new(settings_file),
            &file.contents,
        )
        .is_ok()
    }

    fn enqueue_sandbox_refresh_wake(
        &self,
        notice: &PreparedWorktreeNotice<'_>,
        run_id: &str,
    ) -> bool {
        let item = PendingWakeItem {
            item_id: format!(
                "sandbox-refresh:{run_id}:{}:{}",
                notice.repository_id, notice.admin_dir_name
            ),
            kind: WakeKind::SandboxRefresh,
            sender_id: self.office.owner_member_id().to_owned(),
            sender_name: "Office Supervisor".into(),
            sender_handle: None,
            sender_relation: Some("office_supervisor".into()),
            sender_is_person: true,
            conversation_id: None,
            source_id: notice.task_id.to_owned(),
            summary: format!(
                "Sandbox grants refreshed for task {} worktree `{}` (branch `{}`). Continue your work; git status, git diff, and git commit are now permitted.",
                notice.task_key, notice.worktree_path, notice.branch
            ),
            attachment_ids: Vec::new(),
            enqueued_at_ms: now_ms(),
            session_affinity: None,
            model_override: None,
            effort_override: None,
        };
        self.enqueue_wake_durable(notice.employee_id, item).is_ok()
    }
}

fn snapshot_handle_for_refresh(handle: &super::LiveSessionHandle) -> LiveRefreshSnapshot {
    let permit_free = handle
        .active_permit
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .is_none();
    let st = handle.state.lock().unwrap_or_else(|p| p.into_inner());
    LiveRefreshSnapshot {
        state: Arc::clone(&handle.state),
        run_id: st.run_id.clone().unwrap_or_default(),
        session_id: st.session_id.clone().unwrap_or_default(),
        runtime_id: st.runtime_id.clone(),
        workspace: handle.workspace.clone(),
        preset: st.preset,
        sandbox_enabled: st.sandbox_enabled,
        allow_host_auth: st.allow_host_auth,
        session_state: st.state,
        recovery_required: st.recovery_required,
        permit_free,
        sandbox_plan: handle.sandbox_plan.clone(),
        partitioned: handle.partition.is_some(),
    }
}

fn idle_recycle_candidate(
    emp_id: &str,
    handle: &super::LiveSessionHandle,
) -> Option<IdleRecycleCandidate> {
    let permit_free = handle
        .active_permit
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .is_none();
    if !permit_free {
        return None;
    }
    let st = handle.state.lock().unwrap_or_else(|p| p.into_inner());
    if st.state != SessionState::Idle || st.recovery_required {
        return None;
    }
    Some(IdleRecycleCandidate {
        employee_id: emp_id.to_owned(),
        run_id: st.run_id.clone()?,
        sandbox_plan: handle.sandbox_plan.clone(),
    })
}

fn is_terminal_session_state(state: SessionState) -> bool {
    matches!(
        state,
        SessionState::Stopped
            | SessionState::Stopping
            | SessionState::Suspended
            | SessionState::StallLocked
            | SessionState::RecoveryRequired
    )
}
