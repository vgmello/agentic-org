//! Generic ACP headless launch planning and optional interactive take-over planning.

use serde_json::{json, Value};

use org_agent_runtime::{
    expand_capability_allowlist, runtime_env_add, static_env_names, validate_operator_shell,
    validate_plan_paths, validate_standard_session_kind, Capabilities, EnvName, FileLocation,
    LaunchContext, LaunchPlan, PlanError, PlannedDir, PlannedFile, RelPath, Rule, RuleScope,
    SessionId, SessionKind, SessionSpec, SignalTarget, Stdio, StopPlan, StopSignal, StopStep,
    ToolCapability,
};

use crate::profile::{
    AcpProfile, ExpectedMode, StateDirStrategy, SystemPromptDelivery, TakeOverStrategy,
};

/// Pure ACP session launch parameters carried from `plan_session` into [`crate::session::AcpSession`].
#[derive(Clone)]
pub(crate) struct AcpSessionConfig {
    pub(crate) profile: &'static AcpProfile,
    pub(crate) agent_name: String,
    pub(crate) cwd: String,
    pub(crate) system_prompt_append: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) mcp_servers: Vec<Value>,
    pub(crate) resume_session_id: Option<SessionId>,
    pub(crate) expected_mode: ExpectedMode,
}

const CAP_READ_TOOLS: &[&str] = &["read_file", "view_file", "read", "read_many_files"];
const CAP_SEARCH_TOOLS: &[&str] = &[
    "list_dir",
    "list_directory",
    "find_by_name",
    "grep_search",
    "search_file_content",
    "glob",
    "grep",
];
const CAP_EDIT_TOOLS: &[&str] = &[
    "edit_file",
    "write_file",
    "replace_file_content",
    "multi_replace_file_content",
    "delete_file",
    "replace",
    "edit",
    "write",
];
const CAP_SHELL_TOOLS: &[&str] = &["run_command", "run_shell_command", "bash", "shell"];
const CAP_SUBAGENT_TOOLS: &[&str] = &["invoke_subagent", "spawn_subagent", "subagent"];

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

fn validate_common_spec(
    profile: &'static AcpProfile,
    spec: &SessionSpec,
    ctx: &LaunchContext,
    caps: &Capabilities,
) -> Result<ExpectedMode, PlanError> {
    validate_standard_session_kind(&spec.kind)?;
    validate_plan_paths(spec, ctx)?;
    validate_operator_shell(spec)?;
    validate_deny_rules(&spec.policy.deny)?;
    if spec.effort.is_some() {
        return Err(PlanError::Unsupported("effort"));
    }
    let effective_mode = spec.effective_permission_mode(caps);
    let expected_mode = profile.modes.resolve(effective_mode)?;
    let _ = spec
        .tools
        .as_deref()
        .map(|tools| expand_capability_allowlist(tools, capability_tools))
        .transpose()?;
    Ok(expected_mode)
}

/// Builds a headless ACP stdio [`LaunchPlan`] and [`AcpSessionConfig`] for `profile`.
pub(crate) fn build_headless_plan(
    profile: &'static AcpProfile,
    spec: &SessionSpec,
    ctx: &LaunchContext,
    caps: &Capabilities,
) -> Result<(LaunchPlan, AcpSessionConfig), PlanError> {
    let expected_mode = validate_common_spec(profile, spec, ctx, caps)?;
    let mut args: Vec<String> = profile
        .headless_base_args
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    if spec.sandbox.enabled {
        if let Some(flag) = profile.sandbox_flag {
            args.push(flag.to_owned());
        }
    }
    let dirs = append_state_dir_args(profile, ctx, &mut args);
    if let Some(ro_flag) = profile.read_only_dir_flag {
        for dir in &spec.read_only_dirs {
            args.push(ro_flag.to_owned());
            args.push(dir.as_str().to_owned());
        }
    }

    let mut env_add = runtime_env_add(profile.env_add, profile.config_dir_env, ctx);
    let mut files = Vec::new();
    let standing = Some(spec.standing_prompt()).filter(|p| !p.is_empty());
    let system_prompt_append = stage_system_prompt(
        profile.system_prompt,
        standing,
        ctx,
        &mut env_add,
        &mut files,
    );

    let plan = LaunchPlan {
        program: ctx.runtime_path.clone(),
        args,
        cwd: spec.workspace.clone(),
        env_add,
        dirs,
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
        secret_env_names: static_env_names(profile.secret_env_names),
        auth_env_names: static_env_names(profile.auth_env_names),
    };

    let cfg = AcpSessionConfig {
        profile,
        agent_name: spec.agent.name.as_str().to_owned(),
        cwd: spec.workspace.as_str().to_owned(),
        system_prompt_append,
        model: spec.model.as_ref().map(|m| m.as_str().to_owned()),
        mcp_servers: build_acp_mcp_servers(spec),
        resume_session_id: ctx.resume.clone(),
        expected_mode,
    };

    Ok((plan, cfg))
}

/// Builds an interactive PTY take-over [`LaunchPlan`] when supported by `profile`.
pub(crate) fn build_take_over_plan(
    profile: &'static AcpProfile,
    spec: &SessionSpec,
    ctx: &LaunchContext,
    caps: &Capabilities,
    cols: u16,
    rows: u16,
) -> Result<LaunchPlan, PlanError> {
    if matches!(profile.take_over, TakeOverStrategy::Unsupported) {
        return Err(PlanError::Unsupported("take-over"));
    }
    validate_standard_session_kind(&spec.kind)?;
    if matches!(spec.kind, SessionKind::OneOff { .. }) {
        return Err(PlanError::InvalidSpec(
            "one-off sessions cannot be taken over".to_owned(),
        ));
    }
    let expected_mode = validate_common_spec(profile, spec, ctx, caps)?;
    let Some(resume_id) = ctx.resume.clone() else {
        return Err(PlanError::InvalidSpec(
            "take-over requires LaunchContext::resume to be set".to_owned(),
        ));
    };

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
        if let Some(flag) = profile.sandbox_flag {
            args.push(flag.to_owned());
        }
    }
    if let Some(model) = &spec.model {
        args.push("--model".to_owned());
        args.push(model.as_str().to_owned());
    }
    if let Some(ro_flag) = profile.read_only_dir_flag {
        for dir in &spec.read_only_dirs {
            args.push(ro_flag.to_owned());
            args.push(dir.as_str().to_owned());
        }
    }

    Ok(LaunchPlan {
        program: ctx.runtime_path.clone(),
        args,
        cwd: spec.workspace.clone(),
        env_add: runtime_env_add(profile.env_add, profile.config_dir_env, ctx),
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
        secret_env_names: static_env_names(profile.secret_env_names),
        auth_env_names: static_env_names(profile.auth_env_names),
    })
}

fn append_state_dir_args(
    profile: &'static AcpProfile,
    ctx: &LaunchContext,
    args: &mut Vec<String>,
) -> Vec<PlannedDir> {
    match profile.state_dir {
        StateDirStrategy::None => Vec::new(),
        StateDirStrategy::AgyGeminiDir {
            isolated_rel,
            app_data_rel,
        } => {
            let (gemini_dir, dirs) = if let Some(cfg_dir) = &ctx.config_dir {
                (cfg_dir.as_str().to_owned(), Vec::new())
            } else {
                let dir = format!(
                    "{}/{isolated_rel}",
                    ctx.runtime_dir.as_str().trim_end_matches('/')
                );
                let planned = vec![PlannedDir {
                    location: FileLocation::RuntimeDir,
                    rel_path: RelPath::try_from(isolated_rel).expect("static rel path"),
                }];
                (dir, planned)
            };
            args.push("--gemini-dir".to_owned());
            args.push(gemini_dir);
            args.push("--app-data-dir".to_owned());
            args.push(app_data_rel.to_owned());
            dirs
        }
    }
}

fn stage_system_prompt(
    delivery: SystemPromptDelivery,
    standing: Option<String>,
    ctx: &LaunchContext,
    env_add: &mut Vec<(EnvName, String)>,
    files: &mut Vec<PlannedFile>,
) -> Option<String> {
    let prompt = standing?;
    match delivery {
        SystemPromptDelivery::MetaAntigravityAppend
        | SystemPromptDelivery::FirstTurnPromptBlock => Some(prompt),
        SystemPromptDelivery::PlannedFileEnv { rel_path, env_var } => {
            let full_path = format!(
                "{}/{rel_path}",
                ctx.runtime_dir.as_str().trim_end_matches('/')
            );
            files.push(PlannedFile {
                location: FileLocation::RuntimeDir,
                rel_path: RelPath::try_from(rel_path).expect("static system prompt rel path"),
                contents: prompt.into_bytes(),
                git_exclude: false,
            });
            env_add.push((
                EnvName::try_from(env_var).expect("static system prompt env var"),
                full_path,
            ));
            None
        }
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

#[cfg(test)]
mod plan_unit_tests {
    use std::path::PathBuf;

    use org_agent_runtime::{
        AgentIdentity, AgentName, AgentRuntime, PermissionMode, PermissionPolicy, RuntimeId,
        SandboxSpec, SpecPath, Token,
    };

    use super::*;
    use crate::profiles::{
        AgyRuntime, GeminiRuntime, KimiRuntime, PiRuntime, AGY_CONFIG_CREDENTIAL_FILES,
        AGY_PROFILE, GEMINI_PROFILE, KIMI_PROFILE, PI_PROFILE,
    };

    fn minimal_spec(runtime: &str) -> SessionSpec {
        SessionSpec {
            runtime: RuntimeId::try_from(runtime).unwrap(),
            kind: SessionKind::Home {},
            agent: AgentIdentity {
                name: AgentName::try_from("agent").unwrap(),
                description: "Agent".into(),
                prompt: "You are agent.".into(),
            },
            model: Some(Token::try_from("gemini-4-neon-high").unwrap()),
            effort: None,
            workspace: SpecPath::try_from("/workspace").unwrap(),
            read_only_dirs: vec![SpecPath::try_from("/readonly").unwrap()],
            instructions: "Standing instructions.".into(),
            repository_instructions: String::new(),
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
            operator_shell_prefixes: Vec::new(),
        }
    }

    fn minimal_ctx(bin: &str) -> LaunchContext {
        LaunchContext {
            runtime_path: PathBuf::from(bin),
            runtime_version: None,
            runtime_dir: SpecPath::try_from("/runtime/agent-1").unwrap(),
            tmpdir: SpecPath::try_from("/tmp/agent").unwrap(),
            office_env_names: Vec::new(),
            new_session_id: None,
            resume: None,
            config_dir: None,
        }
    }

    #[test]
    fn acp_plan_uses_isolated_gemini_dir_when_config_dir_absent() {
        let spec = minimal_spec("agy");
        let ctx = minimal_ctx("/usr/local/bin/agy");
        let caps = AgyRuntime::new().capabilities();
        let (plan, cfg) = build_headless_plan(&AGY_PROFILE, &spec, &ctx, &caps).unwrap();
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
            Some("Standing instructions.\n\nYou are agent.")
        );
        assert_eq!(cfg.model.as_deref(), Some("gemini-4-neon-high"));
        assert!(AGY_CONFIG_CREDENTIAL_FILES.contains(&"antigravity-cli/antigravity-oauth-token"));
    }

    #[test]
    fn gemini_kimi_and_pi_plans_use_vendor_specific_flags_and_system_prompt_strategies() {
        let gemini_caps = GeminiRuntime::new().capabilities();
        let (g_plan, g_cfg) = build_headless_plan(
            &GEMINI_PROFILE,
            &minimal_spec("gemini"),
            &minimal_ctx("/usr/local/bin/gemini"),
            &gemini_caps,
        )
        .unwrap();
        assert_eq!(g_plan.args[0], "--acp");
        assert!(g_plan.args.contains(&"--sandbox".to_owned()));
        assert!(g_plan.args.contains(&"--include-directories".to_owned()));
        assert_eq!(g_plan.files.len(), 1);
        assert_eq!(g_plan.files[0].rel_path.as_str(), "gemini-system.md");
        assert!(g_plan.env_add.iter().any(|(k, v)| {
            k.as_str() == "GEMINI_SYSTEM_MD" && v == "/runtime/agent-1/gemini-system.md"
        }));
        assert!(g_cfg.system_prompt_append.is_none());
        assert_eq!(g_cfg.expected_mode.reported_name, "auto_edit");

        let kimi_caps = KimiRuntime::new().capabilities();
        let (k_plan, k_cfg) = build_headless_plan(
            &KIMI_PROFILE,
            &minimal_spec("kimi"),
            &minimal_ctx("/usr/local/bin/kimi"),
            &kimi_caps,
        )
        .unwrap();
        assert_eq!(k_plan.args, vec!["acp".to_owned()]);
        assert!(k_plan.files.is_empty());
        assert_eq!(
            k_cfg.system_prompt_append.as_deref(),
            Some("Standing instructions.\n\nYou are agent.")
        );

        let pi_caps = PiRuntime::new().capabilities();
        let (p_plan, p_cfg) = build_headless_plan(
            &PI_PROFILE,
            &minimal_spec("pi"),
            &minimal_ctx("/usr/local/bin/pi-acp"),
            &pi_caps,
        )
        .unwrap();
        assert!(p_plan.args.is_empty());
        assert!(p_plan.files.is_empty());
        assert_eq!(
            p_cfg.system_prompt_append.as_deref(),
            Some("Standing instructions.\n\nYou are agent.")
        );
    }
}
