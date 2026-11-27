//! Session spec, launch context, initial runtime state, and approval writable roots.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use org_agent_runtime::{
    AgentIdentity, AgentName, LaunchContext, RuntimeId, SessionId, SessionKind, SessionSpec,
    SpecPath, Token,
};
use org_office::organization_family::records::{Employee, PermissionPreset};
use org_proto::sessions::SessionState;
use org_proto::{ApiError, ErrorCode};

use crate::instructions::StandingInstructions;
use crate::preset::PresetConfig;
use crate::pump::SessionRuntimeState;
use crate::recovery::StallWatchdog;
use crate::subagents::BackgroundRegistry;

use super::environment::{office_env_names, sanitize_agent_name};
use super::rust_toolchain::canonicalize_with_tail;
use super::PreparedSessionLaunch;

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
