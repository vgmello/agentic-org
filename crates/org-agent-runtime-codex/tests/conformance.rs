use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use org_agent_runtime::testkit::{self, Script, ScriptApproval};
use org_agent_runtime::{
    AgentIdentity, AgentName, AgentRuntime, ApprovalId, CommandPrefix, Control, ControlError,
    CostBasis, Decision, EnvName, Escalation, ExitInfo, LaunchContext, LimitStatus, McpServer,
    McpServerName, Output, PathPattern, PermissionMode, PermissionPolicy, PlainId, PlanError,
    Probe, ProbeContext, ProbeOutput, ProbeResult, Rule, RuleScope, RuleTool, RuntimeId,
    SandboxSpec, SessionEvent, SessionId, SessionKind, SessionSpec, SpecPath, StartFailure, Token,
    TurnOutcome,
};
use org_agent_runtime_codex::CodexRuntime;

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
        runtime: RuntimeId::try_from("codex").unwrap(),
        kind: SessionKind::Task {
            task_id: PlainId::try_from("t1").unwrap(),
        },
        agent: AgentIdentity {
            name: AgentName::try_from("ac-dev-gpt").unwrap(),
            description: "Backend engineer".to_owned(),
            prompt: "You are gpt.".to_owned(),
        },
        model: Some(Token::try_from("gpt-5.4").unwrap()),
        effort: Some(Token::try_from("high").unwrap()),
        workspace: sp("/work/t-1"),
        read_only_dirs: vec![sp("/homes/gpt")],
        instructions: "Follow workspace rules.".to_owned(),
        permission_mode: PermissionMode::Ask,
        policy: PermissionPolicy {
            version: 1,
            allow: vec![Rule {
                tool: RuleTool::try_from("shell").unwrap(),
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
            allowed_unix_sockets: vec![sp("/office/run/gpt.sock")],
        },
        mcp_servers,
        tools: None,
    }
}

fn sample_ctx() -> LaunchContext {
    LaunchContext {
        runtime_path: PathBuf::from("/opt/homebrew/bin/codex"),
        runtime_version: Some("0.169.0".to_owned()),
        runtime_dir: sp("/office/agents/gpt/runtime"),
        tmpdir: sp("/private/tmp/agentic-org/run-2"),
        office_env_names: vec![EnvName::try_from("AGENT_OS_SESSION_TOKEN").unwrap()],
        new_session_id: Some(SessionId::try_from("codex-thread-1").unwrap()),
        resume: None,
        config_dir: Some(sp("/office/agents/gpt/home/codex")),
    }
}

fn codex_script() -> Script {
    let init_ok =
        br#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"name":"codex","version":"0.169.0"}}}"#
            .to_vec();
    let thread_ok = br#"{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"thr_0193a5c0","sandbox":"workspace-write","approvalPolicy":"untrusted"}}}"#.to_vec();
    let turn_started =
        br#"{"jsonrpc":"2.0","method":"turn/started","params":{"turn":{"id":"turn_1"}}}"#.to_vec();
    let msg_delta =
        br#"{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"delta":"Working."}}"#
            .to_vec();
    let usage = br#"{"jsonrpc":"2.0","method":"thread/tokenUsage/updated","params":{"tokenUsage":{"last":{"inputTokens":50,"outputTokens":25,"cachedInputTokens":10,"cacheWriteInputTokens":4,"reasoningOutputTokens":5}}}}"#.to_vec();
    let approval_req = br#"{"jsonrpc":"2.0","id":"apr-7","method":"item/commandExecution/requestApproval","params":{"itemId":"cmd_1","command":"echo hi","reason":"Check output"}}"#.to_vec();
    let approval_cancel =
        br#"{"jsonrpc":"2.0","method":"serverRequest/resolved","params":{"requestId":"apr-7"}}"#
            .to_vec();
    let turn_done = br#"{"jsonrpc":"2.0","method":"turn/completed","params":{"turn":{"id":"turn_1","status":"completed"}}}"#.to_vec();

    let mut approval = ScriptApproval::new(approval_req, ApprovalId::new("apr-7"));
    approval.cancel = Some(approval_cancel);
    Script {
        opening: vec![init_ok, thread_ok],
        turn: vec![turn_started, msg_delta, usage],
        turn_end: vec![turn_done],
        stderr: vec![b"codex diagnostic".to_vec()],
        approval: Some(approval),
    }
}

#[test]
fn passes_org_agent_runtime_testkit_conformance_and_plan_checks() {
    let runtime = CodexRuntime::new();
    let spec = sample_spec();
    let ctx = sample_ctx();
    let caps = runtime.capabilities();
    assert!(caps.steer);
    assert_eq!(caps.cost, CostBasis::None);
    let (plan, _) = runtime.plan_session(&spec, &ctx).expect("plan_session");

    testkit::check_plan(&plan, &spec, &ctx, &caps);
    let script = codex_script();
    testkit::check_all(
        &mut || runtime.plan_session(&spec, &ctx).expect("plan_session").1,
        &script,
        &caps,
        Instant::now(),
    );
}

#[test]
fn plan_pins_shell_env_policy_clears_mcp_and_enforces_paths_and_take_over() {
    let runtime = CodexRuntime::new();
    let mut spec = sample_spec();
    spec.sandbox
        .writable_roots
        .push(sp("/office/agents/gpt/home/codex"));
    let ctx = sample_ctx();
    let (plan, _) = runtime.plan_session(&spec, &ctx).unwrap();
    assert_eq!(plan.args[0], "app-server");
    assert!(plan.args.contains(&"--stdio".to_owned()));
    assert!(!plan.args.iter().any(|a| a.contains("danger-full-access")));
    assert!(plan
        .args
        .iter()
        .any(|a| a.contains("/private/tmp/agentic-org/run-2")));
    assert!(!plan
        .args
        .iter()
        .any(|a| a.contains("/office/agents/gpt/home/codex")));
    assert!(plan
        .args
        .contains(&"shell_environment_policy.ignore_default_excludes=true".to_owned()));
    assert!(plan
        .args
        .iter()
        .any(|a| a == r#"shell_environment_policy.include_only=["AGENT_OS_SESSION_TOKEN"]"#));
    assert!(plan.args.contains(&"mcp_servers={}".to_owned()));
    assert!(plan
        .env_add
        .iter()
        .any(|(k, v)| k.as_str() == "CODEX_HOME" && v == "/office/agents/gpt/home/codex"));

    let mut bad = sample_spec();
    bad.workspace = sp("/");
    assert!(matches!(
        runtime.plan_session(&bad, &ctx),
        Err(PlanError::InvalidSpec(_))
    ));

    // Take-over refuses missing resume and OneOff sessions.
    assert!(matches!(
        runtime.plan_take_over(&spec, &ctx, 80, 24),
        Err(PlanError::InvalidSpec(_))
    ));
    let mut resume_ctx = sample_ctx();
    resume_ctx.resume = Some(SessionId::try_from("thr_resume_9").unwrap());
    let mut one_off = sample_spec();
    one_off.kind = SessionKind::OneOff {
        run_id: PlainId::try_from("r1").unwrap(),
    };
    assert!(matches!(
        runtime.plan_take_over(&one_off, &resume_ctx, 80, 24),
        Err(PlanError::InvalidSpec(_))
    ));
}

#[test]
fn handshake_validates_sandbox_policy_resume_failure_and_empty_agents() {
    let runtime = CodexRuntime::new();
    let mut plan_spec = sample_spec();
    plan_spec.permission_mode = PermissionMode::Plan;
    let now = Instant::now();

    // Requesting Plan (read-only) must reject workspace-write from server.
    let (_, mut s1) = runtime.plan_session(&plan_spec, &sample_ctx()).unwrap();
    let _ = s1.start(now);
    let _ = s1.on_line(
        br#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"version":"0.169.0"}}}"#,
        now,
    );
    let out = s1.on_line(
        br#"{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"thr_1","sandbox":"workspace-write","approvalPolicy":"untrusted"}}}"#,
        now,
    );
    assert!(out.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::ModeMismatch { .. }
        })
    )));

    // approvalPolicy="never" must fail with ModeMismatch.
    let (_, mut s2) = runtime.plan_session(&sample_spec(), &sample_ctx()).unwrap();
    let _ = s2.start(now);
    let _ = s2.on_line(
        br#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"version":"0.169.0"}}}"#,
        now,
    );
    let out = s2.on_line(
        br#"{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"thr_1","sandbox":"workspace-write","approvalPolicy":"never"}}}"#,
        now,
    );
    assert!(out.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::ModeMismatch { .. }
        })
    )));

    // thread/resume error must emit StartFailure::ResumeFailed.
    let mut resume_ctx = sample_ctx();
    resume_ctx.resume = Some(SessionId::try_from("thr_old").unwrap());
    let (_, mut s3) = runtime.plan_session(&sample_spec(), &resume_ctx).unwrap();
    let _ = s3.start(now);
    let open_writes = s3.on_line(
        br#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"version":"0.169.0"}}}"#,
        now,
    );
    let thread_open_text = open_writes
        .iter()
        .filter_map(|o| match o {
            Output::Write(l) => std::str::from_utf8(l.as_bytes()).ok(),
            _ => None,
        })
        .collect::<String>();
    assert!(thread_open_text.contains(r#""approvalsReviewer":"user""#));
    let out = s3.on_line(
        br#"{"jsonrpc":"2.0","id":2,"error":{"code":-32000,"message":"thread not found"}}"#,
        now,
    );
    assert!(out.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::ResumeFailed { .. }
        })
    )));

    // Ready must report empty agents list for Codex.
    let (_, mut s4) = runtime.plan_session(&sample_spec(), &sample_ctx()).unwrap();
    let _ = s4.start(now);
    let _ = s4.on_line(
        br#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"version":"0.169.0"}}}"#,
        now,
    );
    let out = s4.on_line(
        br#"{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"thr_ok","sandbox":"workspace-write","approvalPolicy":"untrusted"}}}"#,
        now,
    );
    assert!(out.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::Ready { agents, permission_mode, .. })
            if agents.is_empty() && permission_mode.as_deref() == Some("untrusted")
    )));

    // Missing approvalPolicy, missing sandbox, and object-form dangerFullAccess all fail closed.
    for bad_reply in [
        br#"{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"thr_1","sandbox":"workspace-write"}}}"#
            .as_slice(),
        br#"{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"thr_1","approvalPolicy":"untrusted"}}}"#
            .as_slice(),
        br#"{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"thr_1","sandboxPolicy":{"type":"dangerFullAccess"},"approvalPolicy":"untrusted"}}}"#
            .as_slice(),
    ] {
        let (_, mut sx) = runtime.plan_session(&sample_spec(), &sample_ctx()).unwrap();
        let _ = sx.start(now);
        let _ = sx.on_line(
            br#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"version":"0.169.0"}}}"#,
            now,
        );
        let out = sx.on_line(bad_reply, now);
        assert!(
            out.iter().any(|o| matches!(
                o,
                Output::Event(SessionEvent::StartFailed {
                    reason: StartFailure::ModeMismatch { .. }
                })
            )),
            "expected ModeMismatch for {:?}",
            std::str::from_utf8(bad_reply)
        );
    }
}

#[test]
fn turn_error_recovery_multistep_usage_escalations_unknown_server_req_and_rate_limits() {
    let runtime = CodexRuntime::new();
    let spec = sample_spec();
    let now = Instant::now();
    let (_, mut session) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
    let _ = session.start(now);
    assert!(session.start(now).is_empty());
    let _ = session.on_line(
        br#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"version":"0.169.0"}}}"#,
        now,
    );
    let _ = session.on_line(
        br#"{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"thr_ok","sandbox":"workspace-write","approvalPolicy":"untrusted"}}}"#,
        now,
    );

    // Idle Interrupt returns Ok(vec![]) without writing turn/interrupt.
    assert!(session.control(Control::Interrupt, now).unwrap().is_empty());

    // Idle usage notification does NOT leak as a phantom delta.
    assert!(session
        .on_line(
            br#"{"jsonrpc":"2.0","method":"thread/tokenUsage/updated","params":{"tokenUsage":{"last":{"inputTokens":999,"outputTokens":999}}}}"#,
            now
        )
        .is_empty());

    // turn/start RPC error unwinds Phase::Busy so next Send succeeds.
    let start_out = session
        .control(
            Control::Send {
                text: "fail first".into(),
                urgent: false,
            },
            now,
        )
        .unwrap();
    assert!(start_out.iter().any(|o| match o {
        Output::Write(l) => std::str::from_utf8(l.as_bytes())
            .unwrap_or("")
            .contains(r#""approvalPolicy":"untrusted""#),
        _ => false,
    }));
    let err_out = session.on_line(
        br#"{"jsonrpc":"2.0","id":3,"error":{"code":-32000,"message":"rate limited"}}"#,
        now,
    );
    assert!(err_out.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::TurnEnded {
            outcome: TurnOutcome::Failed { .. },
            ..
        })
    )));

    // Second turn succeeds and accumulates multi-step usage + cacheWriteInputTokens.
    let _ = session
        .control(
            Control::Send {
                text: "second turn".into(),
                urgent: false,
            },
            now,
        )
        .unwrap();
    let _ = session.on_line(
        br#"{"jsonrpc":"2.0","method":"thread/tokenUsage/updated","params":{"tokenUsage":{"last":{"inputTokens":10,"outputTokens":5,"cacheWriteInputTokens":2}}}}"#,
        now,
    );
    let _ = session.on_line(
        br#"{"jsonrpc":"2.0","method":"thread/tokenUsage/updated","params":{"tokenUsage":{"last":{"inputTokens":20,"outputTokens":7,"cacheWriteInputTokens":3}}}}"#,
        now,
    );

    // Unknown server request sends JSON-RPC -32601 error reply so server never hangs.
    let unk = session.on_line(
        br#"{"jsonrpc":"2.0","id":99,"method":"unknown/serverMethod","params":{}}"#,
        now,
    );
    assert!(unk.iter().any(|o| match o {
        Output::Write(l) => std::str::from_utf8(l.as_bytes())
            .unwrap_or("")
            .contains("-32601"),
        _ => false,
    }));

    // Escalation classifier handles require_escalated, additionalPermissions, and blockedPath.
    let esc_sb = session.on_line(
        br#"{"jsonrpc":"2.0","id":"e1","method":"item/commandExecution/requestApproval","params":{"sandboxPermissions":"require_escalated","blockedPath":"/etc/hosts"}}"#,
        now,
    );
    let req_sb = esc_sb
        .iter()
        .find_map(|o| match o {
            Output::Event(SessionEvent::ApprovalRequested(r)) => Some(r),
            _ => None,
        })
        .unwrap();
    assert_eq!(req_sb.escalation, Escalation::SandboxBypass);
    assert_eq!(req_sb.blocked_path.as_deref(), Some("/etc/hosts"));

    // Deny { stop_turn: true } sends {"decision":"cancel"}.
    let deny_out = session
        .control(
            Control::Answer {
                request_id: ApprovalId::new("e1"),
                decision: Decision::Deny {
                    message: Some("no".into()),
                    stop_turn: true,
                },
            },
            now,
        )
        .unwrap();
    assert!(deny_out.iter().any(|o| match o {
        Output::Write(l) => std::str::from_utf8(l.as_bytes())
            .unwrap_or("")
            .contains(r#""decision":"cancel""#),
        _ => false,
    }));

    // Rate limits parse both RFC 3339 strings and numeric Unix epoch seconds/ms as RFC 3339 UTC.
    let rl = session.on_line(
        br#"{"jsonrpc":"2.0","method":"account/rateLimits/updated","params":{"rateLimits":{"primary":{"usedPercent":85.0,"resetsAt":1795359600},"secondary":{"usedPercent":50.0,"resetsAt":1795359600000}}}}"#,
        now,
    );
    assert!(rl.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::RateLimit(r))
            if r.status == LimitStatus::Warning && r.resets_at.as_deref() == Some("2026-11-22T15:00:00Z")
    )));
    assert!(rl.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::RateLimit(r))
            if r.status == LimitStatus::Ok && r.resets_at.as_deref() == Some("2026-11-22T15:00:00Z")
    )));

    // Completing turn flushes accumulated usage (10+20=30 input, 5+7=12 output, 2+3=5 cache_write).
    let done = session.on_line(
        br#"{"jsonrpc":"2.0","method":"turn/completed","params":{"turn":{"id":"t2","status":"completed"}}}"#,
        now,
    );
    assert!(done.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::Usage(u))
            if u.input == 30 && u.output == 12 && u.cache_write == 5
    )));

    // Truncated emits SessionEvent::Truncated.
    assert!(matches!(
        session.on_truncated(4096, now).as_slice(),
        [Output::Event(SessionEvent::Truncated { len: 4096 })]
    ));

    // Mid-turn crash emits TurnOutcome::Failed.
    let _ = session
        .control(
            Control::Send {
                text: "crash".into(),
                urgent: false,
            },
            now,
        )
        .unwrap();
    let crashed = session.on_exit(
        ExitInfo {
            code: Some(1),
            signal: None,
        },
        now,
    );
    assert!(crashed.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::TurnEnded {
            outcome: TurnOutcome::Failed { .. },
            ..
        })
    )));

    // Probes: reject root runtime_dir and check version range.
    let mut pctx = ProbeContext::new(PathBuf::from("/opt/homebrew/bin/codex"));
    pctx.runtime_dir = Some(sp("/"));
    assert!(runtime.plan_probe(Probe::Version, &pctx).is_none());
    let out_v = ProbeOutput::new(b"codex-cli 0.169.0\n".to_vec(), Vec::new(), Some(0));
    assert_eq!(
        runtime.parse_probe(Probe::Version, &out_v),
        ProbeResult::Version {
            raw: "codex-cli 0.169.0".to_owned(),
            in_tested_range: Some(true),
        }
    );
    assert_eq!(
        session.control(Control::Close, now),
        Err(ControlError::Closed)
    );
}
