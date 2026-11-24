use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use org_agent_runtime::testkit::{self, Script, ScriptApproval};
use org_agent_runtime::{
    AgentIdentity, AgentName, AgentRuntime, ApprovalId, ApprovalKind, CommandPrefix, Control,
    ControlError, Decision, EnvName, Escalation, ExitInfo, LaunchContext, LimitKind, LimitStatus,
    McpServer, McpServerName, Output, PathPattern, PermissionMode, PermissionPolicy, PlainId,
    PlanError, Probe, ProbeContext, ProbeOutput, ProbeResult, Rule, RuleScope, RuleTool, RuntimeId,
    SandboxSpec, SessionEvent, SessionId, SessionKind, SessionSpec, SpecPath, StartFailure, Token,
    ToolName, TurnOutcome,
};
use org_agent_runtime_agy::AgyRuntime;

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
        runtime: RuntimeId::try_from("agy").unwrap(),
        kind: SessionKind::Task {
            task_id: PlainId::try_from("t1").unwrap(),
        },
        agent: AgentIdentity {
            name: AgentName::try_from("ac-dev-ada").unwrap(),
            description: "Staff engineer".to_owned(),
            prompt: "You are ada.".to_owned(),
        },
        model: Some(Token::try_from("gemini-2.5-pro").unwrap()),
        effort: Some(Token::try_from("high").unwrap()),
        workspace: sp("/work/t-1"),
        read_only_dirs: vec![sp("/homes/ada")],
        instructions: "Follow workspace rules.".to_owned(),
        permission_mode: PermissionMode::Ask,
        policy: PermissionPolicy {
            version: 1,
            allow: vec![Rule {
                tool: RuleTool::try_from("run_command").unwrap(),
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
            ToolName::try_from("read_file").unwrap(),
            ToolName::try_from("edit_file").unwrap(),
            ToolName::try_from("run_command").unwrap(),
        ]),
    }
}

fn sample_ctx() -> LaunchContext {
    LaunchContext {
        runtime_path: PathBuf::from("/usr/local/bin/agy"),
        runtime_version: Some("1.5.14".to_owned()),
        runtime_dir: sp("/office/agents/ada/runtime"),
        tmpdir: sp("/private/tmp/agentic-org/run-1"),
        office_env_names: vec![EnvName::try_from("AGENT_OS_SESSION_TOKEN").unwrap()],
        new_session_id: Some(SessionId::try_from("agy-conv-1").unwrap()),
        resume: None,
        config_dir: Some(sp("/office/agents/ada/home/agy")),
    }
}

fn agy_script() -> Script {
    let init_ok = br#"{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":["ac-dev-ada"],"mode":"ask","pid":4242,"version":"1.5.14"}}}"#.to_vec();
    let turn_init =
        br#"{"type":"system","subtype":"init","conversation_id":"agy-conv-1","mode":"ask"}"#
            .to_vec();
    let assistant = br#"{"type":"assistant","message":{"id":"m1","model":"gemini-2.5-pro","content":[{"type":"text","text":"Working."}],"usage":{"input_tokens":12,"output_tokens":8}}}"#.to_vec();
    let approval_req = br#"{"type":"control_request","request_id":"perm-1","request":{"subtype":"can_use_tool","tool_name":"run_command","input":{"CommandLine":"echo hi"}}}"#.to_vec();
    let approval_cancel = br#"{"type":"control_cancel_request","request_id":"perm-1"}"#.to_vec();
    let result =
        br#"{"type":"result","subtype":"success","is_error":false,"total_cost_usd":0.001}"#
            .to_vec();
    let mut approval = ScriptApproval::new(approval_req, ApprovalId::new("perm-1"));
    approval.cancel = Some(approval_cancel);
    Script {
        opening: vec![init_ok],
        turn: vec![turn_init, assistant],
        turn_end: vec![result],
        stderr: vec![b"agy diagnostic".to_vec()],
        approval: Some(approval),
    }
}

#[test]
fn passes_org_agent_runtime_testkit_conformance_and_plan_checks() {
    let runtime = AgyRuntime::new();
    let spec = sample_spec();
    let ctx = sample_ctx();
    let caps = runtime.capabilities();
    let (plan, _) = runtime.plan_session(&spec, &ctx).expect("plan_session");

    testkit::check_plan(&plan, &spec, &ctx, &caps);
    let script = agy_script();
    testkit::check_all(
        &mut || runtime.plan_session(&spec, &ctx).expect("plan_session").1,
        &script,
        &caps,
        Instant::now(),
    );
}

#[test]
fn plan_enforces_sandbox_config_dir_exclusion_effective_mode_and_take_over_guards() {
    let runtime = AgyRuntime::new();
    let mut spec = sample_spec();
    spec.permission_mode = PermissionMode::AcceptEdits;
    // Deliberately include config_dir in writable_roots; adapter MUST strip it.
    spec.sandbox
        .writable_roots
        .push(sp("/office/agents/ada/home/agy"));
    let ctx = sample_ctx();
    let (plan, _) = runtime.plan_session(&spec, &ctx).unwrap();
    assert!(plan.args.contains(&"--sandbox".to_owned()));
    assert!(plan.args.contains(&"--disable-slash-commands".to_owned()));
    assert!(!plan.args.iter().any(|a| a.contains("dangerously")));
    assert!(!plan.args.iter().any(|a| a.contains("remote-control")));

    let settings_file = plan
        .files
        .iter()
        .find(|f| f.rel_path.as_str() == "agy-settings.json")
        .unwrap();
    let settings: serde_json::Value = serde_json::from_slice(&settings_file.contents).unwrap();
    assert_eq!(settings["disableRemoteControl"], serde_json::json!(true));
    assert_eq!(
        settings["permissions"]["disableBypassMode"],
        serde_json::json!(true)
    );
    // Because enforces_deny_rules=false and denies are present, AcceptEdits narrows to Ask
    // and allow rules are dropped (`keeps_allow_rules == false`).
    assert_eq!(settings["permissions"]["mode"], serde_json::json!("ask"));
    assert_eq!(settings["permissions"]["allow"], serde_json::json!([]));

    let allow_write = settings["sandbox"]["filesystem"]["allowWrite"]
        .as_array()
        .unwrap();
    assert!(allow_write
        .iter()
        .any(|v| v == "/private/tmp/agentic-org/run-1"));
    assert!(!allow_write
        .iter()
        .any(|v| v == "/office/agents/ada/home/agy"));

    let mut bad_ctx = sample_ctx();
    bad_ctx.tmpdir = sp("/");
    assert!(matches!(
        runtime.plan_session(&spec, &bad_ctx),
        Err(PlanError::InvalidSpec(_))
    ));

    // Take-over refuses missing resume and OneOff sessions.
    assert!(matches!(
        runtime.plan_take_over(&spec, &ctx, 80, 24),
        Err(PlanError::InvalidSpec(_))
    ));
    let mut resume_ctx = sample_ctx();
    resume_ctx.resume = Some(SessionId::try_from("agy-conv-9").unwrap());
    let mut one_off = sample_spec();
    one_off.kind = SessionKind::OneOff {
        run_id: PlainId::try_from("run1").unwrap(),
    };
    assert!(matches!(
        runtime.plan_take_over(&one_off, &resume_ctx, 80, 24),
        Err(PlanError::InvalidSpec(_))
    ));
    let take_over = runtime
        .plan_take_over(&spec, &resume_ctx, 120, 40)
        .expect("take-over plan");
    assert!(take_over.args.contains(&"--conversation".to_owned()));
    assert!(take_over.args.contains(&"agy-conv-9".to_owned()));
}

#[test]
fn handshake_enforces_strict_mode_agents_remote_control_and_resume_failure() {
    let runtime = AgyRuntime::new();
    let spec = sample_spec();
    let now = Instant::now();

    // Missing mode must fail closed with ModeMismatch.
    let (_, mut s1) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
    let _ = s1.start(now);
    let out = s1.on_line(
        br#"{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":["ac-dev-ada"]}}}"#,
        now,
    );
    assert!(out.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::ModeMismatch { .. }
        })
    )));

    // Looser non-banned mode (accept-edits when ask requested) must fail closed.
    let (_, mut s2) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
    let _ = s2.start(now);
    let out = s2.on_line(
        br#"{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":["ac-dev-ada"],"mode":"accept-edits"}}}"#,
        now,
    );
    assert!(out.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::ModeMismatch { .. }
        })
    )));

    // Empty agents list must fail closed with AgentMismatch.
    let (_, mut s3) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
    let _ = s3.start(now);
    let out = s3.on_line(
        br#"{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":[],"mode":"ask"}}}"#,
        now,
    );
    assert!(out.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::AgentMismatch { .. }
        })
    )));

    // Resume initialize error must emit StartFailure::ResumeFailed.
    let mut resume_ctx = sample_ctx();
    resume_ctx.resume = Some(SessionId::try_from("agy-missing").unwrap());
    let (_, mut s4) = runtime.plan_session(&spec, &resume_ctx).unwrap();
    let _ = s4.start(now);
    let out = s4.on_line(
        br#"{"type":"control_response","response":{"subtype":"error","request_id":"host-1","error":"conversation not found"}}"#,
        now,
    );
    assert!(out.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::ResumeFailed { .. }
        })
    )));
}

#[test]
fn approvals_classify_unknown_escalations_blocked_path_and_closing_hygiene() {
    let runtime = AgyRuntime::new();
    let spec = sample_spec();
    let now = Instant::now();
    let (_, mut session) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
    let _ = session.start(now);
    let _ = session.on_line(
        br#"{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":["ac-dev-ada"],"mode":"ask"}}}"#,
        now,
    );

    // Unknown tool must be ApprovalKind::Unknown + Escalation::Unknown.
    let out = session.on_line(
        br#"{"type":"control_request","request_id":"unk-1","request":{"subtype":"can_use_tool","tool_name":"mystery_tool","input":{"path":"/etc/passwd"}}}"#,
        now,
    );
    let req = out
        .iter()
        .find_map(|o| match o {
            Output::Event(SessionEvent::ApprovalRequested(r)) => Some(r),
            _ => None,
        })
        .expect("approval requested");
    assert_eq!(req.kind, ApprovalKind::Unknown);
    assert_eq!(req.escalation, Escalation::Unknown);
    assert_eq!(req.blocked_path.as_deref(), Some("/etc/passwd"));

    // Unsandboxed command must be Escalation::SandboxBypass.
    let out = session.on_line(
        br#"{"type":"control_request","request_id":"sb-1","request":{"subtype":"can_use_tool","tool_name":"run_command","input":{"dangerouslyDisableSandbox":true}}}"#,
        now,
    );
    let req = out
        .iter()
        .find_map(|o| match o {
            Output::Event(SessionEvent::ApprovalRequested(r)) => Some(r),
            _ => None,
        })
        .unwrap();
    assert_eq!(req.escalation, Escalation::SandboxBypass);

    // EnterPlanMode must be ApprovalKind::Permissions + Escalation::PermissionChange.
    let out = session.on_line(
        br#"{"type":"control_request","request_id":"pm-1","request":{"subtype":"can_use_tool","tool_name":"EnterPlanMode","input":{}}}"#,
        now,
    );
    let req = out
        .iter()
        .find_map(|o| match o {
            Output::Event(SessionEvent::ApprovalRequested(r)) => Some(r),
            _ => None,
        })
        .unwrap();
    assert_eq!(req.kind, ApprovalKind::Permissions);
    assert_eq!(req.escalation, Escalation::PermissionChange);

    // After Close, incoming approvals are denied immediately without raising ApprovalRequested.
    let _ = session.control(Control::Close, now).unwrap();
    let out = session.on_line(
        br#"{"type":"control_request","request_id":"late-1","request":{"subtype":"can_use_tool","tool_name":"run_command","input":{}}}"#,
        now,
    );
    assert!(out.iter().any(|o| matches!(o, Output::Write(_))));
    assert!(!out
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::ApprovalRequested(_)))));
    assert_eq!(
        session.control(
            Control::Answer {
                request_id: ApprovalId::new("late-1"),
                decision: Decision::AllowOnce,
            },
            now
        ),
        Err(ControlError::Closed)
    );
}

#[test]
fn truncated_invalid_session_id_rate_limits_result_outcomes_stderr_and_crash_exit() {
    let runtime = AgyRuntime::new();
    let spec = sample_spec();
    let now = Instant::now();
    let (_, mut session) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
    let _ = session.start(now);
    // Double start is a no-op.
    assert!(session.start(now).is_empty());
    let _ = session.on_line(
        br#"{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":["ac-dev-ada"],"mode":"ask"}}}"#,
        now,
    );

    // Blank stdout line is ignored.
    assert!(session.on_line(b"   \n", now).is_empty());

    // on_truncated emits Truncated { len }.
    assert!(matches!(
        session.on_truncated(999, now).as_slice(),
        [Output::Event(SessionEvent::Truncated { len: 999 })]
    ));

    // Invalid session_id emits ProtocolError.
    let bad_sid = session.on_line(
        br#"{"type":"system","subtype":"init","conversation_id":"bad id with spaces","mode":"ask"}"#,
        now,
    );
    assert!(bad_sid
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::ProtocolError { .. }))));

    // Rate limit event parses utilization & resetsAt.
    let rl = session.on_line(
        br#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed_warning","rateLimitType":"5h","utilization":0.85,"resetsAt":"2026-11-22T12:00:00Z"}}"#,
        now,
    );
    assert!(rl.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::RateLimit(r))
            if r.status == LimitStatus::Warning && r.resets_at.as_deref() == Some("2026-11-22T12:00:00Z")
    )));

    // Structured AGY_ERROR stderr parses cleanly.
    let err_out = session.on_stderr_line(
        br#"AGY_ERROR: {"code":"retry","message":"transient 503"}"#,
        now,
    );
    assert!(err_out.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::Notice { text, .. }) if text.contains("transient 503")
    )));

    // Permission denials + LimitKind::MaxTurns on result.
    let res = session.on_line(
        br#"{"type":"result","subtype":"error_max_turns","is_error":true,"errors":["hit cap"],"permission_denials":[{"tool_name":"run_command","tool_use_id":"t1","message":"blocked"}]}"#,
        now,
    );
    assert!(res
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::ActionDenied { tool, .. }) if tool == "run_command")));
    assert!(res.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::TurnEnded {
            outcome: TurnOutcome::Limit {
                kind: LimitKind::MaxTurns
            },
            errors
        }) if errors == &["hit cap".to_owned()]
    )));

    // Mid-turn crash emits TurnOutcome::Failed, not Interrupted.
    let _ = session
        .control(
            Control::Send {
                text: "go".into(),
                urgent: false,
            },
            now,
        )
        .unwrap();
    let exit_out = session.on_exit(
        ExitInfo {
            code: Some(1),
            signal: None,
        },
        now,
    );
    assert!(exit_out.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::TurnEnded {
            outcome: TurnOutcome::Failed { .. },
            ..
        })
    )));

    // Auth probe rejects exit-0 error output and root runtime_dir.
    let mut pctx = ProbeContext::new(PathBuf::from("/usr/local/bin/agy"));
    pctx.runtime_dir = Some(sp("/"));
    assert!(runtime.plan_probe(Probe::Auth, &pctx).is_none());
    let bad_auth = ProbeOutput::new(
        b"Error: authentication required\n".to_vec(),
        vec![],
        Some(0),
    );
    assert_eq!(
        runtime.parse_probe(Probe::Auth, &bad_auth),
        ProbeResult::Auth {
            signed_in: false,
            method: None,
        }
    );
    let ok_auth = ProbeOutput::new(b"gemini-2.5-pro\n".to_vec(), vec![], Some(0));
    assert_eq!(
        runtime.parse_probe(Probe::Auth, &ok_auth),
        ProbeResult::Auth {
            signed_in: true,
            method: Some("agy-cli".to_owned()),
        }
    );
}

#[test]
fn resume_usage_baseline_handles_baselined_multiturn_and_unbaselined_first_turn() {
    let runtime = AgyRuntime::new();
    let spec = sample_spec();
    let now = Instant::now();

    // Case 1: Resumed WITH initial baseline in initialize -> turn 1 and turn 2 emit exact deltas.
    let mut resume_ctx = sample_ctx();
    resume_ctx.resume = Some(SessionId::try_from("agy-res-1").unwrap());
    let (_, mut s1) = runtime.plan_session(&spec, &resume_ctx).unwrap();
    let _ = s1.start(now);
    let _ = s1.on_line(
        br#"{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":["ac-dev-ada"],"mode":"ask","usage":{"input_tokens":100,"output_tokens":50}}}}"#,
        now,
    );
    let _ = s1
        .control(
            Control::Send {
                text: "t1".into(),
                urgent: false,
            },
            now,
        )
        .unwrap();
    let out1 = s1.on_line(
        br#"{"type":"result","subtype":"success","usage":{"input_tokens":130,"output_tokens":70}}"#,
        now,
    );
    assert!(out1.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::Usage(u)) if u.input == 30 && u.output == 20
    )));
    let _ = s1
        .control(
            Control::Send {
                text: "t2".into(),
                urgent: false,
            },
            now,
        )
        .unwrap();
    let out2 = s1.on_line(
        br#"{"type":"result","subtype":"success","usage":{"input_tokens":145,"output_tokens":80}}"#,
        now,
    );
    assert!(out2.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::Usage(u)) if u.input == 15 && u.output == 10
    )));

    // Case 2: Resumed WITHOUT initial baseline -> turn 1 captures baseline + emits Notice (no Usage), turn 2 emits delta.
    let (_, mut s2) = runtime.plan_session(&spec, &resume_ctx).unwrap();
    let _ = s2.start(now);
    let _ = s2.on_line(
        br#"{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":["ac-dev-ada"],"mode":"ask"}}}"#,
        now,
    );
    let _ = s2
        .control(
            Control::Send {
                text: "t1".into(),
                urgent: false,
            },
            now,
        )
        .unwrap();
    let out_no_base = s2.on_line(
        br#"{"type":"result","subtype":"success","usage":{"input_tokens":200,"output_tokens":90}}"#,
        now,
    );
    assert!(!out_no_base
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::Usage(_)))));
    assert!(out_no_base
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::Notice { .. }))));
    let _ = s2
        .control(
            Control::Send {
                text: "t2".into(),
                urgent: false,
            },
            now,
        )
        .unwrap();
    let out_after_base = s2.on_line(
        br#"{"type":"result","subtype":"success","usage":{"input_tokens":225,"output_tokens":105}}"#,
        now,
    );
    assert!(out_after_base.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::Usage(u)) if u.input == 25 && u.output == 15
    )));
}

#[test]
fn capability_translation_allows_readonly_init_and_classifies_approvals_and_subagents() {
    use org_agent_runtime::ToolCapability;

    let runtime = AgyRuntime::new();
    let mut spec = sample_spec();
    spec.permission_mode = PermissionMode::Plan;
    spec.policy.allow = vec![
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
    spec.policy.deny.clear();
    spec.sandbox.deny_write.clear();
    spec.sandbox.deny_read.clear();
    spec.tools = Some(vec![
        ToolCapability::Read.to_tool_name(),
        ToolCapability::Search.to_tool_name(),
        ToolCapability::Shell.to_tool_name(),
    ]);

    let (plan, mut session) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
    let settings_file = plan
        .files
        .iter()
        .find(|f| f.rel_path.as_str() == "agy-settings.json")
        .unwrap();
    let settings: serde_json::Value = serde_json::from_slice(&settings_file.contents).unwrap();
    let allowed_tools: Vec<&str> = settings["allowedTools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    for expected in [
        "read_file",
        "view_file",
        "list_dir",
        "find_by_name",
        "grep_search",
        "run_command",
    ] {
        assert!(
            allowed_tools.contains(&expected),
            "missing expanded Agy tool `{expected}` in {allowed_tools:?}"
        );
    }
    assert!(!allowed_tools.iter().any(|t| t.starts_with("cap:")));
    assert!(!allowed_tools.contains(&"edit_file"));

    // system/init with concrete Agy read/search/shell tools must NOT fast-stop in ReadOnly.
    let now = Instant::now();
    let _ = session.start(now);
    let ready = session.on_line(
        br#"{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":["ac-dev-ada"],"mode":"plan"}}}"#,
        now,
    );
    assert!(ready
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::Ready { .. }))));
    let init_ok = session.on_line(
        br#"{"type":"system","subtype":"init","conversation_id":"agy-ro-1","mode":"plan","tools":["read_file","view_file","list_dir","find_by_name","grep_search","run_command"]}"#,
        now,
    );
    assert!(
        !init_ok.iter().any(|o| matches!(o, Output::CloseStdin)),
        "ReadOnly init with concrete Agy tools must not fast-stop: {init_ok:?}"
    );

    // Incoming approval and subagent tool_use events carry normalized ToolCapability.
    let apr = session.on_line(
        br#"{"type":"control_request","request_id":"ro-cmd","request":{"subtype":"can_use_tool","tool_name":"run_command","input":{"CommandLine":"agent-os inbox --json"}}}"#,
        now,
    );
    let req = apr
        .iter()
        .find_map(|o| match o {
            Output::Event(SessionEvent::ApprovalRequested(r)) => Some(r),
            _ => None,
        })
        .expect("run_command approval");
    assert_eq!(req.capability, Some(ToolCapability::Shell));

    let sub = session.on_line(
        br#"{"type":"assistant","message":{"id":"m-sub","content":[{"type":"tool_use","id":"sub-1","name":"invoke_subagent","input":{"prompt":"check"}}]}}"#,
        now,
    );
    assert!(sub.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::ToolStarted {
            tool_use_id,
            capability: Some(ToolCapability::Subagent),
            ..
        }) if tool_use_id == "sub-1"
    )));

    // Exposing edit_file in ReadOnly still fast-stops.
    let init_bad = session.on_line(
        br#"{"type":"system","subtype":"init","conversation_id":"agy-ro-1","mode":"plan","tools":["read_file","edit_file"]}"#,
        now,
    );
    assert!(init_bad.iter().any(|o| matches!(o, Output::CloseStdin)));

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

    // WorkspaceEdits & AutonomousSandboxed preset translation matrix (when keeps_allow_rules=true).
    let mut we_spec = sample_spec();
    we_spec.permission_mode = PermissionMode::AcceptEdits;
    we_spec.policy.deny.clear();
    we_spec.sandbox.deny_write.clear();
    we_spec.sandbox.deny_read.clear();
    we_spec.tools = None;
    we_spec.policy.allow = vec![
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
            scope: RuleScope::CommandPrefix(CommandPrefix::try_from("agent-os").unwrap()),
        },
    ];
    let (we_plan, mut we_session) = runtime.plan_session(&we_spec, &sample_ctx()).unwrap();
    let we_settings: serde_json::Value = serde_json::from_slice(
        &we_plan
            .files
            .iter()
            .find(|f| f.rel_path.as_str() == "agy-settings.json")
            .unwrap()
            .contents,
    )
    .unwrap();
    assert_eq!(
        we_settings["permissions"]["mode"],
        serde_json::json!("accept-edits")
    );
    let we_allow = we_settings["permissions"]["allow"].as_array().unwrap();
    assert!(we_allow.contains(&serde_json::json!({ "tool": "edit_file" })));
    assert!(we_allow.contains(&serde_json::json!({ "tool": "replace_file_content" })));
    assert!(we_allow
        .contains(&serde_json::json!({ "tool": "run_command", "commandPrefix": "agent-os" })));
    let _ = we_session.start(now);
    let we_ready = we_session.on_line(
        br#"{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":["ac-dev-ada"],"mode":"accept-edits"}}}"#,
        now,
    );
    assert!(we_ready
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::Ready { .. }))));
    let we_init = we_session.on_line(
        br#"{"type":"system","subtype":"init","conversation_id":"agy-we-1","mode":"accept-edits","tools":["read_file","edit_file","run_command"]}"#,
        now,
    );
    assert!(!we_init.iter().any(|o| matches!(o, Output::CloseStdin)));
    let we_apr = we_session.on_line(
        br#"{"type":"control_request","request_id":"we-edit","request":{"subtype":"can_use_tool","tool_name":"edit_file","input":{"path":"/work/t-1/lib.rs"}}}"#,
        now,
    );
    assert!(we_apr.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::ApprovalRequested(r))
            if r.capability == Some(ToolCapability::Edit) && r.escalation == Escalation::None
    )));

    let mut auto_spec = we_spec.clone();
    auto_spec.policy.allow.pop();
    auto_spec.policy.allow.push(Rule {
        tool: ToolCapability::Shell.to_rule_tool(),
        scope: RuleScope::Any,
    });
    let (auto_plan, mut auto_session) = runtime.plan_session(&auto_spec, &sample_ctx()).unwrap();
    let auto_settings: serde_json::Value = serde_json::from_slice(
        &auto_plan
            .files
            .iter()
            .find(|f| f.rel_path.as_str() == "agy-settings.json")
            .unwrap()
            .contents,
    )
    .unwrap();
    let auto_allow = auto_settings["permissions"]["allow"].as_array().unwrap();
    assert!(auto_allow.contains(&serde_json::json!({ "tool": "run_command" })));
    let _ = auto_session.start(now);
    let auto_ready = auto_session.on_line(
        br#"{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":["ac-dev-ada"],"mode":"accept-edits"}}}"#,
        now,
    );
    assert!(auto_ready
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::Ready { .. }))));
    let auto_init = auto_session.on_line(
        br#"{"type":"system","subtype":"init","conversation_id":"agy-auto-1","mode":"accept-edits","tools":["read_file","edit_file","run_command"]}"#,
        now,
    );
    assert!(!auto_init.iter().any(|o| matches!(o, Output::CloseStdin)));
    let auto_apr = auto_session.on_line(
        br#"{"type":"control_request","request_id":"auto-cmd","request":{"subtype":"can_use_tool","tool_name":"run_command","input":{"CommandLine":"cargo test"}}}"#,
        now,
    );
    assert!(auto_apr.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::ApprovalRequested(r))
            if r.capability == Some(ToolCapability::Shell) && r.escalation == Escalation::None
    )));

    // Conflicting sandbox-bypass aliases must NOT mask `unsandboxed: true`.
    let (_, mut bypass_session) = runtime.plan_session(&sample_spec(), &sample_ctx()).unwrap();
    let _ = bypass_session.start(now);
    let _ = bypass_session.on_line(
        br#"{"type":"control_response","response":{"subtype":"success","request_id":"host-1","response":{"agents":["ac-dev-ada"],"mode":"ask"}}}"#,
        now,
    );
    let masked_bypass = bypass_session.on_line(
        br#"{"type":"control_request","request_id":"sb-mask","request":{"subtype":"can_use_tool","tool_name":"run_command","input":{"CommandLine":"agent-os inbox","dangerouslyDisableSandbox":false,"unsandboxed":true}}}"#,
        now,
    );
    let masked_req = masked_bypass
        .iter()
        .find_map(|o| match o {
            Output::Event(SessionEvent::ApprovalRequested(r)) => Some(r),
            _ => None,
        })
        .expect("masked bypass approval");
    assert_eq!(masked_req.escalation, Escalation::SandboxBypass);
}
