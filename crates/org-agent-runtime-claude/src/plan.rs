use serde_json::{json, Map, Value};

use org_agent_runtime::{
    Capabilities, EnvName, FileLocation, LaunchContext, LaunchPlan, PermissionMode, PlanError,
    PlannedFile, RelPath, Rule, RuleScope, SessionId, SessionKind, SessionSpec, SignalTarget,
    SpecPath, Stdio, StopPlan, StopSignal, StopStep,
};

const SETTINGS_FILE: &str = "claude-settings.json";
const MCP_FILE: &str = "claude-mcp.json";
const AGENTS_FILE: &str = "claude-agents.json";
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
    let agents_bytes = build_agents_json(spec).into_bytes();

    let settings_path = format!("{}/{SETTINGS_FILE}", ctx.runtime_dir.as_str());
    let mcp_path = format!("{}/{MCP_FILE}", ctx.runtime_dir.as_str());
    let agents_path = format!("{}/{AGENTS_FILE}", ctx.runtime_dir.as_str());
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
        // Under `-p`, `--agents` accepts a path to a file so the full agent prompt never
        // appears in argv / `ps`.
        "--agents".to_owned(),
        agents_path,
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
        PlannedFile {
            location: FileLocation::RuntimeDir,
            rel_path: RelPath::try_from(AGENTS_FILE).expect("constant relative path is valid"),
            contents: agents_bytes,
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
        // Inline JSON, unlike headless: Claude Code 2.1.311 accepts a file path for
        // `--agents` only with `--print` (`claude --help`), and take-over is interactive.
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
    }
    // Always disallow known risky built-in tools (web access, cron/triggers, worktrees,
    // ToolSearch, etc.) unless explicitly granted in `spec.tools` — including when
    // `spec.tools` is `None`.
    let disallowed: Vec<&str> = RISKY_BUILTIN_TOOLS
        .iter()
        .copied()
        .filter(|risky| {
            !spec
                .tools
                .as_ref()
                .is_some_and(|tools| tools.iter().any(|allowed| allowed.as_str() == *risky))
        })
        .collect();
    if !disallowed.is_empty() {
        args.push("--disallowedTools".to_owned());
        args.push(disallowed.join(","));
    }
    for dir in &spec.read_only_dirs {
        args.push("--add-dir".to_owned());
        args.push(dir.as_str().to_owned());
    }
}

/// Convert an absolute filesystem path/pattern to Claude Code's `//abs/path` permission rule
/// syntax (single leading `/` in Claude rules is settings-file-relative, not filesystem-root).
fn claude_rule_path(path: &str) -> String {
    if path.starts_with("//") {
        path.to_owned()
    } else if path.starts_with('/') {
        format!("/{path}")
    } else {
        path.to_owned()
    }
}

fn push_file_write_denies(deny_rules: &mut Vec<Value>, raw_path: &str, include_exact: bool) {
    let trimmed = raw_path.trim_end_matches('/');
    let exact = claude_rule_path(trimmed);
    let glob = format!("{exact}/**");
    for tool in ["Edit", "Write", "MultiEdit", "NotebookEdit"] {
        if include_exact {
            deny_rules.push(Value::String(format!("{tool}({exact})")));
        }
        deny_rules.push(Value::String(format!("{tool}({glob})")));
    }
}

fn push_file_read_denies(deny_rules: &mut Vec<Value>, raw_path: &str, include_glob: bool) {
    let trimmed = raw_path.trim_end_matches('/');
    let exact = claude_rule_path(trimmed);
    deny_rules.push(Value::String(format!("Read({exact})")));
    if include_glob {
        deny_rules.push(Value::String(format!("Read({exact}/**)")));
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
    // `keeps_allow_rules` is false (deny wins over allow). `disableBypassPermissionsMode`
    // is pinned to `"disable"` per agent-sessions.md (B1).
    let mut deny_rules = Vec::new();
    for rule in &spec.policy.deny {
        let formatted =
            format_rule(rule, RuleKind::Deny).ok_or(PlanError::Unsupported("deny rule scope"))?;
        deny_rules.extend(formatted.into_iter().map(Value::String));
    }
    // Mirror sandbox read/write denies onto Claude's file tools using `//` absolute path
    // grammar so Read/Edit/Write/MultiEdit/NotebookEdit cannot bypass the Bash sandbox (B2).
    for path in &spec.sandbox.deny_read {
        push_file_read_denies(&mut deny_rules, path.as_str(), true);
    }
    for path in &spec.sandbox.deny_write {
        push_file_write_denies(&mut deny_rules, path.as_str(), true);
    }
    for dir in &spec.read_only_dirs {
        push_file_write_denies(&mut deny_rules, dir.as_str(), false);
    }
    // Protect vendor config dir from tool writes and its credential file from tool reads (S2).
    if let Some(cfg_dir) = &ctx.config_dir {
        push_file_write_denies(&mut deny_rules, cfg_dir.as_str(), true);
        let cred_file = format!(
            "{}/.credentials.json",
            cfg_dir.as_str().trim_end_matches('/')
        );
        push_file_read_denies(&mut deny_rules, &cred_file, false);
    }

    let mut allow_rules = Vec::new();
    if spec.keeps_allow_rules(caps) {
        for rule in &spec.policy.allow {
            // Grants we cannot express exactly are dropped (never widened).
            if let Some(formatted) = format_rule(rule, RuleKind::Allow) {
                allow_rules.extend(formatted.into_iter().map(Value::String));
            }
        }
    }

    let mut permissions = Map::new();
    permissions.insert(
        "disableBypassPermissionsMode".to_owned(),
        Value::String("disable".to_owned()),
    );
    permissions.insert("deny".to_owned(), Value::Array(deny_rules));
    permissions.insert("allow".to_owned(), Value::Array(allow_rules));
    root.insert("permissions".to_owned(), Value::Object(permissions));

    // Sandbox: always add `ctx.tmpdir` to writable roots; NEVER add `ctx.config_dir` to
    // `allowWrite` (Claude writes it outside the Bash sandbox; tools must not tamper with
    // transcripts or credential files — S2). Pin `allowUnsandboxedCommands: false` (B1).
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

        let mut deny_write: Vec<String> = spec
            .sandbox
            .deny_write
            .iter()
            .map(|p| p.as_str().to_owned())
            .collect();
        let mut deny_read: Vec<String> = spec
            .sandbox
            .deny_read
            .iter()
            .map(|p| p.as_str().to_owned())
            .collect();
        if let Some(cfg_dir) = &ctx.config_dir {
            if !deny_write.iter().any(|p| p == cfg_dir.as_str()) {
                deny_write.push(cfg_dir.as_str().to_owned());
            }
            let cred_file = format!(
                "{}/.credentials.json",
                cfg_dir.as_str().trim_end_matches('/')
            );
            if !deny_read.iter().any(|p| p == &cred_file) {
                deny_read.push(cred_file);
            }
        }

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
                "allowUnsandboxedCommands": false,
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

/// Claude Code's file tools that take a path rule. `Read` rules also cover Claude's other
/// read-only file tools (`Glob`, `Grep`); the edit tools are listed one by one, as for the
/// sandbox mirrors above.
const PATH_RULE_TOOLS: &[&str] = &["Read", "Edit", "Write", "MultiEdit", "NotebookEdit"];

/// Claude's only shell tool, the one a command-prefix rule applies to.
const SHELL_TOOL: &str = "Bash";

/// Formats one policy rule as Claude Code permission rules, or `None` when Claude has no
/// exact form for it (the caller refuses a deny it can't express and drops such an allow).
///
/// A rule for any tool (`*`) is never written as `*(...)`: that isn't a Claude rule form, and
/// a settings file Claude rejects would lose the pinned no-bypass setting with it. It expands
/// to the concrete tools the scope applies to: a path to [`PATH_RULE_TOOLS`], a command
/// prefix to [`SHELL_TOOL`]. Claude has no path rule for `Bash`; the sandbox's deny lists and
/// the office's approval bridge cover shell access to such paths. `*` with no scope (every
/// use of every tool), a path rule for a tool outside [`PATH_RULE_TOOLS`], a command prefix
/// for a tool other than [`SHELL_TOOL`], and an MCP-server rule for a tool that isn't that
/// server's (`mcp__<server>__<tool>`) have no exact Claude form and give `None`.
fn format_rule(rule: &Rule, kind: RuleKind) -> Option<Vec<String>> {
    let tool = rule.tool.as_str();
    let any_tool = tool == "*";
    match &rule.scope {
        RuleScope::Any if any_tool => None,
        RuleScope::Any => Some(vec![tool.to_owned()]),
        RuleScope::Path(pattern) => {
            let path = claude_rule_path(pattern.as_str());
            let tools: &[&str] = if any_tool {
                PATH_RULE_TOOLS
            } else if PATH_RULE_TOOLS.contains(&tool) {
                &[tool]
            } else {
                // Claude has no path rule for this tool (e.g. `Bash(//path)`): written, it
                // would look like a restriction and do nothing.
                return None;
            };
            Some(tools.iter().map(|t| format!("{t}({path})")).collect())
        }
        RuleScope::CommandPrefix(prefix) if any_tool || tool == SHELL_TOOL => {
            command_prefix_rules(prefix.as_str(), kind)
        }
        // Claude has no command-prefix rule for any other tool.
        RuleScope::CommandPrefix(_) => None,
        RuleScope::McpServer(server) => {
            let server_tools = format!("mcp__{}__", server.as_str());
            if any_tool {
                return Some(vec![format!("{server_tools}*")]);
            }
            // One tool of that server, by its full Claude name. A tool that isn't the
            // server's has no Claude form ("this tool, of that server"), and a name holding
            // `__` (`mcp__a__b__c` for server `a`) is refused: it could as well be tool `c`
            // of a server `a__b`.
            tool.strip_prefix(&server_tools)
                .filter(|name| !name.is_empty() && !name.contains("__"))
                .map(|name| vec![format!("{server_tools}{name}")])
        }
        _ => None,
    }
}

/// Whether a rule is written to Claude's `deny` or `allow` list. A prefix rule Claude can
/// only approximate is widened for a deny and narrowed for an allow, never the reverse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuleKind {
    Deny,
    Allow,
}

/// Claude Code rules for shell commands starting with `prefix` (a literal prefix, per
/// `CommandPrefix`), or `None` when Claude 2.1.311 has no form at least as wide (deny) or at
/// most as wide (allow). How 2.1.311 reads the forms used here: `Bash(x:*)` is its legacy
/// word-bounded prefix (`x` or `x ...`); a single trailing ` *` compiles to `^x( .*)?$`
/// (`x` or `x ...`); any other `*` matches anything.
///
/// - A deny keeps the literal prefix, `Bash(rm*)`: it also matches `rmdir`, as the contract's
///   literal prefix does, and a deny must never shrink. A prefix ending in `:` is refused:
///   `Bash(test:*)` would be read as the legacy form and stop covering `test:unit`.
/// - An allow uses the word-bounded form, `Bash(rm *)` (the `Bash(git *)` form
///   `claude --help` documents), which matches `rm` and `rm ...` but never `rmdir`: narrower
///   than the literal prefix, the rest goes to the approval bridge. A prefix ending in
///   whitespace (`git `) is dropped: `Bash(git *)` would also allow bare `git`, which the
///   literal prefix `git ` doesn't cover, and Claude has no exact form for it.
fn command_prefix_rules(prefix: &str, kind: RuleKind) -> Option<Vec<String>> {
    match kind {
        RuleKind::Deny if prefix.ends_with(':') => None,
        RuleKind::Deny => Some(vec![format!("{SHELL_TOOL}({prefix}*)")]),
        RuleKind::Allow if prefix.ends_with(char::is_whitespace) => None,
        RuleKind::Allow => Some(vec![format!("{SHELL_TOOL}({prefix} *)")]),
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
        // Scrub Claude credentials from tool subprocesses (verified present in Claude 2.1.311; S4).
        (
            EnvName::try_from("CLAUDE_CODE_SUBPROCESS_ENV_SCRUB")
                .expect("static env name is valid"),
            "1".to_owned(),
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

pub(crate) fn auth_env_names() -> Vec<EnvName> {
    ["CLAUDE_CODE_OAUTH_TOKEN", "ANTHROPIC_API_KEY"]
        .into_iter()
        .map(|name| EnvName::try_from(name).expect("static auth env name is valid"))
        .collect()
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
