//! Session spec, launch context, initial runtime state, and approval writable roots.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use org_agent_runtime::{
    AgentIdentity, AgentName, LaunchContext, LaunchPlan, PlanError, RuntimeId, Session, SessionId,
    SessionKind, SessionSpec, SpecPath, Token,
};
use org_office::organization_family::records::{Employee, PermissionPreset};
use org_proto::sessions::SessionState;
use org_proto::{ApiError, ErrorCode};

use crate::instructions::StandingInstructions;
use crate::preset::PresetConfig;
use crate::pump::SessionRuntimeState;
use crate::recovery::StallWatchdog;
use crate::router::AdmittedRoute;
use crate::subagents::{
    build_general_subagent_definition, write_subagent_definition, BackgroundRegistry,
};

use super::affinity::next_uuid_v4;
use super::environment::{
    apply_build_concurrency_env, assemble_launch_env, office_env_names, resolve_workspace,
    sanitize_agent_name,
};
use super::operator_home::ensure_operator_home;
use super::registry_guards::ensure_not_stopped_since;
use super::rust_toolchain::canonicalize_with_tail;
use super::sandbox_runner::SandboxPlan;
use super::warm::LaunchRouteOpts;
use super::{next_entropy_hex, PlannedSessionLaunch, PreparedSessionLaunch};

/// The model and reasoning effort a session runs with, as runtime tokens from the admitted
/// route (`TASK-181`). `None` keeps the runtime's default for that setting.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ModelChoice {
    pub(crate) model: Option<Token>,
    pub(crate) effort: Option<Token>,
}

pub(super) fn compute_launch_spec_hash(spec: &SessionSpec, route_digest_suffix: &str) -> String {
    if route_digest_suffix.is_empty() {
        spec.spec_hash()
    } else {
        format!("{}:route={route_digest_suffix}", spec.spec_hash())
    }
}

pub(super) fn build_launch_context(
    runtime_path: PathBuf,
    runtime_dir: &Path,
    tmpdir: &Path,
    session_id: &str,
    resume_runtime_session_id: Option<&str>,
    host_config_subdir: Option<&str>,
    allow_host_auth: bool,
) -> Result<LaunchContext, ApiError> {
    let resume = resume_runtime_session_id.and_then(|raw| SessionId::try_from(raw).ok());
    // Claude Code (`--session-id`), Codex, and Agy accept canonical lowercase UUIDs;
    // Claude Code 2.1.323+ rejects non-UUID strings before the `initialize` handshake.
    let new_session_id = if resume.is_some() {
        None
    } else {
        SessionId::try_from(session_id).ok()
    };
    Ok(LaunchContext {
        runtime_path,
        runtime_version: None,
        runtime_dir: canonical_or_spec(runtime_dir)?,
        tmpdir: canonical_or_spec(tmpdir)?,
        office_env_names: office_env_names(),
        new_session_id,
        resume,
        config_dir: crate::host_auth::host_config_dir_for(host_config_subdir, allow_host_auth),
    })
}

pub(crate) fn build_session_spec(
    runtime: RuntimeId,
    employee: &Employee,
    choice: ModelChoice,
    workspace: SpecPath,
    read_only_dirs: Vec<SpecPath>,
    instructions: StandingInstructions,
    preset_cfg: &PresetConfig,
) -> Result<SessionSpec, ApiError> {
    let safe_name = sanitize_agent_name(&employee.handle);
    let agent_name = AgentName::try_from(safe_name.as_str())
        .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid agent handle"))?;
    let title = employee.title_in_team.as_deref().unwrap_or("Agent");
    Ok(SessionSpec {
        runtime,
        kind: SessionKind::Home {},
        agent: AgentIdentity {
            name: agent_name,
            description: format!("{} ({title})", employee.name),
            prompt: format!("You are {} (@{}).", employee.name, employee.handle),
        },
        model: choice.model,
        effort: choice.effort,
        workspace,
        read_only_dirs,
        instructions: instructions.instructions,
        repository_instructions: instructions.repository_instructions,
        permission_mode: preset_cfg.permission_mode,
        policy: preset_cfg.policy.clone(),
        sandbox: preset_cfg.sandbox.clone(),
        mcp_servers: Default::default(),
        tools: preset_cfg.allowed_tools.clone(),
        operator_shell_prefixes: preset_cfg.operator_shell_prefixes.clone(),
    })
}

pub(super) fn canonical_or_spec(path: &Path) -> Result<SpecPath, ApiError> {
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let text = resolved.display().to_string();
    SpecPath::try_from(text.as_str())
        .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid workspace path"))
}

pub(super) fn build_initial_runtime_state(
    office_id: String,
    stall_timeout: Duration,
    prepared: &PreparedSessionLaunch,
) -> SessionRuntimeState {
    SessionRuntimeState {
        employee_id: prepared.emp_id.clone(),
        office_id,
        state: SessionState::Starting,
        run_id: Some(prepared.run_id.clone()),
        session_id: Some(prepared.session_id.clone()),
        runtime_id: Some(prepared.runtime_id.clone()),
        runtime_session_id: prepared
            .lifecycle
            .runtime_session_id
            .as_ref()
            .map(|s| s.as_str().to_owned()),
        private_home: prepared.home.display().to_string(),
        workspace_spec: prepared.workspace_spec.clone(),
        permission_preset: prepared.preset_cfg.preset.as_str().to_owned(),
        sandbox_enabled: prepared.preset_cfg.sandbox.enabled,
        allow_host_auth: prepared.allow_host_auth,
        spec_hash: prepared.spec_hash.clone(),
        watchdog: StallWatchdog::from_persisted(
            stall_timeout,
            Instant::now(),
            prepared.persisted_stalls,
            false,
        ),
        recovery_required: false,
        last_reply: None,
        last_error: None,
        backgrounds: BackgroundRegistry::new(),
        pending_approvals: Vec::new(),
        policy: prepared.preset_cfg.policy.clone(),
        preset: prepared.preset_cfg.preset,
        allowed_tools: prepared.preset_cfg.allowed_tools.clone(),
        writable_roots: effective_approval_writable_roots(prepared),
        suspended_on_asks: Vec::new(),
        waiting_for_member_id: None,
        waiting_for_name: None,
        suspended_since: None,
        pending_wake_count: 0,
        pending_wake_reason: None,
        queued_turns: 0,
        activity: crate::stream_hub::SessionActivity {
            supports_steer: prepared.runtime.capabilities().supports_native_steer(),
            ..Default::default()
        },
    }
}

pub(super) fn effective_approval_writable_roots(prepared: &PreparedSessionLaunch) -> Vec<SpecPath> {
    effective_approval_writable_roots_from(
        prepared.preset_cfg.preset,
        &prepared.preset_cfg.sandbox.writable_roots,
        &prepared.tmpdir,
    )
}

pub(crate) fn effective_approval_writable_roots_from(
    preset: PermissionPreset,
    sandbox_writable_roots: &[SpecPath],
    tmpdir: &Path,
) -> Vec<SpecPath> {
    let mut roots = sandbox_writable_roots.to_vec();
    if preset.is_read_only_fs() {
        return roots;
    }
    let canon_tmp = canonicalize_with_tail(tmpdir);
    if let Some(spec) = canon_tmp
        .to_str()
        .and_then(|s| SpecPath::try_from(s).ok())
        .filter(|spec| !roots.contains(spec))
    {
        roots.push(spec);
    }
    roots
}

impl super::SessionManager {
    #[expect(
        clippy::too_many_arguments,
        reason = "session spec assembly combines employee, runtime adapter, model choice, paths, preset, and sandbox plan"
    )]
    pub(super) fn assemble_launch_spec(
        &self,
        employee: &Employee,
        adapter: &dyn org_agent_runtime::AgentRuntime,
        choice: ModelChoice,
        route_digest_suffix: &str,
        home: &Path,
        workspace_real: &Path,
        workspace_spec: SpecPath,
        attachments_root: &Path,
        preset_cfg: &PresetConfig,
        sandbox_plan: &super::sandbox_runner::SandboxPlan,
    ) -> Result<(SessionSpec, String), ApiError> {
        let instructions = self.build_instructions(employee, home, workspace_real, preset_cfg)?;
        let mut read_only_dirs = self.collect_session_read_only_dirs(
            adapter,
            home,
            workspace_real,
            preset_cfg.sandbox.enabled,
        );
        Self::append_attachments_read_only_dir(&mut read_only_dirs, attachments_root);
        let mut spec = build_session_spec(
            adapter.id(),
            employee,
            choice,
            workspace_spec,
            read_only_dirs,
            instructions,
            preset_cfg,
        )?;
        self.sandbox_runner
            .adapt_session_spec(sandbox_plan, &mut spec);
        let spec_hash = compute_launch_spec_hash(&spec, route_digest_suffix);
        Ok((spec, spec_hash))
    }

    pub(super) fn prepare_launch_workspace(
        &self,
        employee: &Employee,
        workspace: Option<&str>,
        task_ref: Option<&str>,
        start_stop_epoch: u64,
    ) -> Result<LaunchWorkspace, ApiError> {
        let emp_id = employee.id.as_str();
        // A stop that overtook this start while its watch notices were read skips admission,
        // disk setup and the spawn (registration re-checks atomically, `TASK-190`).
        ensure_not_stopped_since(&self.registry, emp_id, start_stop_epoch)?;
        // Reclaim overdue idle warm sessions and enforce any office RSS budget before launching
        // a new supervised runtime process (`TASK-133`).
        self.sweep_resource_limits();
        // Before creating this agent's runtime directory or session-binding token on disk,
        // ensure no already-running sandboxed sibling lacks isolation for `emp_id` (`TASK-166`).
        self.sandbox_runner
            .enforce_sibling_isolation(self, emp_id)?;
        let home = self
            .office
            .ensure_private_home(emp_id)
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not prepare agent home"))?;
        let workspace_real = resolve_workspace(&home, workspace)?;
        ensure_operator_home(employee, &home, &workspace_real, &self.config.runtimes)?;
        let workspace_spec = canonical_or_spec(&workspace_real)?;
        // Auto-pre-prepare any repository-linked task worktrees BEFORE route admission (which
        // hashes `worktree_sandbox_digest`) and BEFORE sandbox planning so Turn 1 always launches
        // with exact least-privilege bare-cache grants (`TASK-135` AC #4).
        if let Some(task_ref) = task_ref {
            self.preprepare_task_worktrees(emp_id, &[task_ref]);
        }
        Ok(LaunchWorkspace {
            home,
            workspace_real,
            workspace_spec,
        })
    }

    fn prepare_launch_runtime_dirs(
        &self,
        employee: &Employee,
        admitted_preset: PermissionPreset,
        sandbox_enabled: bool,
        workspace_spec: &SpecPath,
        active_host_config_subdir: Option<&str>,
    ) -> Result<PreparedLaunchDirs, ApiError> {
        let emp_id = employee.id.as_str();
        let mut preset_cfg = self.build_employee_preset_cfg_for_runtime(
            emp_id,
            PermissionPreset::effective_for(employee.builtin_key, admitted_preset),
            sandbox_enabled,
            workspace_spec,
            true,
            active_host_config_subdir,
        )?;
        let (runtime_dir, tmpdir) = self.prepare_run_dirs(emp_id)?;
        let attachments_root = crate::turn_attachments::attachments_root(&runtime_dir);
        Self::append_attachments_write_denies(&mut preset_cfg, &attachments_root);
        let subagent_def =
            build_general_subagent_definition(preset_cfg.allowed_tools.as_deref(), None);
        let _ = write_subagent_definition(&runtime_dir, &subagent_def);
        Ok(PreparedLaunchDirs {
            runtime_dir,
            tmpdir,
            attachments_root,
            preset_cfg,
        })
    }

    pub(super) fn plan_runtime_invocation(
        &self,
        employee: &Employee,
        admitted: &mut AdmittedRoute,
        binary: PathBuf,
        workspace: &LaunchWorkspace,
        sandbox_plan: SandboxPlan,
        opts: &LaunchRouteOpts<'_>,
    ) -> Result<RuntimeInvocationPlan, ApiError> {
        let emp_id = employee.id.as_str();
        let host_profile = admitted.adapter.host_profile();
        let allow_host_auth = crate::host_auth::allows_host_subscription_auth(
            &admitted.auth_env,
            admitted.auth_mode,
            host_profile.uses_host_oauth_token_injection,
        );
        let active_host_config_subdir = allow_host_auth
            .then_some(host_profile.host_config_subdir)
            .flatten();
        let dirs = self.prepare_launch_runtime_dirs(
            employee,
            admitted.preset,
            admitted.sandbox_enabled,
            &workspace.workspace_spec,
            active_host_config_subdir,
        )?;
        let model_token = admitted.model_token.take();
        let effort_token = admitted.effort.take();
        let active_model = model_token.as_ref().map(|t| t.as_str().to_owned());
        let active_effort = effort_token.as_ref().map(|t| t.as_str().to_owned());
        let (spec, spec_hash) = self.assemble_launch_spec(
            employee,
            admitted.adapter.as_ref(),
            ModelChoice {
                model: model_token,
                effort: effort_token,
            },
            &admitted.route_digest_suffix,
            &workspace.home,
            &workspace.workspace_real,
            workspace.workspace_spec.clone(),
            &dirs.attachments_root,
            &dirs.preset_cfg,
            &sandbox_plan,
        )?;
        let run_id = format!("run-{}", next_entropy_hex(10)?);
        let session_id = next_uuid_v4()?;
        let mark_str = format!("m-{}", next_entropy_hex(12)?);
        let workspace_spec_str = workspace.workspace_spec.as_str().to_owned();
        let (effective_resume, fallback_notice) = self.resolve_effective_resume_for_launch(
            emp_id,
            admitted.adapter.as_ref(),
            &workspace_spec_str,
            &spec_hash,
            opts,
        );
        let ctx = build_launch_context(
            binary,
            &dirs.runtime_dir,
            &dirs.tmpdir,
            &session_id,
            effective_resume.as_deref(),
            host_profile.host_config_subdir,
            allow_host_auth,
        )?;
        let (mut plan, session) = admitted
            .adapter
            .plan_session(&spec, &ctx)
            .map_err(|err| map_plan_session_error(admitted.adapter.id().as_str(), err))?;
        let mut launch_env = assemble_launch_env(
            std::mem::take(&mut admitted.auth_env),
            std::mem::take(&mut admitted.endpoint_env),
        );
        apply_build_concurrency_env(
            &mut plan,
            &mut launch_env,
            self.config.resource_limits.build_concurrency_jobs,
        );
        Ok(RuntimeInvocationPlan {
            run_id,
            session_id,
            mark_str,
            active_model,
            active_effort,
            allow_host_auth,
            sandbox_plan,
            workspace_spec: workspace_spec_str,
            dirs,
            spec_hash,
            fallback_notice,
            auth_env: launch_env,
            ctx,
            plan,
            session,
        })
    }
}

pub(super) struct LaunchWorkspace {
    pub(super) home: PathBuf,
    pub(super) workspace_real: PathBuf,
    pub(super) workspace_spec: SpecPath,
}

struct PreparedLaunchDirs {
    runtime_dir: PathBuf,
    tmpdir: PathBuf,
    attachments_root: PathBuf,
    preset_cfg: PresetConfig,
}

pub(super) struct RuntimeInvocationPlan {
    run_id: String,
    session_id: String,
    mark_str: String,
    active_model: Option<String>,
    active_effort: Option<String>,
    allow_host_auth: bool,
    sandbox_plan: SandboxPlan,
    workspace_spec: String,
    dirs: PreparedLaunchDirs,
    spec_hash: String,
    fallback_notice: Option<String>,
    auth_env: Vec<(OsString, OsString)>,
    ctx: LaunchContext,
    plan: LaunchPlan,
    session: Box<dyn Session>,
}

fn map_plan_session_error(runtime_id: &str, err: PlanError) -> ApiError {
    match err {
        PlanError::Unsupported("effort") => ApiError::new(
            ErrorCode::InvalidRequest,
            format!(
                "runtime `{runtime_id}` does not support a separate reasoning effort override; omit `effort` and select an effort-specific model id instead"
            ),
        ),
        _ => ApiError::new(ErrorCode::Unavailable, "runtime rejected session spec"),
    }
}

pub(super) fn assemble_planned_session_launch(
    employee: &Employee,
    admitted: AdmittedRoute,
    permit: Option<(String, u64)>,
    workspace: LaunchWorkspace,
    workspace_arg: Option<String>,
    opts: LaunchRouteOpts<'_>,
    invocation: RuntimeInvocationPlan,
) -> PlannedSessionLaunch {
    PlannedSessionLaunch {
        emp_id: employee.id.as_str().to_owned(),
        employee_name: employee.name.clone(),
        employee_handle: employee.handle.clone(),
        run_id: invocation.run_id,
        session_id: invocation.session_id,
        mark_str: invocation.mark_str,
        runtime_id: admitted.adapter.id().as_str().to_owned(),
        runtime: Arc::clone(&admitted.adapter),
        supports_mid_turn: admitted.adapter.capabilities().supports_mid_turn_delivery(),
        model_supports_vision: admitted.model_supports_vision,
        allow_host_auth: invocation.allow_host_auth,
        sandbox_plan: invocation.sandbox_plan,
        connection_id: admitted.connection_id,
        route_digest: admitted.route_digest_suffix,
        oauth_expires_at_ms: admitted.oauth_expires_at_ms,
        oauth_token_fp: admitted.oauth_token_fp,
        oauth_refresh_attempted: opts.oauth_refresh_attempted,
        workspace_arg,
        permit,
        task_ref: opts.task_ref.map(str::to_owned),
        tried_connections: opts.tried_connections,
        start_stop_epoch: opts.start_stop_epoch,
        partition: opts.partition,
        active_model: invocation.active_model,
        active_effort: invocation.active_effort,
        turn_override: opts.turn_override,
        home: workspace.home,
        workspace_real: workspace.workspace_real,
        workspace_spec: invocation.workspace_spec,
        runtime_dir: invocation.dirs.runtime_dir,
        attachments_root: invocation.dirs.attachments_root,
        tmpdir: invocation.dirs.tmpdir,
        preset_cfg: invocation.dirs.preset_cfg,
        spec_hash: invocation.spec_hash,
        fallback_notice: invocation.fallback_notice,
        auth_env: invocation.auth_env,
        ctx: invocation.ctx,
        plan: invocation.plan,
        session: invocation.session,
    }
}
