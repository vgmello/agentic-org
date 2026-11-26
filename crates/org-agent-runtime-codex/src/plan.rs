mod paths;

use std::collections::{BTreeMap, BTreeSet};

use org_agent_runtime::{
    runtime_env_add, static_env_names, validate_plan_paths, validate_standard_session_kind,
    Capabilities, EnvName, FileLocation, LaunchContext, LaunchPlan, PermissionMode, PlanError,
    PlannedFile, RelPath, RuleScope, SessionId, SessionKind, SessionSpec, SignalTarget, Stdio,
    StopPlan, StopSignal, StopStep, ToolCapability,
};

use self::paths::{path_is_same_or_under, paths_overlap_via_alias_only};

const INSTRUCTIONS_FILE: &str = "codex-instructions.md";

/// Office-curated baseline POSIX environment variables preserved in Codex tool shells
/// alongside `ctx.office_env_names` so standard CLI tools (`agent-os`, `git`, `cargo`)
/// resolve on `PATH` without exposing ambient secrets (`TASK-94`, `doc-29` §6).
const BASELINE_SHELL_ENV_NAMES: &[&str] = &["PATH", "HOME", "TMPDIR", "LANG", "TERM"];

/// In-band `thread/start` or `thread/resume` parameters carried into [`crate::session::CodexSession`].
#[derive(Debug, Clone)]
pub(crate) struct CodexThreadConfig {
    pub(crate) cwd: String,
    pub(crate) model: Option<String>,
    pub(crate) effort: Option<String>,
    pub(crate) sandbox_mode: &'static str,
    pub(crate) uses_office_profile: bool,
    pub(crate) writable_roots: Vec<String>,
    pub(crate) instructions: String,
    pub(crate) resume_thread_id: Option<SessionId>,
}

const GRANULAR_APPROVAL_POLICY_TOML: &str =
    "approval_policy={granular={sandbox_approval=false,rules=true,mcp_elicitations=false,request_permissions=false,skill_approval=false}}";

fn map_codex_sandbox_mode(mode: PermissionMode) -> Result<&'static str, PlanError> {
    match mode {
        PermissionMode::Ask => Ok("workspace-write"),
        PermissionMode::AcceptEdits => Ok("workspace-write"),
        PermissionMode::Plan => Ok("read-only"),
        _ => Err(PlanError::Unsupported("permission mode")),
    }
}

fn has_unenforced_legacy_denies(spec: &SessionSpec) -> bool {
    !spec.sandbox.enabled
        && (!spec.policy.deny.is_empty()
            || !spec.sandbox.deny_write.is_empty()
            || !spec.sandbox.deny_read.is_empty()
            || !spec.read_only_dirs.is_empty())
}

fn allows_unscoped_shell(spec: &SessionSpec) -> bool {
    !has_unenforced_legacy_denies(spec)
        && spec.policy.allow.iter().any(|rule| {
            matches!(rule.scope, RuleScope::Any)
                && matches!(rule.tool.as_str(), "*" | "shell" | "cap:shell")
        })
}

fn disables_shell_tool(spec: &SessionSpec) -> bool {
    let disallowed_by_tools = spec.tools.as_ref().is_some_and(|tools| {
        !tools.iter().any(|t| {
            let s = t.as_str();
            s == ToolCapability::Shell.as_rule_token() || s == "shell"
        })
    });
    let denied_by_policy = spec.policy.deny.iter().any(|rule| {
        matches!(rule.scope, RuleScope::Any | RuleScope::CommandPrefix(_))
            && matches!(rule.tool.as_str(), "*" | "shell" | "cap:shell")
    });
    // Codex 0.169.0–0.171.0 removed `approval_policy="untrusted"` and its Starlark execpolicy
    // has no catch-all wildcard prompt rule, so ordinary sandboxed shell commands execute
    // without asking the office. Fail closed (`features.shell_tool=false`) unless the spec
    // explicitly grants unscoped shell (`RuleScope::Any`) with no shell prefix denies and no
    // unenforced legacy denies (`sandbox.enabled == false`).
    let lacks_unscoped_shell_allow = !allows_unscoped_shell(spec);
    disallowed_by_tools || denied_by_policy || lacks_unscoped_shell_allow
}

fn allows_unscoped_edit(spec: &SessionSpec) -> bool {
    !has_unenforced_legacy_denies(spec)
        && matches!(spec.permission_mode, PermissionMode::AcceptEdits)
        && spec.policy.allow.iter().any(|rule| {
            matches!(rule.scope, RuleScope::Any)
                && matches!(rule.tool.as_str(), "*" | "apply_patch" | "cap:edit")
        })
}

fn disables_edit_tool(spec: &SessionSpec) -> bool {
    let disallowed_by_tools = spec.tools.as_ref().is_some_and(|tools| {
        !tools.iter().any(|t| {
            let s = t.as_str();
            s == ToolCapability::Edit.as_rule_token() || s == "apply_patch"
        })
    });
    let denied_by_policy = spec.policy.deny.iter().any(|rule| {
        matches!(rule.scope, RuleScope::Any)
            && matches!(rule.tool.as_str(), "*" | "apply_patch" | "cap:edit")
    });
    // Under Codex's granular approval policy (`sandbox_approval=false, rules=true`),
    // Codex auto-approves `apply_patch` inside any writable root without prompting the
    // office. Fail closed to read-only (`SafeAsk`, `ReadOnly`, or any spec without
    // unscoped `AcceptEdits` edit approval) so no native patch can execute unapproved.
    let lacks_unscoped_edit_allow = !allows_unscoped_edit(spec);
    disallowed_by_tools || denied_by_policy || lacks_unscoped_edit_allow
}

fn effective_codex_sandbox_mode(
    spec: &SessionSpec,
    caps: &Capabilities,
) -> Result<&'static str, PlanError> {
    let effective_mode = spec.effective_permission_mode(caps);
    let mut sandbox_mode = map_codex_sandbox_mode(effective_mode)?;
    if disables_edit_tool(spec) {
        sandbox_mode = "read-only";
    }
    Ok(sandbox_mode)
}

pub(crate) fn build_headless_plan(
    spec: &SessionSpec,
    ctx: &LaunchContext,
    caps: &Capabilities,
) -> Result<(LaunchPlan, CodexThreadConfig), PlanError> {
    validate_standard_session_kind(&spec.kind)?;
    validate_plan_paths(spec, ctx)?;
    validate_deny_rules(spec)?;
    validate_tool_allowlist(spec)?;
    let sandbox_mode = effective_codex_sandbox_mode(spec, caps)?;
    let uses_office_profile = spec.sandbox.enabled;
    let writable_roots = if sandbox_mode == "read-only" {
        Vec::new()
    } else {
        compute_writable_roots(spec, ctx)
    };

    let mut args = vec![
        "app-server".to_owned(),
        "--stdio".to_owned(),
        "-c".to_owned(),
        "analytics.enabled=false".to_owned(),
        "-c".to_owned(),
        GRANULAR_APPROVAL_POLICY_TOML.to_owned(),
    ];
    if !uses_office_profile {
        args.push("-c".to_owned());
        args.push(format!("sandbox_mode=\"{sandbox_mode}\""));
    }
    append_config_overrides(&mut args, spec, ctx, sandbox_mode, &writable_roots)?;

    let mut files = Vec::new();
    let combined_instructions = combine_instructions(spec);
    if !combined_instructions.is_empty() {
        files.push(PlannedFile {
            location: FileLocation::RuntimeDir,
            rel_path: RelPath::try_from(INSTRUCTIONS_FILE).expect("static rel path"),
            contents: combined_instructions.as_bytes().to_vec(),
            git_exclude: false,
        });
    }

    let thread_cfg = CodexThreadConfig {
        cwd: spec.workspace.as_str().to_owned(),
        model: spec.model.as_ref().map(|m| m.as_str().to_owned()),
        effort: spec.effort.as_ref().map(|e| e.as_str().to_owned()),
        sandbox_mode,
        uses_office_profile,
        writable_roots,
        instructions: combined_instructions,
        resume_thread_id: ctx.resume.clone(),
    };

    let plan = LaunchPlan {
        program: ctx.runtime_path.clone(),
        args,
        cwd: spec.workspace.clone(),
        env_add: runtime_env_add(ENV_ADD, CONFIG_DIR_ENV, ctx),
        dirs: Vec::new(),
        files,
        stdio: Stdio::Piped,
        runtime_session_id: ctx.resume.clone(),
        stop: StopPlan {
            steps: vec![
                StopStep::CloseStdin { grace_ms: 5_000 },
                StopStep::Signal {
                    signal: StopSignal::Terminate,
                    target: SignalTarget::Leader,
                    grace_ms: 5_000,
                },
            ],
        },
        cleanup_after_exit: Vec::new(),
        secret_env_names: secret_env_names(),
        auth_env_names: auth_env_names(),
    };
    Ok((plan, thread_cfg))
}

pub(crate) fn build_take_over_plan(
    spec: &SessionSpec,
    ctx: &LaunchContext,
    caps: &Capabilities,
    cols: u16,
    rows: u16,
) -> Result<LaunchPlan, PlanError> {
    validate_standard_session_kind(&spec.kind)?;
    if matches!(spec.kind, SessionKind::OneOff { .. }) {
        return Err(PlanError::InvalidSpec(
            "one-off sessions cannot be taken over".to_owned(),
        ));
    }
    validate_plan_paths(spec, ctx)?;
    validate_deny_rules(spec)?;
    validate_tool_allowlist(spec)?;
    let Some(resume_id) = ctx.resume.clone() else {
        return Err(PlanError::InvalidSpec(
            "take-over requires LaunchContext::resume to be set".to_owned(),
        ));
    };
    let sandbox_mode = effective_codex_sandbox_mode(spec, caps)?;
    let uses_office_profile = spec.sandbox.enabled;
    let writable_roots = if sandbox_mode == "read-only" {
        Vec::new()
    } else {
        compute_writable_roots(spec, ctx)
    };

    let mut args = vec![
        "resume".to_owned(),
        resume_id.as_str().to_owned(),
        "-c".to_owned(),
        GRANULAR_APPROVAL_POLICY_TOML.to_owned(),
    ];
    if !uses_office_profile {
        args.push("-c".to_owned());
        args.push(format!("sandbox_mode=\"{sandbox_mode}\""));
    }
    append_config_overrides(&mut args, spec, ctx, sandbox_mode, &writable_roots)?;

    Ok(LaunchPlan {
        program: ctx.runtime_path.clone(),
        args,
        cwd: spec.workspace.clone(),
        env_add: runtime_env_add(ENV_ADD, CONFIG_DIR_ENV, ctx),
        dirs: Vec::new(),
        files: Vec::new(),
        stdio: Stdio::Pty { cols, rows },
        runtime_session_id: Some(resume_id),
        stop: StopPlan {
            steps: vec![StopStep::Signal {
                signal: StopSignal::Terminate,
                target: SignalTarget::Leader,
                grace_ms: 5_000,
            }],
        },
        cleanup_after_exit: Vec::new(),
        secret_env_names: secret_env_names(),
        auth_env_names: auth_env_names(),
    })
}

fn validate_deny_rules(spec: &SessionSpec) -> Result<(), PlanError> {
    for rule in &spec.policy.deny {
        let raw = rule.tool.as_str();
        if ToolCapability::is_capability_token(raw)
            && ToolCapability::parse_rule_token(raw).is_none()
        {
            return Err(PlanError::Unsupported("tool capability"));
        }
        match &rule.scope {
            RuleScope::Any
            | RuleScope::Path(_)
            | RuleScope::CommandPrefix(_)
            | RuleScope::McpServer(_) => {}
            _ => return Err(PlanError::Unsupported("deny rule scope")),
        }
    }
    Ok(())
}

fn validate_tool_allowlist(spec: &SessionSpec) -> Result<(), PlanError> {
    if let Some(tools) = &spec.tools {
        for tool in tools {
            let raw = tool.as_str();
            if ToolCapability::is_capability_token(raw)
                && ToolCapability::parse_rule_token(raw).is_none()
            {
                return Err(PlanError::Unsupported("tool capability"));
            }
        }
    }
    Ok(())
}

fn compute_writable_roots(spec: &SessionSpec, ctx: &LaunchContext) -> Vec<String> {
    let mut writable: Vec<String> = spec
        .sandbox
        .writable_roots
        .iter()
        .map(|p| p.as_str().to_owned())
        .collect();
    if !writable.iter().any(|p| p == ctx.tmpdir.as_str()) {
        writable.push(ctx.tmpdir.as_str().to_owned());
    }
    if let Some(cfg_dir) = &ctx.config_dir {
        let cfg = cfg_dir.as_str().trim_end_matches('/');
        let prefix = format!("{cfg}/");
        writable.retain(|p| p != cfg && !p.starts_with(&prefix));
    }
    writable
}

fn path_pattern_base(pattern: &str) -> Option<String> {
    let trimmed = pattern.trim();
    if !trimmed.starts_with('/') {
        return None;
    }
    let mut parts = Vec::new();
    for seg in trimmed.split('/').filter(|s| !s.is_empty()) {
        if seg.contains('*') {
            break;
        }
        if seg.contains(['?', '[', ']']) {
            return None;
        }
        parts.push(seg);
    }
    if parts.is_empty() {
        return None;
    }
    Some(format!("/{}", parts.join("/")))
}

fn validate_codex_literal_fs_path(path: &str) -> Result<(), PlanError> {
    if path.contains(['*', '?', '[', ']']) {
        return Err(PlanError::Unsupported(
            "glob metacharacter in literal Codex filesystem path",
        ));
    }
    Ok(())
}

fn access_rank(level: &str) -> u8 {
    match level {
        "none" => 3,
        "read" => 2,
        "write" => 1,
        _ => 0,
    }
}

fn insert_narrow_fs(map: &mut BTreeMap<String, &'static str>, key: String, level: &'static str) {
    match map.get(&key).copied() {
        Some(existing) if access_rank(existing) >= access_rank(level) => {}
        _ => {
            map.insert(key, level);
        }
    }
}

fn validate_profile_literal_paths(
    spec: &SessionSpec,
    ctx: &LaunchContext,
    writable_roots: &[String],
) -> Result<(), PlanError> {
    // Reject literal paths containing Codex glob metacharacters (`*`, `?`, `[`, `]`) before
    // emitting `permissions.office.filesystem` so a literal directory named `**` can never be
    // misinterpreted as a parent-widening glob by Codex.
    validate_codex_literal_fs_path(spec.workspace.as_str())?;
    validate_codex_literal_fs_path(ctx.tmpdir.as_str())?;
    for root in &spec.sandbox.writable_roots {
        validate_codex_literal_fs_path(root.as_str())?;
    }
    for root in writable_roots {
        validate_codex_literal_fs_path(root.as_str())?;
    }
    for dir in &spec.read_only_dirs {
        validate_codex_literal_fs_path(dir.as_str())?;
    }
    if let Some(cfg_dir) = &ctx.config_dir {
        validate_codex_literal_fs_path(cfg_dir.as_str())?;
    }
    Ok(())
}

fn collect_profile_deny_entries(
    spec: &SessionSpec,
) -> Result<Vec<(String, &'static str)>, PlanError> {
    let mut deny_entries: Vec<(String, &'static str)> = Vec::new();
    for pat in &spec.sandbox.deny_write {
        let base = path_pattern_base(pat.as_str())
            .ok_or(PlanError::Unsupported("leading-wildcard path deny pattern"))?;
        deny_entries.push((base, "read"));
    }
    for pat in &spec.sandbox.deny_read {
        let base = path_pattern_base(pat.as_str())
            .ok_or(PlanError::Unsupported("leading-wildcard path deny pattern"))?;
        deny_entries.push((base, "none"));
    }
    for rule in &spec.policy.deny {
        if let RuleScope::Path(pat) = &rule.scope {
            let base = path_pattern_base(pat.as_str())
                .ok_or(PlanError::Unsupported("leading-wildcard path deny pattern"))?;
            let level = match rule.tool.as_str() {
                "cap:edit" | "apply_patch" => "read",
                _ => "none",
            };
            deny_entries.push((base, level));
        }
    }
    Ok(deny_entries)
}

fn collect_config_dir_entries(ctx: &LaunchContext) -> Vec<(String, &'static str)> {
    ctx.config_dir
        .as_ref()
        .map(|cfg_dir| {
            let trimmed = cfg_dir.as_str().trim_end_matches('/');
            vec![
                (trimmed.to_owned(), "read"),
                (format!("{trimmed}/auth.json"), "none"),
            ]
        })
        .unwrap_or_default()
}

fn check_profile_write_conflicts(
    spec: &SessionSpec,
    ctx: &LaunchContext,
    sandbox_mode: &str,
    writable_roots: &[String],
    all_write_denies: &[(String, &'static str)],
) -> Result<(), PlanError> {
    if all_write_denies
        .iter()
        .any(|(deny_base, _)| path_is_same_or_under(ctx.tmpdir.as_str(), deny_base))
    {
        return Err(PlanError::InvalidSpec(
            "tmpdir conflicts with a read-only or deny rule".to_owned(),
        ));
    }

    let mut candidate_writes: Vec<&str> = vec![ctx.tmpdir.as_str()];
    if sandbox_mode != "read-only" {
        candidate_writes.push(spec.workspace.as_str());
        for root in writable_roots {
            candidate_writes.push(root.as_str());
        }
    }
    let has_alias_overlap = candidate_writes.iter().any(|write_path| {
        all_write_denies
            .iter()
            .any(|(deny_base, _)| paths_overlap_via_alias_only(write_path, deny_base))
    });
    if has_alias_overlap {
        return Err(PlanError::InvalidSpec(
            "ambiguous macOS symlink-alias overlap between writable path and deny or read-only rule"
                .to_owned(),
        ));
    }
    Ok(())
}

fn has_ancestor_none_deny(path: &str, deny_entries: &[(String, &'static str)]) -> bool {
    deny_entries.iter().any(|(deny_base, deny_level)| {
        *deny_level == "none" && path_is_same_or_under(path, deny_base)
    })
}

fn insert_filtered_entries(
    fs_entries: &mut BTreeMap<String, &'static str>,
    entries: &[(String, &'static str)],
    deny_entries: &[(String, &'static str)],
) {
    for (base, level) in entries {
        if *level == "read" && has_ancestor_none_deny(base, deny_entries) {
            continue;
        }
        insert_narrow_fs(fs_entries, base.clone(), level);
    }
}

fn build_profile_fs_entries(
    spec: &SessionSpec,
    ctx: &LaunchContext,
    sandbox_mode: &str,
    writable_roots: &[String],
    deny_entries: &[(String, &'static str)],
    config_dir_entries: &[(String, &'static str)],
    read_only_entries: &[(String, &'static str)],
) -> BTreeMap<String, &'static str> {
    let mut fs_entries: BTreeMap<String, &'static str> = BTreeMap::new();
    fs_entries.insert("/".to_owned(), "read");
    if sandbox_mode == "read-only" {
        // When Edit is excluded or unapproved (`ReadOnly`, `SafeAsk`), emit ZERO `"write"`
        // entries anywhere in `permissions.office.filesystem` (including `TMPDIR`) so Codex
        // compiles the profile to `SandboxPolicy::ReadOnly` and blocks all native `apply_patch`
        // writes everywhere.
        fs_entries.insert(spec.workspace.as_str().to_owned(), "read");
        fs_entries.insert(ctx.tmpdir.as_str().to_owned(), "read");
    } else {
        fs_entries.insert(spec.workspace.as_str().to_owned(), "write");
        fs_entries.insert(ctx.tmpdir.as_str().to_owned(), "write");
        for root in writable_roots {
            fs_entries.insert(root.clone(), "write");
        }
    }
    for (dir, level) in read_only_entries {
        insert_narrow_fs(&mut fs_entries, dir.clone(), level);
    }

    // Never emit a `"write"` grant equal to or nested under any explicit deny or `read_only_dirs`
    // base, and never emit a `"read"` grant from `read_only_dirs` nested under a `"none"` deny.
    fs_entries.retain(|path, level| {
        if *level == "write" {
            return !deny_entries
                .iter()
                .chain(config_dir_entries.iter())
                .chain(read_only_entries.iter())
                .any(|(deny_base, _)| path_is_same_or_under(path, deny_base));
        }
        if *level == "read" && path != "/" {
            return !has_ancestor_none_deny(path, deny_entries);
        }
        true
    });

    insert_filtered_entries(&mut fs_entries, deny_entries, deny_entries);
    insert_filtered_entries(&mut fs_entries, config_dir_entries, deny_entries);
    fs_entries
}

fn append_permissions_profile_overrides(
    args: &mut Vec<String>,
    spec: &SessionSpec,
    ctx: &LaunchContext,
    sandbox_mode: &str,
    writable_roots: &[String],
) -> Result<(), PlanError> {
    if !spec.sandbox.enabled {
        return Ok(());
    }
    validate_profile_literal_paths(spec, ctx, writable_roots)?;
    // Emit the named `permissions.office` profile (`doc-29` §6) with `"/"="read"` base plus
    // literal workspace, writable roots, `TMPDIR`, fixed read/write denies, and office-socket-only
    // network isolation.
    let deny_entries = collect_profile_deny_entries(spec)?;
    let config_dir_entries = collect_config_dir_entries(ctx);
    let read_only_entries: Vec<(String, &'static str)> = spec
        .read_only_dirs
        .iter()
        .map(|d| (d.as_str().to_owned(), "read"))
        .collect();
    let all_write_denies: Vec<(String, &'static str)> = deny_entries
        .iter()
        .chain(config_dir_entries.iter())
        .chain(read_only_entries.iter())
        .cloned()
        .collect();
    check_profile_write_conflicts(spec, ctx, sandbox_mode, writable_roots, &all_write_denies)?;

    let fs_entries = build_profile_fs_entries(
        spec,
        ctx,
        sandbox_mode,
        writable_roots,
        &deny_entries,
        &config_dir_entries,
        &read_only_entries,
    );

    let fs_pairs: Vec<String> = fs_entries
        .iter()
        .map(|(k, v)| {
            format!(
                "{}={}",
                serde_json::to_string(k).expect("fs key encodes"),
                serde_json::to_string(v).expect("fs level encodes")
            )
        })
        .collect();
    let sock_pairs: Vec<String> = spec
        .sandbox
        .allowed_unix_sockets
        .iter()
        .map(|s| {
            format!(
                "{}=\"allow\"",
                serde_json::to_string(s.as_str()).expect("socket path encodes")
            )
        })
        .collect();

    args.push("-c".to_owned());
    args.push("features.network_proxy=true".to_owned());
    args.push("-c".to_owned());
    args.push("default_permissions=\"office\"".to_owned());
    args.push("-c".to_owned());
    args.push(format!(
        "permissions.office.filesystem={{{}}}",
        fs_pairs.join(",")
    ));
    args.push("-c".to_owned());
    args.push(format!(
        "permissions.office.network={{enabled=true,unix_sockets={{{}}}}}",
        sock_pairs.join(",")
    ));
    Ok(())
}

fn append_config_overrides(
    args: &mut Vec<String>,
    spec: &SessionSpec,
    ctx: &LaunchContext,
    sandbox_mode: &str,
    writable_roots: &[String],
) -> Result<(), PlanError> {
    if let Some(model) = &spec.model {
        args.push("-c".to_owned());
        args.push(format!("model=\"{}\"", model.as_str()));
    }
    if let Some(effort) = &spec.effort {
        args.push("-c".to_owned());
        args.push(format!("model_reasoning_effort=\"{}\"", effort.as_str()));
    }
    if disables_shell_tool(spec) {
        args.push("-c".to_owned());
        args.push("features.shell_tool=false".to_owned());
    }
    append_permissions_profile_overrides(args, spec, ctx, sandbox_mode, writable_roots)?;
    // Pin Codex's shell environment policy so office-curated baseline POSIX variables
    // (PATH, HOME, TMPDIR, LANG, TERM) and *TOKEN* office variables such as
    // AGENT_OS_SESSION_TOKEN are forwarded to tool shells while ambient secrets stay excluded
    // (`TASK-94`, `doc-29` §6).
    args.push("-c".to_owned());
    args.push("shell_environment_policy.ignore_default_excludes=true".to_owned());
    let excluded: BTreeSet<String> = secret_env_names()
        .into_iter()
        .chain(auth_env_names())
        .map(|n| n.as_str().to_owned())
        .collect();
    let mut env_names: Vec<&str> = BASELINE_SHELL_ENV_NAMES.to_vec();
    for name in &ctx.office_env_names {
        let s = name.as_str();
        if !excluded.contains(s) && !env_names.contains(&s) {
            env_names.push(s);
        }
    }
    let quoted_env: Vec<String> = env_names
        .iter()
        .map(|n| serde_json::to_string(n).expect("env name encodes"))
        .collect();
    args.push("-c".to_owned());
    args.push(format!(
        "shell_environment_policy.include_only=[{}]",
        quoted_env.join(",")
    ));

    // Clear ambient MCP servers before registering office-provided servers.
    args.push("-c".to_owned());
    args.push("mcp_servers={}".to_owned());
    for (name, server) in &spec.mcp_servers {
        let key = name.as_str();
        let cmd = serde_json::to_string(server.command.as_str()).expect("cmd encodes");
        args.push("-c".to_owned());
        args.push(format!("mcp_servers.{key}.command={cmd}"));
        let arr = serde_json::to_string(&server.args).expect("args encode");
        args.push("-c".to_owned());
        args.push(format!("mcp_servers.{key}.args={arr}"));
    }
    Ok(())
}

fn combine_instructions(spec: &SessionSpec) -> String {
    match (spec.agent.prompt.trim(), spec.instructions.trim()) {
        ("", "") => String::new(),
        (p, "") => p.to_owned(),
        ("", i) => i.to_owned(),
        (p, i) => format!("{p}\n\n{i}"),
    }
}

/// Codex's own switches, added to the office's environment.
const ENV_ADD: &[(&str, &str)] = &[("CODEX_DISABLE_TELEMETRY", "1")];

/// Where Codex keeps its config, set to `LaunchContext::config_dir` when given.
const CONFIG_DIR_ENV: &str = "CODEX_HOME";

pub(crate) fn auth_env_names() -> Vec<EnvName> {
    static_env_names(&[
        "OPENAI_API_KEY",
        "CODEX_API_KEY",
        "CODEX_ACCESS_TOKEN",
        "OPENAI_BASE_URL",
    ])
}

fn secret_env_names() -> Vec<EnvName> {
    static_env_names(&[
        "OPENAI_API_KEY",
        "CODEX_API_KEY",
        "CODEX_ACCESS_TOKEN",
        "OPENAI_ORG_ID",
    ])
}
