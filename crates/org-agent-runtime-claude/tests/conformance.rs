use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use org_agent_runtime::testkit::{self, Script, ScriptApproval};
use org_agent_runtime::{
    Activity, AgentIdentity, AgentName, AgentRuntime, ApprovalId, ApprovalKind, CommandPrefix,
    Control, ControlError, CostBasis, Decision, EnvName, Escalation, ExitInfo, LimitStatus,
    McpServer, McpServerName, Output, PathPattern, PermissionMode, PermissionPolicy, PlainId,
    PlanError, Probe, ProbeContext, ProbeOutput, ProbeResult, ProbeRun, Rule, RuleScope, RuleTool,
    RuntimeId, SandboxSpec, SessionEvent, SessionId, SessionKind, SessionSpec, SpecPath,
    StartFailure, Stdio, Token, ToolName, TurnOutcome, UsageScope, Window,
};
use org_agent_runtime_claude::ClaudeRuntime;
use org_supervise::{Environment, SearchPath, SessionMark};

fn sp(s: &str) -> SpecPath {
    SpecPath::try_from(s).unwrap()
}

fn pat(s: &str) -> PathPattern {
    PathPattern::try_from(s).unwrap()
}

fn sample_spec() -> SessionSpec {
    let mut mcp_servers = BTreeMap::new();
    mcp_servers.insert(
        McpServerName::try_from("office").unwrap(),
        McpServer {
            command: sp("/office/bin/org-mcp"),
            args: vec!["--stdio".to_owned()],
        },
    );
    SessionSpec {
        runtime: RuntimeId::try_from("claude").unwrap(),
        kind: SessionKind::Task {
            task_id: PlainId::try_from("t1").unwrap(),
        },
        agent: AgentIdentity {
            name: AgentName::try_from("ac-dev-ada").unwrap(),
            description: "Staff engineer".to_owned(),
            prompt: "You are ada.".to_owned(),
        },
        model: Some(Token::try_from("sonnet").unwrap()),
        effort: Some(Token::try_from("high").unwrap()),
        workspace: sp("/work/t-1"),
        read_only_dirs: vec![sp("/homes/ada")],
        instructions: "Follow workspace rules.".to_owned(),
        permission_mode: PermissionMode::Ask,
        policy: PermissionPolicy {
            version: 1,
            allow: vec![Rule {
                tool: RuleTool::try_from("Bash").unwrap(),
                scope: RuleScope::CommandPrefix(CommandPrefix::try_from("git status").unwrap()),
            }],
            deny: vec![Rule {
                tool: RuleTool::try_from("*").unwrap(),
                scope: RuleScope::Path(pat("/office/**")),
            }],
        },
        sandbox: SandboxSpec {
            enabled: true,
            writable_roots: vec![sp("/work/t-1")],
            deny_write: vec![pat("/work/t-1/.git/hooks/**")],
            deny_read: vec![pat("/office/offices/*/**")],
            allowed_unix_sockets: vec![sp("/office/run/ada.sock")],
        },
        mcp_servers,
        tools: Some(vec![
            ToolName::try_from("Agent").unwrap(),
            ToolName::try_from("Bash").unwrap(),
            ToolName::try_from("Read").unwrap(),
            ToolName::try_from("Edit").unwrap(),
        ]),
    }
}

fn sample_ctx() -> org_agent_runtime::LaunchContext {
    org_agent_runtime::LaunchContext {
        runtime_path: PathBuf::from("/opt/homebrew/bin/claude"),
        runtime_version: Some("2.1.311".to_owned()),
        runtime_dir: sp("/office/agents/ada/runtime"),
        tmpdir: sp("/private/tmp/agentic-org/run-1"),
        office_env_names: vec![
            EnvName::try_from("AGENT_OS_SESSION_TOKEN").unwrap(),
            EnvName::try_from("ORG_AGENT").unwrap(),
        ],
        new_session_id: Some(SessionId::try_from("00000000-0000-0000-0000-000000000001").unwrap()),
        resume: None,
        config_dir: Some(sp("/office/agents/ada/home/claude")),
    }
}

fn claude_script() -> Script {
    let init_ok = br#"{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":["ac-dev-ada"],"current_permission_mode":"default","pid":4242,"version":"2.1.311"}}}"#.to_vec();
    let turn_init = br#"{"type":"system","subtype":"init","session_id":"00000000-0000-0000-0000-000000000001","permissionMode":"default"}"#.to_vec();
    let assistant_msg = br#"{"type":"assistant","message":{"id":"msg_1","model":"claude-sonnet-4-6","content":[{"type":"text","text":"Working on it."}],"usage":{"input_tokens":10,"output_tokens":20,"cache_read_input_tokens":100,"cache_creation_input_tokens":50}}}"#.to_vec();
    let approval_req = br#"{"type":"control_request","request_id":"perm-42","request":{"subtype":"can_use_tool","tool_name":"Bash","display_name":"Bash","input":{"command":"echo hi"},"description":"Echo hi","tool_use_id":"toolu_1"}}"#.to_vec();
    let approval_cancel = br#"{"type":"control_cancel_request","request_id":"perm-42"}"#.to_vec();
    let turn_result = br#"{"type":"result","subtype":"success","is_error":false,"result":"Done.","stop_reason":"end_turn","terminal_reason":"completed","total_cost_usd":0.0025,"session_id":"00000000-0000-0000-0000-000000000001"}"#.to_vec();

    let mut approval = ScriptApproval::new(approval_req, ApprovalId::new("perm-42"));
    approval.cancel = Some(approval_cancel);

    Script {
        opening: vec![init_ok],
        turn: vec![turn_init, assistant_msg],
        turn_end: vec![turn_result],
        stderr: vec![b"claude diagnostic notice".to_vec()],
        approval: Some(approval),
    }
}

#[test]
fn passes_org_agent_runtime_testkit_conformance_and_plan_checks() {
    let runtime = ClaudeRuntime::new();
    let spec = sample_spec();
    let ctx = sample_ctx();
    let caps = runtime.capabilities();
    let (plan, _) = runtime.plan_session(&spec, &ctx).expect("plan_session");

    testkit::check_plan(&plan, &spec, &ctx, &caps);

    let script = claude_script();
    testkit::check_all(
        &mut || runtime.plan_session(&spec, &ctx).expect("plan_session").1,
        &script,
        &caps,
        Instant::now(),
    );
}

#[test]
fn plan_keeps_background_tasks_enabled_and_maps_private_config_dir() {
    let runtime = ClaudeRuntime::new();
    let spec = sample_spec();
    let ctx = sample_ctx();
    let (plan, _) = runtime.plan_session(&spec, &ctx).expect("plan_session");

    assert!(
        !plan
            .env_add
            .iter()
            .any(|(k, _)| k.as_str() == "CLAUDE_CODE_DISABLE_BACKGROUND_TASKS"),
        "ADR-0025: background tasks must remain enabled"
    );
    assert!(
        plan.env_add
            .iter()
            .any(|(k, v)| k.as_str() == "CLAUDE_CODE_SUBPROCESS_ENV_SCRUB" && v == "1"),
        "S4: CLAUDE_CODE_SUBPROCESS_ENV_SCRUB=1 must be set"
    );
    assert!(
        plan.env_add.iter().any(|(k, v)| {
            k.as_str() == "CLAUDE_CONFIG_DIR" && v == "/office/agents/ada/home/claude"
        }),
        "expected CLAUDE_CONFIG_DIR in env_add: {:?}",
        plan.env_add
    );
    assert!(plan.args.contains(&"--forward-subagent-text".to_owned()));
    assert!(plan.args.contains(&"--strict-mcp-config".to_owned()));

    // Low 5: `--agents` in headless mode points to a runtime_dir file (prompt not in argv).
    let agents_pos = plan
        .args
        .iter()
        .position(|a| a == "--agents")
        .expect("--agents flag");
    assert_eq!(
        plan.args[agents_pos + 1],
        "/office/agents/ada/runtime/claude-agents.json"
    );
    assert!(plan
        .files
        .iter()
        .any(|f| f.rel_path.as_str() == "claude-agents.json"));

    // B2: enforcement capability flags stay false until live-verified, keeping office fallback on.
    let caps = runtime.capabilities();
    assert!(!caps.enforces_writable_roots);
    assert!(!caps.enforces_deny_write);
    assert!(!caps.enforces_deny_read);
    assert!(!caps.enforces_unix_socket_allowlist);
    assert!(!caps.enforces_deny_rules);
    // L2: the tool allowlist is unproven live, so the office keeps its own check.
    assert!(!caps.enforces_tool_allowlist);

    let settings_file = plan
        .files
        .iter()
        .find(|f| f.rel_path.as_str() == "claude-settings.json")
        .expect("settings file");
    let settings: serde_json::Value =
        serde_json::from_slice(&settings_file.contents).expect("valid settings JSON");
    assert_eq!(settings["autoMemoryEnabled"], serde_json::json!(false));
    // B1: both no-bypass switches pinned.
    assert_eq!(
        settings["permissions"]["disableBypassPermissionsMode"],
        serde_json::json!("disable")
    );
    assert_eq!(settings["sandbox"]["enabled"], serde_json::json!(true));
    assert_eq!(
        settings["sandbox"]["autoAllowBashIfSandboxed"],
        serde_json::json!(false)
    );
    assert_eq!(
        settings["sandbox"]["allowUnsandboxedCommands"],
        serde_json::json!(false)
    );

    // S2: config_dir must NEVER be in allowWrite; it is in denyWrite, and .credentials.json in denyRead.
    let allow_write = settings["sandbox"]["filesystem"]["allowWrite"]
        .as_array()
        .expect("allowWrite array");
    assert!(allow_write
        .iter()
        .any(|v| v == "/private/tmp/agentic-org/run-1"));
    assert!(!allow_write
        .iter()
        .any(|v| v == "/office/agents/ada/home/claude"));
    let deny_write = settings["sandbox"]["filesystem"]["denyWrite"]
        .as_array()
        .expect("denyWrite array");
    assert!(deny_write
        .iter()
        .any(|v| v == "/office/agents/ada/home/claude"));
    let deny_read = settings["sandbox"]["filesystem"]["denyRead"]
        .as_array()
        .expect("denyRead array");
    assert!(deny_read
        .iter()
        .any(|v| v == "/office/agents/ada/home/claude/.credentials.json"));

    // B2 + Low 2: file-tool denies use Claude's `//abs/path` grammar and include NotebookEdit.
    let deny_rules: Vec<&str> = settings["permissions"]["deny"]
        .as_array()
        .expect("deny array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    // L4: a deny for any tool (`*`) expands to Claude's path-rule tools, never `*(...)`.
    assert!(
        !deny_rules.iter().any(|r| r.starts_with('*')),
        "`*(...)` is not a Claude rule form: {deny_rules:?}"
    );
    for tool in ["Read", "Edit", "Write", "MultiEdit", "NotebookEdit"] {
        let rule = format!("{tool}(//office/**)");
        assert!(deny_rules.contains(&rule.as_str()), "missing {rule}");
    }
    assert!(deny_rules.contains(&"Edit(//homes/ada/**)"));
    assert!(deny_rules.contains(&"NotebookEdit(//homes/ada/**)"));
    assert!(deny_rules.contains(&"Read(//office/agents/ada/home/claude/.credentials.json)"));
    assert!(deny_rules.contains(&"Write(//office/agents/ada/home/claude/**)"));

    // Low 1: --disallowedTools is still emitted when spec.tools is None.
    let mut unrestricted_tools_spec = sample_spec();
    unrestricted_tools_spec.tools = None;
    let (plan_none, _) = runtime
        .plan_session(&unrestricted_tools_spec, &ctx)
        .expect("plan_session with tools=None");
    let dis_pos = plan_none
        .args
        .iter()
        .position(|a| a == "--disallowedTools")
        .expect("--disallowedTools must be emitted even when spec.tools is None");
    assert!(plan_none.args[dis_pos + 1].contains("WebFetch"));
    assert!(plan_none.args[dis_pos + 1].contains("CronCreate"));
}

#[test]
fn plan_rejects_root_paths_and_missing_session_id() {
    let runtime = ClaudeRuntime::new();
    let spec = sample_spec();

    let mut bad_ctx = sample_ctx();
    bad_ctx.new_session_id = None;
    bad_ctx.resume = None;
    assert!(matches!(
        runtime.plan_session(&spec, &bad_ctx),
        Err(PlanError::InvalidSpec(_))
    ));

    let mut root_cfg = sample_ctx();
    root_cfg.config_dir = Some(sp("/"));
    assert!(matches!(
        runtime.plan_session(&spec, &root_cfg),
        Err(PlanError::InvalidSpec(_))
    ));

    let mut one_off = sample_spec();
    one_off.kind = SessionKind::OneOff {
        run_id: PlainId::try_from("r1").unwrap(),
    };
    let mut resume_ctx = sample_ctx();
    resume_ctx.resume = Some(SessionId::try_from("00000000-0000-0000-0000-000000000001").unwrap());
    assert!(matches!(
        runtime.plan_take_over(&one_off, &resume_ctx, 120, 40),
        Err(PlanError::InvalidSpec(_))
    ));

    let take_over = runtime
        .plan_take_over(&spec, &resume_ctx, 120, 40)
        .expect("plan_take_over");
    assert_eq!(
        take_over.stdio,
        Stdio::Pty {
            cols: 120,
            rows: 40
        }
    );
    assert!(take_over
        .args
        .contains(&"--resume=00000000-0000-0000-0000-000000000001".to_owned()));
}

#[test]
fn handles_unsolicited_background_wakeup_and_rate_limits() {
    let runtime = ClaudeRuntime::new();
    let spec = sample_spec();
    let ctx = sample_ctx();
    let (_, mut session) = runtime.plan_session(&spec, &ctx).expect("plan_session");
    let now = Instant::now();

    let _ = session.start(now);
    let ready = session.on_line(
        br#"{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":["ac-dev-ada"],"current_permission_mode":"default","pid":123}}}"#,
        now,
    );
    assert!(ready
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::Ready { .. }))));

    // Background task finishes while session is Idle -> emits Notice + unsolicited TurnStarted.
    let notif = session.on_line(
        br#"{"type":"system","subtype":"task_notification","summary":"Background build finished (exit code 0)"}"#,
        now,
    );
    assert!(notif
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::Notice { .. }))));

    let wakeup = session.on_line(
        br#"{"type":"system","subtype":"init","session_id":"00000000-0000-0000-0000-000000000001"}"#,
        now,
    );
    assert!(wakeup.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::TurnStarted {
            solicited: false,
            ..
        })
    )));
    assert_eq!(
        session.control(
            Control::Send {
                text: "hi".into(),
                urgent: false
            },
            now
        ),
        Err(ControlError::Busy)
    );

    let rl = session.on_line(
        br#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","unifiedWindows":{"five_hour":{"utilization":0.42,"resetsAt":"2026-11-19T19:00:00Z"}}}}"#,
        now,
    );
    assert!(rl.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::RateLimit(r))
            if r.window == Window::FiveHour && r.status == LimitStatus::Ok && r.used_fraction == Some(0.42)
    )));

    let end = session.on_line(
        br#"{"type":"result","subtype":"success","is_error":false,"result":"Build done.","total_cost_usd":0.001}"#,
        now,
    );
    assert!(end.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::TurnEnded {
            outcome: TurnOutcome::Completed,
            ..
        })
    )));
    assert!(end
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::Status(Activity::Idle)))));
}

#[test]
fn classifies_sandbox_bypass_and_permission_change_escalations_and_dedups_aborted_usage() {
    let runtime = ClaudeRuntime::new();
    let spec = sample_spec();
    let ctx = sample_ctx();
    let (_, mut session) = runtime.plan_session(&spec, &ctx).expect("plan_session");
    let now = Instant::now();

    let _ = session.start(now);
    let _ = session.on_line(
        br#"{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":["ac-dev-ada"],"current_permission_mode":"default"}}}"#,
        now,
    );
    let _ = session
        .control(
            Control::Send {
                text: "run".into(),
                urgent: false,
            },
            now,
        )
        .unwrap();

    // Two content blocks with the SAME message id should be deduplicated, not double-counted.
    let _ = session.on_line(
        br#"{"type":"assistant","message":{"id":"msg_dup","model":"claude-haiku-4-5","content":[{"type":"thinking"}],"usage":{"input_tokens":100,"output_tokens":50}}}"#,
        now,
    );
    let _ = session.on_line(
        br#"{"type":"assistant","message":{"id":"msg_dup","model":"claude-haiku-4-5","content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"rm -rf /tmp/x","dangerouslyDisableSandbox":true}}],"usage":{"input_tokens":100,"output_tokens":75}}}"#,
        now,
    );

    let bypass = session.on_line(
        br#"{"type":"control_request","request_id":"req-bypass","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls","dangerouslyDisableSandbox":true}}}"#,
        now,
    );
    assert!(bypass.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::ApprovalRequested(req))
            if req.kind == ApprovalKind::Command && req.escalation == Escalation::SandboxBypass
    )));

    let perm_chg = session.on_line(
        br#"{"type":"control_request","request_id":"req-plan","request":{"subtype":"can_use_tool","tool_name":"ExitPlanMode","input":{}}}"#,
        now,
    );
    assert!(perm_chg.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::ApprovalRequested(req))
            if req.kind == ApprovalKind::Permissions && req.escalation == Escalation::PermissionChange
    )));

    // Deny with stop_turn -> aborted result with zero result.usage still reports msg_dup tokens.
    let _ = session
        .control(
            Control::Answer {
                request_id: ApprovalId::new("req-bypass"),
                decision: Decision::Deny {
                    message: Some("no sandbox bypass".into()),
                    stop_turn: true,
                },
            },
            now,
        )
        .unwrap();

    let aborted = session.on_line(
        br#"{"type":"result","subtype":"error_during_execution","is_error":false,"terminal_reason":"aborted_tools","usage":{"input_tokens":0,"output_tokens":0},"total_cost_usd":0.0005}"#,
        now,
    );
    let usage_idx = aborted
        .iter()
        .position(|o| {
            matches!(
                o,
                Output::Event(SessionEvent::Usage(u))
                    if u.input == 100
                        && u.output == 75
                        && u.token_scope == UsageScope::TurnDelta
                        && u.cost_scope == UsageScope::Cumulative
                        && u.cost_basis == CostBasis::ListPriceEstimate
            )
        })
        .expect("expected deduplicated Usage event before TurnEnded");
    let end_idx = aborted
        .iter()
        .position(|o| {
            matches!(
                o,
                Output::Event(SessionEvent::TurnEnded {
                    outcome: TurnOutcome::Interrupted,
                    ..
                })
            )
        })
        .expect("expected Interrupted TurnEnded");
    assert!(usage_idx < end_idx);
}

#[test]
fn detects_agent_mismatch_mode_mismatch_and_initialize_timeout() {
    let runtime = ClaudeRuntime::new();
    let spec = sample_spec();
    let ctx = sample_ctx();
    let now = Instant::now();

    let (_, mut s1) = runtime.plan_session(&spec, &ctx).unwrap();
    let _ = s1.start(now);
    let mismatch_agent = s1.on_line(
        br#"{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":["someone-else"],"current_permission_mode":"default"}}}"#,
        now,
    );
    assert!(mismatch_agent.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::AgentMismatch { .. }
        })
    )));

    let (_, mut s2) = runtime.plan_session(&spec, &ctx).unwrap();
    let _ = s2.start(now);
    let mismatch_mode = s2.on_line(
        br#"{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":["ac-dev-ada"],"current_permission_mode":"acceptEdits"}}}"#,
        now,
    );
    assert!(mismatch_mode.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::ModeMismatch { .. }
        })
    )));

    let (_, mut s3) = runtime.plan_session(&spec, &ctx).unwrap();
    let _ = s3.start(now);
    let deadline = s3.next_deadline().expect("initialize deadline");
    let timed_out = s3.on_tick(deadline + Duration::from_millis(1));
    assert!(timed_out.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::ProtocolTimeout
        })
    )));
    let ended = s3.on_exit(
        ExitInfo {
            code: Some(1),
            signal: None,
        },
        deadline + Duration::from_secs(1),
    );
    assert!(matches!(
        ended.last(),
        Some(Output::Event(SessionEvent::Ended { .. }))
    ));
}

#[test]
fn parses_version_and_auth_probes_and_prepares_via_org_supervise() {
    let runtime = ClaudeRuntime::new();
    let pctx = ProbeContext::new(PathBuf::from("/opt/homebrew/bin/claude"));

    assert!(matches!(
        runtime.plan_probe(Probe::Version, &pctx),
        Some(ProbeRun::OneShot(_))
    ));
    assert!(matches!(
        runtime.plan_probe(Probe::Auth, &pctx),
        Some(ProbeRun::OneShot(_))
    ));
    assert!(runtime.plan_probe(Probe::Quota, &pctx).is_none());

    let ver = runtime.parse_probe(
        Probe::Version,
        &ProbeOutput::new(b"2.1.311 (Claude Code)\n".to_vec(), vec![], Some(0)),
    );
    assert_eq!(
        ver,
        ProbeResult::Version {
            raw: "2.1.311 (Claude Code)".to_owned(),
            in_tested_range: Some(true),
        }
    );

    let auth = runtime.parse_probe(
        Probe::Auth,
        &ProbeOutput::new(
            br#"{"loggedIn":true,"authMethod":"claude.ai","subscriptionType":"max"}"#.to_vec(),
            vec![],
            Some(0),
        ),
    );
    assert_eq!(
        auth,
        ProbeResult::Auth {
            signed_in: true,
            method: Some("claude.ai (max)".to_owned()),
        }
    );

    // End-to-end probe & launch preparation through org-supervise with private CLAUDE_CONFIG_DIR
    // and adapter-declared runtime auth token (ADR-0015 decision 11 + ADR-0032).
    org_platform::confinement::disable_core_dumps().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let real = std::fs::canonicalize(tmp.path()).unwrap();
    let ws = real.join("ws");
    let rt = real.join("rt");
    let cfg = real.join("cfg");
    let helpers = real.join("helpers");
    for d in [&ws, &rt, &cfg, &helpers] {
        std::fs::create_dir_all(d).unwrap();
    }

    let probe_tmp = real.join("probe-tmp");
    std::fs::create_dir_all(&probe_tmp).unwrap();
    org_platform::fs::set_owner_only_dir(&probe_tmp).unwrap();

    let mut probe_ctx = ProbeContext::new(PathBuf::from("/bin/cat"));
    probe_ctx.config_dir = Some(sp(cfg.to_str().unwrap()));
    let probe_plan = match runtime.plan_probe(Probe::Auth, &probe_ctx).unwrap() {
        ProbeRun::OneShot(p) => p,
        _ => panic!("Claude auth probe must be OneShot"),
    };
    let probe_path =
        SearchPath::new(&helpers, OsString::from("/usr/bin:/bin").as_os_str(), &[]).unwrap();
    let probe_env = Environment::new(probe_path, &[]).unwrap();
    let probe_mark =
        SessionMark::new("probe-claude-00000000000000000000000000000001".to_owned()).unwrap();
    org_supervise::runtime::prepare_probe_with_auth(
        probe_plan,
        &probe_ctx,
        probe_env,
        &[(
            OsString::from("CLAUDE_CODE_OAUTH_TOKEN"),
            OsString::from("synthetic-oauth-token"),
        )],
        probe_mark,
        &probe_tmp,
        org_supervise::runtime::ProbeMode::OneShot,
    )
    .expect("org_supervise::runtime::prepare_probe_with_auth must accept ClaudeRuntime auth probe");

    let mut spec = sample_spec();
    spec.workspace = sp(ws.to_str().unwrap());
    spec.sandbox.writable_roots = vec![sp(ws.to_str().unwrap())];
    let ctx = org_agent_runtime::LaunchContext {
        runtime_path: PathBuf::from("/bin/cat"),
        runtime_version: Some("2.1.311".to_owned()),
        runtime_dir: sp(rt.to_str().unwrap()),
        tmpdir: sp(real.to_str().unwrap()),
        office_env_names: vec![EnvName::try_from("AGENT_OS_SESSION_TOKEN").unwrap()],
        new_session_id: Some(SessionId::try_from("00000000-0000-0000-0000-000000000001").unwrap()),
        resume: None,
        config_dir: Some(sp(cfg.to_str().unwrap())),
    };

    let (plan, _) = runtime.plan_session(&spec, &ctx).expect("plan_session");
    let path = SearchPath::new(&helpers, OsString::from("/usr/bin:/bin").as_os_str(), &[]).unwrap();
    let env = Environment::new(path, &[])
        .unwrap()
        .with_context(&[(
            OsString::from("AGENT_OS_SESSION_TOKEN"),
            OsString::from("synthetic-session-token"),
        )])
        .unwrap();
    let mark = SessionMark::new("run-claude-00000000000000000000000000000001".to_owned()).unwrap();
    let launch = org_supervise::runtime::prepare_with_auth(
        plan,
        &ctx,
        env,
        &[(
            OsString::from("CLAUDE_CODE_OAUTH_TOKEN"),
            OsString::from("synthetic-oauth-token"),
        )],
        mark,
    )
    .expect("org_supervise::runtime::prepare_with_auth must accept ClaudeRuntime plan");

    assert!(rt.join("claude-settings.json").is_file());
    assert!(rt.join("claude-mcp.json").is_file());
    assert!(rt.join("claude-agents.json").is_file());
    assert!(rt.join("claude-instructions.md").is_file());
    assert_eq!(
        launch.lifecycle.runtime_session_id,
        Some(SessionId::try_from("00000000-0000-0000-0000-000000000001").unwrap())
    );
}

#[test]
fn enforces_agent_description_remote_control_init_tools_and_unknown_tool_escalation() {
    let runtime = ClaudeRuntime::new();
    let spec = sample_spec();
    let ctx = sample_ctx();
    let now = Instant::now();

    // Plan hardening: --disable-slash-commands, --disallowedTools, ENABLE_TOOL_SEARCH=false.
    let (plan, _) = runtime.plan_session(&spec, &ctx).unwrap();
    assert!(plan.args.iter().any(|a| a == "--disable-slash-commands"));
    let disallowed_pos = plan
        .args
        .iter()
        .position(|a| a == "--disallowedTools")
        .expect("expected --disallowedTools when spec.tools is an allowlist");
    let disallowed_csv = &plan.args[disallowed_pos + 1];
    assert!(disallowed_csv.contains("ToolSearch"));
    assert!(disallowed_csv.contains("WebFetch"));
    assert!(plan
        .env_add
        .iter()
        .any(|(k, v)| k.as_str() == "ENABLE_TOOL_SEARCH" && v == "false"));

    // 1. Agent name matches but description mismatches -> AgentMismatch.
    let (_, mut s_desc) = runtime.plan_session(&spec, &ctx).unwrap();
    let _ = s_desc.start(now);
    let out = s_desc.on_line(
        br#"{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":[{"name":"ac-dev-ada","description":"Spoofed description"}],"current_permission_mode":"default"}}}"#,
        now,
    );
    assert!(out.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::AgentMismatch { .. }
        })
    )));

    // 2. Remote control active -> StartFailed.
    let (_, mut s_rc) = runtime.plan_session(&spec, &ctx).unwrap();
    let _ = s_rc.start(now);
    let out = s_rc.on_line(
        br#"{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":[{"name":"ac-dev-ada","description":"Staff engineer"}],"current_permission_mode":"default","remote_control_auto_enable":true}}}"#,
        now,
    );
    assert!(out.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::Other { .. }
        })
    )));

    // 3. Unknown tool in can_use_tool -> ApprovalKind::Unknown + Escalation::Unknown.
    let (_, mut s_unk) = runtime.plan_session(&spec, &ctx).unwrap();
    let _ = s_unk.start(now);
    let _ = s_unk.on_line(
        br#"{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":[{"name":"ac-dev-ada","description":"Staff engineer"}],"current_permission_mode":"default"}}}"#,
        now,
    );
    let _ = s_unk
        .control(
            Control::Send {
                text: "check".into(),
                urgent: false,
            },
            now,
        )
        .unwrap();
    let unk = s_unk.on_line(
        br#"{"type":"control_request","request_id":"req-unk","request":{"subtype":"can_use_tool","tool_name":"FutureTool2027","input":{"x":1}}}"#,
        now,
    );
    assert!(unk.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::ApprovalRequested(req))
            if req.kind == ApprovalKind::Unknown && req.escalation == Escalation::Unknown
    )));

    // 4. Mid-session system/init exposing a disallowed tool triggers fast-stop (interrupt + CloseStdin)
    // and auto-denies any subsequent can_use_tool.
    let bad_init = s_unk.on_line(
        br#"{"type":"system","subtype":"init","session_id":"00000000-0000-0000-0000-000000000001","tools":["Read","WebFetch"]}"#,
        now,
    );
    assert!(bad_init.iter().any(|o| matches!(o, Output::Write(_))));
    assert!(bad_init.iter().any(|o| matches!(o, Output::CloseStdin)));

    let denied = s_unk.on_line(
        br#"{"type":"control_request","request_id":"req-late","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"id"}}}"#,
        now,
    );
    assert!(denied.iter().any(|o| matches!(o, Output::Write(_))));
    assert!(!denied
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::ApprovalRequested(_)))));

    // 5. B1: system/init reporting banned `bypassPermissions` triggers fast-stop (never ModeChanged).
    let (_, mut s_bypass) = runtime.plan_session(&spec, &ctx).unwrap();
    let _ = s_bypass.start(now);
    let _ = s_bypass.on_line(
        br#"{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":[{"name":"ac-dev-ada","description":"Staff engineer"}],"current_permission_mode":"default"}}}"#,
        now,
    );
    let bypass_out = s_bypass.on_line(
        br#"{"type":"system","subtype":"init","session_id":"00000000-0000-0000-0000-000000000001","permissionMode":"bypassPermissions"}"#,
        now,
    );
    assert!(bypass_out.iter().any(|o| matches!(o, Output::CloseStdin)));
    assert!(!bypass_out
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::ModeChanged { .. }))));

    // 6. S1: non-builtin plugin, unlisted MCP server, and late remote_control on system/init fast-stop.
    for (label, bad_frame) in [
        (
            "non-builtin plugin",
            br#"{"type":"system","subtype":"init","session_id":"00000000-0000-0000-0000-000000000001","plugins":[{"name":"evil-plugin","builtin":false}]}"#.as_slice(),
        ),
        (
            "unlisted mcp server",
            br#"{"type":"system","subtype":"init","session_id":"00000000-0000-0000-0000-000000000001","mcp_servers":[{"name":"office"},{"name":"rogue-mcp"}]}"#.as_slice(),
        ),
        (
            "late remote control",
            br#"{"type":"system","subtype":"init","session_id":"00000000-0000-0000-0000-000000000001","remote_control_active":true}"#.as_slice(),
        ),
    ] {
        let (_, mut s) = runtime.plan_session(&spec, &ctx).unwrap();
        let _ = s.start(now);
        let _ = s.on_line(
            br#"{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":[{"name":"ac-dev-ada","description":"Staff engineer"}],"current_permission_mode":"default"}}}"#,
            now,
        );
        // Valid built-in plugin and granted MCP server pass cleanly.
        let ok_init = s.on_line(
            br#"{"type":"system","subtype":"init","session_id":"00000000-0000-0000-0000-000000000001","plugins":[{"name":"core","builtin":true}],"mcp_servers":[{"name":"office"}]}"#,
            now,
        );
        assert!(
            !ok_init.iter().any(|o| matches!(o, Output::CloseStdin)),
            "{label}: valid init must not fast-stop"
        );
        let bad = s.on_line(bad_frame, now);
        assert!(
            bad.iter().any(|o| matches!(o, Output::CloseStdin)),
            "{label}: expected fast-stop CloseStdin"
        );
        assert!(
            bad.iter().any(|o| matches!(o, Output::Write(_))),
            "{label}: expected fast-stop interrupt Write"
        );
    }
}

#[test]
fn preserves_idle_background_usage_tracks_streaming_msg_ids_and_handles_idle_edge_cases() {
    let runtime = ClaudeRuntime::new();
    let spec = sample_spec();
    let ctx = sample_ctx();
    let now = Instant::now();

    let (_, mut session) = runtime.plan_session(&spec, &ctx).unwrap();
    let _ = session.start(now);
    let _ = session.on_line(
        br#"{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":[{"name":"ac-dev-ada","description":"Staff engineer"}],"current_permission_mode":"default"}}}"#,
        now,
    );

    // Low 3: stray result while Idle must NOT emit TurnEnded.
    let stray = session.on_line(
        br#"{"type":"result","subtype":"success","is_error":false}"#,
        now,
    );
    assert!(!stray
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::TurnEnded { .. }))));

    // Low 3: approval answered while Idle restores Status(Activity::Idle).
    let _ = session.on_line(
        br#"{"type":"control_request","request_id":"req-idle","request":{"subtype":"can_use_tool","tool_name":"Read","input":{"file_path":"/work/t-1/a.rs"}}}"#,
        now,
    );
    let ans_idle = session
        .control(
            Control::Answer {
                request_id: ApprovalId::new("req-idle"),
                decision: Decision::AllowOnce,
            },
            now,
        )
        .unwrap();
    assert!(ans_idle
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::Status(Activity::Idle)))));

    // S3: assistant frame from a background sub-agent arriving while Idle is flushed before
    // the next Control::Send so tokens are never dropped.
    let _ = session.on_line(
        br#"{"type":"assistant","message":{"id":"msg_bg_1","model":"claude-sonnet-4-6","usage":{"input_tokens":33,"output_tokens":17},"content":[{"type":"text","text":"bg step"}]}}"#,
        now,
    );
    let send_out = session
        .control(
            Control::Send {
                text: "next user turn".into(),
                urgent: false,
            },
            now,
        )
        .unwrap();
    assert!(
        send_out.iter().any(|o| matches!(
            o,
            Output::Event(SessionEvent::Usage(u)) if u.input == 33 && u.output == 17
        )),
        "idle background usage must be flushed before reset on Control::Send"
    );

    // S6: two messages whose IDs sort in reverse order (`msg_z` then `msg_a`):
    // `message_delta` for `msg_a` must update `msg_a`, NOT `msg_z`.
    let _ = session.on_line(
        br#"{"type":"stream_event","event":{"type":"message_start","message":{"id":"msg_z","model":"claude-sonnet-4-6","usage":{"input_tokens":100,"output_tokens":10}}}}"#,
        now,
    );
    let _ = session.on_line(
        br#"{"type":"stream_event","event":{"type":"message_delta","usage":{"output_tokens":40}}}"#,
        now,
    );
    let _ = session.on_line(
        br#"{"type":"stream_event","event":{"type":"message_stop"}}"#,
        now,
    );
    let _ = session.on_line(
        br#"{"type":"stream_event","event":{"type":"message_start","message":{"id":"msg_a","model":"claude-sonnet-4-6","usage":{"input_tokens":50,"output_tokens":5}}}}"#,
        now,
    );
    let _ = session.on_line(
        br#"{"type":"stream_event","event":{"type":"message_delta","usage":{"output_tokens":20}}}"#,
        now,
    );
    let _ = session.on_line(
        br#"{"type":"stream_event","event":{"type":"message_stop"}}"#,
        now,
    );
    let end = session.on_line(
        br#"{"type":"result","subtype":"success","is_error":false,"total_cost_usd":0.01}"#,
        now,
    );
    // Total output must be 40 (msg_z) + 20 (msg_a) = 60, NOT 40 + 5 = 45 (if msg_z had been updated twice).
    assert!(
        end.iter().any(|o| matches!(
            o,
            Output::Event(SessionEvent::Usage(u)) if u.input == 150 && u.output == 60
        )),
        "expected msg_z(40) + msg_a(20) = 60 output tokens, got: {end:?}"
    );

    // S7: scripted Send-vs-unsolicited-wake race stays coherent across both turns.
    let send2 = session
        .control(
            Control::Send {
                text: "racing send".into(),
                urgent: false,
            },
            now,
        )
        .unwrap();
    assert!(send2.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::TurnStarted {
            solicited: true,
            ..
        })
    )));
    // Unsolicited init arrives while Busy -> does not emit duplicate TurnStarted.
    let mid_init = session.on_line(
        br#"{"type":"system","subtype":"init","session_id":"00000000-0000-0000-0000-000000000001"}"#,
        now,
    );
    assert!(!mid_init
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::TurnStarted { .. }))));
    // First result closes open turn; subsequent init starts the queued turn as unsolicited.
    let res1 = session.on_line(
        br#"{"type":"result","subtype":"success","is_error":false}"#,
        now,
    );
    assert!(res1
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::TurnEnded { .. }))));
    let next_init = session.on_line(
        br#"{"type":"system","subtype":"init","session_id":"00000000-0000-0000-0000-000000000001"}"#,
        now,
    );
    assert!(next_init.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::TurnStarted {
            solicited: false,
            ..
        })
    )));
}

#[test]
#[ignore = "opt-in live smoke test: requires local `claude` binary on PATH"]
fn live_smoke_initialize_handshake_with_real_claude() {
    use std::io::{BufRead, BufReader, Write};
    use std::sync::mpsc;

    org_platform::confinement::disable_core_dumps().unwrap();

    // Locate `claude` on the current PATH without mutating user state.
    let claude_path = std::env::var_os("PATH")
        .and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|dir| dir.join("claude"))
                .find(|candidate| candidate.is_file())
        })
        .expect("`claude` binary must be on PATH to run live smoke test");
    let claude_real = std::fs::canonicalize(&claude_path).unwrap();
    let claude_dir = claude_real.parent().unwrap().to_path_buf();

    let tmp = tempfile::tempdir().unwrap();
    let real = std::fs::canonicalize(tmp.path()).unwrap();
    let ws = real.join("ws");
    let rt = real.join("rt");
    let cfg = real.join("cfg");
    let helpers = real.join("helpers");
    for d in [&ws, &rt, &cfg, &helpers] {
        std::fs::create_dir_all(d).unwrap();
    }

    let mut spec = sample_spec();
    spec.mcp_servers.clear();
    spec.workspace = sp(ws.to_str().unwrap());
    spec.read_only_dirs.clear();
    spec.sandbox.writable_roots = vec![sp(ws.to_str().unwrap())];
    spec.sandbox.deny_write.clear();
    spec.sandbox.deny_read.clear();
    spec.sandbox.allowed_unix_sockets.clear();
    let ctx = org_agent_runtime::LaunchContext {
        runtime_path: claude_real.clone(),
        runtime_version: None,
        runtime_dir: sp(rt.to_str().unwrap()),
        tmpdir: sp(real.to_str().unwrap()),
        office_env_names: Vec::new(),
        new_session_id: Some(SessionId::try_from("00000000-0000-4000-8000-000000000099").unwrap()),
        resume: None,
        config_dir: Some(sp(cfg.to_str().unwrap())),
    };

    let runtime = ClaudeRuntime::new();
    let (plan, mut session) = runtime.plan_session(&spec, &ctx).unwrap();

    let sys_path = std::env::var_os("PATH").unwrap_or_else(|| OsString::from("/usr/bin:/bin"));
    let search_path = SearchPath::new(&helpers, &sys_path, &[claude_dir]).unwrap();
    let env = Environment::new(search_path, &[]).unwrap();
    let mark = SessionMark::new("live-claude-00000000000000000000000000000099".to_owned()).unwrap();

    let launch = org_supervise::runtime::prepare(plan, &ctx, env, mark).unwrap();
    let mut spawned = launch
        .prepared
        .record_mark(|_| Ok(()))
        .unwrap()
        .spawn()
        .unwrap()
        .record_identity(|_| Ok(()))
        .unwrap();

    let now = Instant::now();
    for out in session.start(now) {
        if let Output::Write(line) = out {
            spawned.stdin.write_all(line.as_bytes()).unwrap();
            spawned.stdin.flush().unwrap();
        }
    }

    // Low 4: bound stdout reading with a reader thread + 20s deadline so a hung binary cannot hang tests.
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let stdout = spawned.stdout;
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if buf.ends_with(b"\n") {
                        buf.pop();
                    }
                    if tx.send(buf.clone()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut saw_ready = false;
    while !saw_ready {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match rx.recv_timeout(remaining) {
            Ok(line) => {
                for out in session.on_line(&line, Instant::now()) {
                    if let Output::Event(SessionEvent::Ready {
                        runtime_version,
                        permission_mode,
                        ..
                    }) = out
                    {
                        assert!(runtime_version.is_some());
                        assert_eq!(permission_mode.as_deref(), Some("default"));
                        saw_ready = true;
                    }
                }
            }
            Err(_) => break,
        }
    }
    assert!(
        saw_ready,
        "expected SessionEvent::Ready from real claude within 20s"
    );

    let close_out = session.control(Control::Close, Instant::now()).unwrap();
    assert!(close_out.iter().any(|o| matches!(o, Output::CloseStdin)));
    spawned.stdin.close().unwrap();
    let status = spawned
        .child
        .wait_until(Instant::now() + Duration::from_secs(10))
        .unwrap()
        .expect("real claude exited cleanly after stdin close");
    let end_out = session.on_exit(
        ExitInfo {
            code: status.code(),
            signal: None,
        },
        Instant::now(),
    );
    assert!(matches!(
        end_out.last(),
        Some(Output::Event(SessionEvent::Ended { .. }))
    ));
}

fn ready_session(spec: &SessionSpec) -> Box<dyn org_agent_runtime::Session> {
    let runtime = ClaudeRuntime::new();
    let (_, mut session) = runtime.plan_session(spec, &sample_ctx()).unwrap();
    let now = Instant::now();
    let _ = session.start(now);
    let ready = session.on_line(
        br#"{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":[{"name":"ac-dev-ada","description":"Staff engineer"}],"current_permission_mode":"default"}}}"#,
        now,
    );
    assert!(ready
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::Ready { .. }))));
    session
}

fn send_turn(session: &mut dyn org_agent_runtime::Session) {
    let out = session
        .control(
            Control::Send {
                text: "work".into(),
                urgent: false,
            },
            Instant::now(),
        )
        .unwrap();
    assert!(out
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::TurnStarted { .. }))));
}

fn turn_ends(out: &[Output]) -> Vec<&TurnOutcome> {
    out.iter()
        .filter_map(|o| match o {
            Output::Event(SessionEvent::TurnEnded { outcome, .. }) => Some(outcome),
            _ => None,
        })
        .collect()
}

const EXIT_OK: ExitInfo = ExitInfo {
    code: Some(0),
    signal: None,
};

#[test]
fn l2_unproven_tool_allowlist_keeps_accept_edits_and_allow_rules_off() {
    let runtime = ClaudeRuntime::new();
    let mut spec = sample_spec();
    // Nothing else drops a deny: only the tool allowlist decides.
    spec.policy.deny.clear();
    spec.sandbox.deny_write.clear();
    spec.sandbox.deny_read.clear();
    spec.permission_mode = PermissionMode::AcceptEdits;
    assert!(spec.tools.is_some());

    let (plan, _) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
    let mode_pos = plan
        .args
        .iter()
        .position(|a| a == "--permission-mode")
        .expect("--permission-mode");
    assert_eq!(
        plan.args[mode_pos + 1],
        "manual",
        "AcceptEdits must narrow to Ask while the tool allowlist is unproven"
    );
    let settings_file = plan
        .files
        .iter()
        .find(|f| f.rel_path.as_str() == "claude-settings.json")
        .unwrap();
    let settings: serde_json::Value = serde_json::from_slice(&settings_file.contents).unwrap();
    assert_eq!(settings["permissions"]["allow"], serde_json::json!([]));
    // The allowlist is still passed to Claude as a narrowing.
    assert!(plan.args.iter().any(|a| a == "--tools"));
}

#[test]
fn l4_any_tool_rules_use_concrete_claude_forms_or_are_refused() {
    let runtime = ClaudeRuntime::new();
    let ctx = sample_ctx();

    let mut prefix = sample_spec();
    prefix.policy.deny = vec![Rule {
        tool: RuleTool::try_from("*").unwrap(),
        scope: RuleScope::CommandPrefix(CommandPrefix::try_from("rm -rf").unwrap()),
    }];
    let (plan, _) = runtime.plan_session(&prefix, &ctx).unwrap();
    let settings_file = plan
        .files
        .iter()
        .find(|f| f.rel_path.as_str() == "claude-settings.json")
        .unwrap();
    let settings: serde_json::Value = serde_json::from_slice(&settings_file.contents).unwrap();
    let deny = settings["permissions"]["deny"].as_array().unwrap();
    assert!(deny.iter().any(|v| v == "Bash(rm -rf*)"), "{deny:?}");
    assert!(!deny
        .iter()
        .any(|v| v.as_str().is_some_and(|r| r.starts_with('*'))));

    // `*` with no scope has no exact Claude form: refused rather than silently dropped.
    let mut every = sample_spec();
    every.policy.deny = vec![Rule {
        tool: RuleTool::try_from("*").unwrap(),
        scope: RuleScope::Any,
    }];
    assert!(matches!(
        runtime.plan_session(&every, &ctx),
        Err(PlanError::Unsupported(_))
    ));
}

#[test]
fn l5_close_during_a_turn_still_ends_the_turn() {
    // Close, then the runtime's result: the turn ends with the runtime's outcome.
    let mut session = ready_session(&sample_spec());
    send_turn(session.as_mut());
    let close = session.control(Control::Close, Instant::now()).unwrap();
    assert!(close.contains(&Output::CloseStdin));
    let result = session.on_line(
        br#"{"type":"result","subtype":"success","is_error":false,"result":"Done."}"#,
        Instant::now(),
    );
    assert_eq!(turn_ends(&result), vec![&TurnOutcome::Completed]);
    let exit = session.on_exit(EXIT_OK, Instant::now());
    assert!(turn_ends(&exit).is_empty(), "TurnEnded must come once");
    assert!(matches!(
        exit.last(),
        Some(Output::Event(SessionEvent::Ended { .. }))
    ));

    // Close, then the process ends with no result: the turn is interrupted, before Ended.
    let mut session = ready_session(&sample_spec());
    send_turn(session.as_mut());
    let _ = session.control(Control::Close, Instant::now()).unwrap();
    let closed = session.on_stdout_closed(Instant::now());
    assert_eq!(turn_ends(&closed), vec![&TurnOutcome::Interrupted]);
    let exit = session.on_exit(EXIT_OK, Instant::now());
    assert!(turn_ends(&exit).is_empty(), "TurnEnded must come once");

    // Close, then exit without stdout closing first.
    let mut session = ready_session(&sample_spec());
    send_turn(session.as_mut());
    let _ = session.control(Control::Close, Instant::now()).unwrap();
    let exit = session.on_exit(EXIT_OK, Instant::now());
    assert_eq!(turn_ends(&exit), vec![&TurnOutcome::Interrupted]);
    let ended_at = exit
        .iter()
        .position(|o| matches!(o, Output::Event(SessionEvent::Ended { .. })))
        .unwrap();
    assert_eq!(ended_at, exit.len() - 1);
}

#[test]
fn l5_fast_stop_during_a_turn_fails_the_turn() {
    let mut session = ready_session(&sample_spec());
    send_turn(session.as_mut());
    let stop = session.on_line(
        br#"{"type":"system","subtype":"init","session_id":"00000000-0000-0000-0000-000000000001","tools":["Read","WebFetch"]}"#,
        Instant::now(),
    );
    assert!(stop.contains(&Output::CloseStdin));
    // Even if Claude reports success, a turn the adapter stopped for policy has failed.
    let result = session.on_line(
        br#"{"type":"result","subtype":"success","is_error":false}"#,
        Instant::now(),
    );
    let ends = turn_ends(&result);
    assert!(
        matches!(ends.as_slice(), [TurnOutcome::Failed { reason }] if reason.contains("disallowed tools")),
        "{result:?}"
    );

    let mut session = ready_session(&sample_spec());
    send_turn(session.as_mut());
    let _ = session.on_line(
        br#"{"type":"system","subtype":"init","session_id":"00000000-0000-0000-0000-000000000001","remote_control_active":true}"#,
        Instant::now(),
    );
    let exit = session.on_exit(EXIT_OK, Instant::now());
    assert!(matches!(
        turn_ends(&exit).as_slice(),
        [TurnOutcome::Failed { .. }]
    ));
}

#[test]
fn l6_probe_refuses_a_root_config_dir() {
    let runtime = ClaudeRuntime::new();
    let mut pctx = ProbeContext::new(PathBuf::from("/opt/homebrew/bin/claude"));
    pctx.config_dir = Some(sp("/"));
    assert!(runtime.plan_probe(Probe::Version, &pctx).is_none());
    assert!(runtime.plan_probe(Probe::Auth, &pctx).is_none());

    pctx.config_dir = Some(sp("/office/agents/ada/home/claude"));
    assert!(runtime.plan_probe(Probe::Auth, &pctx).is_some());
}

#[test]
fn l7_a_pending_approval_survives_a_result_and_is_cancelled_when_the_process_ends() {
    // A background sub-agent's approval can still be pending when the main turn's result
    // arrives (ADR-0025): it stays answerable, and nothing announces a cancellation.
    let mut session = ready_session(&sample_spec());
    send_turn(session.as_mut());
    let _ = session.on_line(
        br#"{"type":"control_request","request_id":"req-bg","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"}}}"#,
        Instant::now(),
    );
    let result = session.on_line(
        br#"{"type":"result","subtype":"success","is_error":false}"#,
        Instant::now(),
    );
    assert!(
        !result
            .iter()
            .any(|o| matches!(o, Output::Event(SessionEvent::ApprovalCancelled { .. }))),
        "Claude did not withdraw the approval: {result:?}"
    );
    assert_eq!(turn_ends(&result), vec![&TurnOutcome::Completed]);
    assert_eq!(
        result.last(),
        Some(&Output::Event(SessionEvent::Status(
            Activity::WaitingOnApproval
        ))),
        "an approval is still pending after the turn"
    );
    let answer = session
        .control(
            Control::Answer {
                request_id: ApprovalId::new("req-bg"),
                decision: Decision::AllowOnce,
            },
            Instant::now(),
        )
        .expect("the approval is still answerable after the result");
    let written = answer
        .iter()
        .find_map(|o| match o {
            Output::Write(line) => Some(String::from_utf8_lossy(line.as_bytes()).into_owned()),
            _ => None,
        })
        .expect("the answer is written to Claude");
    assert!(written.contains("\"control_response\"") && written.contains("req-bg"));
    assert!(answer.contains(&Output::Event(SessionEvent::Status(Activity::Idle))));

    // Stdout closing (the process is going) cancels a pending approval.
    let mut session = ready_session(&sample_spec());
    send_turn(session.as_mut());
    let _ = session.on_line(
        br#"{"type":"control_request","request_id":"req-eof","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"}}}"#,
        Instant::now(),
    );
    let _ = session.on_line(
        br#"{"type":"result","subtype":"success","is_error":false}"#,
        Instant::now(),
    );
    let closed = session.on_stdout_closed(Instant::now());
    assert!(
        closed.contains(&Output::Event(SessionEvent::ApprovalCancelled {
            id: ApprovalId::new("req-eof"),
        }))
    );

    // So does the exit.
    let mut session = ready_session(&sample_spec());
    send_turn(session.as_mut());
    let _ = session.on_line(
        br#"{"type":"control_request","request_id":"req-exit","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"}}}"#,
        Instant::now(),
    );
    let exit = session.on_exit(EXIT_OK, Instant::now());
    let cancelled = exit
        .iter()
        .position(|o| {
            o == &Output::Event(SessionEvent::ApprovalCancelled {
                id: ApprovalId::new("req-exit"),
            })
        })
        .expect("ApprovalCancelled on exit");
    assert!(cancelled < exit.len() - 1, "Ended stays last");
}

#[test]
fn l4_path_rules_for_tools_without_a_claude_path_form_are_not_written() {
    let runtime = ClaudeRuntime::new();
    let ctx = sample_ctx();
    let bash_path = Rule {
        tool: RuleTool::try_from("Bash").unwrap(),
        scope: RuleScope::Path(pat("/office/**")),
    };

    // A deny Claude can't express is refused (fail closed).
    let mut deny = sample_spec();
    deny.policy.deny = vec![bash_path.clone()];
    assert!(matches!(
        runtime.plan_session(&deny, &ctx),
        Err(PlanError::Unsupported(_))
    ));

    // An allow Claude can't express is dropped; a known path tool is kept.
    let mut allow = sample_spec();
    allow.policy.deny.clear();
    allow.sandbox.deny_write.clear();
    allow.sandbox.deny_read.clear();
    allow.tools = None;
    allow.policy.allow = vec![
        bash_path,
        Rule {
            tool: RuleTool::try_from("Edit").unwrap(),
            scope: RuleScope::Path(pat("/work/t-1/**")),
        },
    ];
    let (plan, _) = runtime.plan_session(&allow, &ctx).unwrap();
    let settings_file = plan
        .files
        .iter()
        .find(|f| f.rel_path.as_str() == "claude-settings.json")
        .unwrap();
    let settings: serde_json::Value = serde_json::from_slice(&settings_file.contents).unwrap();
    assert_eq!(
        settings["permissions"]["allow"],
        serde_json::json!(["Edit(//work/t-1/**)"])
    );
}

fn settings_rules(spec: &SessionSpec, list: &str) -> Result<Vec<String>, PlanError> {
    let (plan, _) = ClaudeRuntime::new().plan_session(spec, &sample_ctx())?;
    let settings_file = plan
        .files
        .iter()
        .find(|f| f.rel_path.as_str() == "claude-settings.json")
        .unwrap();
    let settings: serde_json::Value = serde_json::from_slice(&settings_file.contents).unwrap();
    Ok(settings["permissions"][list]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect())
}

/// A spec that keeps allow rules (nothing droppable denies anything).
fn allow_keeping_spec(allow: Vec<Rule>) -> SessionSpec {
    let mut spec = sample_spec();
    spec.policy.deny.clear();
    spec.sandbox.deny_write.clear();
    spec.sandbox.deny_read.clear();
    spec.tools = None;
    spec.policy.allow = allow;
    spec
}

fn rule(tool: &str, scope: RuleScope) -> Rule {
    Rule {
        tool: RuleTool::try_from(tool).unwrap(),
        scope,
    }
}

fn prefix(p: &str) -> RuleScope {
    RuleScope::CommandPrefix(CommandPrefix::try_from(p).unwrap())
}

fn mcp(server: &str) -> RuleScope {
    RuleScope::McpServer(McpServerName::try_from(server).unwrap())
}

#[test]
fn l4_command_prefix_and_mcp_rules_only_for_tools_claude_can_express() {
    // A command prefix is only for Bash (or `*`): refused as a deny, dropped as an allow.
    let mut deny = sample_spec();
    deny.policy.deny = vec![rule("Edit", prefix("rm"))];
    assert!(matches!(
        settings_rules(&deny, "deny"),
        Err(PlanError::Unsupported(_))
    ));
    let allow = allow_keeping_spec(vec![rule("Edit", prefix("git status"))]);
    assert_eq!(
        settings_rules(&allow, "allow").unwrap(),
        Vec::<String>::new()
    );

    // An MCP-server rule is only for `*` or one of that server's tools.
    let mut deny = sample_spec();
    deny.policy.deny = vec![rule("Bash", mcp("office"))];
    assert!(matches!(
        settings_rules(&deny, "deny"),
        Err(PlanError::Unsupported(_))
    ));
    let mut deny = sample_spec();
    deny.policy.deny = vec![rule("mcp__other__send", mcp("office"))];
    assert!(matches!(
        settings_rules(&deny, "deny"),
        Err(PlanError::Unsupported(_))
    ));
    let mut deny = sample_spec();
    deny.policy.deny = vec![
        rule("mcp__office__send", mcp("office")),
        rule("*", mcp("office")),
    ];
    let rules = settings_rules(&deny, "deny").unwrap();
    assert!(rules.contains(&"mcp__office__send".to_owned()), "{rules:?}");
    assert!(rules.contains(&"mcp__office__*".to_owned()), "{rules:?}");
    let allow = allow_keeping_spec(vec![
        rule("Read", mcp("office")),
        rule("mcp__office__read", mcp("office")),
    ]);
    assert_eq!(
        settings_rules(&allow, "allow").unwrap(),
        vec!["mcp__office__read".to_owned()]
    );
}

#[test]
fn l4_prefix_allows_are_word_bounded_and_denies_keep_the_literal_prefix() {
    // `rm` must not allow `rmdir`: Claude 2.1.311 reads `Bash(rm *)` as `^rm( .*)?$`, which
    // matches `rm` and `rm ...` only, so no separate `Bash(rm)` is needed.
    let allow = allow_keeping_spec(vec![
        rule("Bash", prefix("rm")),
        rule("*", prefix("npm run test:")),
    ]);
    let rules = settings_rules(&allow, "allow").unwrap();
    assert_eq!(
        rules,
        vec!["Bash(rm *)".to_owned(), "Bash(npm run test: *)".to_owned()]
    );

    // A prefix ending in whitespace has no exact allow form (`Bash(git *)` would also
    // allow bare `git`; trimming `git  ` or `git\u{3000}` would allow `git status`): dropped.
    for p in ["git ", "git  ", "git\u{3000}"] {
        let allow = allow_keeping_spec(vec![rule("Bash", prefix(p))]);
        assert_eq!(
            settings_rules(&allow, "allow").unwrap(),
            Vec::<String>::new(),
            "allow prefix {p:?} must be dropped"
        );
    }

    // A deny keeps the literal prefix (it also covers `rmdir`; a deny never shrinks).
    let mut deny = sample_spec();
    deny.policy.deny = vec![rule("Bash", prefix("rm")), rule("Bash", prefix("git "))];
    let rules = settings_rules(&deny, "deny").unwrap();
    assert!(rules.contains(&"Bash(rm*)".to_owned()), "{rules:?}");
    assert!(rules.contains(&"Bash(git *)".to_owned()), "{rules:?}");

    // A deny prefix ending in `:` would become Claude's legacy `Bash(x:*)` form, which stops
    // covering `npm run test:unit`: refused.
    let mut deny = sample_spec();
    deny.policy.deny = vec![rule("Bash", prefix("npm run test:"))];
    assert!(matches!(
        settings_rules(&deny, "deny"),
        Err(PlanError::Unsupported(_))
    ));
}

#[test]
fn l4_mcp_tool_names_holding_a_double_underscore_are_ambiguous() {
    // `mcp__a__b__c` for server `a` could be tool `c` of a server `a__b`.
    let mut deny = sample_spec();
    deny.policy.deny = vec![rule("mcp__a__b__c", mcp("a"))];
    assert!(matches!(
        settings_rules(&deny, "deny"),
        Err(PlanError::Unsupported(_))
    ));
    let allow = allow_keeping_spec(vec![
        rule("mcp__a__b__c", mcp("a")),
        rule("mcp__a__b", mcp("a")),
    ]);
    assert_eq!(
        settings_rules(&allow, "allow").unwrap(),
        vec!["mcp__a__b".to_owned()]
    );
}

#[test]
fn l7_an_unsolicited_turn_keeps_waiting_on_approval_while_approvals_are_pending() {
    let mut session = ready_session(&sample_spec());
    send_turn(session.as_mut());
    let _ = session.on_line(
        br#"{"type":"control_request","request_id":"req-bg","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"}}}"#,
        Instant::now(),
    );
    let _ = session.on_line(
        br#"{"type":"result","subtype":"success","is_error":false}"#,
        Instant::now(),
    );
    // A background wake starts a turn by itself while the approval is still pending.
    let wake = session.on_line(
        br#"{"type":"system","subtype":"init","session_id":"00000000-0000-0000-0000-000000000001"}"#,
        Instant::now(),
    );
    assert!(wake.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::TurnStarted {
            solicited: false,
            ..
        })
    )));
    assert!(
        wake.contains(&Output::Event(SessionEvent::Status(
            Activity::WaitingOnApproval
        ))),
        "{wake:?}"
    );
    assert!(!wake.contains(&Output::Event(SessionEvent::Status(Activity::Busy))));
}

#[test]
fn l7_a_send_keeps_waiting_on_approval_while_background_approvals_are_pending() {
    let mut session = ready_session(&sample_spec());
    send_turn(session.as_mut());
    let _ = session.on_line(
        br#"{"type":"control_request","request_id":"req-bg","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"}}}"#,
        Instant::now(),
    );
    let _ = session.on_line(
        br#"{"type":"result","subtype":"success","is_error":false}"#,
        Instant::now(),
    );
    let send = session
        .control(
            Control::Send {
                text: "next".into(),
                urgent: false,
            },
            Instant::now(),
        )
        .unwrap();
    assert!(
        send.contains(&Output::Event(SessionEvent::Status(
            Activity::WaitingOnApproval
        ))),
        "{send:?}"
    );
    assert!(!send.contains(&Output::Event(SessionEvent::Status(Activity::Busy))));

    // Once it's answered, the running turn is Busy again.
    let answer = session
        .control(
            Control::Answer {
                request_id: ApprovalId::new("req-bg"),
                decision: Decision::AllowOnce,
            },
            Instant::now(),
        )
        .unwrap();
    assert!(answer.contains(&Output::Event(SessionEvent::Status(Activity::Busy))));
}

#[test]
fn capability_translation_covers_presets_rules_init_and_approvals() {
    use org_agent_runtime::ToolCapability;

    let runtime = ClaudeRuntime::new();

    // ReadOnly preset with cap:read, cap:search, cap:shell + CommandPrefix("agent-os").
    let mut ro_spec = sample_spec();
    ro_spec.permission_mode = PermissionMode::Plan;
    ro_spec.policy.deny.clear();
    ro_spec.sandbox.deny_write.clear();
    ro_spec.sandbox.deny_read.clear();
    ro_spec.policy.allow = vec![
        Rule {
            tool: ToolCapability::Read.to_rule_tool(),
            scope: RuleScope::Any,
        },
        Rule {
            tool: ToolCapability::Search.to_rule_tool(),
            scope: RuleScope::Any,
        },
        Rule {
            tool: ToolCapability::Shell.to_rule_tool(),
            scope: RuleScope::CommandPrefix(CommandPrefix::try_from("agent-os").unwrap()),
        },
    ];
    ro_spec.tools = Some(vec![
        ToolCapability::Read.to_tool_name(),
        ToolCapability::Search.to_tool_name(),
        ToolCapability::Shell.to_tool_name(),
    ]);

    let (plan, mut session) = runtime.plan_session(&ro_spec, &sample_ctx()).unwrap();
    let tools_pos = plan.args.iter().position(|a| a == "--tools").unwrap();
    assert_eq!(plan.args[tools_pos + 1], "Read,Glob,Grep,Bash");
    let dis_pos = plan
        .args
        .iter()
        .position(|a| a == "--disallowedTools")
        .unwrap();
    assert!(plan.args[dis_pos + 1].contains("NotebookEdit"));

    // Agents file carries expanded concrete tools, never raw cap:* tokens.
    let agents_file = plan
        .files
        .iter()
        .find(|f| f.rel_path.as_str() == "claude-agents.json")
        .unwrap();
    let agents_json: serde_json::Value = serde_json::from_slice(&agents_file.contents).unwrap();
    assert_eq!(
        agents_json["ac-dev-ada"]["tools"],
        serde_json::json!(["Read", "Glob", "Grep", "Bash"])
    );

    // Settings file expands cap:read/search/shell allow rules when keeps_allow_rules is true.
    let mut allow_check_spec = ro_spec.clone();
    allow_check_spec.tools = None;
    let (allow_plan, _) = runtime
        .plan_session(&allow_check_spec, &sample_ctx())
        .unwrap();
    let settings_file = allow_plan
        .files
        .iter()
        .find(|f| f.rel_path.as_str() == "claude-settings.json")
        .unwrap();
    let settings: serde_json::Value = serde_json::from_slice(&settings_file.contents).unwrap();
    let allow_rules: Vec<&str> = settings["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        allow_rules,
        vec!["Read", "Glob", "Grep", "Bash(agent-os *)"]
    );

    // WorkspaceEdits `cap:edit` expands to Edit/Write/MultiEdit (keeping NotebookEdit disallowed),
    // and its session accepts Edit/Write/Bash on init and classifies Edit approvals as Edit.
    let mut we_spec = allow_check_spec.clone();
    we_spec.permission_mode = PermissionMode::AcceptEdits;
    we_spec.policy.allow.push(Rule {
        tool: ToolCapability::Edit.to_rule_tool(),
        scope: RuleScope::Any,
    });
    we_spec.tools = Some(vec![ToolCapability::Edit.to_tool_name()]);
    let (we_plan, mut we_session) = runtime.plan_session(&we_spec, &sample_ctx()).unwrap();
    let we_tools_pos = we_plan.args.iter().position(|a| a == "--tools").unwrap();
    assert_eq!(we_plan.args[we_tools_pos + 1], "Edit,Write,MultiEdit");
    let we_dis_pos = we_plan
        .args
        .iter()
        .position(|a| a == "--disallowedTools")
        .unwrap();
    assert!(we_plan.args[we_dis_pos + 1].contains("NotebookEdit"));

    let now_matrix = Instant::now();
    let _ = we_session.start(now_matrix);
    let we_ready = we_session.on_line(
        br#"{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":["ac-dev-ada"],"current_permission_mode":"default","pid":43}}}"#,
        now_matrix,
    );
    assert!(we_ready
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::Ready { .. }))));
    let we_init = we_session.on_line(
        br#"{"type":"system","subtype":"init","session_id":"00000000-0000-0000-0000-000000000001","permissionMode":"default","tools":["Edit","Write","MultiEdit"]}"#,
        now_matrix,
    );
    assert!(!we_init.iter().any(|o| matches!(o, Output::CloseStdin)));
    let we_apr = we_session.on_line(
        br#"{"type":"control_request","request_id":"we-1","request":{"subtype":"can_use_tool","tool_name":"Edit","input":{"file_path":"/work/t-1/a.rs"}}}"#,
        now_matrix,
    );
    assert!(we_apr.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::ApprovalRequested(r))
            if r.capability == Some(ToolCapability::Edit) && r.escalation == Escalation::None
    )));

    // AutonomousSandboxed `cap:shell` Any expands to bare `Bash` alongside Read/Search/Edit,
    // and its session initializes and classifies Bash + Edit approvals.
    let mut auto_spec = allow_check_spec.clone();
    auto_spec.permission_mode = PermissionMode::AcceptEdits;
    auto_spec.policy.allow = vec![
        Rule {
            tool: ToolCapability::Read.to_rule_tool(),
            scope: RuleScope::Any,
        },
        Rule {
            tool: ToolCapability::Search.to_rule_tool(),
            scope: RuleScope::Any,
        },
        Rule {
            tool: ToolCapability::Edit.to_rule_tool(),
            scope: RuleScope::Any,
        },
        Rule {
            tool: ToolCapability::Shell.to_rule_tool(),
            scope: RuleScope::Any,
        },
    ];
    let (auto_plan, mut auto_session) = runtime.plan_session(&auto_spec, &sample_ctx()).unwrap();
    let auto_settings_file = auto_plan
        .files
        .iter()
        .find(|f| f.rel_path.as_str() == "claude-settings.json")
        .unwrap();
    let auto_settings: serde_json::Value =
        serde_json::from_slice(&auto_settings_file.contents).unwrap();
    let auto_allow: Vec<&str> = auto_settings["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        auto_allow,
        vec!["Read", "Glob", "Grep", "Edit", "Write", "MultiEdit", "Bash"]
    );
    let _ = auto_session.start(now_matrix);
    let auto_ready = auto_session.on_line(
        br#"{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":["ac-dev-ada"],"current_permission_mode":"acceptEdits","pid":44}}}"#,
        now_matrix,
    );
    assert!(auto_ready
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::Ready { .. }))));
    let auto_init = auto_session.on_line(
        br#"{"type":"system","subtype":"init","session_id":"00000000-0000-0000-0000-000000000001","permissionMode":"acceptEdits","tools":["Read","Glob","Grep","Edit","Write","Bash"]}"#,
        now_matrix,
    );
    assert!(!auto_init.iter().any(|o| matches!(o, Output::CloseStdin)));
    let auto_apr = auto_session.on_line(
        br#"{"type":"control_request","request_id":"auto-1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"cargo test"}}}"#,
        now_matrix,
    );
    assert!(auto_apr.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::ApprovalRequested(r))
            if r.capability == Some(ToolCapability::Shell) && r.escalation == Escalation::None
    )));

    // ReadOnly system/init accepts Read/Glob/Grep/Bash and rejects Edit.
    let now = Instant::now();
    let _ = session.start(now);
    let _ = session.on_line(
        br#"{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":["ac-dev-ada"],"current_permission_mode":"plan","pid":42}}}"#,
        now,
    );
    let ok_init = session.on_line(
        br#"{"type":"system","subtype":"init","session_id":"00000000-0000-0000-0000-000000000001","permissionMode":"plan","tools":["Read","Glob","Grep","Bash"]}"#,
        now,
    );
    assert!(!ok_init.iter().any(|o| matches!(o, Output::CloseStdin)));

    // Approvals and ToolStarted events carry normalized ToolCapability.
    let bash_apr = session.on_line(
        br#"{"type":"control_request","request_id":"req-1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"agent-os inbox"}}}"#,
        now,
    );
    let req = bash_apr
        .iter()
        .find_map(|o| match o {
            Output::Event(SessionEvent::ApprovalRequested(r)) => Some(r),
            _ => None,
        })
        .unwrap();
    assert_eq!(req.capability, Some(ToolCapability::Shell));

    let sub_ev = session.on_line(
        br#"{"type":"assistant","message":{"id":"m-sub","content":[{"type":"tool_use","id":"tu-sub","name":"Agent","input":{"prompt":"inspect"}}]}}"#,
        now,
    );
    assert!(sub_ev.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::ToolStarted {
            tool_use_id,
            capability: Some(ToolCapability::Subagent),
            ..
        }) if tool_use_id == "tu-sub"
    )));

    // Exposing Edit in ReadOnly fast-stops.
    let bad_init = session.on_line(
        br#"{"type":"system","subtype":"init","session_id":"00000000-0000-0000-0000-000000000001","permissionMode":"plan","tools":["Read","Edit"]}"#,
        now,
    );
    assert!(bad_init.iter().any(|o| matches!(o, Output::CloseStdin)));

    // Unknown cap:* token fails closed in tools and deny rules.
    let mut bad_tools = sample_spec();
    bad_tools.tools = Some(vec![ToolName::try_from("cap:network").unwrap()]);
    assert!(matches!(
        runtime.plan_session(&bad_tools, &sample_ctx()),
        Err(PlanError::Unsupported(_))
    ));
    let mut bad_deny = sample_spec();
    bad_deny.policy.deny.push(Rule {
        tool: RuleTool::try_from("cap:network").unwrap(),
        scope: RuleScope::Any,
    });
    assert!(matches!(
        runtime.plan_session(&bad_deny, &sample_ctx()),
        Err(PlanError::Unsupported(_))
    ));
}
