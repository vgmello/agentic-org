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
    assert!(deny_rules.contains(&"*(//office/**)"));
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
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&probe_tmp, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

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
