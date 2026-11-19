use serde_json::{json, Map, Value};

use org_agent_runtime::{
    Capabilities, EnvName, FileLocation, LaunchContext, LaunchPlan, PermissionMode, PlanError,
    PlannedFile, RelPath, Rule, RuleScope, SessionId, SessionKind, SessionSpec, SignalTarget,
    SpecPath, Stdio, StopPlan, StopSignal, StopStep,
};

const SETTINGS_FILE: &str = "claude-settings.json";
const MCP_FILE: &str = "claude-mcp.json";
const INSTRUCTIONS_FILE: &str = "claude-instructions.md";

/// Known Claude Code built-in tools that act outside the office/sandbox or defer tools.
/// Whenever `spec.tools` is an explicit allowlist, every tool here that is not in the
/// allowlist is also passed to `--disallowedTools`.
const RISKY_BUILTIN_TOOLS: &[&str] = &[
    "ToolSearch",
    "WebFetch",
    "WebSearch",
    "EnterWorktree",
    "ExitWorktree",
    "CronCreate",
    "CronDelete",
    "CronList",
    "RemoteTrigger",
    "ScheduleWakeup",
    "Monitor",
    "SendMessage",
    "NotebookEdit",
];

/// Expected Claude permission-mode string reported by `initialize` / `system/init`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExpectedMode {
    pub(crate) cli_flag: &'static str,
    pub(crate) reported_name: &'static str,
}

pub(crate) fn map_permission_mode(mode: PermissionMode) -> Result<ExpectedMode, PlanError> {
    match mode {
        PermissionMode::Ask => Ok(ExpectedMode {
            cli_flag: "manual",
            reported_name: "default",
        }),
        PermissionMode::AcceptEdits => Ok(ExpectedMode {
            cli_flag: "acceptEdits",
            reported_name: "acceptEdits",
        }),
        PermissionMode::Plan => Ok(ExpectedMode {
            cli_flag: "plan",
            reported_name: "plan",
        }),
        _ => Err(PlanError::Unsupported("permission mode")),
    }
}

pub(crate) fn build_headless_plan(
    spec: &SessionSpec,
    ctx: &LaunchContext,
    caps: &Capabilities,
) -> Result<(LaunchPlan, ExpectedMode), PlanError> {
    validate_paths(spec, ctx)?;
    let (session_id, is_resume) = resolve_session_id(ctx)?;
    let effective_mode = spec.effective_permission_mode(caps);
    let expected_mode = map_permission_mode(effective_mode)?;

    let settings_bytes = build_settings_json(spec, ctx, caps)?;
    let mcp_bytes = build_mcp_json(spec);
    let agents_json = build_agents_json(spec);

    let settings_path = format!("{}/{SETTINGS_FILE}", ctx.runtime_dir.as_str());
    let mcp_path = format!("{}/{MCP_FILE}", ctx.runtime_dir.as_str());
    let instructions_path = format!("{}/{INSTRUCTIONS_FILE}", ctx.runtime_dir.as_str());

    let mut args = vec![
        "-p".to_owned(),
        "--input-format".to_owned(),
        "stream-json".to_owned(),
        "--output-format".to_owned(),
        "stream-json".to_owned(),
        "--verbose".to_owned(),
        "--include-partial-messages".to_owned(),
        "--forward-subagent-text".to_owned(),
        "--disable-slash-commands".to_owned(),
        "--setting-sources".to_owned(),
        String::new(),
        "--strict-mcp-config".to_owned(),
        "--mcp-config".to_owned(),
        mcp_path,
        "--settings".to_owned(),
        settings_path,
        "--permission-mode".to_owned(),
        expected_mode.cli_flag.to_owned(),
        "--permission-prompt-tool".to_owned(),
        "stdio".to_owned(),
        "--agents".to_owned(),
        agents_json,
        "--agent".to_owned(),
        spec.agent.name.as_str().to_owned(),
    ];

    if !spec.instructions.is_empty() {
        args.push("--append-system-prompt-file".to_owned());
        args.push(instructions_path);
    }

    if is_resume {
        args.push(format!("--resume={}", session_id.as_str()));
    } else {
        args.push(format!("--session-id={}", session_id.as_str()));
    }

    append_common_options(&mut args, spec);

    let mut files = vec![
        PlannedFile {
            location: FileLocation::RuntimeDir,
            rel_path: RelPath::try_from(SETTINGS_FILE).expect("constant relative path is valid"),
            contents: settings_bytes,
            git_exclude: false,
        },
        PlannedFile {
            location: FileLocation::RuntimeDir,
            rel_path: RelPath::try_from(MCP_FILE).expect("constant relative path is valid"),
            contents: mcp_bytes,
            git_exclude: false,
        },
    ];
    if !spec.instructions.is_empty() {
        files.push(PlannedFile {
            location: FileLocation::RuntimeDir,
            rel_path: RelPath::try_from(INSTRUCTIONS_FILE)
                .expect("constant relative path is valid"),
            contents: spec.instructions.as_bytes().to_vec(),
            git_exclude: false,
        });
    }

    let plan = LaunchPlan {
        program: ctx.runtime_path.clone(),
        args,
        cwd: spec.workspace.clone(),
        env_add: build_env_add(ctx),
        dirs: Vec::new(),
        files,
        stdio: Stdio::Piped,
        runtime_session_id: Some(session_id),
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

    let settings_bytes = build_settings_json(spec, ctx, caps)?;
    let mcp_bytes = build_mcp_json(spec);
    let agents_json = build_agents_json(spec);

    let settings_path = format!("{}/{SETTINGS_FILE}", ctx.runtime_dir.as_str());
    let mcp_path = format!("{}/{MCP_FILE}", ctx.runtime_dir.as_str());
    let instructions_path = format!("{}/{INSTRUCTIONS_FILE}", ctx.runtime_dir.as_str());

    let mut args = vec![
        format!("--resume={}", resume_id.as_str()),
        "--setting-sources".to_owned(),
        String::new(),
        "--strict-mcp-config".to_owned(),
        "--mcp-config".to_owned(),
        mcp_path,
        "--settings".to_owned(),
        settings_path,
        "--permission-mode".to_owned(),
        expected_mode.cli_flag.to_owned(),
        "--agents".to_owned(),
        agents_json,
        "--agent".to_owned(),
        spec.agent.name.as_str().to_owned(),
    ];

    if !spec.instructions.is_empty() {
        args.push("--append-system-prompt-file".to_owned());
        args.push(instructions_path);
    }

    append_common_options(&mut args, spec);

    let mut files = vec![
        PlannedFile {
            location: FileLocation::RuntimeDir,
            rel_path: RelPath::try_from(SETTINGS_FILE).expect("constant relative path is valid"),
            contents: settings_bytes,
            git_exclude: false,
        },
        PlannedFile {
            location: FileLocation::RuntimeDir,
            rel_path: RelPath::try_from(MCP_FILE).expect("constant relative path is valid"),
            contents: mcp_bytes,
            git_exclude: false,
        },
    ];
    if !spec.instructions.is_empty() {
        files.push(PlannedFile {
            location: FileLocation::RuntimeDir,
            rel_path: RelPath::try_from(INSTRUCTIONS_FILE)
                .expect("constant relative path is valid"),
            contents: spec.instructions.as_bytes().to_vec(),
            git_exclude: false,
        });
    }

    Ok(LaunchPlan {
        program: ctx.runtime_path.clone(),
        args,
        cwd: spec.workspace.clone(),
        env_add: build_env_add(ctx),
        dirs: Vec::new(),
        files,
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

fn resolve_session_id(ctx: &LaunchContext) -> Result<(SessionId, bool), PlanError> {
    if let Some(resume) = &ctx.resume {
        Ok((resume.clone(), true))
    } else if let Some(new_id) = &ctx.new_session_id {
        Ok((new_id.clone(), false))
    } else {
        Err(PlanError::InvalidSpec(
            "Claude sessions require either LaunchContext::new_session_id or LaunchContext::resume"
                .to_owned(),
        ))
    }
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
    if let Some(tools) = &spec.tools {
        args.push("--tools".to_owned());
        if tools.is_empty() {
            args.push(String::new());
        } else {
            let joined = tools
                .iter()
                .map(|t| t.as_str())
                .collect::<Vec<_>>()
                .join(",");
            args.push(joined);
        }
        let disallowed: Vec<&str> = RISKY_BUILTIN_TOOLS
            .iter()
            .copied()
            .filter(|risky| !tools.iter().any(|allowed| allowed.as_str() == *risky))
            .collect();
        if !disallowed.is_empty() {
            args.push("--disallowedTools".to_owned());
            args.push(disallowed.join(","));
        }
    }
    for dir in &spec.read_only_dirs {
        args.push("--add-dir".to_owned());
        args.push(dir.as_str().to_owned());
    }
}

fn build_settings_json(
    spec: &SessionSpec,
    ctx: &LaunchContext,
    caps: &Capabilities,
) -> Result<Vec<u8>, PlanError> {
    let mut root = Map::new();
    root.insert("autoMemoryEnabled".to_owned(), Value::Bool(false));

    // Permissions: deny always written before allow; allow rules are dropped whenever
    // `keeps_allow_rules` is false (deny wins over allow).
    let mut deny_rules = Vec::new();
    for rule in &spec.policy.deny {
        let formatted = format_rule(rule).ok_or(PlanError::Unsupported("deny rule scope"))?;
        deny_rules.push(Value::String(formatted));
    }
    // Also deny Edit/Write into read-only directories so file tools respect read_only_dirs.
    for dir in &spec.read_only_dirs {
        let glob = format!("{}/**", dir.as_str().trim_end_matches('/'));
        for tool in ["Edit", "Write", "MultiEdit"] {
            deny_rules.push(Value::String(format!("{tool}({glob})")));
        }
    }

    let mut allow_rules = Vec::new();
    if spec.keeps_allow_rules(caps) {
        for rule in &spec.policy.allow {
            // Grants we cannot express exactly are dropped (never widened).
            if let Some(formatted) = format_rule(rule) {
                allow_rules.push(Value::String(formatted));
            }
        }
    }

    let mut permissions = Map::new();
    permissions.insert("deny".to_owned(), Value::Array(deny_rules));
    permissions.insert("allow".to_owned(), Value::Array(allow_rules));
    root.insert("permissions".to_owned(), Value::Object(permissions));

    // Sandbox: always add `ctx.tmpdir` (and `ctx.config_dir` when set) to writable roots.
    if spec.sandbox.enabled {
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
            if !writable.iter().any(|p| p == cfg_dir.as_str()) {
                writable.push(cfg_dir.as_str().to_owned());
            }
        }

        let deny_write: Vec<Value> = spec
            .sandbox
            .deny_write
            .iter()
            .map(|p| Value::String(p.as_str().to_owned()))
            .collect();
        let deny_read: Vec<Value> = spec
            .sandbox
            .deny_read
            .iter()
            .map(|p| Value::String(p.as_str().to_owned()))
            .collect();
        let allow_sockets: Vec<Value> = spec
            .sandbox
            .allowed_unix_sockets
            .iter()
            .map(|p| Value::String(p.as_str().to_owned()))
            .collect();

        root.insert(
            "sandbox".to_owned(),
            json!({
                "enabled": true,
                "autoAllowBashIfSandboxed": false,
                "filesystem": {
                    "allowWrite": writable,
                    "denyWrite": deny_write,
                    "denyRead": deny_read,
                },
                "network": {
                    "allowUnixSockets": allow_sockets,
                }
            }),
        );
    } else {
        root.insert("sandbox".to_owned(), json!({ "enabled": false }));
    }

    Ok(serde_json::to_vec(&Value::Object(root)).expect("settings JSON serialization cannot fail"))
}

fn format_rule(rule: &Rule) -> Option<String> {
    let tool = rule.tool.as_str();
    match &rule.scope {
        RuleScope::Any => Some(tool.to_owned()),
        RuleScope::Path(pattern) => Some(format!("{tool}({})", pattern.as_str())),
        RuleScope::CommandPrefix(prefix) => Some(format!("{tool}({}*)", prefix.as_str())),
        RuleScope::McpServer(server) => Some(format!("mcp__{}__*", server.as_str())),
        _ => None,
    }
}

fn build_mcp_json(spec: &SessionSpec) -> Vec<u8> {
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
    serde_json::to_vec(&json!({ "mcpServers": servers }))
        .expect("MCP JSON serialization cannot fail")
}

fn build_agents_json(spec: &SessionSpec) -> String {
    let mut def = Map::new();
    def.insert(
        "description".to_owned(),
        Value::String(spec.agent.description.clone()),
    );
    def.insert(
        "prompt".to_owned(),
        Value::String(spec.agent.prompt.clone()),
    );
    if let Some(tools) = &spec.tools {
        def.insert(
            "tools".to_owned(),
            Value::Array(
                tools
                    .iter()
                    .map(|t| Value::String(t.as_str().to_owned()))
                    .collect(),
            ),
        );
    }

    let mut agents = Map::new();
    agents.insert(spec.agent.name.as_str().to_owned(), Value::Object(def));
    serde_json::to_string(&Value::Object(agents)).expect("agents JSON serialization cannot fail")
}

fn build_env_add(ctx: &LaunchContext) -> Vec<(EnvName, String)> {
    let mut env = vec![
        (
            EnvName::try_from("DISABLE_AUTOUPDATER").expect("static env name is valid"),
            "1".to_owned(),
        ),
        (
            EnvName::try_from("ENABLE_TOOL_SEARCH").expect("static env name is valid"),
            "false".to_owned(),
        ),
    ];
    if let Some(cfg_dir) = &ctx.config_dir {
        env.push((
            EnvName::try_from("CLAUDE_CONFIG_DIR").expect("static env name is valid"),
            cfg_dir.as_str().to_owned(),
        ));
    }
    env
}

fn secret_env_names() -> Vec<EnvName> {
    [
        "CLAUDE_CODE_OAUTH_TOKEN",
        "CLAUDE_CODE_MESSAGING_TOKEN",
        "ANTHROPIC_API_KEY",
    ]
    .into_iter()
    .map(|name| EnvName::try_from(name).expect("static secret env name is valid"))
    .collect()
}
