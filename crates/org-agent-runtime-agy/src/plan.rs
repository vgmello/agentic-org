use serde_json::{json, Map, Value};

use org_agent_runtime::{
    Capabilities, EnvName, FileLocation, LaunchContext, LaunchPlan, PermissionMode, PlanError,
    PlannedFile, RelPath, Rule, RuleScope, SessionKind, SessionSpec, SignalTarget, SpecPath, Stdio,
    StopPlan, StopSignal, StopStep,
};

const SETTINGS_FILE: &str = "agy-settings.json";
const INSTRUCTIONS_FILE: &str = "agy-instructions.md";

/// Expected Antigravity execution mode name (never `yolo` or bypass).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExpectedMode {
    pub(crate) cli_flag: Option<&'static str>,
    pub(crate) reported_name: &'static str,
}

pub(crate) fn map_permission_mode(mode: PermissionMode) -> Result<ExpectedMode, PlanError> {
    match mode {
        PermissionMode::Ask => Ok(ExpectedMode {
            cli_flag: None,
            reported_name: "ask",
        }),
        PermissionMode::AcceptEdits => Ok(ExpectedMode {
            cli_flag: Some("accept-edits"),
            reported_name: "accept-edits",
        }),
        PermissionMode::Plan => Ok(ExpectedMode {
            cli_flag: Some("plan"),
            reported_name: "plan",
        }),
        _ => Err(PlanError::Unsupported("permission mode")),
    }
}

pub(crate) fn validate_session_kind(kind: &SessionKind) -> Result<(), PlanError> {
    match kind {
        SessionKind::Home {} | SessionKind::Task { .. } | SessionKind::OneOff { .. } => Ok(()),
        _ => Err(PlanError::Unsupported("session kind")),
    }
}

pub(crate) fn build_headless_plan(
    spec: &SessionSpec,
    ctx: &LaunchContext,
    caps: &Capabilities,
) -> Result<(LaunchPlan, ExpectedMode), PlanError> {
    validate_session_kind(&spec.kind)?;
    validate_paths(spec, ctx)?;
    let effective_mode = spec.effective_permission_mode(caps);
    let expected_mode = map_permission_mode(effective_mode)?;

    let settings_bytes = build_settings_json(spec, ctx, caps, &expected_mode)?;
    let settings_path = format!("{}/{SETTINGS_FILE}", ctx.runtime_dir.as_str());

    let mut args = vec![
        "-p".to_owned(),
        "--input-format".to_owned(),
        "stream-json".to_owned(),
        "--output-format".to_owned(),
        "stream-json".to_owned(),
        "--disable-slash-commands".to_owned(),
        "--settings".to_owned(),
        settings_path,
        "--agent".to_owned(),
        spec.agent.name.as_str().to_owned(),
    ];
    if let Some(flag) = expected_mode.cli_flag {
        args.push("--mode".to_owned());
        args.push(flag.to_owned());
    }
    if spec.sandbox.enabled {
        args.push("--sandbox".to_owned());
    }
    if let Some(resume_id) = &ctx.resume {
        args.push("--conversation".to_owned());
        args.push(resume_id.as_str().to_owned());
    }
    append_common_options(&mut args, spec);

    let files = build_planned_files(spec, settings_bytes);
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
    Ok((plan, expected_mode))
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
    let Some(resume_id) = ctx.resume.clone() else {
        return Err(PlanError::InvalidSpec(
            "take-over requires LaunchContext::resume to be set".to_owned(),
        ));
    };
    let effective_mode = spec.effective_permission_mode(caps);
    let expected_mode = map_permission_mode(effective_mode)?;
    let settings_bytes = build_settings_json(spec, ctx, caps, &expected_mode)?;
    let settings_path = format!("{}/{SETTINGS_FILE}", ctx.runtime_dir.as_str());

    let mut args = vec![
        "--conversation".to_owned(),
        resume_id.as_str().to_owned(),
        "--settings".to_owned(),
        settings_path,
        "--agent".to_owned(),
        spec.agent.name.as_str().to_owned(),
    ];
    if let Some(flag) = expected_mode.cli_flag {
        args.push("--mode".to_owned());
        args.push(flag.to_owned());
    }
    if spec.sandbox.enabled {
        args.push("--sandbox".to_owned());
    }
    append_common_options(&mut args, spec);

    Ok(LaunchPlan {
        program: ctx.runtime_path.clone(),
        args,
        cwd: spec.workspace.clone(),
        env_add: build_env_add(ctx),
        dirs: Vec::new(),
        files: build_planned_files(spec, settings_bytes),
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

fn append_common_options(args: &mut Vec<String>, spec: &SessionSpec) {
    if let Some(model) = &spec.model {
        args.push("--model".to_owned());
        args.push(model.as_str().to_owned());
    }
    if let Some(effort) = &spec.effort {
        args.push("--effort".to_owned());
        args.push(effort.as_str().to_owned());
    }
    for dir in &spec.read_only_dirs {
        args.push("--add-dir".to_owned());
        args.push(dir.as_str().to_owned());
    }
}

fn build_planned_files(spec: &SessionSpec, settings_bytes: Vec<u8>) -> Vec<PlannedFile> {
    let mut files = vec![PlannedFile {
        location: FileLocation::RuntimeDir,
        rel_path: RelPath::try_from(SETTINGS_FILE).expect("static rel path"),
        contents: settings_bytes,
        git_exclude: false,
    }];
    if !spec.instructions.is_empty() {
        files.push(PlannedFile {
            location: FileLocation::RuntimeDir,
            rel_path: RelPath::try_from(INSTRUCTIONS_FILE).expect("static rel path"),
            contents: spec.instructions.as_bytes().to_vec(),
            git_exclude: false,
        });
    }
    files
}

fn build_settings_json(
    spec: &SessionSpec,
    ctx: &LaunchContext,
    caps: &Capabilities,
    expected_mode: &ExpectedMode,
) -> Result<Vec<u8>, PlanError> {
    let mut root = Map::new();
    root.insert("disableRemoteControl".to_owned(), Value::Bool(true));
    root.insert("disableAutoUpdate".to_owned(), Value::Bool(true));
    root.insert(
        "permissions".to_owned(),
        build_permissions_value(spec, ctx, caps, expected_mode)?,
    );
    root.insert("sandbox".to_owned(), build_sandbox_value(spec, ctx));
    root.insert("mcpServers".to_owned(), build_mcp_value(spec));
    root.insert("agent".to_owned(), build_agent_value(spec, ctx));
    if let Some(tools) = &spec.tools {
        root.insert(
            "allowedTools".to_owned(),
            Value::Array(
                tools
                    .iter()
                    .map(|t| Value::String(t.as_str().to_owned()))
                    .collect(),
            ),
        );
    }
    Ok(serde_json::to_vec(&Value::Object(root)).expect("settings JSON serializes"))
}

fn build_permissions_value(
    spec: &SessionSpec,
    ctx: &LaunchContext,
    caps: &Capabilities,
    expected_mode: &ExpectedMode,
) -> Result<Value, PlanError> {
    let mut deny_rules = Vec::new();
    for rule in &spec.policy.deny {
        let formatted = format_rule(rule).ok_or(PlanError::Unsupported("deny rule scope"))?;
        deny_rules.push(formatted);
    }
    for path in &spec.sandbox.deny_read {
        deny_rules.push(json!({ "tool": "read_file", "path": path.as_str() }));
    }
    for path in &spec.sandbox.deny_write {
        deny_rules.push(json!({ "tool": "write_file", "path": path.as_str() }));
        deny_rules.push(json!({ "tool": "edit_file", "path": path.as_str() }));
    }
    for dir in &spec.read_only_dirs {
        deny_rules.push(json!({ "tool": "write_file", "path": format!("{}/**", dir.as_str()) }));
        deny_rules.push(json!({ "tool": "edit_file", "path": format!("{}/**", dir.as_str()) }));
    }
    if let Some(cfg_dir) = &ctx.config_dir {
        let trimmed = cfg_dir.as_str().trim_end_matches('/');
        deny_rules.push(json!({ "tool": "write_file", "path": format!("{trimmed}/**") }));
        deny_rules.push(json!({ "tool": "edit_file", "path": format!("{trimmed}/**") }));
    }

    let mut allow_rules = Vec::new();
    if spec.keeps_allow_rules(caps) {
        for rule in &spec.policy.allow {
            if let Some(formatted) = format_rule(rule) {
                allow_rules.push(formatted);
            }
        }
    }

    Ok(json!({
        "mode": expected_mode.reported_name,
        "disableBypassMode": true,
        "deny": deny_rules,
        "allow": allow_rules,
    }))
}

fn build_sandbox_value(spec: &SessionSpec, ctx: &LaunchContext) -> Value {
    if !spec.sandbox.enabled {
        return json!({ "enabled": false });
    }
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
    let mut deny_write: Vec<String> = spec
        .sandbox
        .deny_write
        .iter()
        .map(|p| p.as_str().to_owned())
        .collect();
    if let Some(cfg_dir) = &ctx.config_dir {
        if !deny_write.iter().any(|p| p == cfg_dir.as_str()) {
            deny_write.push(cfg_dir.as_str().to_owned());
        }
    }
    let deny_read: Vec<String> = spec
        .sandbox
        .deny_read
        .iter()
        .map(|p| p.as_str().to_owned())
        .collect();
    let sockets: Vec<String> = spec
        .sandbox
        .allowed_unix_sockets
        .iter()
        .map(|p| p.as_str().to_owned())
        .collect();
    json!({
        "enabled": true,
        "allowUnsandboxedCommands": false,
        "filesystem": {
            "allowWrite": writable,
            "denyWrite": deny_write,
            "denyRead": deny_read,
        },
        "network": {
            "allowUnixSockets": sockets,
        }
    })
}

fn build_mcp_value(spec: &SessionSpec) -> Value {
    let mut servers = Map::new();
    for (name, server) in &spec.mcp_servers {
        servers.insert(
            name.as_str().to_owned(),
            json!({
                "command": server.command.as_str(),
                "args": server.args,
            }),
        );
    }
    Value::Object(servers)
}

fn build_agent_value(spec: &SessionSpec, ctx: &LaunchContext) -> Value {
    let mut obj = Map::new();
    obj.insert(
        "name".to_owned(),
        Value::String(spec.agent.name.as_str().to_owned()),
    );
    obj.insert(
        "description".to_owned(),
        Value::String(spec.agent.description.clone()),
    );
    obj.insert(
        "prompt".to_owned(),
        Value::String(spec.agent.prompt.clone()),
    );
    if !spec.instructions.is_empty() {
        obj.insert(
            "instructionsFile".to_owned(),
            Value::String(format!("{}/{INSTRUCTIONS_FILE}", ctx.runtime_dir.as_str())),
        );
    }
    Value::Object(obj)
}

fn format_rule(rule: &Rule) -> Option<Value> {
    let tool = rule.tool.as_str();
    match &rule.scope {
        RuleScope::Any if tool == "*" => None,
        RuleScope::Any => Some(json!({ "tool": tool })),
        RuleScope::Path(p) => Some(json!({ "tool": tool, "path": p.as_str() })),
        RuleScope::CommandPrefix(prefix) => {
            Some(json!({ "tool": tool, "commandPrefix": prefix.as_str() }))
        }
        RuleScope::McpServer(server) => Some(json!({ "tool": tool, "mcpServer": server.as_str() })),
        _ => None,
    }
}

fn build_env_add(ctx: &LaunchContext) -> Vec<(EnvName, String)> {
    let mut env = vec![
        (
            EnvName::try_from("AGY_DISABLE_AUTOUPDATE").expect("static env"),
            "1".to_owned(),
        ),
        (
            EnvName::try_from("AGY_SUBPROCESS_ENV_SCRUB").expect("static env"),
            "1".to_owned(),
        ),
    ];
    if let Some(cfg_dir) = &ctx.config_dir {
        env.push((
            EnvName::try_from("AGY_CONFIG_DIR").expect("static env"),
            cfg_dir.as_str().to_owned(),
        ));
    }
    env
}

pub(crate) fn auth_env_names() -> Vec<EnvName> {
    [
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "ANTIGRAVITY_API_KEY",
        "AGY_AUTH_TOKEN",
        "GOOGLE_GEMINI_BASE_URL",
    ]
    .into_iter()
    .map(|name| EnvName::try_from(name).expect("static auth env"))
    .collect()
}

fn secret_env_names() -> Vec<EnvName> {
    [
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "ANTIGRAVITY_API_KEY",
        "AGY_AUTH_TOKEN",
        "GOOGLE_APPLICATION_CREDENTIALS",
        "AGY_SESSION_TOKEN",
    ]
    .into_iter()
    .map(|name| EnvName::try_from(name).expect("static secret env"))
    .collect()
}
