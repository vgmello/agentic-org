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
        repository_instructions: String::new(),
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
        operator_shell_prefixes: Vec::new(),
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
    let session_ok = br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"11111111-1111-4111-8111-111111111111","models":{"currentModelId":"gemini-4-neon-high","availableModels":[{"modelId":"gemini-4-neon-high","name":"Neon High"}]},"modes":{"currentModeId":"default"}}}"#.to_vec();
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
        "Follow workspace rules.\n\nYou are ada."
    );

    // Server defaults to neon-high; adapter must emit `session/set_model` for neon-xhigh before Ready.
    let set_model_writes = session.on_line(
        br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sid-xhigh","models":{"currentModelId":"gemini-4-neon-high"},"modes":{"currentModeId":"default"}}}"#,
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

    // Headless plan rejects `spec.effort` (encoded in ACP modelId, not CLI flag).
    let mut effort_spec = sample_spec();
    effort_spec.effort = Some(Token::try_from("high").unwrap());
    assert!(matches!(
        runtime.plan_session(&effort_spec, &sample_ctx()),
        Err(PlanError::Unsupported("effort"))
    ));
}

#[test]
fn chained_mid_turn_steers_handle_intermediate_and_superseded_errors_cleanly() {
    let runtime = AgyRuntime::new();
    let spec = sample_spec();
    let now = Instant::now();

    // Case A: Send(3), Steer(4, "d1"), Steer(5, "d2"); id=4 errors -> d1 fails, id=3 result
    // hands off to d2 (never falsely acking d1!), and id=5 error leaves turn cleanly ended.
    let (_, mut s_a) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
    let _ = s_a.start(now);
    for line in &agy_acp_script().opening {
        let _ = s_a.on_line(line, now);
    }
    let _ = s_a
        .control(Control::Send { text: "go".into() }, now)
        .unwrap();
    let _ = s_a
        .control(
            Control::DeliverMidTurn {
                delivery_id: "d1".into(),
                text: "s1".into(),
            },
            now,
        )
        .unwrap();
    let _ = s_a
        .control(
            Control::DeliverMidTurn {
                delivery_id: "d2".into(),
                text: "s2".into(),
            },
            now,
        )
        .unwrap();
    let err4 = s_a.on_line(
        br#"{"jsonrpc":"2.0","id":4,"error":{"code":-32603,"message":"steer 1 failed"}}"#,
        now,
    );
    assert!(err4.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::MidTurnDeliveryFailed { delivery_id, .. }) if delivery_id == "d1"
    )));
    assert!(!err4
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::TurnEnded { .. }))));
    // If id=5 also errors while id=3 is still running, turn stays Busy until id=3 resolves.
    let err5 = s_a.on_line(
        br#"{"jsonrpc":"2.0","id":5,"error":{"code":-32603,"message":"steer 2 failed"}}"#,
        now,
    );
    assert!(err5.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::MidTurnDeliveryFailed { delivery_id, .. }) if delivery_id == "d2"
    )));
    assert!(!err5
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::TurnEnded { .. }))));
    assert_eq!(
        s_a.control(
            Control::Send {
                text: "nope".into()
            },
            now
        ),
        Err(ControlError::Busy)
    );
    let end3 = s_a.on_line(
        br#"{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn"}}"#,
        now,
    );
    assert!(!end3
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::MidTurnDelivered { .. }))));
    assert!(end3.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::TurnEnded {
            outcome: TurnOutcome::Completed,
            ..
        })
    )));

    // Case B: Send(3), Steer(4, "d1"), Steer(5, "d2"); id=3 errors -> id=4 result acks BOTH d1
    // and d2 handoff without false failure when id=5 completes.
    let (_, mut s_b) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
    let _ = s_b.start(now);
    for line in &agy_acp_script().opening {
        let _ = s_b.on_line(line, now);
    }
    let _ = s_b
        .control(Control::Send { text: "go".into() }, now)
        .unwrap();
    let _ = s_b
        .control(
            Control::DeliverMidTurn {
                delivery_id: "d1".into(),
                text: "s1".into(),
            },
            now,
        )
        .unwrap();
    let _ = s_b
        .control(
            Control::DeliverMidTurn {
                delivery_id: "d2".into(),
                text: "s2".into(),
            },
            now,
        )
        .unwrap();
    let err3 = s_b.on_line(
        br#"{"jsonrpc":"2.0","id":3,"error":{"code":-32001,"message":"orig aborted"}}"#,
        now,
    );
    assert!(!err3
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::TurnEnded { .. }))));
    let res4 = s_b.on_line(
        br#"{"jsonrpc":"2.0","id":4,"result":{"stopReason":"end_turn"}}"#,
        now,
    );
    assert!(res4.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::MidTurnDelivered { delivery_id }) if delivery_id == "d1"
    )));
    assert!(res4.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::MidTurnDelivered { delivery_id }) if delivery_id == "d2"
    )));
    let res5 = s_b.on_line(
        br#"{"jsonrpc":"2.0","id":5,"result":{"stopReason":"end_turn"}}"#,
        now,
    );
    assert!(!res5
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::MidTurnDeliveryFailed { .. }))));
    assert!(res5.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::TurnEnded {
            outcome: TurnOutcome::Completed,
            ..
        })
    )));
}

#[test]
fn usage_deduplicates_streamed_updates_and_falls_back_on_eof() {
    let runtime = AgyRuntime::new();
    let spec = sample_spec();
    let now = Instant::now();
    let (_, mut session) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
    let _ = session.start(now);
    for line in &agy_acp_script().opening {
        let _ = session.on_line(line, now);
    }

    // Idle usage_update outside any open turn is ignored.
    let _ = session.on_line(
        br#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"usage_update","usage":{"inputTokens":999,"outputTokens":999}}}}"#,
        now,
    );

    // During a turn: cumulative usage_update snapshots + authoritative result.usage must NOT double-count.
    let _ = session
        .control(Control::Send { text: "t1".into() }, now)
        .unwrap();
    let _ = session.on_line(
        br#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"usage_update","usage":{"inputTokens":50,"outputTokens":10}}}}"#,
        now,
    );
    let _ = session.on_line(
        br#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"usage_update","usage":{"inputTokens":100,"outputTokens":20}}}}"#,
        now,
    );
    let done = session.on_line(
        br#"{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn","usage":{"inputTokens":120,"outputTokens":25}}}"#,
        now,
    );
    assert!(done.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::Usage(u)) if u.input == 120 && u.output == 25
    )));

    // Crash/EOF mid-turn without result.usage flushes the max streamed usage snapshot once.
    let _ = session
        .control(Control::Send { text: "t2".into() }, now)
        .unwrap();
    let _ = session.on_line(
        br#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"usage_update","usage":{"inputTokens":40,"outputTokens":8}}}}"#,
        now,
    );
    let _ = session.on_line(
        br#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"usage_update","usage":{"inputTokens":70,"outputTokens":14}}}}"#,
        now,
    );
    let eof = session.on_stdout_closed(now);
    assert!(eof.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::Usage(u)) if u.input == 70 && u.output == 14
    )));
}

#[test]
fn modes_jsonrpc_nulls_auth_error_option_selection_and_idle_interrupt_hardening() {
    let runtime = AgyRuntime::new();
    let spec = sample_spec();
    let now = Instant::now();

    // 1. JSON-RPC `"error": null` with realistic `modes.currentModeId` reaches Ready cleanly.
    let (_, mut s_none) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
    let _ = s_none.start(now);
    let _ = s_none.on_line(
        br#"{"jsonrpc":"2.0","id":1,"error":null,"result":{"protocolVersion":1,"agentInfo":{"version":"1.5.17"}}}"#,
        now,
    );
    let ready_none = s_none.on_line(
        br#"{"jsonrpc":"2.0","id":2,"error":null,"result":{"sessionId":"sid-none","models":{"currentModelId":"gemini-4-neon-high"},"modes":{"currentModeId":"default"}}}"#,
        now,
    );
    assert!(ready_none.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::Ready {
            permission_mode: Some(m),
            ..
        }) if m == "default"
    )));
    // Idle Interrupt emits zero frames.
    assert!(s_none.control(Control::Interrupt, now).unwrap().is_empty());

    // 2. Permission option ordering: prefer `allow_once`/`reject_once` over `allow_always`.
    let perm = s_none.on_line(
        br#"{"jsonrpc":"2.0","id":"opt-1","method":"session/request_permission","params":{"toolCall":{"toolCallId":"tc-opt","title":"Edit file","kind":"edit","locations":[{"path":"/work/t-1/a.rs"}]},"options":[{"optionId":"allow","kind":"allow_always"},{"optionId":"once-allow","kind":"allow_once"},{"optionId":"deny","kind":"reject_always"},{"optionId":"once-deny","kind":"reject_once"}]}}"#,
        now,
    );
    let req = perm
        .iter()
        .find_map(|o| match o {
            Output::Event(SessionEvent::ApprovalRequested(r)) => Some(r),
            _ => None,
        })
        .unwrap();
    assert_eq!(req.kind, ApprovalKind::FileChange);
    assert_eq!(req.blocked_path.as_deref(), Some("/work/t-1/a.rs"));
    let ans = s_none
        .control(
            Control::Answer {
                request_id: ApprovalId::new("opt-1"),
                decision: Decision::AllowOnce,
            },
            now,
        )
        .unwrap();
    let ans_json: serde_json::Value = match &ans[0] {
        Output::Write(line) => serde_json::from_slice(line.as_bytes()).unwrap(),
        other => panic!("expected Write, got {other:?}"),
    };
    assert_eq!(ans_json["result"]["outcome"]["optionId"], "once-allow");

    // 3. `tool_call_update status=cancelled` withdraws pending approval; `Deny { stop_turn: true }` sends `session/cancel`.
    let _ = s_none
        .control(Control::Send { text: "go".into() }, now)
        .unwrap();
    let _ = s_none.on_line(
        br#"{"jsonrpc":"2.0","id":"c-1","method":"session/request_permission","params":{"toolCall":{"toolCallId":"tc-cancel","name":"run_command"}}}"#,
        now,
    );
    let cancelled = s_none.on_line(
        br#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"tool_call_update","toolCallId":"tc-cancel","status":"cancelled"}}}"#,
        now,
    );
    assert!(cancelled.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::ApprovalCancelled { id }) if id == &ApprovalId::new("c-1")
    )));
    let _ = s_none.on_line(
        br#"{"jsonrpc":"2.0","id":"d-stop","method":"session/request_permission","params":{"toolCall":{"toolCallId":"tc-ds","name":"run_command"}}}"#,
        now,
    );
    let deny_stop = s_none
        .control(
            Control::Answer {
                request_id: ApprovalId::new("d-stop"),
                decision: Decision::Deny {
                    message: None,
                    stop_turn: true,
                },
            },
            now,
        )
        .unwrap();
    assert!(deny_stop.iter().any(|o| match o {
        Output::Write(line) => String::from_utf8_lossy(line.as_bytes()).contains("session/cancel"),
        _ => false,
    }));

    // 4. `current_mode_update` with banned mode emits ModeChanged + fast-stops session.
    let mode_upd = s_none.on_line(
        br#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"current_mode_update","currentModeId":"yolo"}}}"#,
        now,
    );
    assert!(mode_upd.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::ModeChanged { mode: Some(m) }) if m == "yolo"
    )));
    assert!(mode_upd.iter().any(|o| matches!(o, Output::CloseStdin)));
}

#[test]
fn opening_mode_mismatch_auth_error_and_resume_failure_scoping() {
    let runtime = AgyRuntime::new();
    let spec = sample_spec();
    let now = Instant::now();

    // Real `agy 1.5.17` ACP reports `modes.currentModeId = "default"` for Ask mode.
    let (_, mut s_default) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
    let _ = s_default.start(now);
    let _ = s_default.on_line(
        br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"version":"1.5.17"}}}"#,
        now,
    );
    let ready_default = s_default.on_line(
        br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sid-def","models":{"currentModelId":"gemini-4-neon-high"},"modes":{"currentModeId":"default"}}}"#,
        now,
    );
    assert!(ready_default.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::Ready {
            permission_mode: Some(m),
            ..
        }) if m == "default"
    )));
    // Runtime drift from `default` into `accept-edits` or unknown permissive mode must fast-stop.
    for bad_runtime_mode in ["accept-edits", "acceptEdits", "dontAsk"] {
        let (_, mut s_drift) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
        let _ = s_drift.start(now);
        let _ = s_drift.on_line(
            br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"version":"1.5.17"}}}"#,
            now,
        );
        let _ = s_drift.on_line(
            br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sid-d","models":{"currentModelId":"gemini-4-neon-high"},"modes":{"currentModeId":"default"}}}"#,
            now,
        );
        let frame = format!(
            r#"{{"jsonrpc":"2.0","method":"session/update","params":{{"update":{{"sessionUpdate":"current_mode_update","currentModeId":"{bad_runtime_mode}"}}}}}}"#
        );
        let drift = s_drift.on_line(frame.as_bytes(), now);
        assert!(
            drift.iter().any(|o| matches!(o, Output::CloseStdin)),
            "expected CloseStdin on runtime drift to `{bad_runtime_mode}`"
        );
    }
    // Under PermissionMode::Plan, runtime drift to `default` or `accept-edits` must fast-stop.
    let mut plan_spec = sample_spec();
    plan_spec.permission_mode = PermissionMode::Plan;
    for bad_plan_drift in ["default", "accept-edits"] {
        let (_, mut s_plan) = runtime.plan_session(&plan_spec, &sample_ctx()).unwrap();
        let _ = s_plan.start(now);
        let _ = s_plan.on_line(
            br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"version":"1.5.17"}}}"#,
            now,
        );
        let _ = s_plan.on_line(
            br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sid-p","models":{"currentModelId":"gemini-4-neon-high"},"modes":{"currentModeId":"plan"}}}"#,
            now,
        );
        let frame = format!(
            r#"{{"jsonrpc":"2.0","method":"session/update","params":{{"update":{{"sessionUpdate":"current_mode_update","currentModeId":"{bad_plan_drift}"}}}}}}"#
        );
        let drift = s_plan.on_line(frame.as_bytes(), now);
        assert!(
            drift.iter().any(|o| matches!(o, Output::CloseStdin)),
            "expected CloseStdin on Plan runtime drift to `{bad_plan_drift}`"
        );
    }

    // Banned `modes.currentModeId` in `session/new` fails immediately with ModeMismatch + CloseStdin.
    for banned_mode in ["yolo", "auto"] {
        let (_, mut s_bad) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
        let _ = s_bad.start(now);
        let _ = s_bad.on_line(
            br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}"#,
            now,
        );
        let frame = format!(
            r#"{{"jsonrpc":"2.0","id":2,"result":{{"sessionId":"sid-m","modes":{{"currentModeId":"{banned_mode}"}}}}}}"#
        );
        let res = s_bad.on_line(frame.as_bytes(), now);
        assert!(res.iter().any(|o| matches!(
            o,
            Output::Event(SessionEvent::StartFailed {
                reason: StartFailure::ModeMismatch { reported, .. }
            }) if reported == banned_mode
        )));
        assert!(res.iter().any(|o| matches!(o, Output::CloseStdin)));
    }

    // Non-banned mismatched `modes.currentModeId` in `session/new` enters OpeningMode, emits
    // `session/set_mode`, and fails closed with ModeMismatch if `session/set_mode` returns an error.
    for mismatch_mode in ["accept-edits", "ask"] {
        let (_, mut s_mis) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
        let _ = s_mis.start(now);
        let _ = s_mis.on_line(
            br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}"#,
            now,
        );
        let frame = format!(
            r#"{{"jsonrpc":"2.0","id":2,"result":{{"sessionId":"sid-m","modes":{{"currentModeId":"{mismatch_mode}"}}}}}}"#
        );
        let switch_out = s_mis.on_line(frame.as_bytes(), now);
        assert!(!switch_out.iter().any(|o| matches!(
            o,
            Output::Event(SessionEvent::Ready { .. } | SessionEvent::StartFailed { .. })
        )));
        let set_mode_json: serde_json::Value = match &switch_out[0] {
            Output::Write(line) => serde_json::from_slice(line.as_bytes()).unwrap(),
            other => panic!("expected session/set_mode Write, got {other:?}"),
        };
        assert_eq!(set_mode_json["method"], "session/set_mode");
        assert_eq!(set_mode_json["id"], 3);
        assert_eq!(set_mode_json["params"]["sessionId"], "sid-m");
        assert_eq!(set_mode_json["params"]["modeId"], "default");

        let err_out = s_mis.on_line(
            br#"{"jsonrpc":"2.0","id":3,"error":{"code":-32602,"message":"mode switch rejected"}}"#,
            now,
        );
        assert!(err_out.iter().any(|o| matches!(
            o,
            Output::Event(SessionEvent::StartFailed {
                reason: StartFailure::ModeMismatch { requested, reported }
            }) if requested == "default" && reported == mismatch_mode
        )));
    }

    // 6. Auth RPC error (-32000) during opening emits StartFailure::AuthRequired;
    // error in OpeningInit on resumed session does NOT emit ResumeFailed.
    let mut resume_ctx = sample_ctx();
    resume_ctx.resume = Some(SessionId::try_from("agy-res").unwrap());
    let (_, mut s_auth) = runtime.plan_session(&spec, &resume_ctx).unwrap();
    let _ = s_auth.start(now);
    let auth_fail = s_auth.on_line(
        br#"{"jsonrpc":"2.0","id":1,"result":null,"error":{"code":-32000,"message":"Authentication required"}}"#,
        now,
    );
    assert!(auth_fail.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::AuthRequired
        })
    )));
    let (_, mut s_init_err) = runtime.plan_session(&spec, &resume_ctx).unwrap();
    let _ = s_init_err.start(now);
    let init_fail = s_init_err.on_line(
        br#"{"jsonrpc":"2.0","id":1,"error":{"code":-32603,"message":"internal boot error"}}"#,
        now,
    );
    assert!(init_fail.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::Other { .. }
        })
    )));
}

#[test]
fn pass2_allow_always_fail_closed_opening_mode_update_and_null_fallbacks() {
    let runtime = AgyRuntime::new();
    let mut spec = sample_spec();
    spec.model = Some(Token::try_from("gemini-4-neon-xhigh").unwrap());
    let now = Instant::now();

    // 1. Opening-phase `current_mode_update` with mismatched or banned mode during OpeningModel
    // MUST emit StartFailure::ModeMismatch (and CloseStdin for banned) rather than reaching Ready.
    for (bad_mode, expect_close) in [("accept-edits", false), ("yolo", true)] {
        let (_, mut s_open) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
        let _ = s_open.start(now);
        let _ = s_open.on_line(
            br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}"#,
            now,
        );
        let _ = s_open.on_line(
            br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sid-om","models":{"currentModelId":"gemini-4-neon-high"},"modes":{"currentModeId":"default"}}}"#,
            now,
        );
        let frame = format!(
            r#"{{"jsonrpc":"2.0","method":"session/update","params":{{"update":{{"sessionUpdate":"current_mode_update","currentModeId":"{bad_mode}"}}}}}}"#
        );
        let res = s_open.on_line(frame.as_bytes(), now);
        assert!(res.iter().any(|o| matches!(
            o,
            Output::Event(SessionEvent::StartFailed {
                reason: StartFailure::ModeMismatch { reported, .. }
            }) if reported == bad_mode
        )));
        assert_eq!(
            res.iter().any(|o| matches!(o, Output::CloseStdin)),
            expect_close
        );
        // Subsequent set_model response must NOT emit Ready.
        let late_model = s_open.on_line(br#"{"jsonrpc":"2.0","id":3,"result":{}}"#, now);
        assert!(!late_model
            .iter()
            .any(|o| matches!(o, Output::Event(SessionEvent::Ready { .. }))));
    }

    // 2. When `options` offers only `allow_always` (no `allow_once`), `AllowOnce` MUST fail
    // closed with `cancelled` + warning Notice and NEVER emit `optionId: "allow"`.
    // Also verify null `"path": null` falls through to `toolCall.locations[0].path` and
    // `"usage": null` in prompt result preserves streamed usage fallback.
    let base_spec = sample_spec();
    let (_, mut s) = runtime.plan_session(&base_spec, &sample_ctx()).unwrap();
    let _ = s.start(now);
    for line in &agy_acp_script().opening {
        let _ = s.on_line(line, now);
    }
    let perm = s.on_line(
        br#"{"jsonrpc":"2.0","id":"aa-1","method":"session/request_permission","params":{"toolCall":{"toolCallId":"tc-aa","name":null,"title":"Edit file","kind":"edit","rawInput":{"path":null},"locations":[{"path":"/work/t-1/b.rs"}]},"options":[{"optionId":"allow","kind":"allow_always"},{"optionId":"deny","kind":"reject_once"}]}}"#,
        now,
    );
    let req = perm
        .iter()
        .find_map(|o| match o {
            Output::Event(SessionEvent::ApprovalRequested(r)) => Some(r),
            _ => None,
        })
        .unwrap();
    assert_eq!(req.blocked_path.as_deref(), Some("/work/t-1/b.rs"));
    let ans = s
        .control(
            Control::Answer {
                request_id: ApprovalId::new("aa-1"),
                decision: Decision::AllowOnce,
            },
            now,
        )
        .unwrap();
    let ans_json: serde_json::Value = match &ans[0] {
        Output::Write(line) => serde_json::from_slice(line.as_bytes()).unwrap(),
        other => panic!("expected Write, got {other:?}"),
    };
    assert_eq!(ans_json["result"]["outcome"]["outcome"], "cancelled");
    assert!(ans_json["result"]["outcome"].get("optionId").is_none());
    assert!(ans
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::Notice { .. }))));

    let _ = s.control(Control::Send { text: "go".into() }, now).unwrap();
    let _ = s.on_line(
        br#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"usage_update","usage":{"inputTokens":null,"input_tokens":55,"outputTokens":11}}}}"#,
        now,
    );
    let done = s.on_line(
        br#"{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn","usage":null}}"#,
        now,
    );
    assert!(done.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::Usage(u)) if u.input == 55 && u.output == 11
    )));
}

#[test]
fn opening_set_mode_switches_from_default_to_plan_then_sets_model_and_ready() {
    let runtime = AgyRuntime::new();
    let mut spec = sample_spec();
    spec.permission_mode = PermissionMode::Plan;
    spec.model = Some(Token::try_from("gemini-4-neon-xhigh").unwrap());
    let now = Instant::now();

    let (_, mut session) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
    let _ = session.start(now);
    let _ = session.on_line(
        br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"name":"antigravity","version":"1.5.17"}}}"#,
        now,
    );

    // Realistic `agy 1.5.17` opens in `"default"` even when ReadOnly/Plan requested `"plan"`.
    let mode_writes = session.on_line(
        br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sid-plan","models":{"currentModelId":"gemini-4-neon-high"},"modes":{"currentModeId":"default","availableModes":[{"id":"default"},{"id":"plan"},{"id":"accept-edits"}]}}}"#,
        now,
    );
    assert!(!mode_writes
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::Ready { .. }))));
    let mode_json: serde_json::Value = match &mode_writes[0] {
        Output::Write(line) => serde_json::from_slice(line.as_bytes()).unwrap(),
        other => panic!("expected session/set_mode Write, got {other:?}"),
    };
    assert_eq!(mode_json["method"], "session/set_mode");
    assert_eq!(mode_json["id"], 3);
    assert_eq!(mode_json["params"]["sessionId"], "sid-plan");
    assert_eq!(mode_json["params"]["modeId"], "plan");

    // Pre-switch echo of starting mode `"default"` is tolerated during OpeningMode,
    // and `current_mode_update` for `"plan"` updates reported_mode without emitting ModeChanged.
    let echo = session.on_line(
        br#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"current_mode_update","currentModeId":"default"}}}"#,
        now,
    );
    assert!(echo.is_empty());
    let switched = session.on_line(
        br#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"current_mode_update","currentModeId":"plan"}}}"#,
        now,
    );
    assert!(switched.is_empty());

    // `session/set_mode` result proceeds to `session/set_model` (id=4).
    let model_writes = session.on_line(br#"{"jsonrpc":"2.0","id":3,"result":{}}"#, now);
    assert!(!model_writes
        .iter()
        .any(|o| matches!(o, Output::Event(SessionEvent::Ready { .. }))));
    let model_json: serde_json::Value = match &model_writes[0] {
        Output::Write(line) => serde_json::from_slice(line.as_bytes()).unwrap(),
        other => panic!("expected session/set_model Write, got {other:?}"),
    };
    assert_eq!(model_json["method"], "session/set_model");
    assert_eq!(model_json["id"], 4);
    assert_eq!(model_json["params"]["sessionId"], "sid-plan");
    assert_eq!(model_json["params"]["modelId"], "gemini-4-neon-xhigh");

    // `session/set_model` result finishes opening with Ready { permission_mode: Some("plan") }.
    let ready = session.on_line(br#"{"jsonrpc":"2.0","id":4,"result":{}}"#, now);
    assert!(ready.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::SessionStarted { runtime_session_id })
            if runtime_session_id.as_str() == "sid-plan"
    )));
    assert!(ready.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::Ready {
            permission_mode: Some(m),
            ..
        }) if m == "plan"
    )));

    // OpeningMode fails closed on banned mode update (with CloseStdin) and on auth error.
    let (_, mut s_banned) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
    let _ = s_banned.start(now);
    let _ = s_banned.on_line(
        br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}"#,
        now,
    );
    let _ = s_banned.on_line(
        br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sid-p2","modes":{"currentModeId":"default"}}}"#,
        now,
    );
    let banned_upd = s_banned.on_line(
        br#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"current_mode_update","currentModeId":"yolo"}}}"#,
        now,
    );
    assert!(banned_upd.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::ModeMismatch { requested, reported }
        }) if requested == "plan" && reported == "yolo"
    )));
    assert!(banned_upd.iter().any(|o| matches!(o, Output::CloseStdin)));

    let (_, mut s_auth) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
    let _ = s_auth.start(now);
    let _ = s_auth.on_line(
        br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}"#,
        now,
    );
    let _ = s_auth.on_line(
        br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sid-p3","modes":{"currentModeId":"default"}}}"#,
        now,
    );
    let auth_err = s_auth.on_line(
        br#"{"jsonrpc":"2.0","id":3,"error":{"code":-32000,"message":"Authentication required"}}"#,
        now,
    );
    assert!(auth_err.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::ModeMismatch { requested, reported }
        }) if requested == "plan" && reported == "default"
    )));
    assert!(auth_err.iter().any(|o| matches!(o, Output::CloseStdin)));
}

#[test]
fn missing_modes_current_mode_id_fails_closed() {
    let runtime = AgyRuntime::new();
    let spec = sample_spec();
    let now = Instant::now();

    let payloads: &[&[u8]] = &[
        br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sid-1","models":{"currentModelId":"gemini-4-neon-high"}}}"#,
        br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sid-2","models":{"currentModelId":"gemini-4-neon-high"},"modes":{"currentModeId":null}}}"#,
        br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sid-3","models":{"currentModelId":"gemini-4-neon-high"},"modes":{"currentModeId":""}}}"#,
    ];
    for payload in payloads {
        let (_, mut session) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
        let _ = session.start(now);
        let _ = session.on_line(
            br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"version":"1.5.17"}}}"#,
            now,
        );
        let res = session.on_line(payload, now);
        assert!(res.iter().any(|o| matches!(
            o,
            Output::Event(SessionEvent::StartFailed {
                reason: StartFailure::ModeMismatch { requested, reported }
            }) if requested == "default" && reported == "<missing>"
        )));
        assert!(!res
            .iter()
            .any(|o| matches!(o, Output::Event(SessionEvent::Ready { .. }))));
    }
}

#[test]
fn bypass_sandbox_variants_and_acp_kind_tool_capability_fallback() {
    let runtime = AgyRuntime::new();
    let spec = sample_spec();
    let now = Instant::now();
    let (_, mut session) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
    let _ = session.start(now);
    for line in &agy_acp_script().opening {
        let _ = session.on_line(line, now);
    }

    // BypassSandbox variants fail closed as Escalation::SandboxBypass; explicit false stays None.
    let escalation_cases = [
        (
            "esc-1",
            r#"{"BypassSandbox":true}"#,
            Escalation::SandboxBypass,
        ),
        (
            "esc-2",
            r#"{"bypassSandbox":true}"#,
            Escalation::SandboxBypass,
        ),
        (
            "esc-3",
            r#"{"bypass_sandbox":true}"#,
            Escalation::SandboxBypass,
        ),
        (
            "esc-4",
            r#"{"BypassSandbox":"yes"}"#,
            Escalation::SandboxBypass,
        ),
        ("esc-5", r#"{"BypassSandbox":false}"#, Escalation::None),
    ];
    for (id, raw_input, expected) in escalation_cases {
        let frame = format!(
            r#"{{"jsonrpc":"2.0","id":"{id}","method":"session/request_permission","params":{{"toolCall":{{"toolCallId":"tc-{id}","name":"run_command","rawInput":{raw_input}}}}}}}"#
        );
        let out = session.on_line(frame.as_bytes(), now);
        let req = out
            .iter()
            .find_map(|o| match o {
                Output::Event(SessionEvent::ApprovalRequested(r)) => Some(r),
                _ => None,
            })
            .unwrap();
        assert_eq!(req.escalation, expected, "case {id} ({raw_input})");
    }

    // Missing `toolCall.name` derives ToolCapability from ACP `kind` in ApprovalRequested and ToolStarted.
    let cap_cases = [
        ("read", ToolCapability::Read),
        ("search", ToolCapability::Search),
        ("edit", ToolCapability::Edit),
        ("delete", ToolCapability::Edit),
        ("move", ToolCapability::Edit),
        ("execute", ToolCapability::Shell),
        ("subagent", ToolCapability::Subagent),
    ];
    for (idx, (kind, expected_cap)) in cap_cases.into_iter().enumerate() {
        let perm_frame = format!(
            r#"{{"jsonrpc":"2.0","id":"cap-{idx}","method":"session/request_permission","params":{{"toolCall":{{"toolCallId":"tc-cap-{idx}","title":"ACP {kind} action","kind":"{kind}"}}}}}}"#
        );
        let perm_out = session.on_line(perm_frame.as_bytes(), now);
        let req = perm_out
            .iter()
            .find_map(|o| match o {
                Output::Event(SessionEvent::ApprovalRequested(r)) => Some(r),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            req.capability,
            Some(expected_cap),
            "ApprovalRequested capability for kind={kind}"
        );

        let tool_frame = format!(
            r#"{{"jsonrpc":"2.0","method":"session/update","params":{{"update":{{"sessionUpdate":"tool_call","toolCallId":"tc-start-{idx}","title":"ACP {kind} action","kind":"{kind}"}}}}}}"#
        );
        let tool_out = session.on_line(tool_frame.as_bytes(), now);
        assert!(
            tool_out.iter().any(|o| matches!(
                o,
                Output::Event(SessionEvent::ToolStarted { capability, .. })
                    if *capability == Some(expected_cap)
            )),
            "ToolStarted capability for kind={kind}"
        );
    }
}

#[test]
fn plan_session_and_take_over_both_reject_effort() {
    let runtime = AgyRuntime::new();
    let mut effort_spec = sample_spec();
    effort_spec.effort = Some(Token::try_from("high").unwrap());
    let mut resume_ctx = sample_ctx();
    resume_ctx.resume = Some(SessionId::try_from("agy-conv-9").unwrap());

    assert!(matches!(
        runtime.plan_session(&effort_spec, &sample_ctx()),
        Err(PlanError::Unsupported("effort"))
    ));
    assert!(matches!(
        runtime.plan_take_over(&effort_spec, &resume_ctx, 120, 40),
        Err(PlanError::Unsupported("effort"))
    ));
}

#[test]
fn explicit_tool_name_blocks_acp_kind_reclassification_and_title_never_classifies() {
    let runtime = AgyRuntime::new();
    let spec = sample_spec();
    let now = Instant::now();
    let (_, mut session) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
    let _ = session.start(now);
    for line in &agy_acp_script().opening {
        let _ = session.on_line(line, now);
    }

    // Known explicit tool names MUST NOT be reclassified by misleading ACP `kind` values.
    let explicit_cases = [
        (
            "bash",
            "search",
            ApprovalKind::Command,
            Some(ToolCapability::Shell),
            Escalation::None,
        ),
        (
            "shell",
            "read",
            ApprovalKind::Command,
            Some(ToolCapability::Shell),
            Escalation::None,
        ),
        (
            "write",
            "read",
            ApprovalKind::FileChange,
            Some(ToolCapability::Edit),
            Escalation::None,
        ),
        (
            "edit",
            "search",
            ApprovalKind::FileChange,
            Some(ToolCapability::Edit),
            Escalation::None,
        ),
        (
            "websearch",
            "search",
            ApprovalKind::Other,
            None,
            Escalation::None,
        ),
        (
            "mcp_any_tool",
            "read",
            ApprovalKind::Other,
            None,
            Escalation::None,
        ),
        (
            "set_permission_mode",
            "read",
            ApprovalKind::Permissions,
            None,
            Escalation::PermissionChange,
        ),
    ];
    for (idx, (tool_name, acp_kind, exp_kind, exp_cap, exp_esc)) in
        explicit_cases.into_iter().enumerate()
    {
        let perm_frame = format!(
            r#"{{"jsonrpc":"2.0","id":"exp-{idx}","method":"session/request_permission","params":{{"toolCall":{{"toolCallId":"tc-exp-{idx}","name":"{tool_name}","title":"Innocent title","kind":"{acp_kind}"}}}}}}"#
        );
        let perm_out = session.on_line(perm_frame.as_bytes(), now);
        let req = perm_out
            .iter()
            .find_map(|o| match o {
                Output::Event(SessionEvent::ApprovalRequested(r)) => Some(r),
                _ => None,
            })
            .unwrap();
        assert_eq!(req.kind, exp_kind, "kind for {tool_name} + kind={acp_kind}");
        assert_eq!(
            req.capability, exp_cap,
            "capability for {tool_name} + kind={acp_kind}"
        );
        assert_eq!(
            req.escalation, exp_esc,
            "escalation for {tool_name} + kind={acp_kind}"
        );

        let tool_frame = format!(
            r#"{{"jsonrpc":"2.0","method":"session/update","params":{{"update":{{"sessionUpdate":"tool_call","toolCallId":"tc-exp-s-{idx}","name":"{tool_name}","title":"Innocent title","kind":"{acp_kind}"}}}}}}"#
        );
        let tool_out = session.on_line(tool_frame.as_bytes(), now);
        assert!(
            tool_out.iter().any(|o| matches!(
                o,
                Output::Event(SessionEvent::ToolStarted { capability, .. })
                    if *capability == exp_cap
            )),
            "ToolStarted capability for {tool_name} + kind={acp_kind}"
        );
    }

    // Missing or whitespace-only `name` with spoofed `title: "read_file"` and `kind: "execute"`
    // MUST classify strictly from ACP `kind` (Command + Shell), NEVER from `title`.
    for (idx, name_field) in ["", r#""name":"  ","#].into_iter().enumerate() {
        let perm_frame = format!(
            r#"{{"jsonrpc":"2.0","id":"spoof-{idx}","method":"session/request_permission","params":{{"toolCall":{{"toolCallId":"tc-sp-{idx}",{name_field}"title":"read_file","kind":"execute"}}}}}}"#
        );
        let perm_out = session.on_line(perm_frame.as_bytes(), now);
        let req = perm_out
            .iter()
            .find_map(|o| match o {
                Output::Event(SessionEvent::ApprovalRequested(r)) => Some(r),
                _ => None,
            })
            .unwrap();
        assert_eq!(req.tool, "read_file");
        assert_eq!(req.kind, ApprovalKind::Command);
        assert_eq!(req.capability, Some(ToolCapability::Shell));

        let tool_frame = format!(
            r#"{{"jsonrpc":"2.0","method":"session/update","params":{{"update":{{"sessionUpdate":"tool_call","toolCallId":"tc-sp-s-{idx}",{name_field}"title":"read_file","kind":"execute"}}}}}}"#
        );
        let tool_out = session.on_line(tool_frame.as_bytes(), now);
        assert!(tool_out.iter().any(|o| matches!(
            o,
            Output::Event(SessionEvent::ToolStarted {
                name,
                capability: Some(ToolCapability::Shell),
                ..
            }) if name == "read_file"
        )));
    }
}

#[test]
fn sandbox_bypass_detects_alternate_casing_nested_keys_and_non_bools_without_string_false_positives(
) {
    let runtime = AgyRuntime::new();
    let spec = sample_spec();
    let now = Instant::now();
    let (_, mut session) = runtime.plan_session(&spec, &sample_ctx()).unwrap();
    let _ = session.start(now);
    for line in &agy_acp_script().opening {
        let _ = session.on_line(line, now);
    }

    let cases = [
        ("null", r#"{"BypassSandbox":null}"#, Escalation::None),
        ("num-1", r#"{"BypassSandbox":1}"#, Escalation::SandboxBypass),
        ("num-0", r#"{"BypassSandbox":0}"#, Escalation::SandboxBypass),
        ("obj", r#"{"BypassSandbox":{}}"#, Escalation::SandboxBypass),
        ("arr", r#"{"BypassSandbox":[]}"#, Escalation::SandboxBypass),
        (
            "unsandboxed-cap",
            r#"{"Unsandboxed":true}"#,
            Escalation::SandboxBypass,
        ),
        (
            "bypass-upper",
            r#"{"BYPASS_SANDBOX":true}"#,
            Escalation::SandboxBypass,
        ),
        (
            "danger-pascal",
            r#"{"DangerouslyDisableSandbox":true}"#,
            Escalation::SandboxBypass,
        ),
        (
            "bypass-kebab",
            r#"{"bypass-sandbox":true}"#,
            Escalation::SandboxBypass,
        ),
        (
            "nested-obj",
            r#"{"options":{"BypassSandbox":true}}"#,
            Escalation::SandboxBypass,
        ),
        (
            "nested-arr",
            r#"{"args":[{"bypass_sandbox":"1"}]}"#,
            Escalation::SandboxBypass,
        ),
        (
            "cmd-string-no-fp",
            r#"{"CommandLine":"echo BypassSandbox dangerouslyDisableSandbox unsandboxed"}"#,
            Escalation::None,
        ),
    ];
    for (label, raw_input, expected) in cases {
        let frame = format!(
            r#"{{"jsonrpc":"2.0","id":"sb-{label}","method":"session/request_permission","params":{{"toolCall":{{"toolCallId":"tc-sb-{label}","name":"run_command","rawInput":{raw_input}}}}}}}"#
        );
        let out = session.on_line(frame.as_bytes(), now);
        let req = out
            .iter()
            .find_map(|o| match o {
                Output::Event(SessionEvent::ApprovalRequested(r)) => Some(r),
                _ => None,
            })
            .unwrap();
        assert_eq!(req.escalation, expected, "case {label}: {raw_input}");
    }
}

#[test]
fn opening_mode_edge_cases_banned_variants_and_terminal_mode_update_guard() {
    let runtime = AgyRuntime::new();
    let now = Instant::now();

    // 1. Trimmed/banned modes in `session/new` fail immediately with CloseStdin and NEVER emit `session/set_mode`.
    let ask_spec = sample_spec();
    for banned in [" yolo ", "bypassPermissions", "danger-full-access"] {
        let (_, mut s) = runtime.plan_session(&ask_spec, &sample_ctx()).unwrap();
        let _ = s.start(now);
        let _ = s.on_line(
            br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}"#,
            now,
        );
        let frame = format!(
            r#"{{"jsonrpc":"2.0","id":2,"result":{{"sessionId":"sid-ban","modes":{{"currentModeId":"{banned}"}}}}}}"#
        );
        let out = s.on_line(frame.as_bytes(), now);
        assert!(out.iter().any(|o| matches!(
            o,
            Output::Event(SessionEvent::StartFailed {
                reason: StartFailure::ModeMismatch { reported, .. }
            }) if reported == banned
        )));
        assert!(out.iter().any(|o| matches!(o, Output::CloseStdin)));
        assert!(!out.iter().any(|o| matches!(o, Output::Write(_))));
        // Late `current_mode_update` after terminal Phase::Failed MUST be ignored (no ModeChanged).
        let late = s.on_line(
            br#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"current_mode_update","currentModeId":"default"}}}"#,
            now,
        );
        assert!(late.is_empty());
    }

    // 2. OpeningMode: Control::Send -> NotReady, on_tick past deadline -> ProtocolTimeout.
    let mut plan_spec = sample_spec();
    plan_spec.permission_mode = PermissionMode::Plan;
    let (_, mut s_wait) = runtime.plan_session(&plan_spec, &sample_ctx()).unwrap();
    let _ = s_wait.start(now);
    let _ = s_wait.on_line(
        br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}"#,
        now,
    );
    let _ = s_wait.on_line(
        br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sid-w","models":{"currentModelId":"gemini-4-neon-high"},"modes":{"currentModeId":"default"}}}"#,
        now,
    );
    assert_eq!(
        s_wait.control(Control::Send { text: "hi".into() }, now),
        Err(ControlError::NotReady)
    );
    let timed = s_wait.on_tick(now + Duration::from_secs(31));
    assert!(timed.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::ProtocolTimeout
        })
    )));

    // 3. OpeningMode drift to third non-banned mode ("accept-edits" when "plan" requested)
    // fails closed with ModeMismatch, ignores late id=3 response, and ignores late mode updates.
    let (_, mut s_drift) = runtime.plan_session(&plan_spec, &sample_ctx()).unwrap();
    let _ = s_drift.start(now);
    let _ = s_drift.on_line(
        br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}"#,
        now,
    );
    let _ = s_drift.on_line(
        br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sid-dr","models":{"currentModelId":"gemini-4-neon-high"},"modes":{"currentModeId":"default"}}}"#,
        now,
    );
    let drift = s_drift.on_line(
        br#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"current_mode_update","currentModeId":"accept-edits"}}}"#,
        now,
    );
    assert!(drift.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::ModeMismatch { requested, reported }
        }) if requested == "plan" && reported == "accept-edits"
    )));
    assert!(s_drift
        .on_line(br#"{"jsonrpc":"2.0","id":3,"result":{}}"#, now)
        .is_empty());
    assert!(s_drift
        .on_line(
            br#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"current_mode_update","currentModeId":"plan"}}}"#,
            now,
        )
        .is_empty());

    // 4. `session/set_mode` with `result: null` fails closed; if `current_mode_update(expected)`
    // arrived before error/null, reported falls back to `"<rejected>"` (never `reported == expected`).
    let (_, mut s_null) = runtime.plan_session(&plan_spec, &sample_ctx()).unwrap();
    let _ = s_null.start(now);
    let _ = s_null.on_line(
        br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}"#,
        now,
    );
    let _ = s_null.on_line(
        br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sid-n","models":{"currentModelId":"gemini-4-neon-high"},"modes":{"currentModeId":"default"}}}"#,
        now,
    );
    let null_res = s_null.on_line(br#"{"jsonrpc":"2.0","id":3,"result":null}"#, now);
    assert!(null_res.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::ModeMismatch { requested, reported }
        }) if requested == "plan" && reported == "default"
    )));

    let (_, mut s_pre) = runtime.plan_session(&plan_spec, &sample_ctx()).unwrap();
    let _ = s_pre.start(now);
    let _ = s_pre.on_line(
        br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}"#,
        now,
    );
    let _ = s_pre.on_line(
        br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sid-pre","models":{"currentModelId":"gemini-4-neon-high"},"modes":{"currentModeId":"default"}}}"#,
        now,
    );
    let _ = s_pre.on_line(
        br#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"current_mode_update","currentModeId":"plan"}}}"#,
        now,
    );
    let pre_err = s_pre.on_line(
        br#"{"jsonrpc":"2.0","id":3,"error":{"code":-32602,"message":"rejected"}}"#,
        now,
    );
    assert!(pre_err.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::StartFailed {
            reason: StartFailure::ModeMismatch { requested, reported }
        }) if requested == "plan" && reported == "<rejected>"
    )));

    // 5. `session/set_mode` transitions directly to Ready when `currentModelId` already matches.
    let (_, mut s_direct) = runtime.plan_session(&plan_spec, &sample_ctx()).unwrap();
    let _ = s_direct.start(now);
    let _ = s_direct.on_line(
        br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"version":"1.5.17"}}}"#,
        now,
    );
    let mode_req = s_direct.on_line(
        br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sid-dir","models":{"currentModelId":"gemini-4-neon-high"},"modes":{"currentModeId":"default"}}}"#,
        now,
    );
    assert!(matches!(mode_req.as_slice(), [Output::Write(_)]));
    let ready = s_direct.on_line(br#"{"jsonrpc":"2.0","id":3,"result":{}}"#, now);
    assert!(!ready.iter().any(|o| matches!(o, Output::Write(_))));
    assert!(ready.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::SessionStarted { runtime_session_id })
            if runtime_session_id.as_str() == "sid-dir"
    )));
    assert!(ready.iter().any(|o| matches!(
        o,
        Output::Event(SessionEvent::Ready {
            permission_mode: Some(m),
            ..
        }) if m == "plan"
    )));
}
