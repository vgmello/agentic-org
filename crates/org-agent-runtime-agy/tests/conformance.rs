use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use org_agent_runtime::testkit::{self, Script, ScriptApproval};
use org_agent_runtime::{
    AgentIdentity, AgentName, AgentRuntime, ApprovalId, ApprovalKind, CommandPrefix, Control,
    ControlError, CostBasis, Decision, EnvName, Escalation, ExitInfo, LaunchContext, LimitKind,
    McpServer, McpServerName, MidTurnDelivery, Output, PathPattern, PermissionMode,
    PermissionPolicy, PlainId, PlanError, Probe, ProbeContext, ProbeOutput, ProbeResult, Rule,
    RuleScope, RuleTool, RuntimeId, SandboxSpec, SessionEvent, SessionId, SessionKind, SessionSpec,
    SpecPath, StartFailure, Token, ToolCapability, ToolName, TurnOutcome,
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
        model: Some(Token::try_from("gemini-4-neon-high").unwrap()),
        effort: None,
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
        runtime_version: Some("1.5.17".to_owned()),
        runtime_dir: sp("/office/agents/ada/runtime"),
        tmpdir: sp("/private/tmp/agentic-org/run-1"),
        office_env_names: vec![EnvName::try_from("AGENT_OS_SESSION_TOKEN").unwrap()],
        new_session_id: Some(SessionId::try_from("agy-conv-1").unwrap()),
        resume: None,
        config_dir: Some(sp("/office/agents/ada/home/gemini")),
    }
}

/// Real recorded Antigravity ACP 1.5.17 wire frames for `initialize`, `session/new`,
/// `session/update`, `session/request_permission`, `$/cancel_request`, and mid-turn steering.
fn agy_acp_script() -> Script {
    let init_ok = br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"name":"antigravity","version":"1.5.17"},"agentCapabilities":{"sessionCapabilities":{"resume":{},"close":{},"load":{},"list":{},"additionalDirectories":{}}},"_meta":{"antigravity":{"tier":true,"systemPrompt":{"append":true}}}}}"#.to_vec();
    // Server already reports `currentModelId: "gemini-4-neon-high"` matching `sample_spec()`,
    // so `session/new` (id=2) transitions directly to Idle and the first turn prompt gets id=3.
    let session_ok = br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"11111111-1111-4111-8111-111111111111","models":{"currentModelId":"gemini-4-neon-high","availableModels":[{"modelId":"gemini-4-neon-high","name":"Neon High"}]}}}"#.to_vec();
    let chunk = br#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"11111111-1111-4111-8111-111111111111","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Working."}}}}"#.to_vec();
    let approval_req = br#"{"jsonrpc":"2.0","id":"perm-1","method":"session/request_permission","params":{"sessionId":"11111111-1111-4111-8111-111111111111","toolCall":{"toolCallId":"tc-1","name":"run_command","title":"Run echo hi","rawInput":{"CommandLine":"echo hi"}},"options":[{"optionId":"allow","name":"Allow","kind":"allow_once"},{"optionId":"deny","name":"Deny","kind":"reject_once"}]}}"#.to_vec();
    let approval_cancel =
        br#"{"jsonrpc":"2.0","method":"$/cancel_request","params":{"id":"perm-1"}}"#.to_vec();
    // On live `agy 1.5.17`, sending a mid-turn `session/prompt` (id=4) immediately resolves
    // superseded prompt id=3 with `end_turn` without aborting tools and transfers turn ownership
    // to id=4; emitting id=3 then id=4 cleanly ends both non-steered and steered testkit paths.
    let prompt3_done = br#"{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn","usage":{"inputTokens":120,"outputTokens":25,"cachedReadTokens":80,"thoughtTokens":10}}}"#.to_vec();
    let prompt4_done = br#"{"jsonrpc":"2.0","id":4,"result":{"stopReason":"end_turn","usage":{"inputTokens":60,"outputTokens":15,"cachedReadTokens":40}}}"#.to_vec();

    let mut approval = ScriptApproval::new(approval_req, ApprovalId::new("perm-1"));
    approval.cancel = Some(approval_cancel);
    Script {
        opening: vec![init_ok, session_ok],
        turn: vec![chunk],
        turn_end: vec![prompt3_done.clone(), prompt4_done],
        stderr: vec![b"agy acp diagnostic".to_vec()],
        approval: Some(approval),
        mid_turn_step: Some(prompt3_done),
    }
}

#[test]
fn passes_org_agent_runtime_testkit_conformance_and_plan_checks() {
    let runtime = AgyRuntime::new();
    let spec = sample_spec();
    let ctx = sample_ctx();
    let caps = runtime.capabilities();
    assert_eq!(caps.mid_turn_delivery, MidTurnDelivery::NativeSteer);
    assert!(caps.supports_mid_turn_delivery());
    assert!(caps.interrupt_in_band);
    assert!(caps.approval_bridge);
    assert_eq!(caps.cost, CostBasis::None);
    assert_eq!(caps.tested_versions.min, "1.5.16");
    assert_eq!(caps.tested_versions.max, "1.5.17");

    let (plan, _) = runtime.plan_session(&spec, &ctx).expect("plan_session");
    testkit::check_plan(&plan, &spec, &ctx, &caps);
    let script = agy_acp_script();
    testkit::check_all(
        &mut || runtime.plan_session(&spec, &ctx).expect("plan_session").1,
        &script,
        &caps,
        Instant::now(),
    );
}

#[test]
fn plan_launches_agy_acp_with_isolation_flags_and_take_over_guards() {
    let runtime = AgyRuntime::new();
    let spec = sample_spec();
    let ctx = sample_ctx();
    let (plan, _) = runtime.plan_session(&spec, &ctx).unwrap();

    assert_eq!(plan.args[0], "acp");
    assert_eq!(plan.args[1], "--stdio");
    assert!(plan
        .args
        .contains(&"--include-default-customizations=false".to_owned()));
    assert!(plan.args.contains(&"--disable-telemetry".to_owned()));
    assert!(plan.args.contains(&"--sandbox".to_owned()));
    assert!(plan.args.contains(&"--add-dir".to_owned()));
    assert!(plan.args.contains(&"/homes/ada".to_owned()));
    assert!(!plan.args.contains(&"--model".to_owned()));
    assert!(!plan.args.contains(&"-p".to_owned()));
    assert!(!plan.args.iter().any(|a| a.contains("dangerously")));
    assert!(!plan.args.iter().any(|a| a.contains("remote-control")));
    let gemini_pos = plan.args.iter().position(|a| a == "--gemini-dir").unwrap();
    assert_eq!(plan.args[gemini_pos + 1], "/office/agents/ada/home/gemini");
    let app_pos = plan
        .args
        .iter()
        .position(|a| a == "--app-data-dir")
        .unwrap();
    assert_eq!(plan.args[app_pos + 1], "antigravity-cli");

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
fn handshake_injects_system_prompt_selects_model_and_supports_resume_and_timeout() {
    let runtime = AgyRuntime::new();
    let mut spec = sample_spec();
    spec.model = Some(Token::try_from("gemini-4-neon-xhigh").unwrap());
    let now = Instant::now();

    let (_, mut session) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
    let init_writes = session.start(now);
    assert!(session.start(now).is_empty());
    let init_json: serde_json::Value = match &init_writes[0] {
        Output::Write(line) => serde_json::from_slice(line.as_bytes()).unwrap(),
        other => panic!("expected Write, got {other:?}"),
    };
    assert_eq!(init_json["method"], "initialize");
    assert_eq!(init_json["params"]["protocolVersion"], 1);

    let open_writes = session.on_line(
        br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"name":"antigravity","version":"1.5.17"}}}"#,
        now,
    );
    let open_json: serde_json::Value = match &open_writes[0] {
        Output::Write(line) => serde_json::from_slice(line.as_bytes()).unwrap(),
        other => panic!("expected Write, got {other:?}"),
    };
    assert_eq!(open_json["method"], "session/new");
    assert_eq!(open_json["params"]["cwd"], "/work/t-1");
    assert_eq!(open_json["params"]["mcpServers"][0]["name"], "office");
    assert_eq!(
        open_json["params"]["_meta"]["antigravity"]["systemPrompt"]["append"],
        "You are ada.\n\nFollow workspace rules."
    );

    // Server defaults to neon-high; adapter must emit `session/set_model` for neon-xhigh before Ready.
    let set_model_writes = session.on_line(
        br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sid-xhigh","models":{"currentModelId":"gemini-4-neon-high"}}}"#,
        now,
    );
    assert!(!set_model_writes
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::Ready { .. }))));
    let model_json: serde_json::Value = match &set_model_writes[0] {
        Output::Write(line) => serde_json::from_slice(line.as_bytes()).unwrap(),
        other => panic!("expected set_model Write, got {other:?}"),
    };
    assert_eq!(model_json["method"], "session/set_model");
    assert_eq!(model_json["params"]["sessionId"], "sid-xhigh");
    assert_eq!(model_json["params"]["modelId"], "gemini-4-neon-xhigh");

    let ready_events = session.on_line(br#"{"jsonrpc":"2.0","id":3,"result":{}}"#, now);
    assert!(ready_events.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::SessionStarted { runtime_session_id })
            if runtime_session_id.as_str() == "sid-xhigh"
    )));
    assert!(ready_events.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::Ready {
            runtime_version: Some(v),
            ..
        }) if v == "1.5.17"
    )));

    // Resume uses `session/resume` and maps RPC error to StartFailure::ResumeFailed.
    let mut resume_ctx = sample_ctx();
    resume_ctx.resume = Some(SessionId::try_from("agy-missing").unwrap());
    let (_, mut resumed) = runtime.plan_session(&spec, &resume_ctx).unwrap();
    let _ = resumed.start(now);
    let res_open = resumed.on_line(
        br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"version":"1.5.17"}}}"#,
        now,
    );
    let res_json: serde_json::Value = match &res_open[0] {
        Output::Write(line) => serde_json::from_slice(line.as_bytes()).unwrap(),
        other => panic!("expected session/resume Write, got {other:?}"),
    };
    assert_eq!(res_json["method"], "session/resume");
    assert_eq!(res_json["params"]["sessionId"], "agy-missing");
    let fail = resumed.on_line(
        br#"{"jsonrpc":"2.0","id":2,"error":{"code":-32001,"message":"session not found"}}"#,
        now,
    );
    assert!(fail.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::ResumeFailed { .. }
        })
    )));

    // Timeout during handshake emits ProtocolTimeout.
    let (_, mut slow) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
    let _ = slow.start(now);
    let timed = slow.on_tick(now + Duration::from_secs(31));
    assert!(timed.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::ProtocolTimeout
        })
    )));
}

#[test]
fn mid_turn_steer_transfers_ownership_without_aborting_and_in_band_cancel_keeps_session_alive() {
    let runtime = AgyRuntime::new();
    let spec = sample_spec();
    let now = Instant::now();
    let (_, mut session) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
    let _ = session.start(now);
    for line in &agy_acp_script().opening {
        let _ = session.on_line(line, now);
    }

    // Start turn (id=3) and run a tool call.
    let send_out = session
        .control(
            Control::Send {
                text: "Run sleep 3".into(),
            },
            now,
        )
        .unwrap();
    assert!(send_out
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::TurnStarted { .. }))));

    let tool_start = session.on_line(
        br#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"11111111-1111-4111-8111-111111111111","update":{"sessionUpdate":"tool_call","toolCallId":"tc-sleep","name":"run_command","rawInput":{"CommandLine":"sleep 3 && echo SLEEP_DONE"}}}}"#,
        now,
    );
    assert!(tool_start.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::ToolStarted {
            tool_use_id,
            capability: Some(ToolCapability::Shell),
            ..
        }) if tool_use_id == "tc-sleep"
    )));

    // Mid-turn steer (id=4): emits session/prompt with zero cancel/interrupt frames.
    let steer_writes = session
        .control(
            Control::DeliverMidTurn {
                delivery_id: "deliv-99".into(),
                text: "Urgent steer CEDAR-99".into(),
            },
            now,
        )
        .unwrap();
    let steer_json: serde_json::Value = match &steer_writes[0] {
        Output::Write(line) => serde_json::from_slice(line.as_bytes()).unwrap(),
        other => panic!("expected session/prompt Write, got {other:?}"),
    };
    assert_eq!(steer_json["method"], "session/prompt");
    assert_eq!(steer_json["id"], 4);

    // Live agy 1.5.17 resolves superseded id=3 immediately without ending the turn.
    let sup_out = session.on_line(
        br#"{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn","usage":{"inputTokens":100,"outputTokens":10,"cachedReadTokens":50}}}"#,
        now,
    );
    assert!(sup_out.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::MidTurnDelivered { delivery_id }) if delivery_id == "deliv-99"
    )));
    assert!(!sup_out
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::TurnEnded { .. }))));
    // Turn is still Busy owned by id=4.
    assert_eq!(
        session.control(
            Control::Send {
                text: "nope".into()
            },
            now
        ),
        Err(ControlError::Busy)
    );

    // Tool finishes uninterrupted, then id=4 completes with accumulated usage.
    let tool_end = session.on_line(
        br#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"11111111-1111-4111-8111-111111111111","update":{"sessionUpdate":"tool_call_update","toolCallId":"tc-sleep","status":"completed"}}}"#,
        now,
    );
    assert!(tool_end.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::ToolFinished {
            tool_use_id,
            is_error: false
        }) if tool_use_id == "tc-sleep"
    )));
    let _ = session.on_line(
        br#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"11111111-1111-4111-8111-111111111111","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"SLEEP_DONE + CEDAR-99"}}}}"#,
        now,
    );
    let final_out = session.on_line(
        br#"{"jsonrpc":"2.0","id":4,"result":{"stopReason":"end_turn","usage":{"inputTokens":40,"outputTokens":20,"cachedReadTokens":30}}}"#,
        now,
    );
    assert!(final_out.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::AssistantText { text, .. }) if text == "SLEEP_DONE + CEDAR-99"
    )));
    assert!(final_out.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::Usage(u)) if u.input == 140 && u.output == 30 && u.cache_read == 80
    )));
    assert!(final_out.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::TurnEnded {
            outcome: TurnOutcome::Completed,
            ..
        })
    )));

    // In-band Interrupt emits `session/cancel` and keeps warm session alive for next turn.
    let _ = session
        .control(Control::Send { text: "t2".into() }, now)
        .unwrap();
    let cancel_out = session.control(Control::Interrupt, now).unwrap();
    let cancel_json: serde_json::Value = match &cancel_out[0] {
        Output::Write(line) => serde_json::from_slice(line.as_bytes()).unwrap(),
        other => panic!("expected session/cancel Write, got {other:?}"),
    };
    assert_eq!(cancel_json["method"], "session/cancel");
    assert!(cancel_json.get("id").is_none());
    let cancelled = session.on_line(
        br#"{"jsonrpc":"2.0","id":5,"result":{"stopReason":"cancelled"}}"#,
        now,
    );
    assert!(cancelled.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::TurnEnded {
            outcome: TurnOutcome::Interrupted,
            ..
        })
    )));
    // Warm session still accepts another turn!
    assert!(session
        .control(Control::Send { text: "t3".into() }, now)
        .is_ok());
}

#[test]
fn approvals_classify_escalations_answer_allow_deny_and_handle_close_and_errors() {
    let runtime = AgyRuntime::new();
    let spec = sample_spec();
    let now = Instant::now();
    let (_, mut session) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
    let _ = session.start(now);
    for line in &agy_acp_script().opening {
        let _ = session.on_line(line, now);
    }

    // Unknown tool -> ApprovalKind::Unknown + Escalation::Unknown.
    let unk = session.on_line(
        br#"{"jsonrpc":"2.0","id":101,"method":"session/request_permission","params":{"toolCall":{"toolCallId":"tc-u","name":"mystery_tool","rawInput":{"path":"/etc/passwd"}},"options":[{"optionId":"allow","kind":"allow_once"},{"optionId":"deny","kind":"reject_once"}]}}"#,
        now,
    );
    let req = unk
        .iter()
        .find_map(|o| match o {
            Output::Event(SessionEvent::ApprovalRequested(r)) => Some(r),
            _ => None,
        })
        .unwrap();
    assert_eq!(req.id, ApprovalId::new("101"));
    assert_eq!(req.kind, ApprovalKind::Unknown);
    assert_eq!(req.escalation, Escalation::Unknown);
    assert_eq!(req.blocked_path.as_deref(), Some("/etc/passwd"));

    // Answer deny preserves integer JSON-RPC id 101 and selects "deny" optionId.
    let ans_deny = session
        .control(
            Control::Answer {
                request_id: ApprovalId::new("101"),
                decision: Decision::Deny {
                    message: Some("no".into()),
                    stop_turn: false,
                },
            },
            now,
        )
        .unwrap();
    let deny_json: serde_json::Value = match &ans_deny[0] {
        Output::Write(line) => serde_json::from_slice(line.as_bytes()).unwrap(),
        other => panic!("expected Write, got {other:?}"),
    };
    assert_eq!(deny_json["id"], 101);
    assert_eq!(deny_json["result"]["outcome"]["outcome"], "selected");
    assert_eq!(deny_json["result"]["outcome"]["optionId"], "deny");

    // Unsandboxed & SandboxNetworkAccess -> SandboxBypass.
    let sb = session.on_line(
        br#"{"jsonrpc":"2.0","id":"sb-1","method":"session/request_permission","params":{"toolCall":{"name":"run_command","rawInput":{"dangerouslyDisableSandbox":false,"unsandboxed":true}}}}"#,
        now,
    );
    assert!(sb.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::ApprovalRequested(r))
            if r.escalation == Escalation::SandboxBypass
    )));
    let _ = session
        .control(
            Control::Answer {
                request_id: ApprovalId::new("sb-1"),
                decision: Decision::AllowOnce,
            },
            now,
        )
        .unwrap();

    // LimitKind::MaxTokens + AGY_ERROR stderr + Close emits session/close + CloseStdin.
    let _ = session
        .control(Control::Send { text: "go".into() }, now)
        .unwrap();
    let max_tok = session.on_line(
        br#"{"jsonrpc":"2.0","id":3,"result":{"stopReason":"max_tokens"}}"#,
        now,
    );
    assert!(max_tok.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::TurnEnded {
            outcome: TurnOutcome::Limit {
                kind: LimitKind::MaxTokens
            },
            ..
        })
    )));
    let err_out = session.on_stderr_line(
        br#"AGY_ERROR: {"code":"retry","message":"transient 503"}"#,
        now,
    );
    assert!(err_out.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::Notice { text, .. }) if text.contains("transient 503")
    )));
    let close_out = session.control(Control::Close, now).unwrap();
    assert!(close_out.iter().any(|o| matches!(o, Output::CloseStdin)));
    let late = session.on_line(
        br#"{"jsonrpc":"2.0","id":"late-1","method":"session/request_permission","params":{"toolCall":{"name":"run_command"}}}"#,
        now,
    );
    assert!(late.iter().any(|o| matches!(o, Output::Write(_))));
    assert!(!late
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::ApprovalRequested(_)))));

    // Mid-turn steer rejected with JSON-RPC error restores prior prompt ownership.
    let (_, mut s_err) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
    let _ = s_err.start(now);
    for line in &agy_acp_script().opening {
        let _ = s_err.on_line(line, now);
    }
    let _ = s_err
        .control(Control::Send { text: "t1".into() }, now)
        .unwrap();
    let _ = s_err
        .control(
            Control::DeliverMidTurn {
                delivery_id: "d-fail".into(),
                text: "steer".into(),
            },
            now,
        )
        .unwrap();
    let steer_err = s_err.on_line(
        br#"{"jsonrpc":"2.0","id":4,"error":{"code":-32603,"message":"steer rejected"}}"#,
        now,
    );
    assert!(steer_err.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::MidTurnDeliveryFailed { delivery_id, .. })
            if delivery_id == "d-fail"
    )));
    // Prior prompt id=3 still ends the turn cleanly!
    let orig_end = s_err.on_line(
        br#"{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn"}}"#,
        now,
    );
    assert!(orig_end.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::TurnEnded {
            outcome: TurnOutcome::Completed,
            ..
        })
    )));

    // Crash during turn fails pending mid-turn delivery and emits Failed.
    let _ = s_err
        .control(Control::Send { text: "t2".into() }, now)
        .unwrap();
    let _ = s_err
        .control(
            Control::DeliverMidTurn {
                delivery_id: "d-crash".into(),
                text: "steer".into(),
            },
            now,
        )
        .unwrap();
    let crash = s_err.on_exit(
        ExitInfo {
            code: Some(1),
            signal: None,
        },
        now,
    );
    assert!(crash.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::MidTurnDeliveryFailed { delivery_id, .. })
            if delivery_id == "d-crash"
    )));
    assert!(crash.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::TurnEnded {
            outcome: TurnOutcome::Failed { .. },
            ..
        })
    )));
}

#[test]
fn capability_validation_and_probes_fail_closed() {
    let runtime = AgyRuntime::new();

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

    // Version & auth probes.
    let mut pctx = ProbeContext::new(PathBuf::from("/usr/local/bin/agy"));
    pctx.runtime_dir = Some(sp("/"));
    assert!(runtime.plan_probe(Probe::Auth, &pctx).is_none());
    let ver_ok = ProbeOutput::new(b"1.5.17\n".to_vec(), vec![], Some(0));
    assert_eq!(
        runtime.parse_probe(Probe::Version, &ver_ok),
        ProbeResult::Version {
            raw: "1.5.17".to_owned(),
            in_tested_range: Some(true),
        }
    );
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
    let ok_auth = ProbeOutput::new(b"gemini-4-neon-high\n".to_vec(), vec![], Some(0));
    assert_eq!(
        runtime.parse_probe(Probe::Auth, &ok_auth),
        ProbeResult::Auth {
            signed_in: true,
            method: Some("agy-cli".to_owned()),
        }
    );
}
