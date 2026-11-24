use org_agent_runtime::{
    Capabilities, EnvName, FileLocation, LaunchContext, LaunchPlan, PermissionMode, PlanError,
    PlannedFile, RelPath, RuleScope, SessionId, SessionKind, SessionSpec, SignalTarget, SpecPath,
    Stdio, StopPlan, StopSignal, StopStep,
};

const INSTRUCTIONS_FILE: &str = "codex-instructions.md";

/// In-band `thread/start` or `thread/resume` parameters carried into [`crate::session::CodexSession`].
#[derive(Debug, Clone)]
pub(crate) struct CodexThreadConfig {
    pub(crate) cwd: String,
    pub(crate) model: Option<String>,
    pub(crate) effort: Option<String>,
    pub(crate) sandbox_mode: &'static str,
    pub(crate) approval_policy: &'static str,
    pub(crate) writable_roots: Vec<String>,
    pub(crate) instructions: String,
    pub(crate) resume_thread_id: Option<SessionId>,
}

fn map_codex_modes(mode: PermissionMode) -> Result<(&'static str, &'static str), PlanError> {
    match mode {
        PermissionMode::Ask => Ok(("workspace-write", "untrusted")),
        PermissionMode::AcceptEdits => Ok(("workspace-write", "on-request")),
        PermissionMode::Plan => Ok(("read-only", "untrusted")),
        _ => Err(PlanError::Unsupported("permission mode")),
    }
}

fn validate_session_kind(kind: &SessionKind) -> Result<(), PlanError> {
    match kind {
        SessionKind::Home {} | SessionKind::Task { .. } | SessionKind::OneOff { .. } => Ok(()),
        _ => Err(PlanError::Unsupported("session kind")),
    }
}

pub(crate) fn build_headless_plan(
    spec: &SessionSpec,
    ctx: &LaunchContext,
    caps: &Capabilities,
) -> Result<(LaunchPlan, CodexThreadConfig), PlanError> {
    validate_session_kind(&spec.kind)?;
    validate_paths(spec, ctx)?;
    validate_deny_rules(spec)?;
    let effective_mode = spec.effective_permission_mode(caps);
    let (sandbox_mode, approval_policy) = map_codex_modes(effective_mode)?;
    let writable_roots = compute_writable_roots(spec, ctx);

    let mut args = vec![
        "app-server".to_owned(),
        "--stdio".to_owned(),
        "-c".to_owned(),
        "analytics.enabled=false".to_owned(),
        "-c".to_owned(),
        format!("sandbox_mode=\"{sandbox_mode}\""),
        "-c".to_owned(),
        format!("approval_policy=\"{approval_policy}\""),
    ];
    append_config_overrides(&mut args, spec, ctx, &writable_roots);

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
        approval_policy,
        writable_roots,
        instructions: combined_instructions,
        resume_thread_id: ctx.resume.clone(),
    };

    let plan = LaunchPlan {
        program: ctx.runtime_path.clone(),
        args,
        cwd: spec.workspace.clone(),
        env_add: build_env_add(ctx),
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
    validate_session_kind(&spec.kind)?;
    if matches!(spec.kind, SessionKind::OneOff { .. }) {
        return Err(PlanError::InvalidSpec(
            "one-off sessions cannot be taken over".to_owned(),
        ));
    }
    validate_paths(spec, ctx)?;
    validate_deny_rules(spec)?;
    let Some(resume_id) = ctx.resume.clone() else {
        return Err(PlanError::InvalidSpec(
            "take-over requires LaunchContext::resume to be set".to_owned(),
        ));
    };
    let effective_mode = spec.effective_permission_mode(caps);
    let (sandbox_mode, approval_policy) = map_codex_modes(effective_mode)?;
    let writable_roots = compute_writable_roots(spec, ctx);

    let mut args = vec![
        "resume".to_owned(),
        resume_id.as_str().to_owned(),
        "-c".to_owned(),
        format!("sandbox_mode=\"{sandbox_mode}\""),
        "-c".to_owned(),
        format!("approval_policy=\"{approval_policy}\""),
    ];
    append_config_overrides(&mut args, spec, ctx, &writable_roots);

    Ok(LaunchPlan {
        program: ctx.runtime_path.clone(),
        args,
        cwd: spec.workspace.clone(),
        env_add: build_env_add(ctx),
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

fn validate_paths(spec: &SessionSpec, ctx: &LaunchContext) -> Result<(), PlanError> {
    if spec.workspace.is_root() {
        return Err(PlanError::InvalidSpec(
            "workspace must not be `/`".to_owned(),
        ));
    }
    if ctx.tmpdir.is_root() {
        return Err(PlanError::InvalidSpec("tmpdir must not be `/`".to_owned()));
    }
    if ctx.runtime_dir.is_root() {
        return Err(PlanError::InvalidSpec(
            "runtime_dir must not be `/`".to_owned(),
        ));
    }
    if ctx.config_dir.as_ref().is_some_and(SpecPath::is_root) {
        return Err(PlanError::InvalidSpec(
            "config_dir must not be `/`".to_owned(),
        ));
    }
    if spec.sandbox.writable_roots.iter().any(SpecPath::is_root) {
        return Err(PlanError::InvalidSpec(
            "sandbox writable_roots must not include `/`".to_owned(),
        ));
    }
    Ok(())
}

fn validate_deny_rules(spec: &SessionSpec) -> Result<(), PlanError> {
    for rule in &spec.policy.deny {
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

fn append_config_overrides(
    args: &mut Vec<String>,
    spec: &SessionSpec,
    ctx: &LaunchContext,
    writable_roots: &[String],
) {
    if let Some(model) = &spec.model {
        args.push("-c".to_owned());
        args.push(format!("model=\"{}\"", model.as_str()));
    }
    if let Some(effort) = &spec.effort {
        args.push("-c".to_owned());
        args.push(format!("model_reasoning_effort=\"{}\"", effort.as_str()));
    }
    if spec.sandbox.enabled && !writable_roots.is_empty() {
        let quoted: Vec<String> = writable_roots
            .iter()
            .map(|p| serde_json::to_string(p).expect("path string encodes"))
            .collect();
        args.push("-c".to_owned());
        args.push(format!(
            "sandbox_workspace_write.writable_roots=[{}]",
            quoted.join(",")
        ));
    }
    // Pin Codex's shell environment policy so *TOKEN* variables such as
    // AGENT_OS_SESSION_TOKEN are forwarded to tool shells (`doc-29` §6).
    args.push("-c".to_owned());
    args.push("shell_environment_policy.ignore_default_excludes=true".to_owned());
    let mut env_names: Vec<&str> = Vec::new();
    for name in &ctx.office_env_names {
        let s = name.as_str();
        if !env_names.contains(&s) {
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
}

fn combine_instructions(spec: &SessionSpec) -> String {
    match (spec.agent.prompt.trim(), spec.instructions.trim()) {
        ("", "") => String::new(),
        (p, "") => p.to_owned(),
        ("", i) => i.to_owned(),
        (p, i) => format!("{p}\n\n{i}"),
    }
}

fn build_env_add(ctx: &LaunchContext) -> Vec<(EnvName, String)> {
    let mut env = vec![(
        EnvName::try_from("CODEX_DISABLE_TELEMETRY").expect("static env"),
        "1".to_owned(),
    )];
    if let Some(cfg_dir) = &ctx.config_dir {
        env.push((
            EnvName::try_from("CODEX_HOME").expect("static env"),
            cfg_dir.as_str().to_owned(),
        ));
    }
    env
}

pub(crate) fn auth_env_names() -> Vec<EnvName> {
    [
        "OPENAI_API_KEY",
        "CODEX_API_KEY",
        "CODEX_ACCESS_TOKEN",
        "OPENAI_BASE_URL",
    ]
    .into_iter()
    .map(|name| EnvName::try_from(name).expect("static auth env"))
    .collect()
}

fn secret_env_names() -> Vec<EnvName> {
    [
        "OPENAI_API_KEY",
        "CODEX_API_KEY",
        "CODEX_ACCESS_TOKEN",
        "OPENAI_ORG_ID",
    ]
    .into_iter()
    .map(|name| EnvName::try_from(name).expect("static secret env"))
    .collect()
}
