use serde_json::{json, Value};

use org_agent_runtime::{
    Capabilities, EnvName, FileLocation, LaunchContext, LaunchPlan, PermissionMode, PlanError,
    PlannedDir, RelPath, Rule, RuleScope, SessionId, SessionKind, SessionSpec, SignalTarget,
    SpecPath, Stdio, StopPlan, StopSignal, StopStep, ToolCapability, ToolName,
};

const APP_DATA_DIR_REL: &str = "antigravity-cli";
const ISOLATED_GEMINI_DIR_REL: &str = "gemini";

/// Host credential paths inside `<gemini-dir>` that must remain denied to tools (`S2`).
pub const AGY_CONFIG_CREDENTIAL_FILES: &[&str] = &[
    "antigravity-cli/antigravity-oauth-token",
    "oauth_creds.json",
    "google_accounts.json",
];

/// Expected Antigravity execution mode (never `yolo` or bypass).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExpectedMode {
    pub(crate) cli_flag: Option<&'static str>,
    pub(crate) reported_name: &'static str,
}

/// Pure ACP session launch parameters carried from `plan_session` into [`crate::session::AgySession`].
#[derive(Debug, Clone)]
pub(crate) struct AgySessionConfig {
    pub(crate) agent_name: String,
    pub(crate) cwd: String,
    pub(crate) system_prompt_append: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) mcp_servers: Vec<Value>,
    pub(crate) resume_session_id: Option<SessionId>,
    pub(crate) expected_mode: ExpectedMode,
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

fn validate_deny_rules(rules: &[Rule]) -> Result<(), PlanError> {
    for rule in rules {
        let tool = rule.tool.as_str();
        if ToolCapability::is_capability_token(tool) {
            let cap = ToolCapability::parse_rule_token(tool)
                .ok_or(PlanError::Unsupported("deny rule capability"))?;
            if capability_tools(cap).is_none() {
                return Err(PlanError::Unsupported("deny rule capability"));
            }
            validate_capability_scope(cap, &rule.scope)?;
            continue;
        }
        match &rule.scope {
            RuleScope::Any if tool == "*" => {
                return Err(PlanError::Unsupported("deny rule scope"));
            }
            RuleScope::Any
            | RuleScope::Path(_)
            | RuleScope::CommandPrefix(_)
            | RuleScope::McpServer(_) => {}
            _ => return Err(PlanError::Unsupported("deny rule scope")),
        }
    }
    Ok(())
}

fn validate_capability_scope(cap: ToolCapability, scope: &RuleScope) -> Result<(), PlanError> {
    match scope {
        RuleScope::Any => Ok(()),
        RuleScope::Path(_)
            if matches!(
                cap,
                ToolCapability::Read | ToolCapability::Search | ToolCapability::Edit
            ) =>
        {
            Ok(())
        }
        RuleScope::CommandPrefix(_) if cap == ToolCapability::Shell => Ok(()),
        _ => Err(PlanError::Unsupported("deny rule scope")),
    }
}

/// Builds an ACP `agy acp --stdio` headless [`LaunchPlan`] and [`AgySessionConfig`].
pub(crate) fn build_headless_plan(
    spec: &SessionSpec,
    ctx: &LaunchContext,
    caps: &Capabilities,
) -> Result<(LaunchPlan, AgySessionConfig), PlanError> {
    validate_session_kind(&spec.kind)?;
    validate_paths(spec, ctx)?;
    validate_deny_rules(&spec.policy.deny)?;
    let effective_mode = spec.effective_permission_mode(caps);
    let expected_mode = map_permission_mode(effective_mode)?;
    let _ = spec
        .tools
        .as_deref()
        .map(expand_tool_allowlist)
        .transpose()?;

    let (gemini_dir, dirs) = resolve_acp_gemini_dir(ctx);
    let mut args = vec![
        "acp".to_owned(),
        "--stdio".to_owned(),
        "--include-default-customizations=false".to_owned(),
        "--disable-telemetry".to_owned(),
    ];
    if spec.sandbox.enabled {
        args.push("--sandbox".to_owned());
    }
    args.push("--gemini-dir".to_owned());
    args.push(gemini_dir);
    args.push("--app-data-dir".to_owned());
    args.push(APP_DATA_DIR_REL.to_owned());
    for dir in &spec.read_only_dirs {
        args.push("--add-dir".to_owned());
        args.push(dir.as_str().to_owned());
    }

    let plan = LaunchPlan {
        program: ctx.runtime_path.clone(),
        args,
        cwd: spec.workspace.clone(),
        env_add: build_env_add(ctx),
        dirs,
        files: Vec::new(),
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

    let cfg = AgySessionConfig {
        agent_name: spec.agent.name.as_str().to_owned(),
        cwd: spec.workspace.as_str().to_owned(),
        system_prompt_append: combine_system_prompt(&spec.agent.prompt, &spec.instructions),
        model: spec.model.as_ref().map(|m| m.as_str().to_owned()),
        mcp_servers: build_acp_mcp_servers(spec),
        resume_session_id: ctx.resume.clone(),
        expected_mode,
    };

    Ok((plan, cfg))
}

/// Builds an interactive PTY take-over [`LaunchPlan`] resuming an existing Agy conversation.
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
    validate_deny_rules(&spec.policy.deny)?;
    let Some(resume_id) = ctx.resume.clone() else {
        return Err(PlanError::InvalidSpec(
            "take-over requires LaunchContext::resume to be set".to_owned(),
        ));
    };
    let effective_mode = spec.effective_permission_mode(caps);
    let expected_mode = map_permission_mode(effective_mode)?;
    let _ = spec
        .tools
        .as_deref()
        .map(expand_tool_allowlist)
        .transpose()?;

    let mut args = vec![
        "--conversation".to_owned(),
        resume_id.as_str().to_owned(),
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
    append_take_over_options(&mut args, spec);

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

/// Resolves `--gemini-dir` for `agy acp --stdio`.
/// When `ctx.config_dir` is set (host subscription OAuth via `host_auth`), points `--gemini-dir`
/// at it while `--app-data-dir antigravity-cli` stays relative (required by `agy 1.5.17`).
/// Otherwise isolates state inside `<runtime_dir>/gemini`.
fn resolve_acp_gemini_dir(ctx: &LaunchContext) -> (String, Vec<PlannedDir>) {
    if let Some(cfg_dir) = &ctx.config_dir {
        return (cfg_dir.as_str().to_owned(), Vec::new());
    }
    let dir = format!(
        "{}/{ISOLATED_GEMINI_DIR_REL}",
        ctx.runtime_dir.as_str().trim_end_matches('/')
    );
    let planned = vec![PlannedDir {
        location: FileLocation::RuntimeDir,
        rel_path: RelPath::try_from(ISOLATED_GEMINI_DIR_REL).expect("static rel path"),
    }];
    (dir, planned)
}

fn combine_system_prompt(agent_prompt: &str, instructions: &str) -> Option<String> {
    let p = agent_prompt.trim();
    let i = instructions.trim();
    match (p.is_empty(), i.is_empty()) {
        (true, true) => None,
        (false, true) => Some(p.to_owned()),
        (true, false) => Some(i.to_owned()),
        (false, false) => Some(format!("{p}\n\n{i}")),
    }
}

fn build_acp_mcp_servers(spec: &SessionSpec) -> Vec<Value> {
    spec.mcp_servers
        .iter()
        .map(|(name, server)| {
            json!({
                "name": name.as_str(),
                "command": server.command.as_str(),
                "args": server.args,
                "env": [],
            })
        })
        .collect()
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

fn append_take_over_options(args: &mut Vec<String>, spec: &SessionSpec) {
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

#[cfg(test)]
mod plan_unit_tests {
    use super::*;
    use org_agent_runtime::{
        AgentIdentity, AgentName, AgentRuntime, PermissionPolicy, RuntimeId, SandboxSpec,
    };
    use std::path::PathBuf;

    fn minimal_spec() -> SessionSpec {
        SessionSpec {
            runtime: RuntimeId::try_from("agy").unwrap(),
            kind: SessionKind::Home {},
            agent: AgentIdentity {
                name: AgentName::try_from("agent").unwrap(),
                description: "Agent".into(),
                prompt: "You are agent.".into(),
            },
            model: Some(org_agent_runtime::Token::try_from("gemini-4-neon-high").unwrap()),
            effort: None,
            workspace: SpecPath::try_from("/workspace").unwrap(),
            read_only_dirs: vec![SpecPath::try_from("/readonly").unwrap()],
            instructions: "Standing instructions.".into(),
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
        }
    }

    #[test]
    fn acp_plan_uses_isolated_gemini_dir_when_config_dir_absent() {
        let spec = minimal_spec();
        let ctx = LaunchContext {
            runtime_path: PathBuf::from("/usr/local/bin/agy"),
            runtime_version: None,
            runtime_dir: SpecPath::try_from("/runtime/agent-1").unwrap(),
            tmpdir: SpecPath::try_from("/tmp/agent").unwrap(),
            office_env_names: Vec::new(),
            new_session_id: None,
            resume: None,
            config_dir: None,
        };
        let caps = crate::AgyRuntime::new().capabilities();
        let (plan, cfg) = build_headless_plan(&spec, &ctx, &caps).unwrap();
        assert_eq!(plan.args[0], "acp");
        assert_eq!(plan.args[1], "--stdio");
        assert!(plan
            .args
            .contains(&"--include-default-customizations=false".to_owned()));
        assert!(plan.args.contains(&"--disable-telemetry".to_owned()));
        assert!(plan.args.contains(&"--sandbox".to_owned()));
        assert!(!plan.args.contains(&"--model".to_owned()));
        assert_eq!(plan.dirs.len(), 1);
        assert_eq!(plan.dirs[0].rel_path.as_str(), "gemini");
        let gemini_pos = plan.args.iter().position(|a| a == "--gemini-dir").unwrap();
        assert_eq!(plan.args[gemini_pos + 1], "/runtime/agent-1/gemini");
        let app_pos = plan
            .args
            .iter()
            .position(|a| a == "--app-data-dir")
            .unwrap();
        assert_eq!(plan.args[app_pos + 1], "antigravity-cli");
        assert_eq!(
            cfg.system_prompt_append.as_deref(),
            Some("You are agent.\n\nStanding instructions.")
        );
        assert_eq!(cfg.model.as_deref(), Some("gemini-4-neon-high"));
        assert!(AGY_CONFIG_CREDENTIAL_FILES.contains(&"antigravity-cli/antigravity-oauth-token"));
    }
}
