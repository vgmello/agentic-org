use serde_json::{json, Map, Value};

use org_agent_runtime::{
    Capabilities, EnvName, FileLocation, LaunchContext, LaunchPlan, PermissionMode, PlanError,
    PlannedFile, RelPath, Rule, RuleScope, SessionId, SessionKind, SessionSpec, SignalTarget,
    SpecPath, Stdio, StopPlan, StopSignal, StopStep, ToolCapability, ToolName,
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

/// Concrete Claude Code tools for each runtime-neutral [`ToolCapability`].
/// `CAP_EDIT_ALLOW` excludes `NotebookEdit` so it remains in `--disallowedTools` unless
/// explicitly listed; `CAP_EDIT_DENY` includes it so deny rules never narrow.
const CAP_READ_TOOLS: &[&str] = &["Read"];
const CAP_SEARCH_TOOLS: &[&str] = &["Glob", "Grep"];
const CAP_EDIT_ALLOW_TOOLS: &[&str] = &["Edit", "Write", "MultiEdit"];
const CAP_EDIT_DENY_TOOLS: &[&str] = &["Edit", "Write", "MultiEdit", "NotebookEdit"];
const CAP_SHELL_TOOLS: &[&str] = &["Bash"];
const CAP_SUBAGENT_TOOLS: &[&str] = &["Task", "Agent"];

fn capability_tools(cap: ToolCapability, kind: RuleKind) -> Option<&'static [&'static str]> {
    match cap {
        ToolCapability::Read => Some(CAP_READ_TOOLS),
        ToolCapability::Search => Some(CAP_SEARCH_TOOLS),
        ToolCapability::Edit => Some(match kind {
            RuleKind::Deny => CAP_EDIT_DENY_TOOLS,
            RuleKind::Allow => CAP_EDIT_ALLOW_TOOLS,
        }),
        ToolCapability::Shell => Some(CAP_SHELL_TOOLS),
        ToolCapability::Subagent => Some(CAP_SUBAGENT_TOOLS),
        _ => None,
    }
}

/// Expands a `SessionSpec::tools` allowlist by translating runtime-neutral `cap:*` capability
/// tokens into Claude Code's concrete tool names while passing concrete tool names through.
/// Fails closed on any unrecognized `cap:*` token.
pub(crate) fn expand_tool_allowlist(tools: &[ToolName]) -> Result<Vec<String>, PlanError> {
    let mut expanded = Vec::new();
    for tool in tools {
        let raw = tool.as_str();
        if ToolCapability::is_capability_token(raw) {
            let cap = ToolCapability::parse_rule_token(raw)
                .ok_or(PlanError::Unsupported("tool capability"))?;
            let concrete = capability_tools(cap, RuleKind::Allow)
                .ok_or(PlanError::Unsupported("tool capability"))?;
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
    validate_paths(spec, ctx)?;
    let (session_id, is_resume) = resolve_session_id(ctx)?;
    let effective_mode = spec.effective_permission_mode(caps);
    let expected_mode = map_permission_mode(effective_mode)?;
    let expanded_tools = spec
        .tools
        .as_deref()
        .map(expand_tool_allowlist)
        .transpose()?;

    let settings_bytes = build_settings_json(spec, ctx, caps)?;
    let mcp_bytes = build_mcp_json(spec);
    let agents_bytes = build_agents_json(spec, expanded_tools.as_deref()).into_bytes();

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

    append_common_options(&mut args, spec, expanded_tools.as_deref());

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

    Ok((plan, expected_mode, expanded_tools))
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
    let expanded_tools = spec
        .tools
        .as_deref()
        .map(expand_tool_allowlist)
        .transpose()?;

    let settings_bytes = build_settings_json(spec, ctx, caps)?;
    let mcp_bytes = build_mcp_json(spec);
    let agents_json = build_agents_json(spec, expanded_tools.as_deref());

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

    append_common_options(&mut args, spec, expanded_tools.as_deref());

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

fn append_common_options(
    args: &mut Vec<String>,
    spec: &SessionSpec,
    expanded_tools: Option<&[String]>,
) {
    if let Some(model) = &spec.model {
        args.push("--model".to_owned());
        args.push(model.as_str().to_owned());
    }
    if let Some(effort) = &spec.effort {
        args.push("--effort".to_owned());
        args.push(effort.as_str().to_owned());
    }
    if let Some(tools) = expanded_tools {
        args.push("--tools".to_owned());
        if tools.is_empty() {
            args.push(String::new());
        } else {
            args.push(tools.join(","));
        }
    }
    // Always disallow known risky built-in tools (web access, cron/triggers, worktrees,
    // ToolSearch, etc.) unless explicitly granted in `spec.tools` — including when
    // `spec.tools` is `None`.
    let disallowed: Vec<&str> = RISKY_BUILTIN_TOOLS
        .iter()
        .copied()
        .filter(|risky| {
            !expanded_tools.is_some_and(|tools| tools.iter().any(|allowed| allowed == *risky))
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
    let is_linux_bwrap = !org_platform::confinement::unix_socket_path_filter_supported();
    // Mirror sandbox read/write denies onto Claude's file tools using `//` absolute path
    // grammar so Read/Edit/Write/MultiEdit/NotebookEdit cannot bypass the Bash sandbox (B2).
    // On Linux `bwrap`, omit trailing `/**` on concrete read-deny directories (`TASK-163`) so
    // `@anthropic-ai/sandbox-runtime` (which merges `permissions.deny` into `bwrap` mounts)
    // mounts `--tmpfs <dir>` atomically without traversing symlinks inside sibling homes.
    for path in &spec.sandbox.deny_read {
        push_file_read_denies(&mut deny_rules, path.as_str(), !is_linux_bwrap);
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
    root.insert("sandbox".to_owned(), build_sandbox_json(spec, ctx));

    Ok(serde_json::to_vec(&Value::Object(root)).expect("settings JSON serialization cannot fail"))
}

fn build_sandbox_json(spec: &SessionSpec, ctx: &LaunchContext) -> Value {
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

    let is_linux_bwrap = !org_platform::confinement::unix_socket_path_filter_supported();
    let allow_all_unix = !spec.sandbox.allowed_unix_sockets.is_empty() && is_linux_bwrap;
    apply_linux_bwrap_sandbox_rules(
        &mut deny_read,
        &mut deny_write,
        &writable,
        is_linux_bwrap,
        allow_all_unix,
    );
    let network = build_sandbox_network_json(&spec.sandbox.allowed_unix_sockets, allow_all_unix);

    json!({
        "enabled": true,
        "autoAllowBashIfSandboxed": false,
        "allowUnsandboxedCommands": false,
        "filesystem": {
            "allowWrite": writable,
            "denyWrite": deny_write,
            "denyRead": deny_read,
        },
        "network": network,
    })
}

fn apply_linux_bwrap_sandbox_rules(
    deny_read: &mut Vec<String>,
    deny_write: &mut Vec<String>,
    writable: &[String],
    is_linux_bwrap: bool,
    allow_all_unix: bool,
) {
    if allow_all_unix {
        append_linux_privileged_socket_denies(deny_read);
    }
    if is_linux_bwrap {
        // Linux `bwrap` mounts `--tmpfs <dir>` cleanly on concrete directory paths, whereas
        // `dir/**` causes `@anthropic-ai/sandbox-runtime` to walk inside denied directories and
        // follow symlinks (e.g. a sibling agent's `.cargo/registry`, `TASK-163`). Strip trailing
        // `/**`/`/*` on concrete non-wildcard prefixes in `denyRead` so bwrap masks the directory
        // atomically at its boundary.
        *deny_read = normalize_linux_bwrap_deny_read(std::mem::take(deny_read));
        // Linux `bwrap` mounts `/` read-only (`--ro-bind / /`) and only `allowWrite` roots are
        // writable. Omitting `denyWrite` entries outside `allowWrite` keeps write protection
        // intact while preventing `@anthropic-ai/sandbox-runtime` from creating 0-byte `0444`
        // regular files on the real host for absent paths (`TASK-144`). Full write denies remain
        // enforced in `permissions.deny` and the office approval bridge.
        *deny_write = filter_linux_bwrap_deny_write(std::mem::take(deny_write), writable);
    }
}

pub(crate) fn normalize_linux_bwrap_deny_read(deny_read: Vec<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(deny_read.len());
    for entry in deny_read {
        let candidate = entry
            .strip_suffix("/**")
            .or_else(|| entry.strip_suffix("/*"))
            .filter(|prefix| {
                !prefix.is_empty()
                    && !prefix.contains('*')
                    && !prefix.contains('?')
                    && !prefix.contains('[')
            })
            .map(str::to_owned)
            .unwrap_or(entry);
        if !out.iter().any(|existing| existing == &candidate) {
            out.push(candidate);
        }
    }
    out
}

fn split_path_segments(raw: &str) -> Vec<&str> {
    raw.split('/').filter(|seg| !seg.is_empty()).collect()
}

/// Returns `true` when `pattern` could match any path at, inside, or enclosing `root`.
/// Used on Linux `bwrap` (where `/` is read-only) so `denyWrite` retains every write deny that
/// overlaps an `allowWrite` root while dropping disjoint entries outside `allowWrite`.
fn pattern_overlaps_root(pattern: &str, root: &str) -> bool {
    let trimmed = pattern
        .strip_suffix("/**")
        .or_else(|| pattern.strip_suffix("/*"))
        .unwrap_or(pattern);
    let pat_segs = split_path_segments(trimmed);
    let root_segs = split_path_segments(root);
    for (p, r) in pat_segs.iter().zip(root_segs.iter()) {
        if *p == "**" {
            return true;
        }
        if p.contains('*') || p.contains('?') || p.contains('[') {
            continue;
        }
        if p != r {
            return false;
        }
    }
    true
}

pub(crate) fn filter_linux_bwrap_deny_write(
    deny_write: Vec<String>,
    writable: &[String],
) -> Vec<String> {
    deny_write
        .into_iter()
        .filter(|pat| writable.iter().any(|root| pattern_overlaps_root(pat, root)))
        .collect()
}

const LINUX_PRIVILEGED_SOCKET_DENIES: &[&str] = &[
    "/run/user/*/bus",
    "/run/user/*/systemd/**",
    "/run/user/*/podman/**",
    "/run/dbus/system_bus_socket",
    "/var/run/dbus/system_bus_socket",
    "/var/run/docker.sock",
    "/run/docker.sock",
    "/run/podman/**",
];

fn append_linux_privileged_socket_denies(deny_read: &mut Vec<String>) {
    // Defense-in-depth when Linux seccomp requires `allowAllUnixSockets`: hide well-known
    // privileged host sockets (systemd user D-Bus, system D-Bus, Docker/Podman) inside bwrap.
    // Under Linux `bwrap` `/` is already read-only, so these only need `denyRead` (`TASK-144`).
    for &pat in LINUX_PRIVILEGED_SOCKET_DENIES {
        if !deny_read.iter().any(|p| p == pat) {
            deny_read.push(pat.to_owned());
        }
    }
}

fn build_sandbox_network_json(
    allowed_unix_sockets: &[SpecPath],
    allow_all_unix_sockets: bool,
) -> Value {
    let allow_sockets: Vec<Value> = allowed_unix_sockets
        .iter()
        .map(|p| Value::String(p.as_str().to_owned()))
        .collect();
    let mut network = Map::new();
    network.insert("allowUnixSockets".to_owned(), Value::Array(allow_sockets));
    if allow_all_unix_sockets {
        // Linux seccomp filters `socket(AF_UNIX)` without inspecting `connect()` paths,
        // so Claude's `bwrap` sandbox blocks `agent-os` unless `allowAllUnixSockets` is set.
        network.insert("allowAllUnixSockets".to_owned(), Value::Bool(true));
    }
    Value::Object(network)
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
    if ToolCapability::is_capability_token(tool) {
        let cap = ToolCapability::parse_rule_token(tool)?;
        return format_capability_rule(cap, &rule.scope, kind);
    }
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

fn format_capability_rule(
    cap: ToolCapability,
    scope: &RuleScope,
    kind: RuleKind,
) -> Option<Vec<String>> {
    match scope {
        RuleScope::Any => {
            capability_tools(cap, kind).map(|tools| tools.iter().map(|&t| t.to_owned()).collect())
        }
        RuleScope::Path(pattern) => {
            let path = claude_rule_path(pattern.as_str());
            let tools: &[&str] = match cap {
                ToolCapability::Read => CAP_READ_TOOLS,
                ToolCapability::Edit => match kind {
                    RuleKind::Deny => CAP_EDIT_DENY_TOOLS,
                    RuleKind::Allow => CAP_EDIT_ALLOW_TOOLS,
                },
                _ => return None,
            };
            Some(tools.iter().map(|t| format!("{t}({path})")).collect())
        }
        RuleScope::CommandPrefix(prefix) if cap == ToolCapability::Shell => {
            command_prefix_rules(prefix.as_str(), kind)
        }
        RuleScope::CommandPrefix(_) | RuleScope::McpServer(_) => None,
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

fn build_agents_json(spec: &SessionSpec, expanded_tools: Option<&[String]>) -> String {
    let mut def = Map::new();
    def.insert(
        "description".to_owned(),
        Value::String(spec.agent.description.clone()),
    );
    def.insert(
        "prompt".to_owned(),
        Value::String(spec.agent.prompt.clone()),
    );
    if let Some(tools) = expanded_tools {
        def.insert(
            "tools".to_owned(),
            Value::Array(tools.iter().map(|t| Value::String(t.clone())).collect()),
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
        // Scrub Claude credentials from tool subprocesses (empirically verified in Claude 2.1.311
        // scrub list `lts` + `U7r`; defense-in-depth alongside CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR).
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
    [
        "CLAUDE_CODE_OAUTH_TOKEN",
        "CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR",
        "ANTHROPIC_API_KEY",
        "CLAUDE_CODE_API_KEY_FILE_DESCRIPTOR",
        "ANTHROPIC_BASE_URL",
    ]
    .into_iter()
    .map(|name| EnvName::try_from(name).expect("static auth env name is valid"))
    .collect()
}

fn secret_env_names() -> Vec<EnvName> {
    [
        "CLAUDE_CODE_OAUTH_TOKEN",
        "CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR",
        "CLAUDE_CODE_MESSAGING_TOKEN",
        "ANTHROPIC_API_KEY",
        "CLAUDE_CODE_API_KEY_FILE_DESCRIPTOR",
    ]
    .into_iter()
    .map(|name| EnvName::try_from(name).expect("static secret env name is valid"))
    .collect()
}

#[cfg(test)]
mod sandbox_network_tests {
    use super::*;

    #[test]
    fn build_sandbox_network_json_sets_allow_all_only_when_requested() {
        let sock = SpecPath::try_from("/run/office/off.sock").unwrap();
        let strict = build_sandbox_network_json(std::slice::from_ref(&sock), false);
        assert_eq!(strict["allowUnixSockets"], json!(["/run/office/off.sock"]));
        assert!(strict.get("allowAllUnixSockets").is_none());

        let linux = build_sandbox_network_json(std::slice::from_ref(&sock), true);
        assert_eq!(linux["allowUnixSockets"], json!(["/run/office/off.sock"]));
        assert_eq!(linux["allowAllUnixSockets"], json!(true));
    }

    #[test]
    fn linux_privileged_socket_denies_mask_dbus_and_docker_without_duplicates() {
        let mut deny_read = vec!["/run/docker.sock".to_owned()];
        append_linux_privileged_socket_denies(&mut deny_read);
        for expected in [
            "/run/user/*/bus",
            "/run/user/*/systemd/**",
            "/run/dbus/system_bus_socket",
            "/var/run/docker.sock",
            "/run/docker.sock",
        ] {
            assert!(
                deny_read.iter().any(|p| p == expected),
                "deny_read missing {expected}"
            );
        }
        assert_eq!(
            deny_read
                .iter()
                .filter(|p| *p == "/run/docker.sock")
                .count(),
            1
        );

        // `TASK-163`: concrete `dir/**` entries normalize to `dir` (deduplicated) while wildcard
        // patterns like `/run/user/*/systemd/**` stay intact.
        let normalized = normalize_linux_bwrap_deny_read(vec![
            "/office/agents/sibling-1/**".into(),
            "/office/agents/sibling-1".into(),
            "/office/durable/state/**".into(),
            "/run/user/*/systemd/**".into(),
            "/home/agents/.cargo/credentials.toml".into(),
        ]);
        assert_eq!(
            normalized,
            vec![
                "/office/agents/sibling-1",
                "/office/durable/state",
                "/run/user/*/systemd/**",
                "/home/agents/.cargo/credentials.toml",
            ]
        );
    }

    #[test]
    fn filter_linux_bwrap_deny_write_keeps_only_entries_overlapping_allow_write() {
        let writable = vec![
            "/work/t-1".to_owned(),
            "/run/user/1001/t/abc".to_owned(),
            "/var/lib/org/repos/r1.git/objects".to_owned(),
            "/var/lib/org/repos/r1.git/worktrees/wt1".to_owned(),
        ];
        let deny_write = vec![
            "/work/t-1/.git/hooks/**".to_owned(),
            "/var/lib/org/repos/r1.git/objects/info".to_owned(),
            "/var/lib/org/repos/r1.git/objects/info/**".to_owned(),
            "/var/lib/org/repos/r1.git/worktrees/wt1/commondir".to_owned(),
            "/var/lib/org/repos/r1.git/hooks".to_owned(),
            "/var/lib/org/repos/r1.git/hooks/**".to_owned(),
            "/var/lib/org/repos/r1.git/config".to_owned(),
            "/office/durable/authority/**".to_owned(),
            "/home/agents/.claude".to_owned(),
            "/run/user/*/bus".to_owned(),
            "/var/lib/org/repos/**".to_owned(),
        ];
        let kept = filter_linux_bwrap_deny_write(deny_write, &writable);
        for must_keep in [
            "/work/t-1/.git/hooks/**",
            "/var/lib/org/repos/r1.git/objects/info",
            "/var/lib/org/repos/r1.git/objects/info/**",
            "/var/lib/org/repos/r1.git/worktrees/wt1/commondir",
            "/var/lib/org/repos/**",
        ] {
            assert!(
                kept.iter().any(|p| p == must_keep),
                "expected {must_keep} to remain in denyWrite: {kept:?}"
            );
        }
        for must_drop in [
            "/var/lib/org/repos/r1.git/hooks",
            "/var/lib/org/repos/r1.git/hooks/**",
            "/var/lib/org/repos/r1.git/config",
            "/office/durable/authority/**",
            "/home/agents/.claude",
            "/run/user/*/bus",
        ] {
            assert!(
                !kept.iter().any(|p| p == must_drop),
                "expected {must_drop} outside allowWrite to be dropped: {kept:?}"
            );
        }
    }
}
