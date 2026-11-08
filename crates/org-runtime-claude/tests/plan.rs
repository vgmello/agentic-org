//! Launch plans: argv, the generated files and settings, deny-wins, secrets and refusals.

mod common;

use std::time::Instant;

use common::*;
use org_runtime::{
    AgentRuntime, CleanupPath, EnvName, FileLocation, LaunchPlan, McpServerName, PermissionMode,
    PlainId, PlanError, RuleScope, SessionEvent, SessionId, SessionKind, SignalTarget, Stdio,
    StopSignal, StopStep, Token,
};
use org_runtime_claude::ClaudeRuntime;
use serde_json::{json, Value};

fn file(plan: &LaunchPlan, name: &str) -> Vec<u8> {
    let f = plan
        .files
        .iter()
        .find(|f| f.rel_path.as_str() == name)
        .unwrap_or_else(|| panic!("no {name}"));
    assert_eq!(f.location, FileLocation::RuntimeDir);
    assert!(!f.git_exclude);
    f.contents.clone()
}

fn settings(plan: &LaunchPlan) -> Value {
    serde_json::from_slice(&file(plan, "settings.json")).unwrap()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_owned())
        .collect()
}

fn refused(spec: &org_runtime::SessionSpec, c: &org_runtime::LaunchContext) -> PlanError {
    match ClaudeRuntime::new().plan_session(spec, c) {
        Ok(_) => panic!("planned a spec that should be refused"),
        Err(e) => e,
    }
}

#[test]
fn headless_argv() {
    let mut s = spec("ac-dev-ada", PermissionMode::Ask);
    s.model = Some(Token::try_from("opus").unwrap());
    s.effort = Some(Token::try_from("high").unwrap());
    let (plan, _) = common::plan(&s, &ctx());
    let agents = r#"{"ac-dev-ada":{"description":"Staff engineer","prompt":"You are Ada."}}"#;
    let rt = RUNTIME_DIR;
    let expected: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--setting-sources",
        "",
        "--settings",
        &format!("{rt}/settings.json"),
        "--strict-mcp-config",
        "--mcp-config",
        &format!("{rt}/mcp.json"),
        "--permission-mode",
        "manual",
        "--permission-prompt-tool",
        "stdio",
        "--agents",
        agents,
        "--agent",
        "ac-dev-ada",
        "--append-system-prompt-file",
        &format!("{rt}/instructions.md"),
        "--model",
        "opus",
        "--effort",
        "high",
        "--add-dir",
        HOME,
        &format!("--session-id={NEW_ID}"),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(plan.args, expected);
    assert_eq!(plan.program, ctx().runtime_path);
    assert_eq!(plan.cwd.as_str(), WORKSPACE);
    assert_eq!(plan.stdio, Stdio::Piped);
    assert_eq!(
        plan.runtime_session_id,
        Some(SessionId::try_from(NEW_ID).unwrap())
    );
    assert_eq!(
        plan.env_add,
        vec![(
            EnvName::try_from("DISABLE_AUTOUPDATER").unwrap(),
            "1".into()
        )]
    );
    assert!(plan.dirs.is_empty());
    assert_eq!(
        file(&plan, "instructions.md"),
        b"Do the task.\nThen stop.".to_vec()
    );
    let mcp: Value = serde_json::from_slice(&file(&plan, "mcp.json")).unwrap();
    assert_eq!(
        mcp,
        json!({"mcpServers": {"office": {"type": "stdio",
            "command": "/private/var/office/bin/org-mcp", "args": ["--stdio"]}}})
    );
}

#[test]
fn resume_uses_the_flag_value_form() {
    let mut c = ctx();
    c.resume = Some(SessionId::try_from("abc-123").unwrap());
    let (plan, _) = common::plan(&spec("ada", PermissionMode::Ask), &c);
    assert_eq!(plan.args.last().unwrap(), "--resume=abc-123");
    assert!(!plan.args.iter().any(|a| a.starts_with("--session-id")));
    // --permission-mode on every start and resume.
    assert!(plan
        .args
        .windows(2)
        .any(|w| w == ["--permission-mode", "manual"]));
    assert_eq!(
        plan.runtime_session_id,
        Some(SessionId::try_from("abc-123").unwrap())
    );
}

#[test]
fn settings_file() {
    let (plan, _) = common::plan(&spec("ada", PermissionMode::AcceptEdits), &ctx());
    assert!(plan
        .args
        .windows(2)
        .any(|w| w == ["--permission-mode", "acceptEdits"]));
    let s = settings(&plan);
    assert_eq!(s["autoMemoryEnabled"], json!(false));
    assert_eq!(
        s["permissions"]["disableBypassPermissionsMode"],
        json!("disable")
    );
    assert_eq!(strings(&s["permissions"]["allow"]), ["Bash(git status*)"]);
    let home = HOME;
    assert_eq!(
        strings(&s["permissions"]["deny"]),
        [
            // Fixed: every agent's transcripts and every Claude's messaging socket, each
            // directory both as `X/**` and as `X` (gitignore's `X/**` doesn't match `X`).
            "Read(~/.claude/**)".to_owned(),
            "Read(~/.claude)".into(),
            "Read(~/.claude.json)".into(),
            "Read(//tmp/cc-socks/**)".into(),
            "Read(//tmp/cc-socks)".into(),
            "Read(//private/tmp/cc-socks/**)".into(),
            "Read(//private/tmp/cc-socks)".into(),
            // Fixed: it needs content back.
            "AskUserQuestion".into(),
            // Fixed: no file tool writes the user's Claude config.
            "Edit(~/.claude/**)".into(),
            "Edit(~/.claude)".into(),
            "Edit(~/.claude.json)".into(),
            "Write(~/.claude/**)".into(),
            "Write(~/.claude)".into(),
            "Write(~/.claude.json)".into(),
            "NotebookEdit(~/.claude/**)".into(),
            "NotebookEdit(~/.claude)".into(),
            "NotebookEdit(~/.claude.json)".into(),
            // The read-only home: no file tool writes it.
            format!("Edit(/{home}/**)"),
            format!("Edit(/{home})"),
            format!("Write(/{home}/**)"),
            format!("Write(/{home})"),
            format!("NotebookEdit(/{home}/**)"),
            format!("NotebookEdit(/{home})"),
            // deny_write, for the file tools.
            "Edit(//private/var/office/work.git/HEAD)".into(),
            "Write(//private/var/office/work.git/HEAD)".into(),
            "NotebookEdit(//private/var/office/work.git/HEAD)".into(),
            // deny_read, for the file tools.
            "Read(//private/var/office/offices/*/office.db)".into(),
        ]
    );
    assert_eq!(
        s["sandbox"],
        json!({
            "enabled": true,
            "autoAllowBashIfSandboxed": false,
            "allowUnsandboxedCommands": false,
            "filesystem": {
                "allowWrite": [WORKSPACE, "/private/var/office/work.git/objects", TMPDIR],
                "denyWrite": [HOME, "/private/var/office/work.git/HEAD"],
                "denyRead": ["~/.claude", "~/.claude.json", "/tmp/cc-socks",
                    "/private/tmp/cc-socks", "/private/var/office/offices/*/office.db"],
            },
            "network": {"allowUnixSockets": ["/private/var/office/run/ada.sock"]},
        })
    );
}

#[test]
fn read_only_dirs_are_never_written_by_the_file_tools() {
    // Pinned: every edit tool Claude 2.1.311 has gets a deny on each read-only dir, so
    // acceptEdits (which accepts edits in every --add-dir) can't write the agent's home.
    let mut s = spec("ada", PermissionMode::AcceptEdits);
    s.read_only_dirs = vec![p(HOME), p("/private/var/office/shared")];
    let (plan, _) = common::plan(&s, &ctx());
    let deny = strings(&settings(&plan)["permissions"]["deny"]);
    for dir in [HOME, "/private/var/office/shared"] {
        for tool in ["Edit", "Write", "NotebookEdit"] {
            for rule in [format!("{tool}(/{dir}/**)"), format!("{tool}(/{dir})")] {
                assert!(deny.contains(&rule), "{rule} missing from {deny:?}");
            }
        }
        // And for Bash, explicitly, by the sandbox.
        let deny_write = strings(&settings(&plan)["sandbox"]["filesystem"]["denyWrite"]);
        assert!(deny_write.contains(&dir.to_owned()), "{dir}");
        assert!(plan.args.windows(2).any(|w| w == ["--add-dir", dir]));
    }
    // Never writable through the sandbox either.
    let allow_write = strings(&settings(&plan)["sandbox"]["filesystem"]["allowWrite"]);
    assert!(!allow_write.iter().any(|w| w.starts_with(HOME)));
}

#[test]
fn tmpdir_is_always_writable() {
    let mut s = spec("ada", PermissionMode::Ask);
    s.sandbox.writable_roots = vec![p(WORKSPACE)];
    let (plan, _) = common::plan(&s, &ctx());
    assert_eq!(
        strings(&settings(&plan)["sandbox"]["filesystem"]["allowWrite"]),
        [WORKSPACE, TMPDIR]
    );
}

#[test]
fn deny_wins_when_a_deny_is_dropped() {
    // With the sandbox off, its deny lists are dropped: so every allow goes, and
    // accept_edits narrows to asking.
    let mut s = spec("zephyr", PermissionMode::AcceptEdits);
    s.sandbox.enabled = false;
    s.policy
        .allow
        .push(rule("Read", path_scope("/private/var/office/docs/**")));
    let (plan, mut session) = common::plan(&s, &ctx());
    let set = settings(&plan);
    assert_eq!(set["permissions"]["allow"], json!([]));
    assert_eq!(set["sandbox"], json!({"enabled": false}));
    assert!(plan
        .args
        .windows(2)
        .any(|w| w == ["--permission-mode", "manual"]));
    // The dropped lists are gone from the rules too; the fixed read denies stay.
    let deny = strings(&set["permissions"]["deny"]);
    assert!(deny.contains(&"Read(~/.claude/**)".to_owned()));
    assert!(!deny
        .iter()
        .any(|d| d.contains("office.db") || d.contains("work.git/HEAD")));
    // And the session expects Claude to report the narrowed mode.
    let now = Instant::now();
    session.start(now);
    let ev = events(&session.on_line(&bytes(&recorded_initialize_reply("org-1")), now));
    assert!(matches!(ev[0], SessionEvent::Ready { .. }), "{ev:?}");

    // Nothing denied, nothing dropped: the allows stay.
    let mut s = spec("ada", PermissionMode::AcceptEdits);
    s.sandbox.enabled = false;
    s.sandbox.deny_write.clear();
    s.sandbox.deny_read.clear();
    let (plan, _) = common::plan(&s, &ctx());
    assert_eq!(
        settings(&plan)["permissions"]["allow"],
        json!(["Bash(git status*)"])
    );
    assert!(plan
        .args
        .windows(2)
        .any(|w| w == ["--permission-mode", "acceptEdits"]));
}

#[test]
fn rule_translation() {
    let mut s = spec("ada", PermissionMode::Ask);
    s.policy.allow = vec![
        rule("Bash", prefix("cargo test")),
        rule("*", prefix("git log")),
        rule("Bash", prefix("npm:")), // legacy-syntax lookalike: wider, dropped
        rule("Read", path_scope("/private/var/office/docs/**")),
        rule("Edit", path_scope(&format!("{WORKSPACE}/src/**"))),
        rule("Edit", path_scope("/private/var/elsewhere/**")), // outside the roots: dropped
        rule("Write", path_scope(&format!("{WORKSPACE}/x"))),  // Edit(...) would widen: dropped
        rule("*", RuleScope::Any),                             // no spelling: dropped
        rule("Edit", RuleScope::Any),                          // edits anywhere: dropped
        rule("WebFetch", RuleScope::Any),
        rule(
            "*",
            RuleScope::McpServer(McpServerName::try_from("office").unwrap()),
        ),
        rule("Grep", path_scope("/private/var/office/docs/**")), // Read(...) would widen
        // gitignore would grant everything under a matched directory: only `/**` grants.
        rule("Read", path_scope("/private/var/office/notes/*")),
        rule("Read", path_scope("/private/var/office/notes/file")),
        // Claude matches each part of a compound command: not a literal prefix.
        rule("Bash", prefix("cd /x && make")),
        // `mcp__a__b` would read as server `a`, tool `b`.
        rule(
            "*",
            RuleScope::McpServer(McpServerName::try_from("a__b").unwrap()),
        ),
    ];
    s.policy.deny = vec![
        rule("WebSearch", RuleScope::Any),
        rule("Bash", prefix("rm ")),
        rule("Grep", path_scope("/private/var/secret/**")),
        rule("*", path_scope("/private/var/vault/**")),
        rule("Bash", path_scope("/private/var/keys")),
        rule(
            "mcp__office__delete",
            RuleScope::McpServer(McpServerName::try_from("office").unwrap()),
        ),
    ];
    let (plan, _) = common::plan(&s, &ctx());
    let set = settings(&plan);
    assert_eq!(
        strings(&set["permissions"]["allow"]),
        [
            "Bash(cargo test*)",
            "Bash(git log*)",
            "Read(//private/var/office/docs/**)",
            &format!("Edit(/{WORKSPACE}/src/**)"),
            "WebFetch",
            "mcp__office",
        ]
    );
    let deny = strings(&set["permissions"]["deny"]);
    for expected in [
        "WebSearch",
        "Bash(rm *)",
        "Read(//private/var/secret/**)",
        "Read(//private/var/secret)",
        "Read(//private/var/vault/**)",
        "Read(//private/var/vault)",
        "Edit(//private/var/vault/**)",
        "Write(//private/var/vault/**)",
        "NotebookEdit(//private/var/vault/**)",
        "mcp__office__delete",
    ] {
        assert!(
            deny.contains(&expected.to_owned()),
            "{expected} not in {deny:?}"
        );
    }
    // The shell is held to path denies by the sandbox.
    let fs = &set["sandbox"]["filesystem"];
    for path in ["/private/var/vault", "/private/var/keys"] {
        assert!(
            strings(&fs["denyRead"]).contains(&path.to_owned()),
            "{path}"
        );
        assert!(
            strings(&fs["denyWrite"]).contains(&path.to_owned()),
            "{path}"
        );
    }
}

#[test]
fn tool_allowlist() {
    let mut s = spec("ada", PermissionMode::Ask);
    s.tools = Some(vec![tool("Read"), tool("Bash"), tool("mcp__office__send")]);
    let (plan, _) = common::plan(&s, &ctx());
    let i = plan.args.iter().position(|a| a == "--agents").unwrap();
    let agents: Value = serde_json::from_str(&plan.args[i + 1]).unwrap();
    assert_eq!(
        agents["ada"]["tools"],
        json!(["Read", "Bash", "mcp__office__send"])
    );
    assert!(!plan.args.contains(&"--tools".to_owned()));

    // No tools at all: `--tools ""` as well.
    s.tools = Some(vec![]);
    let (plan, _) = common::plan(&s, &ctx());
    assert!(plan.args.windows(2).any(|w| w == ["--tools", ""]));
    let agents: Value = serde_json::from_str(&plan.args[i + 1]).unwrap();
    assert_eq!(agents["ada"]["tools"], json!([]));

    s.tools = None;
    let (plan, _) = common::plan(&s, &ctx());
    let agents: Value = serde_json::from_str(&plan.args[i + 1]).unwrap();
    assert!(agents["ada"].get("tools").is_none());
}

#[test]
fn stopping_and_cleaning_up() {
    let (plan, _) = common::plan(&spec("ada", PermissionMode::Ask), &ctx());
    // SIGTERM to Claude itself first: its tools left its process group, and it kills them.
    assert_eq!(
        plan.stop.steps,
        vec![
            StopStep::CloseStdin { grace_ms: 30_000 },
            StopStep::Signal {
                signal: StopSignal::Terminate,
                target: SignalTarget::Leader,
                grace_ms: 10_000
            },
            StopStep::Signal {
                signal: StopSignal::Terminate,
                target: SignalTarget::Group,
                grace_ms: 5_000
            },
        ]
    );
    assert_eq!(
        plan.cleanup_after_exit,
        vec![CleanupPath::try_from("/private/tmp/cc-socks/{pid}.sock").unwrap()]
    );
    assert_eq!(
        plan.cleanup_after_exit[0].path_for(42).to_str(),
        Some("/private/tmp/cc-socks/42.sock")
    );
    assert_eq!(
        plan.secret_env_names,
        vec![EnvName::try_from("CLAUDE_CODE_MESSAGING_TOKEN").unwrap()]
    );
}

#[test]
fn no_secret_or_bypass_reaches_the_plan() {
    let mut c = ctx();
    c.office_env_names = vec![
        EnvName::try_from("ORG_SESSION_TOKEN").unwrap(),
        EnvName::try_from("ORG_SESSION_MARK").unwrap(),
    ];
    let rt = ClaudeRuntime::new();
    let (headless, _) = rt
        .plan_session(&spec("ada", PermissionMode::Ask), &c)
        .unwrap();
    c.resume = Some(SessionId::try_from(NEW_ID).unwrap());
    let take_over = rt
        .plan_take_over(&spec("ada", PermissionMode::Ask), &c, 120, 40)
        .unwrap();
    for plan in [&headless, &take_over] {
        let mut text: Vec<String> = plan.args.clone();
        text.extend(
            plan.env_add
                .iter()
                .map(|(k, v)| format!("{}={v}", k.as_str())),
        );
        text.extend(
            plan.files
                .iter()
                .map(|f| String::from_utf8(f.contents.clone()).unwrap()),
        );
        let all = text.join("\n");
        for forbidden in [
            "ORG_SESSION_TOKEN",
            "ORG_SESSION_MARK",
            "CLAUDE_CODE_MESSAGING_TOKEN",
            "dangerously",
            "bypassPermissions",
            "CLAUDE_CONFIG_DIR",
            "CLAUDE_SECURESTORAGE_CONFIG_DIR",
        ] {
            assert!(!all.contains(forbidden), "{forbidden} in the plan");
        }
    }
}

#[test]
fn take_over_plan() {
    let mut c = ctx();
    c.resume = Some(SessionId::try_from(NEW_ID).unwrap());
    let s = spec("ada", PermissionMode::Ask);
    let plan = ClaudeRuntime::new()
        .plan_take_over(&s, &c, 120, 40)
        .unwrap();
    assert_eq!(
        plan.stdio,
        Stdio::Pty {
            cols: 120,
            rows: 40
        }
    );
    for gone in [
        "-p",
        "--input-format",
        "--output-format",
        "--verbose",
        "--include-partial-messages",
        "--permission-prompt-tool",
    ] {
        assert!(!plan.args.contains(&gone.to_owned()), "{gone}");
    }
    // The rest of the spec is replayed: settings (sandbox included), MCP, agent, mode.
    for kept in [
        "--settings",
        "--strict-mcp-config",
        "--agent",
        "--append-system-prompt-file",
    ] {
        assert!(plan.args.contains(&kept.to_owned()), "{kept}");
    }
    assert!(plan.args.windows(2).any(|w| w == ["--setting-sources", ""]));
    assert!(plan
        .args
        .windows(2)
        .any(|w| w == ["--permission-mode", "manual"]));
    assert_eq!(plan.args.last().unwrap(), &format!("--resume={NEW_ID}"));
    assert_eq!(settings(&plan)["sandbox"]["enabled"], json!(true));
    assert!(!plan
        .stop
        .steps
        .iter()
        .any(|s| matches!(s, StopStep::CloseStdin { .. })));
}

#[test]
fn refused_specs() {
    let rt = ClaudeRuntime::new();
    let base = spec("ada", PermissionMode::Ask);

    // A one-off can't be taken over; a take-over needs a conversation.
    let mut one_off = base.clone();
    one_off.kind = SessionKind::OneOff {
        run_id: PlainId::try_from("r1").unwrap(),
    };
    let mut resuming = ctx();
    resuming.resume = Some(SessionId::try_from(NEW_ID).unwrap());
    assert!(matches!(
        rt.plan_take_over(&one_off, &resuming, 80, 24),
        Err(PlanError::InvalidSpec(_))
    ));
    assert!(rt.plan_session(&one_off, &ctx()).is_ok());
    assert!(matches!(
        rt.plan_take_over(&base, &ctx(), 80, 24),
        Err(PlanError::InvalidSpec(_))
    ));

    // A new session needs the office's id.
    let mut no_id = ctx();
    no_id.new_session_id = None;
    assert!(matches!(refused(&base, &no_id), PlanError::InvalidSpec(_)));

    type Mutate = Box<dyn Fn(&mut org_runtime::SessionSpec)>;
    let cases: Vec<(&str, Mutate)> = vec![
        (
            "another runtime's spec",
            Box::new(|s| s.runtime = org_runtime::RuntimeId::try_from("codex").unwrap()),
        ),
        (
            "a deny of every tool",
            Box::new(|s| s.policy.deny.push(rule("*", RuleScope::Any))),
        ),
        (
            "a path deny for a tool with no path rules",
            Box::new(|s| s.policy.deny.push(rule("WebFetch", path_scope("/x/**")))),
        ),
        (
            "a command-prefix deny for a tool that isn't the shell",
            Box::new(|s| s.policy.deny.push(rule("Read", prefix("cat ")))),
        ),
        (
            "an MCP deny naming another server's tool",
            Box::new(|s| {
                s.policy.deny.push(rule(
                    "mcp__other__x",
                    RuleScope::McpServer(McpServerName::try_from("office").unwrap()),
                ))
            }),
        ),
        (
            "a shell path deny with the sandbox off",
            Box::new(|s| {
                s.sandbox.enabled = false;
                s.policy.deny.push(rule("Bash", path_scope("/x/**")));
            }),
        ),
        (
            "a writable root that reads as a glob",
            Box::new(|s| {
                s.sandbox
                    .writable_roots
                    .push(p("/private/var/work/[draft]"))
            }),
        ),
        (
            "a socket that reads as a glob",
            Box::new(|s| {
                s.sandbox
                    .allowed_unix_sockets
                    .push(p("/private/var/run/*.sock"))
            }),
        ),
        (
            "a read-only dir holding the workspace",
            Box::new(|s| s.read_only_dirs.push(p("/private/var/office/work"))),
        ),
        (
            "the workspace outside the writable roots",
            Box::new(|s| s.sandbox.writable_roots = vec![p("/private/var/office/other")]),
        ),
        (
            "a path that reads as rule syntax",
            Box::new(|s| s.read_only_dirs = vec![p("/private/var/office/a)b")]),
        ),
        (
            "a command-prefix deny with a shell operator",
            Box::new(|s| s.policy.deny.push(rule("Bash", prefix("cd /x && rm")))),
        ),
        (
            "a command-prefix deny with a quote",
            Box::new(|s| s.policy.deny.push(rule("Bash", prefix("echo 'x")))),
        ),
        (
            "an MCP deny on a server whose name holds __",
            Box::new(|s| {
                s.policy.deny.push(rule(
                    "*",
                    RuleScope::McpServer(McpServerName::try_from("a__b").unwrap()),
                ))
            }),
        ),
        (
            "a tool allowlist with a subagent tool",
            Box::new(|s| s.tools = Some(vec![tool("Read"), tool("Task")])),
        ),
        (
            "a tool allowlist with the Agent tool",
            Box::new(|s| s.tools = Some(vec![tool("Agent")])),
        ),
        (
            "an agent named like a built-in one",
            Box::new(|s| {
                s.agent.name = org_runtime::AgentName::try_from("general-purpose").unwrap()
            }),
        ),
        (
            "an agent named claude",
            Box::new(|s| s.agent.name = org_runtime::AgentName::try_from("claude").unwrap()),
        ),
    ];
    for (what, mutate) in cases {
        let mut s = base.clone();
        mutate(&mut s);
        let err = refused(&s, &ctx());
        assert!(matches!(err, PlanError::InvalidSpec(_)), "{what}: {err:?}");
    }
}
