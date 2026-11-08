//! [`LaunchPlan`]s: the argv, the generated files and how to stop Claude Code.

use org_runtime::{
    Capabilities, CleanupPath, EnvName, FileLocation, LaunchContext, LaunchPlan, PermissionMode,
    PlanError, PlannedFile, RelPath, SessionId, SessionKind, SessionSpec, SignalTarget, SpecPath,
    Stdio, StopPlan, StopSignal, StopStep,
};
use serde_json::{json, Map, Value};

use crate::rules;

/// The generated files, in [`LaunchContext::runtime_dir`].
pub(crate) const SETTINGS_FILE: &str = "settings.json";
pub(crate) const MCP_FILE: &str = "mcp.json";
pub(crate) const INSTRUCTIONS_FILE: &str = "instructions.md";

/// The socket every Claude process listens on, which a SIGKILL leaves behind (spike fact 9),
/// and the folder it must stay in. By its real path: `/tmp` is a symlink on macOS, and the
/// supervisor follows none.
pub(crate) const MESSAGING_SOCKET_DIR: &str = "/private/tmp/cc-socks";
pub(crate) const MESSAGING_SOCKET: &str = "/private/tmp/cc-socks/{pid}.sock";

/// The secret Claude puts into every tool's environment (spike fact 9).
pub(crate) const MESSAGING_TOKEN_VAR: &str = "CLAUDE_CODE_MESSAGING_TOKEN";

/// The agents Claude 2.1.311 brings itself (`initialize`'s `agents`, case aside). A spec's
/// agent can't take one of their names: if Claude ever dropped the inline definition, the
/// session's agent check would pass on the built-in one, which has every tool.
const BUILT_IN_AGENTS: &[&str] = &[
    "claude",
    "explore",
    "general-purpose",
    "plan",
    "statusline-setup",
];

/// Tools that start a subagent. A subagent gets its own tools, not the main thread's, so an
/// allowlist that holds one of them can't be enforced exactly.
const AGENT_TOOLS: &[&str] = &["Task", "Agent"];

/// How long a closed stdin gets before the signals: Claude finishes the turn it's in first.
const CLOSE_STDIN_GRACE_MS: u64 = 30_000;
/// After SIGTERM to Claude itself, which kills its tools' trees (they left its group).
const TERM_LEADER_GRACE_MS: u64 = 10_000;
/// After SIGTERM to what's left of the group, before the supervisor's SIGKILL.
const TERM_GROUP_GRACE_MS: u64 = 5_000;

/// What the plan is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// A headless stream-json session, driven by a `Session` over pipes.
    Headless,
    /// The interactive TUI on a terminal, for a person.
    TakeOver { cols: u16, rows: u16 },
}

/// `--permission-mode`'s value for a mode, and the mode Claude then reports (`initialize`'s
/// `current_permission_mode`, `init`'s `permissionMode`). The spike ran `manual`, which
/// Claude reports as `default`.
pub(crate) fn permission_mode_words(mode: PermissionMode) -> Option<(&'static str, &'static str)> {
    match mode {
        PermissionMode::Ask => Some(("manual", "default")),
        PermissionMode::AcceptEdits => Some(("acceptEdits", "acceptEdits")),
        PermissionMode::Plan => Some(("plan", "plan")),
        _ => None,
    }
}

/// Everything a session needs from its plan.
pub(crate) struct Planned {
    pub plan: LaunchPlan,
    /// The mode Claude must report, in its words.
    pub reported_mode: &'static str,
    /// The writable roots (TMPDIR included) while the sandbox enforces them.
    pub writable: Option<Vec<String>>,
}

/// Builds the plan for a headless session or a take-over.
pub(crate) fn plan(
    spec: &SessionSpec,
    ctx: &LaunchContext,
    caps: &Capabilities,
    mode: Mode,
) -> Result<Planned, PlanError> {
    if spec.runtime.as_str() != crate::RUNTIME_ID {
        return Err(PlanError::InvalidSpec(format!(
            "the spec is for the runtime {:?}, not {:?}",
            spec.runtime.as_str(),
            crate::RUNTIME_ID
        )));
    }
    match &spec.kind {
        SessionKind::Home {} | SessionKind::Task { .. } => {}
        SessionKind::OneOff { .. } if matches!(mode, Mode::TakeOver { .. }) => {
            return Err(PlanError::InvalidSpec(
                "a one-off can't be taken over".to_owned(),
            ))
        }
        SessionKind::OneOff { .. } => {}
        _ => {
            return Err(PlanError::Unsupported(
                "a session kind this adapter doesn't know",
            ))
        }
    }
    if BUILT_IN_AGENTS.contains(&spec.agent.name.as_str()) {
        return Err(PlanError::InvalidSpec(format!(
            "the agent name {:?} is one of Claude's built-in agents",
            spec.agent.name.as_str()
        )));
    }
    if let Some(tool) = spec
        .tools
        .iter()
        .flatten()
        .find(|t| AGENT_TOOLS.contains(&t.as_str()))
    {
        return Err(PlanError::InvalidSpec(format!(
            "a tool allowlist holding {:?} can't be enforced: its subagents have their own tools",
            tool.as_str()
        )));
    }
    // `/` as the cwd, a writable root, TMPDIR or the runtime dir would open (or hand over)
    // the whole disk.
    let roots = [&spec.workspace, &ctx.tmpdir, &ctx.runtime_dir]
        .into_iter()
        .chain(&spec.sandbox.writable_roots);
    if roots.into_iter().any(SpecPath::is_root) {
        return Err(PlanError::InvalidSpec(
            "`/` can't be the workspace, a writable root, the TMPDIR or the runtime dir".to_owned(),
        ));
    }
    // The runtime dir holds the settings and instructions: never agent-writable.
    let writable_paths = spec
        .sandbox
        .writable_roots
        .iter()
        .chain([&spec.workspace, &ctx.tmpdir]);
    for path in writable_paths {
        let (rt, w) = (ctx.runtime_dir.as_str(), path.as_str());
        if rules::within(rt, w) || rules::within(w, rt) {
            return Err(PlanError::InvalidSpec(format!(
                "the runtime dir {rt:?} overlaps the writable path {w:?}"
            )));
        }
    }
    let effective = spec.effective_permission_mode(caps);
    let (mode_arg, reported_mode) = permission_mode_words(effective).ok_or(
        PlanError::Unsupported("a permission mode this adapter doesn't know"),
    )?;

    let session_id: SessionId = match (&ctx.resume, &ctx.new_session_id, mode) {
        (Some(id), _, _) => id.clone(),
        (None, _, Mode::TakeOver { .. }) => {
            return Err(PlanError::InvalidSpec(
                "a take-over resumes a conversation, and none was given".to_owned(),
            ))
        }
        (None, Some(id), Mode::Headless) => id.clone(),
        (None, None, Mode::Headless) => {
            return Err(PlanError::InvalidSpec(
                "a new Claude session needs LaunchContext::new_session_id".to_owned(),
            ))
        }
    };

    let policy = rules::translate(spec, ctx, caps)?;
    let rt = ctx.runtime_dir.as_str();
    let in_rt = |name: &str| format!("{rt}/{name}");

    let mut args: Vec<String> = Vec::new();
    let mut arg = |a: &str| args.push(a.to_owned());
    if mode == Mode::Headless {
        for a in [
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
        ] {
            arg(a);
        }
    }
    arg("--setting-sources");
    arg("");
    arg("--settings");
    arg(&in_rt(SETTINGS_FILE));
    arg("--strict-mcp-config");
    arg("--mcp-config");
    arg(&in_rt(MCP_FILE));
    arg("--permission-mode");
    arg(mode_arg);
    if mode == Mode::Headless {
        arg("--permission-prompt-tool");
        arg("stdio");
    }
    arg("--agents");
    arg(&agents_json(spec));
    arg("--agent");
    arg(spec.agent.name.as_str());
    arg("--append-system-prompt-file");
    arg(&in_rt(INSTRUCTIONS_FILE));
    if let Some(model) = &spec.model {
        arg("--model");
        arg(model.as_str());
    }
    if let Some(effort) = &spec.effort {
        arg("--effort");
        arg(effort.as_str());
    }
    for dir in &spec.read_only_dirs {
        arg("--add-dir");
        arg(dir.as_str());
    }
    // An empty allowlist: `--tools ""` (spike item 5c), since an agent's `"tools": []` isn't
    // verified to mean "none". A non-empty one is the agent's `tools` (item 6).
    if spec.tools.as_ref().is_some_and(|t| t.is_empty()) {
        arg("--tools");
        arg("");
    }
    // Session ids are runtime text: only in the `--flag=value` form, so one can never read
    // as a flag. Last, after every variadic option.
    if ctx.resume.is_some() {
        args.push(format!("--resume={}", session_id.as_str()));
    } else {
        args.push(format!("--session-id={}", session_id.as_str()));
    }

    let files = vec![
        planned_file(SETTINGS_FILE, settings_json(&policy)),
        planned_file(MCP_FILE, mcp_json(spec)),
        PlannedFile {
            location: FileLocation::RuntimeDir,
            rel_path: rel(INSTRUCTIONS_FILE),
            contents: spec.instructions.as_bytes().to_vec(),
            git_exclude: false,
        },
    ];

    let stop = match mode {
        Mode::Headless => vec![
            StopStep::CloseStdin {
                grace_ms: CLOSE_STDIN_GRACE_MS,
            },
            term(SignalTarget::Leader, TERM_LEADER_GRACE_MS),
            term(SignalTarget::Group, TERM_GROUP_GRACE_MS),
        ],
        // A terminal has no stdin to close; the person normally leaves with /exit.
        Mode::TakeOver { .. } => vec![
            term(SignalTarget::Leader, TERM_LEADER_GRACE_MS),
            term(SignalTarget::Group, TERM_GROUP_GRACE_MS),
        ],
    };

    let plan = LaunchPlan {
        program: ctx.runtime_path.clone(),
        args,
        cwd: spec.workspace.clone(),
        env_add: vec![(env_name("DISABLE_AUTOUPDATER"), "1".to_owned())],
        dirs: vec![],
        files,
        stdio: match mode {
            Mode::Headless => Stdio::Piped,
            Mode::TakeOver { cols, rows } => Stdio::Pty { cols, rows },
        },
        runtime_session_id: Some(session_id),
        stop: StopPlan { steps: stop },
        cleanup_after_exit: vec![messaging_socket()],
        secret_env_names: vec![env_name(MESSAGING_TOKEN_VAR)],
    };
    Ok(Planned {
        plan,
        reported_mode,
        writable: spec.sandbox.enabled.then(|| {
            let mut roots: Vec<String> = spec
                .sandbox
                .writable_roots
                .iter()
                .map(|p| p.as_str().to_owned())
                .collect();
            roots.push(ctx.tmpdir.as_str().to_owned());
            roots
        }),
    })
}

/// `--agents`: the one inline agent, keyed by its name, with its tools when the spec limits
/// them.
fn agents_json(spec: &SessionSpec) -> String {
    let mut agent = Map::new();
    agent.insert("description".into(), json!(spec.agent.description));
    agent.insert("prompt".into(), json!(spec.agent.prompt));
    if let Some(tools) = &spec.tools {
        let names: Vec<&str> = tools.iter().map(|t| t.as_str()).collect();
        agent.insert("tools".into(), json!(names));
    }
    let mut agents = Map::new();
    agents.insert(spec.agent.name.as_str().to_owned(), Value::Object(agent));
    serde_json::to_string(&Value::Object(agents)).expect("JSON values always serialize")
}

/// The office's `--settings` file: the only settings Claude loads (`--setting-sources ""`).
fn settings_json(policy: &rules::PolicySettings) -> Value {
    json!({
        "autoMemoryEnabled": false,
        "permissions": {
            "allow": policy.allow,
            "deny": policy.deny,
            "disableBypassPermissionsMode": "disable",
        },
        "sandbox": policy.sandbox,
    })
}

/// `--mcp-config`: only the office's servers (`--strict-mcp-config` ignores every other).
fn mcp_json(spec: &SessionSpec) -> Value {
    let servers: Map<String, Value> = spec
        .mcp_servers
        .iter()
        .map(|(name, server)| {
            (
                name.as_str().to_owned(),
                json!({
                    "type": "stdio",
                    "command": server.command.as_str(),
                    "args": server.args,
                }),
            )
        })
        .collect();
    json!({ "mcpServers": servers })
}

fn planned_file(name: &str, value: Value) -> PlannedFile {
    let mut contents = serde_json::to_vec_pretty(&value).expect("JSON values always serialize");
    contents.push(b'\n');
    PlannedFile {
        location: FileLocation::RuntimeDir,
        rel_path: rel(name),
        contents,
        git_exclude: false,
    }
}

fn messaging_socket() -> CleanupPath {
    let root = SpecPath::try_from(MESSAGING_SOCKET_DIR).expect("a valid path");
    CleanupPath::new(root, MESSAGING_SOCKET).expect("the messaging socket is a valid cleanup path")
}

fn term(target: SignalTarget, grace_ms: u64) -> StopStep {
    StopStep::Signal {
        signal: StopSignal::Terminate,
        target,
        grace_ms,
    }
}

fn rel(name: &str) -> RelPath {
    RelPath::try_from(name).expect("the file names are valid relative paths")
}

pub(crate) fn env_name(name: &str) -> EnvName {
    EnvName::try_from(name).expect("the adapter's variable names are valid")
}
