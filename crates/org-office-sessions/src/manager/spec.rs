//! Session spec, launch context, initial runtime state, approval writable roots, and Linux Claude
//! `bwrap` deny pruning (`TASK-144`).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use org_agent_runtime::{
    AgentIdentity, AgentName, LaunchContext, PathPattern, RuntimeId, SandboxSpec, SessionId,
    SessionKind, SessionSpec, SpecPath, Token,
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

pub(super) fn is_linux_claude_bwrap(sandbox_enabled: bool, runtime_id: &str) -> bool {
    sandbox_enabled
        && runtime_id == "claude"
        && !org_platform::confinement::unix_socket_path_filter_supported()
}

pub(super) fn apply_linux_claude_bwrap_pruning(spec: &mut SessionSpec) {
    // Keep `preset_cfg.policy.deny` 100% complete in `SessionManager` (enforced by
    // `decide_escalation_with_roots` over `--permission-mode manual` on every tool call),
    // while pruning absent concrete paths from Linux Claude's `spec.sandbox` and
    // `spec.policy.deny` so Claude 2.1.x `@anthropic-ai/sandbox-runtime` (`bwrap`), which
    // merges `permissions.deny` into `bwrap` mounts, never materializes permanent 0-byte
    // `0444` files on the host (`TASK-144`).
    if prune_nonexistent_bwrap_denies(&mut spec.sandbox, &mut spec.policy) {
        // Pin approval-bridge mediation explicitly whenever any policy deny rule is pruned:
        // never rely on `capabilities().enforces_deny_rules` or surviving deny count to clear
        // `permissions.allow` or downgrade `AcceptEdits -> Ask`.
        spec.policy.allow.clear();
        if spec.permission_mode == org_agent_runtime::PermissionMode::AcceptEdits {
            spec.permission_mode = org_agent_runtime::PermissionMode::Ask;
        }
    }
}

/// The model and reasoning effort a session runs with, as runtime tokens from the admitted
/// route (`TASK-181`). `None` keeps the runtime's default for that setting.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ModelChoice {
    pub(crate) model: Option<Token>,
    pub(crate) effort: Option<Token>,
}

#[expect(
    clippy::too_many_arguments,
    reason = "launch spec builder takes runtime, employee, model choice, workspace, read-only dirs, instructions, preset, and bwrap flag"
)]
pub(super) fn build_session_spec_for_launch(
    runtime: RuntimeId,
    employee: &Employee,
    choice: ModelChoice,
    workspace: SpecPath,
    read_only_dirs: Vec<SpecPath>,
    instructions: StandingInstructions,
    preset_cfg: &PresetConfig,
    linux_claude_bwrap: bool,
) -> Result<SessionSpec, ApiError> {
    let mut spec = build_session_spec(
        runtime,
        employee,
        choice,
        workspace,
        read_only_dirs,
        instructions,
        preset_cfg,
    )?;
    if linux_claude_bwrap {
        apply_linux_claude_bwrap_pruning(&mut spec);
    }
    Ok(spec)
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
    runtime_name: &str,
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
        config_dir: crate::host_auth::host_config_dir_for(runtime_name, allow_host_auth),
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

/// `raw` without its trailing directory glob: any run of `/**` (a malformed `dir/**/**`
/// included, `TASK-190`), or else one `/*`.
fn strip_dir_glob_suffix(raw: &str) -> &str {
    let mut stripped = raw;
    while let Some(shorter) = stripped.strip_suffix("/**") {
        stripped = shorter;
    }
    if stripped.len() == raw.len() {
        return raw.strip_suffix("/*").unwrap_or(raw);
    }
    stripped
}

pub(super) fn pattern_exists_for_bwrap(pat: &PathPattern) -> bool {
    let raw = pat.as_str();
    let stripped = strip_dir_glob_suffix(raw);
    let prefix = if stripped.is_empty() { "/" } else { stripped };
    if prefix.contains('*') || prefix.contains('?') || prefix.contains('[') {
        return true;
    }
    std::fs::symlink_metadata(prefix).is_ok()
}

/// Normalizes a concrete existing directory glob (`<dir>/**` or `<dir>/*` with no internal
/// wildcards) into `<dir>` for Linux Claude `bwrap` so `@anthropic-ai/sandbox-runtime` mounts
/// `--tmpfs <dir>` atomically at the directory boundary instead of traversing inside `<dir>` and
/// following symlinks (such as a sibling agent's `.cargo/registry` or `.rustup`, `TASK-163`).
pub(super) fn normalize_bwrap_concrete_dir_pattern(pat: &PathPattern) -> (PathPattern, bool) {
    let raw = pat.as_str();
    let stripped = strip_dir_glob_suffix(raw);
    if stripped.len() == raw.len()
        || stripped.is_empty()
        || stripped.contains('*')
        || stripped.contains('?')
        || stripped.contains('[')
    {
        return (pat.clone(), false);
    }
    if std::fs::symlink_metadata(stripped).is_ok_and(|m| m.file_type().is_dir()) {
        if let Ok(normalized) = PathPattern::try_from(stripped) {
            return (normalized, true);
        }
    }
    (pat.clone(), false)
}

/// Drops non-existent concrete paths from Linux Claude `bwrap` `sandbox.deny_read`,
/// `sandbox.deny_write`, and `policy.deny` `RuleScope::Path` entries (`TASK-144`), and normalizes
/// concrete existing directory globs (`dir/**` -> `dir`) on `deny_read`, `deny_write` and
/// `policy.deny` so `@anthropic-ai/sandbox-runtime` never traverses symlinks inside denied
/// sibling agent homes (`TASK-163`, `TASK-190`). Returns `true` if any `policy.deny` rule was pruned or normalized. Callers must
/// leave `preset_cfg.policy.deny` untouched so `SessionManager`'s approval bridge
/// (`decide_escalation_with_roots`) retains 100% coverage of absent/future and glob paths.
pub(crate) fn prune_nonexistent_bwrap_denies(
    sandbox: &mut SandboxSpec,
    policy: &mut org_agent_runtime::PermissionPolicy,
) -> bool {
    sandbox.deny_read.retain(pattern_exists_for_bwrap);
    for pat in &mut sandbox.deny_read {
        let (norm, _) = normalize_bwrap_concrete_dir_pattern(pat);
        *pat = norm;
    }
    sandbox.deny_write.retain(pattern_exists_for_bwrap);
    for pat in &mut sandbox.deny_write {
        let (norm, _) = normalize_bwrap_concrete_dir_pattern(pat);
        *pat = norm;
    }
    let before = policy.deny.len();
    policy.deny.retain(|rule| match &rule.scope {
        org_agent_runtime::RuleScope::Path(pat) => pattern_exists_for_bwrap(pat),
        _ => true,
    });
    let mut modified = policy.deny.len() < before;
    for rule in &mut policy.deny {
        if let org_agent_runtime::RuleScope::Path(pat) = &mut rule.scope {
            let (norm, changed) = normalize_bwrap_concrete_dir_pattern(pat);
            if changed {
                *pat = norm;
                modified = true;
            }
        }
    }
    modified
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
