use serde_json::{json, Map, Value};

use org_agent_runtime::{
    Capabilities, EnvName, FileLocation, LaunchContext, LaunchPlan, PermissionMode, PlanError,
    PlannedFile, RelPath, Rule, RuleScope, SessionKind, SessionSpec, SignalTarget, SpecPath, Stdio,
    StopPlan, StopSignal, StopStep, ToolCapability, ToolName,
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

const CAP_READ_TOOLS: &[&str] = &["read_file", "view_file"];
const CAP_SEARCH_TOOLS: &[&str] = &["list_dir", "find_by_name", "grep_search"];
const CAP_EDIT_TOOLS: &[&str] = &[
    "edit_file",
    "write_file",
    "replace_file_content",
    "multi_replace_file_content",
    "delete_file",
];
const CAP_SHELL_TOOLS: &[&str] = &["run_command"];
const CAP_SUBAGENT_TOOLS: &[&str] = &["invoke_subagent"];

fn capability_tools(cap: ToolCapability) -> Option<&'static [&'static str]> {
    match cap {
        ToolCapability::Read => Some(CAP_READ_TOOLS),
        ToolCapability::Search => Some(CAP_SEARCH_TOOLS),
        ToolCapability::Edit => Some(CAP_EDIT_TOOLS),
        ToolCapability::Shell => Some(CAP_SHELL_TOOLS),
        ToolCapability::Subagent => Some(CAP_SUBAGENT_TOOLS),
        _ => None,
    }
}

/// Expands a `SessionSpec::tools` allowlist by translating runtime-neutral `cap:*` capability
/// tokens into Antigravity's concrete tool names while passing concrete tool names through.
/// Fails closed on any unrecognized `cap:*` token.
pub(crate) fn expand_tool_allowlist(tools: &[ToolName]) -> Result<Vec<String>, PlanError> {
    let mut expanded = Vec::new();
    for tool in tools {
        let raw = tool.as_str();
        if ToolCapability::is_capability_token(raw) {
            let cap = ToolCapability::parse_rule_token(raw)
                .ok_or(PlanError::Unsupported("tool capability"))?;
            let concrete =
                capability_tools(cap).ok_or(PlanError::Unsupported("tool capability"))?;
            for &name in concrete {
                if !expanded.iter().any(|existing| existing == name) {
                    expanded.push(name.to_owned());
                }
            }
        } else if !expanded.iter().any(|existing| existing == raw) {
            expanded.push(raw.to_owned());
        }
    }
    Ok(expanded)
}

pub(crate) fn build_headless_plan(
    spec: &SessionSpec,
    ctx: &LaunchContext,
    caps: &Capabilities,
) -> Result<(LaunchPlan, ExpectedMode, Option<Vec<String>>), PlanError> {
    validate_session_kind(&spec.kind)?;
    validate_paths(spec, ctx)?;
    let effective_mode = spec.effective_permission_mode(caps);
    let expected_mode = map_permission_mode(effective_mode)?;
    let expanded_tools = spec
        .tools
        .as_deref()
        .map(expand_tool_allowlist)
        .transpose()?;

    let settings_bytes =
        build_settings_json(spec, ctx, caps, &expected_mode, expanded_tools.as_deref())?;
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
    Ok((plan, expected_mode, expanded_tools))
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
    let expanded_tools = spec
        .tools
        .as_deref()
        .map(expand_tool_allowlist)
        .transpose()?;
    let settings_bytes =
        build_settings_json(spec, ctx, caps, &expected_mode, expanded_tools.as_deref())?;
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
    expanded_tools: Option<&[String]>,
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
    if let Some(tools) = expanded_tools {
        root.insert(
            "allowedTools".to_owned(),
            Value::Array(tools.iter().map(|t| Value::String(t.clone())).collect()),
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
        deny_rules.extend(formatted);
    }
    for path in &spec.sandbox.deny_read {
        for &tool in CAP_READ_TOOLS {
            deny_rules.push(json!({ "tool": tool, "path": path.as_str() }));
        }
    }
    for path in &spec.sandbox.deny_write {
        for &tool in CAP_EDIT_TOOLS {
            deny_rules.push(json!({ "tool": tool, "path": path.as_str() }));
        }
    }
    for dir in &spec.read_only_dirs {
        let glob = format!("{}/**", dir.as_str());
        for &tool in CAP_EDIT_TOOLS {
            deny_rules.push(json!({ "tool": tool, "path": glob }));
        }
    }
    if let Some(cfg_dir) = &ctx.config_dir {
        append_config_dir_permission_denies(&mut deny_rules, cfg_dir);
    }

    let mut allow_rules = Vec::new();
    if spec.keeps_allow_rules(caps) {
        for rule in &spec.policy.allow {
            if let Some(formatted) = format_rule(rule) {
                allow_rules.extend(formatted);
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
    let mut deny_read: Vec<String> = spec
        .sandbox
        .deny_read
        .iter()
        .map(|p| p.as_str().to_owned())
        .collect();
    if let Some(cfg_dir) = &ctx.config_dir {
        let trimmed = cfg_dir.as_str().trim_end_matches('/');
        for cred_name in AGY_CONFIG_CREDENTIAL_FILES {
            let cred_path = format!("{trimmed}/{cred_name}");
            if !deny_read.iter().any(|p| p == &cred_path) {
                deny_read.push(cred_path);
            }
        }
    }
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

fn format_rule(rule: &Rule) -> Option<Vec<Value>> {
    let tool = rule.tool.as_str();
    if ToolCapability::is_capability_token(tool) {
        let cap = ToolCapability::parse_rule_token(tool)?;
        return format_capability_rule(cap, &rule.scope);
    }
    match &rule.scope {
        RuleScope::Any if tool == "*" => None,
        RuleScope::Any => Some(vec![json!({ "tool": tool })]),
        RuleScope::Path(p) => Some(vec![json!({ "tool": tool, "path": p.as_str() })]),
        RuleScope::CommandPrefix(prefix) => Some(vec![
            json!({ "tool": tool, "commandPrefix": prefix.as_str() }),
        ]),
        RuleScope::McpServer(server) => {
            Some(vec![json!({ "tool": tool, "mcpServer": server.as_str() })])
        }
        _ => None,
    }
}

fn format_capability_rule(cap: ToolCapability, scope: &RuleScope) -> Option<Vec<Value>> {
    match scope {
        RuleScope::Any => capability_tools(cap)
            .map(|tools| tools.iter().map(|&tool| json!({ "tool": tool })).collect()),
        RuleScope::Path(p) => match cap {
            ToolCapability::Read | ToolCapability::Search | ToolCapability::Edit => {
                capability_tools(cap).map(|tools| {
                    tools
                        .iter()
                        .map(|&tool| json!({ "tool": tool, "path": p.as_str() }))
                        .collect()
                })
            }
            _ => None,
        },
        RuleScope::CommandPrefix(prefix) if cap == ToolCapability::Shell => Some(
            CAP_SHELL_TOOLS
                .iter()
                .map(|&tool| json!({ "tool": tool, "commandPrefix": prefix.as_str() }))
                .collect(),
        ),
        RuleScope::CommandPrefix(_) | RuleScope::McpServer(_) => None,
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

const AGY_CONFIG_CREDENTIAL_FILES: &[&str] = &["oauth_creds.json", "google_accounts.json"];

fn append_config_dir_permission_denies(deny_rules: &mut Vec<Value>, cfg_dir: &SpecPath) {
    let trimmed = cfg_dir.as_str().trim_end_matches('/');
    let glob = format!("{trimmed}/**");
    for &tool in CAP_EDIT_TOOLS {
        deny_rules.push(json!({ "tool": tool, "path": glob }));
    }
    for cred_name in AGY_CONFIG_CREDENTIAL_FILES {
        let cred_path = format!("{trimmed}/{cred_name}");
        for &tool in CAP_READ_TOOLS {
            deny_rules.push(json!({ "tool": tool, "path": cred_path }));
        }
    }
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

#[cfg(test)]
mod config_dir_tests {
    use super::*;
    use org_agent_runtime::{
        AgentIdentity, AgentName, PermissionMode, PermissionPolicy, RuntimeId, SandboxSpec,
        SessionKind,
    };
    use std::path::PathBuf;

    #[test]
    fn config_dir_denies_agy_credential_reads() {
        let spec = SessionSpec {
            runtime: RuntimeId::try_from("agy").unwrap(),
            kind: SessionKind::Home {},
            agent: AgentIdentity {
                name: AgentName::try_from("agent").unwrap(),
                description: "Agent".into(),
                prompt: "Prompt".into(),
            },
            model: None,
            effort: None,
            workspace: SpecPath::try_from("/workspace").unwrap(),
            read_only_dirs: Vec::new(),
            instructions: "Instructions".into(),
            permission_mode: PermissionMode::AcceptEdits,
            policy: PermissionPolicy {
                version: 1,
                allow: Vec::new(),
                deny: Vec::new(),
            },
            sandbox: SandboxSpec {
                enabled: true,
                writable_roots: vec![SpecPath::try_from("/workspace").unwrap()],
                deny_write: Vec::new(),
                deny_read: Vec::new(),
                allowed_unix_sockets: Vec::new(),
            },
            mcp_servers: Default::default(),
            tools: None,
        };
        let ctx = LaunchContext {
            runtime_path: PathBuf::from("/usr/local/bin/agy"),
            runtime_version: None,
            runtime_dir: SpecPath::try_from("/runtime").unwrap(),
            tmpdir: SpecPath::try_from("/tmp/agent").unwrap(),
            office_env_names: Vec::new(),
            new_session_id: None,
            resume: None,
            config_dir: Some(SpecPath::try_from("/home/u/.gemini").unwrap()),
        };
        let sandbox = build_sandbox_value(&spec, &ctx);
        let deny_read = sandbox["filesystem"]["denyRead"].as_array().unwrap();
        assert!(deny_read.contains(&json!("/home/u/.gemini/oauth_creds.json")));
        assert!(deny_read.contains(&json!("/home/u/.gemini/google_accounts.json")));
    }
}
